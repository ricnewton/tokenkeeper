//! Minimal blocking HTTPS client on WinHTTP. Using the OS stack keeps the
//! binary small and honours the system proxy and certificate store, which
//! matters on corporate networks with TLS inspection.

use std::ffi::c_void;

use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DECOMPRESSION,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) tokenkeeper/",
    env!("CARGO_PKG_VERSION")
);
const TIMEOUT_MS: i32 = 15_000;
/// WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE.
const DECOMPRESS_ALL: u32 = 0x3;

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

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn check(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!(
            "{what} failed: {}",
            windows::core::Error::from_thread().message()
        ))
    } else {
        Ok(Handle(h))
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

pub fn request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Response, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("only https URLs are supported")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let agent = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("User-Agent"))
        .map_or(USER_AGENT, |(_, v)| *v);
    let headers: Vec<_> = headers
        .iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("User-Agent"))
        .collect();

    unsafe {
        let session = check(
            WinHttpOpen(
                &HSTRING::from(agent),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        let _ = WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
        let flags = DECOMPRESS_ALL.to_ne_bytes();
        let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_DECOMPRESSION, Some(&flags));

        let connect = check(
            WinHttpConnect(session.0, &HSTRING::from(host), 443, 0),
            "WinHttpConnect",
        )?;
        let req = check(
            WinHttpOpenRequest(
                connect.0,
                &HSTRING::from(method),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;

        let header_text: String = headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}\r\n"))
            .collect();
        let header_wide: Vec<u16> = header_text.encode_utf16().collect();
        let body = body.unwrap_or(&[]);
        WinHttpSendRequest(
            req.0,
            if header_wide.is_empty() {
                None
            } else {
                Some(&header_wide)
            },
            if body.is_empty() {
                None
            } else {
                Some(body.as_ptr().cast())
            },
            body.len() as u32,
            body.len() as u32,
            0,
        )
        .map_err(|e| format!("request to {host} failed: {}", e.message()))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())
            .map_err(|e| format!("no response from {host}: {}", e.message()))?;

        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("bad response from {host}: {}", e.message()))?;

        let mut out = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read)
                .map_err(|e| format!("reading response from {host} failed: {}", e.message()))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        Ok(Response { status, body: out })
    }
}
