//! Blocking HTTPS in-process, with rustls and the system certificate verifier.
use std::sync::OnceLock;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig};
pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) tokenkeeper/",
    env!("CARGO_PKG_VERSION")
);
pub struct Response {
    pub status: u32,
    pub body: Vec<u8>,
}
impl Response {
    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body)
            .map_err(|e| format!("invalid JSON (HTTP {}): {e}", self.status))
    }
}
fn proxy_from(
    mut value: impl FnMut(&str) -> Option<String>,
) -> Result<Option<ureq::Proxy>, String> {
    // Curl gives protocol-specific settings priority over ALL_PROXY.
    let proxy = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"]
        .into_iter()
        .find_map(|key| value(key).filter(|v| !v.is_empty()));
    let Some(proxy) = proxy else { return Ok(None) };
    let parsed = ureq::Proxy::new(&proxy).map_err(|_| "Invalid HTTPS proxy configuration")?;
    let mut builder = ureq::Proxy::builder(parsed.protocol())
        .host(parsed.host())
        .port(parsed.port())
        .resolve_target(parsed.resolve_target());
    if let Some(user) = parsed.username() {
        builder = builder.username(user);
    }
    if let Some(password) = parsed.password() {
        builder = builder.password(password);
    }
    if let Some(bypass) = value("no_proxy").or_else(|| value("NO_PROXY")) {
        for host in bypass.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            builder = builder.no_proxy(host);
            // Curl's bare domain entries also match subdomains.
            if host != "*" && !host.starts_with('.') && host.parse::<std::net::IpAddr>().is_err() {
                builder = builder.no_proxy(&format!(".{host}"));
            }
        }
    }
    builder
        .build()
        .map(Some)
        .map_err(|_| "Invalid HTTPS proxy configuration".into())
}
fn configuration(proxy: Option<ureq::Proxy>) -> ureq::config::Config {
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(15)))
        // Providers inspect status/body to handle pending grants and refresh.
        .http_status_as_error(false)
        // Match curl's previous behavior: never forward credentials on redirects.
        .max_redirects(0)
        .proxy(proxy)
        .build()
}
fn agent() -> Result<&'static ureq::Agent, String> {
    static AGENT: OnceLock<Result<ureq::Agent, String>> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let proxy = proxy_from(|key| std::env::var(key).ok())?;
            Ok(configuration(proxy).new_agent())
        })
        .as_ref()
        .map_err(Clone::clone)
}
pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
    request("GET", url, headers, None)
}
pub fn post_json(
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
) -> Result<Response, String> {
    let mut all = vec![("Content-Type", "application/json")];
    all.extend_from_slice(headers);
    request("POST", url, &all, Some(body.to_string().as_bytes()))
}
pub fn request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Response, String> {
    if !url.starts_with("https://") {
        return Err("only https URLs are supported".into());
    }
    if !matches!(method, "GET" | "POST") {
        return Err("unsupported HTTP method".into());
    }
    execute(agent()?, method, url, headers, body)
}
fn execute(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Response, String> {
    let mut request = ureq::http::Request::builder().method(method).uri(url);
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("User-Agent"))
    {
        request = request.header("User-Agent", USER_AGENT);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(body.unwrap_or_default())
        .map_err(|_| "Invalid HTTPS request headers or URL".to_string())?;
    let mut response = agent
        .run(request)
        .map_err(|e| format!("HTTPS request failed: {e}"))?;
    let status = u32::from(response.status().as_u16());
    let body = response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("Reading HTTPS response failed: {e}"))?;
    Ok(Response { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    fn exchange(
        method: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
        status: &str,
        extra: &str,
    ) -> (Response, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usage", listener.local_addr().unwrap());
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Length: 13\r\nConnection: close\r\n{extra}\r\n{{\"test\":true}}"
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut received = Vec::new();
            let header_end = loop {
                let mut b = [0; 1];
                stream.read_exact(&mut b).unwrap();
                received.push(b[0]);
                if received.ends_with(b"\r\n\r\n") {
                    break received.len();
                }
            };
            let text = String::from_utf8(received.clone()).unwrap();
            let size = text
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            received.resize(header_end + size, 0);
            stream.read_exact(&mut received[header_end..]).unwrap();
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8(received).unwrap()
        });
        // Exercise the actual HTTP transport on loopback; public entry remains HTTPS-only.
        let response = execute(
            &configuration(None).new_agent(),
            method,
            &url,
            headers,
            body,
        )
        .unwrap();
        (response, server.join().unwrap())
    }
    #[test]
    fn honors_https_proxy_precedence_socks_and_bypass() {
        let environment = std::collections::HashMap::from([
            ("HTTPS_PROXY", "socks5h://user:pass@127.0.0.1:1080"),
            ("ALL_PROXY", "http://127.0.0.1:8080"),
            ("NO_PROXY", "localhost,example.com"),
        ]);
        let proxy = proxy_from(|key| environment.get(key).map(|v| v.to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(proxy.protocol(), ureq::ProxyProtocol::Socks5h);
        assert_eq!(proxy.port(), 1080);
        assert!(!proxy.resolve_target());
        assert!(proxy.is_no_proxy(&"https://localhost/".parse().unwrap()));
        assert!(proxy.is_no_proxy(&"https://api.example.com/".parse().unwrap()));
        assert!(!proxy.is_no_proxy(&"https://api.openai.com/".parse().unwrap()));
        assert!(proxy_from(|_| Some("not a valid proxy".into())).is_err());
    }
    #[test]
    fn keeps_unauthorized_body_for_refresh_and_sends_auth_headers() {
        let (response, received) = exchange(
            "GET",
            &[("Authorization", "Bearer sample")],
            None,
            "401 Unauthorized",
            "",
        );
        assert_eq!(response.status, 401);
        assert_eq!(response.json().unwrap()["test"], true);
        assert!(
            received
                .to_lowercase()
                .contains("authorization: bearer sample")
        );
    }
    #[test]
    fn posts_json_and_honors_provider_user_agent() {
        let body = br#"{"code":"sample"}"#;
        let (_, received) = exchange(
            "POST",
            &[
                ("Content-Type", "application/json"),
                ("User-Agent", "claude-test"),
            ],
            Some(body),
            "200 OK",
            "",
        );
        assert!(received.starts_with("POST /usage "));
        assert!(
            received
                .to_lowercase()
                .contains("user-agent: claude-test\r\n")
        );
        assert!(!received.contains(USER_AGENT));
        assert!(received.ends_with(std::str::from_utf8(body).unwrap()));
    }
    #[test]
    fn returns_redirect_without_forwarding_credentials() {
        let (response, _) = exchange(
            "GET",
            &[("Authorization", "Bearer sample")],
            None,
            "302 Found",
            "Location: http://127.0.0.1:1/leak\r\n",
        );
        assert_eq!(response.status, 302);
    }
    #[test]
    fn rejects_insecure_urls_and_header_injection() {
        assert!(get("http://127.0.0.1:1", &[]).is_err());
        assert!(request("DELETE", "https://example.com", &[], None).is_err());
        let result = execute(
            &configuration(None).new_agent(),
            "GET",
            "http://127.0.0.1:1",
            &[("Authorization", "Bearer x\r\nInjected: y")],
            None,
        );
        assert_eq!(
            result.err().unwrap(),
            "Invalid HTTPS request headers or URL"
        );
    }
}
