# Http

The `http` module is the single blocking HTTP/HTTPS client for TontooOS. It
replaces per-app `ureq` / `reqwest` stacks with one sync-first API on top of
`ureq` with `rustls` (no OpenSSL dependency). All calls block the current
thread; async wrappers run the same code on
`foundation::async_runtime::spawn_blocking`. JSON goes through Foundation
(`JsonValue`); there is no serde dependency.

## HttpMethod

```rust
pub enum HttpMethod { Get, Post, Put, Delete, Patch, Head, Options }
```

### HttpMethod::as_str

```rust
pub fn as_str(&self) -> &'static str
```

- Returns the uppercase method name, e.g. `"GET"`.
- Never fails.

### HttpMethod::parse

```rust
pub fn parse(raw: &str) -> Result<Self>
```

- Parses case-insensitively with surrounding whitespace trimmed.
- Returns `Err(NetworkError::HttpError)` for unknown methods.

```rust
let method = HttpMethod::parse("post").unwrap();
```

## HttpResponse

```rust
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub url: String,
}
```

- `status` is the final HTTP status after redirects.
- `url` is the final URL after redirects.
- HTTP 4xx/5xx statuses are returned as `Ok(HttpResponse)`, never as `Err`.
- Transport failures return `Err(NetworkError::HttpError)` or
  `Err(NetworkError::Timeout)`.

### HttpResponse::is_success

```rust
pub fn is_success(&self) -> bool
```

- True for 2xx status codes.

### HttpResponse::header

```rust
pub fn header(&self, name: &str) -> Option<&str>
```

- Case-insensitive lookup of the first matching header.
- Returns `None` when the header is absent.

### HttpResponse::text

```rust
pub fn text(&self) -> Result<String>
```

- Decodes the body as UTF-8.
- Returns `Err(NetworkError::ParseError)` on invalid UTF-8.

### JSON bodies

Parse response text with Foundation's `JsonValue::parse`; send pre-rendered
JSON with `json_body_str` / `post_json`:

```rust
let value = foundation::serialization::JsonValue::parse(&resp.text()?).unwrap();
let resp = client.post("https://example.com").json_body_str(r#"{"a":1}"#).send()?;
```

## HttpRequest

Builder for a single request.

```rust
pub struct HttpRequest { /* method, url, headers, body, timeout, ... */ }
```

### Constructors

```rust
pub fn new(method: HttpMethod, url: &str) -> Self
pub fn get(url: &str) -> Self
pub fn post(url: &str) -> Self
pub fn put(url: &str) -> Self
pub fn delete(url: &str) -> Self
```

### Builder methods

```rust
pub fn header(self, name: &str, value: &str) -> Self
pub fn headers(self, headers: Vec<(String, String)>) -> Self
pub fn body(self, body: Vec<u8>) -> Self
pub fn body_str(self, body: &str) -> Self
pub fn json_body_str(self, json: &str) -> Self
pub fn timeout(self, timeout: Duration) -> Self
pub fn user_agent(self, agent: &str) -> Self
pub fn max_redirects(self, max: u32) -> Self
```

- `json_body_str` sends pre-rendered JSON and sets
  `Content-Type: application/json; charset=utf-8` when absent.

### HttpRequest::send

```rust
pub fn send(&self) -> Result<HttpResponse>
```

- Blocking send. Validates that the URL starts with `http://` or `https://`.
- Returns `Err(NetworkError::InvalidUrl)` for other schemes or missing schemes.
- Follows redirects (default 5, `max_redirects_will_error(false)`).
- `GET`, `DELETE`, `HEAD` and `OPTIONS` never send a body (ureq 3 models
  them as `WithoutBody`); only `POST`, `PUT` and `PATCH` send one.

```rust
let resp = HttpRequest::get("https://example.com").send().unwrap();
```

### HttpRequest::send_async

```rust
pub async fn send_async(&self) -> Result<HttpResponse>
```

- Same as `send`, executed on `foundation::async_runtime::spawn_blocking`.
- Must be called inside a Tokio runtime.
- Returns `Err(NetworkError::HttpError)` when the blocking task fails to join.

## HttpClient

Session client with shared defaults and connection reuse.

```rust
pub struct HttpClient { /* default_headers, timeout, user_agent, ... */ }
```

### Constructors

```rust
pub fn new() -> Self
pub fn with_user_agent(agent: &str) -> Self
```

### Configuration

