//! Files in `%APPDATA%\ai-usage`: DPAPI-protected credentials, caches and
//! diagnostics.

use std::path::PathBuf;

use serde_json::Value;

use crate::crypto;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("ai-usage")
}

fn secret_path(name: &str) -> PathBuf {
    data_dir().join(format!("{name}-tokens.bin"))
}

/// Loads credentials saved with `save_secret`, readable only by this Windows user.
pub fn load_secret(name: &str) -> Option<Value> {
    let blob = std::fs::read(secret_path(name)).ok()?;
    serde_json::from_slice(&crypto::unprotect(&blob).ok()?).ok()
}

pub fn save_secret(name: &str, v: &Value) -> Result<(), String> {
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let blob = crypto::protect(v.to_string().as_bytes())?;
    std::fs::write(secret_path(name), blob).map_err(|e| e.to_string())
}

pub fn delete_secret(name: &str) {
    let _ = std::fs::remove_file(secret_path(name));
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

/// Appends a timestamped line to `debug\log.txt`, starting afresh past 256 KB.
pub fn log(line: &str) {
    if !debug_enabled() {
        return;
    }
    use std::io::Write;
    let dir = data_dir().join("debug");
    let path = dir.join("log.txt");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    let t = crate::time::local(crate::time::now_ms());
    let stamp = format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{stamp} {line}");
        }
    }
}

/// Records a failure or unexpected response in the data dir for diagnosis.
pub fn save_debug(name: &str, body: &[u8]) {
    if !debug_enabled() {
        return;
    }
    let dir = data_dir().join("debug");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(name), body);
    }
}
