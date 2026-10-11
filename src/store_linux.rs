//! XDG data files and Secret Service credentials (KWallet on KDE).
//! Legacy ai-usage identifiers preserve credentials and history after renaming.
use crate::time;
use serde_json::Value;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
#[repr(C)]
struct Attr {
    name: *const c_char,
    kind: i32,
}
// Layout follows libsecret/secret-schema.h: name, flags, 32 attributes,
// a reserved gint, then seven reserved gpointer slots.
// https://sources.debian.org/src/libsecret/0.21.7-1/libsecret/secret-schema.h
#[repr(C)]
struct Schema {
    name: *const c_char,
    flags: i32,
    attrs: [Attr; 32],
    reserved: i32,
    padding: [*mut c_void; 7],
}
#[link(name = ":libsecret-1.so.0")]
unsafe extern "C" {
    fn secret_password_lookup_sync(
        schema: *const Schema,
        cancel: *mut c_void,
        error: *mut *mut c_void,
        ...
    ) -> *mut c_char;
    fn secret_password_store_sync(
        schema: *const Schema,
        collection: *const c_char,
        label: *const c_char,
        password: *const c_char,
        cancel: *mut c_void,
        error: *mut *mut c_void,
        ...
    ) -> i32;
    fn secret_password_clear_sync(
        schema: *const Schema,
        cancel: *mut c_void,
        error: *mut *mut c_void,
        ...
    ) -> i32;
    fn secret_password_free(password: *mut c_char);
}
#[link(name = ":libglib-2.0.so.0")]
unsafe extern "C" {
    fn g_error_free(error: *mut c_void);
}
const SCHEMA_NAME: &[u8] = b"org.ai-usage.credentials\0";
const ATTR_NAME: &[u8] = b"provider\0";
fn schema() -> Schema {
    let mut attrs = [const {
        Attr {
            name: std::ptr::null(),
            kind: 0,
        }
    }; 32];
    attrs[0] = Attr {
        name: ATTR_NAME.as_ptr().cast(),
        kind: 0,
    };
    Schema {
        name: SCHEMA_NAME.as_ptr().cast(),
        flags: 0,
        attrs,
        reserved: 0,
        padding: [std::ptr::null_mut(); 7],
    }
}
fn attr_key() -> *const c_char {
    ATTR_NAME.as_ptr().cast()
}
fn name(n: &str) -> Option<CString> {
    CString::new(n).ok()
}
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("ai-usage")
}
pub fn load_secret(n: &str) -> Option<Value> {
    let n = name(n)?;
    let mut error = std::ptr::null_mut();
    let ptr = unsafe {
        secret_password_lookup_sync(
            &schema(),
            std::ptr::null_mut(),
            &mut error,
            attr_key(),
            n.as_ptr(),
            std::ptr::null::<c_char>(),
        )
    };
    if !error.is_null() {
        unsafe { g_error_free(error) };
        return None;
    }
    if ptr.is_null() {
        return None;
    }
    let raw = unsafe { CStr::from_ptr(ptr).to_bytes().to_vec() };
    unsafe { secret_password_free(ptr) };
    serde_json::from_slice(&raw).ok()
}
pub fn save_secret(n: &str, v: &Value) -> Result<(), String> {
    let n = name(n).ok_or("invalid provider name")?;
    let pw = CString::new(v.to_string()).map_err(|e| e.to_string())?;
    let mut error = std::ptr::null_mut();
    let ok = unsafe {
        secret_password_store_sync(
            &schema(),
            std::ptr::null(),
            b"TokenKeeper credentials\0".as_ptr().cast(),
            pw.as_ptr(),
            std::ptr::null_mut(),
            &mut error,
            attr_key(),
            n.as_ptr(),
            std::ptr::null::<c_char>(),
        )
    };
    if !error.is_null() {
        unsafe { g_error_free(error) };
        return Err("Secret Service could not store credentials".into());
    }
    if ok == 0 {
        return Err("Secret Service refused credentials".into());
    }
    Ok(())
}
pub fn delete_secret(n: &str) {
    if let Some(n) = name(n) {
        let mut error = std::ptr::null_mut();
        unsafe {
            secret_password_clear_sync(
                &schema(),
                std::ptr::null_mut(),
                &mut error,
                attr_key(),
                n.as_ptr(),
                std::ptr::null::<c_char>(),
            );
            if !error.is_null() {
                g_error_free(error);
            }
        }
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
pub fn log(line: &str) {
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
    let dir = data_dir().join("debug");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(n), body);
    }
}
