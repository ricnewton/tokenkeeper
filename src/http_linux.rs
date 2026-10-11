//! Blocking HTTPS through curl, respecting the desktop's proxy and CA store.
use std::process::Command;
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
fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
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
    // Keep bearer tokens and cookies out of the process list. Curl reads all
    // options (including request body) from a private stdin pipe.
    let mut config = format!("url = {}\nrequest = {}\n", quoted(url), quoted(method));
    let agent = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("User-Agent"))
        .map_or(USER_AGENT, |(_, v)| *v);
    config.push_str(&format!("user-agent = {}\n", quoted(agent)));
    for (k, v) in headers {
        if !k.eq_ignore_ascii_case("User-Agent") {
            config.push_str(&format!("header = {}\n", quoted(&format!("{k}: {v}"))));
        }
    }
    if let Some(data) = body {
        let data = std::str::from_utf8(data).map_err(|_| "non-UTF8 request body")?;
        config.push_str(&format!("data-binary = {}\n", quoted(data)));
    }
    let mut child = Command::new("curl")
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--max-time",
            "15",
            "--output",
            "-",
            "--write-out",
            "\nAI_USAGE_HTTP_STATUS:%{http_code}",
            "--config",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl unavailable: {e}"))?;
    use std::io::Write;
    if let Err(e) = child.stdin.take().unwrap().write_all(config.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("writing HTTPS request failed: {e}"));
    }
    parse(child.wait_with_output().map_err(|e| e.to_string())?)
}
fn parse(out: std::process::Output) -> Result<Response, String> {
    if !out.status.success() {
        return Err(format!(
            "HTTPS request failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mark = b"\nAI_USAGE_HTTP_STATUS:";
    let pos = out
        .stdout
        .windows(mark.len())
        .rposition(|w| w == mark)
        .ok_or("missing HTTP status")?;
    let status = std::str::from_utf8(&out.stdout[pos + mark.len()..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or("invalid HTTP status")?;
    Ok(Response {
        status,
        body: out.stdout[..pos].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn curl_config_escapes_header_data() {
        assert_eq!(quoted("Bearer x\"\\\n"), "\"Bearer x\\\"\\\\\\n\"");
    }
    #[test]
    fn parses_status_suffix_without_touching_body() {
        let out = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"{\"ok\":true}\nAI_USAGE_HTTP_STATUS:200".to_vec(),
            stderr: vec![],
        };
        let response = parse(out).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.json().unwrap()["ok"], true);
    }
}
