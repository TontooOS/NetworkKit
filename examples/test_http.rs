use networkkit::http::{HttpClient, HttpRequest};
use std::time::Duration;

fn main() {
    // Simple free-function style GET.
    match HttpRequest::get("https://example.com")
        .timeout(Duration::from_secs(10))
        .send()
    {
        Ok(resp) => {
            println!("GET https://example.com -> {}", resp.status);
            println!("final url: {}", resp.url);
            let preview: String = resp.text().unwrap_or_default().chars().take(200).collect();
            println!("body preview: {}", preview);
        }
        Err(e) => eprintln!("get: {}", e),
    }

    // Session client with default headers (URLSession equivalent).
    let client = HttpClient::with_user_agent("TontooOS-Demo/1.0")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10));

    match client
        .get("https://httpbin.org/get")
        .header("X-Demo", "networkkit")
        .send()
    {
        Ok(resp) => println!("session GET -> {} ({} bytes)", resp.status, resp.body.len()),
        Err(e) => eprintln!("session get: {}", e),
    }

    // Invalid URLs fail fast without network traffic.
    match HttpRequest::get("not-a-url").send() {
        Ok(_) => println!("unexpected success"),
        Err(e) => println!("invalid url correctly rejected: {}", e),
    }
}
