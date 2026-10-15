//! XDG data files and Rust D-Bus Secret Service credentials (KWallet on KDE).
//! Preserve libsecret's schema attributes so existing accounts keep working.
use crate::time;
use secret_service::{EncryptionType, blocking::SecretService};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

const SCHEMA_NAME: &str = "org.ai-usage.credentials";
fn attributes(n: &str) -> HashMap<&str, &str> {
    HashMap::from([("xdg:schema", SCHEMA_NAME), ("provider", n)])
}
fn service() -> Result<SecretService<'static>, String> {
    SecretService::connect(EncryptionType::Dh)
        .map_err(|_| "Could not connect to the desktop Secret Service".into())
}
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("ai-usage")
}
pub fn load_secret(n: &str) -> Option<Value> {
    let service = service().ok()?;
    let found = service.search_items(attributes(n)).ok()?;
    let item = found
        .unlocked
        .into_iter()
        .next()
        .or_else(|| found.locked.into_iter().next())?;
    item.unlock().ok()?;
    serde_json::from_slice(&item.get_secret().ok()?).ok()
}
pub fn save_secret(n: &str, v: &Value) -> Result<(), String> {
    let service = service()?;
    let collection = service
        .get_default_collection()
        .or_else(|error| match error {
            secret_service::Error::NoResult => {
                service.create_collection("Default keyring", "default")
            }
            other => Err(other),
        })
        .map_err(|_| "The desktop wallet could not open or create its default collection")?;
    collection
        .unlock()
        .map_err(|_| "The desktop wallet could not be unlocked")?;
    collection
        .create_item(
            "TokenKeeper credentials",
            attributes(n),
            v.to_string().as_bytes(),
            true,
            "text/plain",
        )
        .map_err(|_| "Secret Service could not store credentials")?;
    Ok(())
}
pub fn delete_secret(n: &str) {
    let result = (|| -> Result<(), String> {
        let service = service()?;
        let found = service
            .search_items(attributes(n))
            .map_err(|_| "Could not find wallet credentials")?;
        for item in found.unlocked.into_iter().chain(found.locked) {
            item.unlock()
                .map_err(|_| "Could not unlock wallet credentials")?;
            item.delete()
                .map_err(|_| "Could not delete wallet credentials")?;
        }
        Ok(())
    })();
    if let Err(message) = result {
        log(&format!("Credential removal failed: {message}"));
    }
}
pub fn load_json(file: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(data_dir().join(file)).ok()?).ok()
}
pub fn save_json(file: &str, v: &Value) {
    if std::fs::create_dir_all(data_dir()).is_ok() {
        let _ = std::fs::write(data_dir().join(file), v.to_string());
    }
}
/// Diagnostics (`debug/log.txt` and response dumps) are off unless the
/// `TOKENKEEPER_DEBUG` environment variable is set.
fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TOKENKEEPER_DEBUG").is_some())
}

pub fn log(line: &str) {
    if !debug_enabled() {
        return;
    }
    use std::io::Write;
    static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = data_dir().join("debug");
    let path = dir.join("log.txt");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let t = time::local(time::now_ms());
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                f,
                "{}-{:02}-{:02} {:02}:{:02}:{:02} {line}",
                t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
            );
        }
    }
}
pub fn save_debug(n: &str, body: &[u8]) {
    if !debug_enabled() {
        return;
    }
    let dir = data_dir().join("debug");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(n), body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wallet_attributes_preserve_libsecret_schema() {
        assert_eq!(
            attributes("opencode"),
            HashMap::from([
                ("xdg:schema", "org.ai-usage.credentials"),
                ("provider", "opencode")
            ])
        );
    }
    #[test]
    #[ignore = "requires a running unlocked desktop Secret Service"]
    fn live_wallet_roundtrip_and_replacement() {
        let n = format!("tokenkeeper-test-{}", crate::crypto::uuid_v4());
        let initial = serde_json::json!({"test": 1});
        let updated = serde_json::json!({"test": 2});
        save_secret(&n, &initial).unwrap();
        let first = load_secret(&n);
        let replacement = save_secret(&n, &updated);
        let second = load_secret(&n);
        delete_secret(&n);
        assert_eq!(first, Some(initial));
        replacement.unwrap();
        assert_eq!(second, Some(updated));
        assert!(load_secret(&n).is_none());
    }
}
