//! Blocking HTTP/HTTPS client for TontooOS.
//!
//! Sync-first API on top of `ureq` with `rustls` (no OpenSSL dependency).
//! All calls are blocking, matching the rest of NetworkKit. Async wrappers
//! run the same blocking code on `foundation::async_runtime::spawn_blocking`,
//! so apps can unify the previous `ureq` (sync) and `reqwest`+`tokio`
//! (async) stacks on this single module. JSON goes through Foundation
//! (`JsonValue`); there is no serde dependency.
//!
//! Quick start:
//!
//! ```rust,no_run
//! use networkkit::http::{HttpClient, HttpRequest};
//!
//! let text = HttpRequest::get("https://example.com").send()?.text()?;
//! let client = HttpClient::new();
//! let resp = client.get("https://example.com").send()?;
//! assert!(resp.is_success());
//! # Ok::<(), networkkit::NetworkError>(())
//! ```

use crate::localnet::DiscoveredHost;
use crate::types::{NetworkError, Result};
use std::net::IpAddr;
use std::time::Duration;
use ureq::ResponseExt;

pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_USER_AGENT: &str = "TontooOS-NetworkKit/26.1";

/// HTTP methods supported by [`HttpRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            "PUT" => Ok(HttpMethod::Put),
            "DELETE" => Ok(HttpMethod::Delete),
            "PATCH" => Ok(HttpMethod::Patch),
            "HEAD" => Ok(HttpMethod::Head),
            "OPTIONS" => Ok(HttpMethod::Options),
            other => Err(NetworkError::HttpError(format!(
                "unsupported method {}",
                other
            ))),
        }
    }

    /// True for methods that send a body in this client
    /// (`POST`, `PUT`, `PATCH`). `GET`, `DELETE`, `HEAD` and `OPTIONS`
    /// never send a body because ureq 3 models them as `WithoutBody`.
    pub fn has_body(&self) -> bool {
        matches!(
            self,
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
        )
    }
}

/// A single HTTP response.
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub url: String,
}

impl HttpResponse {
    /// True for 2xx status codes.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First header value for `name` (case-insensitive), if present.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Body bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// Body decoded as UTF-8. Returns `ParseError` on invalid UTF-8.
    ///
    /// For JSON, parse the text with Foundation's `JsonValue::parse`.
    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone())
            .map_err(|e| NetworkError::ParseError(e.to_string()))
    }
}

/// Builder for one HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    method: HttpMethod,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    timeout: Duration,
    user_agent: String,
    max_redirects: u32,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: &str) -> Self {
        Self {
            method,
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            max_redirects: 5,
        }
    }

    pub fn get(url: &str) -> Self {
        Self::new(HttpMethod::Get, url)
    }

    pub fn post(url: &str) -> Self {
        Self::new(HttpMethod::Post, url)
    }

    pub fn put(url: &str) -> Self {
        Self::new(HttpMethod::Put, url)
    }

    pub fn delete(url: &str) -> Self {
        Self::new(HttpMethod::Delete, url)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn body_str(mut self, body: &str) -> Self {
        self.body = Some(body.as_bytes().to_vec());
        self
    }

    /// JSON body from pre-rendered text (e.g. via Foundation's `JsonObject`
    /// or `JsonValue::stringify`). Sets `Content-Type: application/json`
    /// unless already present.
    pub fn json_body_str(mut self, json: &str) -> Self {
        if !self
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            self.headers.push((
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            ));
        }
        self.body = Some(json.as_bytes().to_vec());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn user_agent(mut self, agent: &str) -> Self {
        self.user_agent = agent.to_string();
        self
    }

    pub fn max_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Blocking send. Returns `Err` on transport errors or invalid URLs.
    /// HTTP 4xx/5xx statuses are returned as `Ok` responses, never as errors.
    pub fn send(&self) -> Result<HttpResponse> {
        validate_url(&self.url)?;
        let agent = build_agent(
            self.timeout,
            &self.user_agent,
            self.max_redirects,
        );
        execute(&agent, self.method, &self.url, &self.headers, self.body.as_deref())
    }

    /// Async variant. Runs the blocking request on a blocking thread.
    /// Must be called inside a Tokio runtime.
    pub async fn send_async(&self) -> Result<HttpResponse> {
        let req = self.clone();
        foundation::async_runtime::spawn_blocking(move || req.send())
            .await
            .map_err(|e| NetworkError::HttpError(e.to_string()))?
    }
}