```rust
pub fn header(self, name: &str, value: &str) -> Self
pub fn timeout(self, timeout: Duration) -> Self
pub fn max_redirects(self, max: u32) -> Self
```

### HttpClient::request

```rust
pub fn request(
    &self,
    method: HttpMethod,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse>
```

- Merges session `default_headers` under per-call `headers`.
- Returns `Err(NetworkError::InvalidUrl)` for non-HTTP(S) URLs.

### HttpClient::request_async

```rust
pub async fn request_async(
    &self,
    method: HttpMethod,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse>
```

- Async variant of `request` via `spawn_blocking`. Requires a Tokio runtime.

### Convenience builders

```rust
pub fn get(&self, url: &str) -> RequestBuilder<'_>
pub fn post(&self, url: &str) -> RequestBuilder<'_>
pub fn put(&self, url: &str) -> RequestBuilder<'_>
pub fn delete(&self, url: &str) -> RequestBuilder<'_>
```

```rust
let client = HttpClient::with_user_agent("MyApp/1.0");
let resp = client.get("https://example.com").header("Accept", "text/html").send().unwrap();
```

## Free Functions

```rust
pub fn get(url: &str) -> Result<HttpResponse>
pub fn get_with_headers(url: &str, headers: Vec<(String, String)>) -> Result<HttpResponse>
pub fn post(url: &str, body: &[u8]) -> Result<HttpResponse>
pub fn post_json(url: &str, json: &str) -> Result<HttpResponse>
pub fn put(url: &str, body: &[u8]) -> Result<HttpResponse>
pub fn delete(url: &str) -> Result<HttpResponse>
pub async fn get_async(url: String) -> Result<HttpResponse>
```

- All are blocking except `get_async`.
- Returns `Err(NetworkError::InvalidUrl)` when the URL has no `http(s)://` prefix.

## Discovery Bridge

```rust
pub fn fetch_from_host(host: IpAddr, port: u16, path: &str, use_tls: bool) -> Result<HttpResponse>
pub fn fetch_from_discovered(host: &DiscoveredHost, path: &str) -> Result<HttpResponse>
```

- `fetch_from_host` builds `http(s)://host:port/path` and GETs it. IPv6 hosts
  are bracketed. A missing leading `/` in `path` is added.
- `fetch_from_discovered` uses `host.port` or `80` with plain HTTP.
- Returns transport errors as `Err`; 4xx/5xx come back as `Ok`.

```rust
let host: IpAddr = "192.168.1.42".parse().unwrap();
let resp = fetch_from_host(host, 8080, "/status", false).unwrap();
```

## C FFI

| Function | Return | Meaning |
|---|---|---|
| `tontoo_networkkit_http_get(url, error_out)` | `char*` JSON or null | JSON `{status, headers, body, url}` on success, null + `error_out` on failure |
| `tontoo_networkkit_http_post(url, body, body_len, error_out)` | `char*` JSON or null | Same JSON shape for POST with raw bytes |
| `tontoo_networkkit_string_free(s)` | `void` | Frees strings returned by this API |

```c
char *err = NULL;
char *json = tontoo_networkkit_http_get("https://example.com", &err);
// ... parse json ...
tontoo_networkkit_string_free(json);
tontoo_networkkit_string_free(err);
```

## Errors

| Error | Meaning |
|---|---|
| `NetworkError::InvalidUrl(url)` | URL without `http://` or `https://` prefix |
| `NetworkError::Timeout` | Global timeout elapsed (mapped from `ureq::Error::Timeout`) |
| `NetworkError::HttpError(msg)` | Transport, TLS, redirect or join failure |
| `NetworkError::ParseError(msg)` | `text()` on invalid UTF-8 |

## Usage / Example

```rust
use networkkit::http::{HttpClient, HttpRequest};
use std::time::Duration;

let text = HttpRequest::get("https://example.com")
    .timeout(Duration::from_secs(10))
    .send()?
    .text()?;

let client = HttpClient::with_user_agent("MyApp/1.0").header("Accept", "application/json");
let text = client
    .get("https://httpbin.org/get")
    .send()?
    .text()?;
let value = foundation::serialization::JsonValue::parse(&text)?;
```

See `examples/test_http.rs` for a runnable demo.

## Cross References

- [LocalNetwork.md](LocalNetwork.md) – discover hosts via mDNS/TCP sweep, then fetch from them with `fetch_from_discovered`
- [MAIN.md](MAIN.md) – library overview and feature index
- [Localization.md](Localization.md) – `invalid_url` and `http_error` message keys
