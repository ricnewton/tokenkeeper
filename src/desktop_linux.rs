//! Browser launching through the desktop portal, without external commands.
fn open_uri(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Unsupported browser URL".into());
    }
    pollster::block_on(async {
        let uri: ashpd::Uri = url.parse().map_err(|_| "Invalid browser URL".to_owned())?;
        ashpd::desktop::open_uri::OpenFileRequest::default()
            .send_uri(&uri)
            .await
            .map_err(|_| "The desktop portal could not open the browser".to_owned())?
            .response()
            .map_err(|_| "The desktop declined opening the browser".to_owned())
    })
}
pub fn open_url(url: &str) {
    let url = url.to_owned();
    std::thread::spawn(move || {
        if let Err(message) = open_uri(&url) {
            crate::store::log(&message);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_web_launches() {
        assert!(open_uri("file:///etc/passwd").is_err());
        assert!(open_uri("javascript:alert(1)").is_err());
    }
    #[test]
    #[ignore = "opens a browser through the running desktop portal"]
    fn live_browser_portal() {
        open_uri("https://github.com/ricnewton/tokenkeeper").unwrap();
    }
}