/// Session client holding default headers, timeout and user agent.
///
/// Reuses connections across requests via one internal `ureq` agent.
#[derive(Debug, Clone)]
pub struct HttpClient {
    default_headers: Vec<(String, String)>,
    timeout: Duration,
    user_agent: String,
    max_redirects: u32,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            default_headers: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            max_redirects: 5,
        }
    }

    pub fn with_user_agent(agent: &str) -> Self {
        Self {
            user_agent: agent.to_string(),
            ..Self::new()
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.default_headers
            .push((name.to_string(), value.to_string()));
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn max_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    fn agent(&self) -> ureq::Agent {
        build_agent(self.timeout, &self.user_agent, self.max_redirects)
    }

    /// Blocking request with the session defaults merged under per-call headers.
    pub fn request(
        &self,
        method: HttpMethod,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse> {
        validate_url(url)?;
        let mut merged = self.default_headers.clone();
        merged.extend(headers);
        execute(&self.agent(), method, url, &merged, body.as_deref())
    }

    /// Async variant of [`HttpClient::request`]. Must run inside Tokio.
    pub async fn request_async(
        &self,
        method: HttpMethod,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse> {
        let client = self.clone();
        foundation::async_runtime::spawn_blocking(move || {
            client.request(method, &url, headers, body)
        })
        .await
        .map_err(|e| NetworkError::HttpError(e.to_string()))?
    }

    pub fn get(&self, url: &str) -> RequestBuilder<'_> {
        RequestBuilder {
            client: self,
            request: HttpRequest {
                timeout: self.timeout,
                user_agent: self.user_agent.clone(),
                max_redirects: self.max_redirects,
                ..HttpRequest::new(HttpMethod::Get, url)
            },
        }
    }

    pub fn post(&self, url: &str) -> RequestBuilder<'_> {
        RequestBuilder {
            client: self,
            request: HttpRequest {
                timeout: self.timeout,
                user_agent: self.user_agent.clone(),
                max_redirects: self.max_redirects,
                ..HttpRequest::new(HttpMethod::Post, url)
            },
        }
    }

    pub fn put(&self, url: &str) -> RequestBuilder<'_> {
        RequestBuilder {
            client: self,
            request: HttpRequest {
                timeout: self.timeout,
                user_agent: self.user_agent.clone(),
                max_redirects: self.max_redirects,
                ..HttpRequest::new(HttpMethod::Put, url)
            },
        }
    }

    pub fn delete(&self, url: &str) -> RequestBuilder<'_> {
        RequestBuilder {
            client: self,
            request: HttpRequest {
                timeout: self.timeout,
                user_agent: self.user_agent.clone(),
                max_redirects: self.max_redirects,
                ..HttpRequest::new(HttpMethod::Delete, url)
            },
        }
    }
}

/// Fluent builder binding a [`HttpClient`] session to one request.
pub struct RequestBuilder<'a> {
    client: &'a HttpClient,
    request: HttpRequest,
}

impl<'a> RequestBuilder<'a> {
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.request = self.request.header(name, value);
        self
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.request = self.request.body(body);
        self
    }

    pub fn body_str(mut self, body: &str) -> Self {
        self.request = self.request.body_str(body);
        self
    }

    pub fn json_body_str(mut self, json: &str) -> Self {
        self.request = self.request.json_body_str(json);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request = self.request.timeout(timeout);
        self
    }

    pub fn send(&self) -> Result<HttpResponse> {
        let mut merged = self.client.default_headers.clone();
        merged.extend(self.request.headers.clone());
        validate_url(&self.request.url)?;
        execute(
            &self.client.agent(),
            self.request.method,
            &self.request.url,
            &merged,
            self.request.body.as_deref(),
        )
    }

    pub async fn send_async(&self) -> Result<HttpResponse> {
        let client = self.client.clone();
        let request = self.request.clone();
        foundation::async_runtime::spawn_blocking(move || {
            let mut merged = client.default_headers.clone();
            merged.extend(request.headers.clone());
            validate_url(&request.url)?;
            execute(
                &client.agent(),
                request.method,
                &request.url,
                &merged,
                request.body.as_deref(),
            )
        })
        .await
        .map_err(|e| NetworkError::HttpError(e.to_string()))?
    }
}

/// Blocking GET. Returns 4xx/5xx as `Ok` responses.
pub fn get(url: &str) -> Result<HttpResponse> {
    HttpRequest::get(url).send()
}

/// Blocking GET with extra headers.
pub fn get_with_headers(url: &str, headers: Vec<(String, String)>) -> Result<HttpResponse> {
    let mut req = HttpRequest::get(url);
    req = req.headers(headers);
    req.send()
}

/// Blocking POST of raw bytes.
pub fn post(url: &str, body: &[u8]) -> Result<HttpResponse> {
    HttpRequest::post(url).body(body.to_vec()).send()
}

/// Blocking POST of pre-rendered JSON text.
pub fn post_json(url: &str, json: &str) -> Result<HttpResponse> {
    HttpRequest::post(url).json_body_str(json).send()
}

/// Blocking PUT of raw bytes.
pub fn put(url: &str, body: &[u8]) -> Result<HttpResponse> {
    HttpRequest::put(url).body(body.to_vec()).send()
}

/// Blocking DELETE.
pub fn delete(url: &str) -> Result<HttpResponse> {
    HttpRequest::delete(url).send()
}

