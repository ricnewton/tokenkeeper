//! Offline tests of the production download, signature, extraction and replacement path.
use super::*;
use self_update::http_client::{HeaderMap, HttpClient, HttpResponse};
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const NEW_BINARY: &[u8] = b"TokenKeeper signed update fixture\n";
const OLD_BINARY: &[u8] = b"previous installed version\n";
const ASSET_URL: &str = "https://api.github.com/repos/ricnewton/tokenkeeper/releases/assets/99";

struct Response {
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl HttpResponse for Response {
    fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    fn body(self: Box<Self>) -> Box<dyn Read> {
        Box::new(Cursor::new(self.bytes))
    }
}

struct ReleaseServer {
    asset_name: String,
    archive: Vec<u8>,
    downloads: AtomicUsize,
}

impl HttpClient for ReleaseServer {
    fn get(
        &self,
        url: &str,
        _headers: &HeaderMap,
        _timeout: Option<Duration>,
    ) -> self_update::Result<Box<dyn HttpResponse>> {
        let bytes = match url {
            "https://api.github.com/repos/ricnewton/tokenkeeper/releases/tags/v99.0.0" => {
                serde_json::to_vec(&serde_json::json!({
                    "tag_name": "v99.0.0",
                    "created_at": "2026-11-12T00:00:00Z",
                    "assets": [{ "name": self.asset_name, "url": ASSET_URL }]
                }))
                .unwrap()
            }
            ASSET_URL => {
                self.downloads.fetch_add(1, Ordering::SeqCst);
                self.archive.clone()
            }
            _ => panic!("unexpected HTTP request: {url}"),
        };
        Ok(Box::new(Response {
            headers: HeaderMap::new(),
            bytes,
        }))
    }
}

struct Installation(PathBuf);

impl Installation {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".local/update-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(BINARY);
        std::fs::write(&path, OLD_BINARY).unwrap();
        Self(path)
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn fixture(signed: bool) -> Vec<u8> {
    let name = archive_name("99.0.0");
    let name = if signed {
        name
    } else {
        name.replace(".tar.gz", "-unsigned.tar.gz")
            .replace(".zip", "-unsigned.zip")
    };
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/updater")
            .join(name),
    )
    .unwrap()
}

fn update(installation: &Installation, server: Arc<ReleaseServer>) -> self_update::Result<()> {
    let expected = archive_name("99.0.0");
    builder()
        .release_tag("v99.0.0")
        .asset_matcher(move |assets| assets.iter().find(|a| a.name() == expected).cloned())
        // Always target a disposable file, never current_exe() or the real app.
        .bin_install_path(&installation.0)
        .http_client(server)
        .build()?
        .update()
        .map(|_| ())
}

#[test]
fn signed_release_replaces_only_the_disposable_installed_binary() {
    let installation = Installation::new();
    let server = Arc::new(ReleaseServer {
        asset_name: archive_name("99.0.0"),
        archive: fixture(true),
        downloads: AtomicUsize::new(0),
    });
    update(&installation, server.clone()).unwrap();
    assert_eq!(std::fs::read(&installation.0).unwrap(), NEW_BINARY);
    assert_eq!(server.downloads.load(Ordering::SeqCst), 1);
}

#[test]
fn untrusted_downloads_leave_the_installed_binary_untouched() {
    for scenario in ["tampered", "unsigned", "wrong asset name"] {
        let installation = Installation::new();
        let mut archive = fixture(scenario != "unsigned");
        if scenario == "tampered" {
            let index = archive.len() / 2;
            archive[index] ^= 1;
        }
        let server = Arc::new(ReleaseServer {
            asset_name: if scenario == "wrong asset name" {
                "unrelated-platform-99.0.0.zip".into()
            } else {
                archive_name("99.0.0")
            },
            archive,
            downloads: AtomicUsize::new(0),
        });
        let error = update(&installation, server.clone()).unwrap_err();
        if scenario != "wrong asset name" {
            assert!(
                matches!(error, self_update::Error::Signature(_)),
                "{scenario}: {error}"
            );
        }
        assert_eq!(
            std::fs::read(&installation.0).unwrap(),
            OLD_BINARY,
            "{scenario}"
        );
        assert_eq!(
            server.downloads.load(Ordering::SeqCst),
            usize::from(scenario != "wrong asset name"),
            "{scenario}"
        );
    }
}