/// Async GET. Must run inside a Tokio runtime.
pub async fn get_async(url: String) -> Result<HttpResponse> {
    HttpRequest::get(&url).send_async().await
}

/// Build an `http(s)://host:port/path` URL and GET it.
///
/// Bridge between mDNS/discovery results and actual usage: discover a host
/// with `LocalNetwork`, then fetch from it without a second HTTP dependency.
pub fn fetch_from_host(host: IpAddr, port: u16, path: &str, use_tls: bool) -> Result<HttpResponse> {
    let scheme = if use_tls { "https" } else { "http" };
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    let url = match host {
        IpAddr::V4(v4) => format!("{}://{}:{}{}", scheme, v4, port, path),
        IpAddr::V6(v6) => format!("{}://[{}]:{}{}", scheme, v6, port, path),
    };
    get(&url)
}

/// GET `path` from a host previously found via discovery.
pub fn fetch_from_discovered(host: &DiscoveredHost, path: &str) -> Result<HttpResponse> {
    let port = host.port.unwrap_or(80);
    fetch_from_host(host.address, port, path, false)
}

fn validate_url(url: &str) -> Result<()> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(())
    } else {
        Err(NetworkError::InvalidUrl(url.to_string()))
    }
}

fn build_agent(timeout: Duration, user_agent: &str, max_redirects: u32) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(user_agent)
        .max_redirects(max_redirects)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build();
    config.into()
}

fn execute(
    agent: &ureq::Agent,
    method: HttpMethod,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<HttpResponse> {
    // ureq 3 splits builders into WithoutBody (GET/DELETE/HEAD/OPTIONS)
    // and WithBody (POST/PUT/PATCH), so each arm runs separately.
    match method {
        HttpMethod::Get => {
            let mut builder = agent.get(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            to_response(builder.call().map_err(NetworkError::from)?)
        }
        HttpMethod::Delete => {
            let mut builder = agent.delete(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            to_response(builder.call().map_err(NetworkError::from)?)
        }
        HttpMethod::Head => {
            let mut builder = agent.head(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            to_response(builder.call().map_err(NetworkError::from)?)
        }
        HttpMethod::Options => {
            let mut builder = agent.options(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            to_response(builder.call().map_err(NetworkError::from)?)
        }
        HttpMethod::Post => {
            let mut builder = agent.post(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            match body {
                Some(bytes) => to_response(builder.send(bytes).map_err(NetworkError::from)?),
                None => to_response(builder.send_empty().map_err(NetworkError::from)?),
            }
        }
        HttpMethod::Put => {
            let mut builder = agent.put(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            match body {
                Some(bytes) => to_response(builder.send(bytes).map_err(NetworkError::from)?),
                None => to_response(builder.send_empty().map_err(NetworkError::from)?),
            }
        }
        HttpMethod::Patch => {
            let mut builder = agent.patch(url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            match body {
                Some(bytes) => to_response(builder.send(bytes).map_err(NetworkError::from)?),
                None => to_response(builder.send_empty().map_err(NetworkError::from)?),
            }
        }
    }
}

fn to_response(mut response: ureq::http::Response<ureq::Body>) -> Result<HttpResponse> {
    let status = response.status().as_u16();
    let headers_out = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let final_url = response.get_uri().to_string();
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(NetworkError::from)?;
    Ok(HttpResponse {
        status,
        headers: headers_out,
        body: bytes,
        url: final_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_roundtrip() {
        assert_eq!(HttpMethod::parse("get").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert!(HttpMethod::parse("BREW").is_err());
    }

    #[test]
    fn rejects_non_http_urls() {
        assert!(matches!(
            get("ftp://example.com/x"),
            Err(NetworkError::InvalidUrl(_))
        ));
        assert!(matches!(
            get("example.com/no-scheme"),
            Err(NetworkError::InvalidUrl(_))
        ));
    }

    #[test]
    fn response_helpers() {
        let resp = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: b"{\"a\":1}".to_vec(),
            url: "https://example.com/".to_string(),
        };
        assert!(resp.is_success());
        assert_eq!(resp.header("content-type"), Some("text/plain"));
        assert_eq!(resp.text().unwrap(), "{\"a\":1}");
        let v = foundation::serialization::JsonValue::parse(&resp.text().unwrap()).unwrap();
        assert_eq!(v.get("a").and_then(|x| x.as_i64()), Some(1));
    }

    #[test]
    fn invalid_utf8_is_parse_error() {
        let resp = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: vec![0xff, 0xfe],
            url: String::new(),
        };
        assert!(matches!(resp.text(), Err(NetworkError::ParseError(_))));
    }

    #[test]
    fn fetch_from_host_builds_url() {
        let ip: IpAddr = "192.168.1.42".parse().unwrap();
        let err = fetch_from_host(ip, 8080, "index.html", false).unwrap_err();
        match err {
            NetworkError::HttpError(_) | NetworkError::Timeout => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }
}
