//! One credential-owning process per user, including command-line diagnostics.
//! Legacy lock identifiers also exclude a still-running ai-usage version.
#[cfg(target_os = "linux")]
mod platform {
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    pub struct Guard {
        _file: File,
    }
    fn lock_file(path: &Path) -> Result<Option<Guard>, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == std::io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(e.to_string())
            };
        }
        Ok(Some(Guard { _file: file }))
    }
    pub fn acquire() -> Result<Option<Guard>, String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/ai-usage")))
            .ok_or("No per-user directory is available for the instance lock")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        lock_file(&dir.join("ai-usage-instance.lock"))
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn lock_blocks_duplicate_process_and_releases_on_drop() {
            let path = std::env::temp_dir()
                .join(format!("ai-usage-lock-test-{}", crate::crypto::uuid_v4()));
            let first = lock_file(&path).unwrap().unwrap();
            assert!(lock_file(&path).unwrap().is_none());
            drop(first);
            drop(lock_file(&path).unwrap().unwrap());
            std::fs::remove_file(path).unwrap();
        }
    }
}
#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;
    pub struct Guard {
        handle: HANDLE,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
    pub fn acquire() -> Result<Option<Guard>, String> {
        let handle =
            unsafe { CreateMutexW(None, true, w!("Local\\ai-usage-tray-single-instance")) }
                .map_err(|e| e.to_string())?;
        let guard = Guard { handle };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            Ok(None)
        } else {
            Ok(Some(guard))
        }
    }
}
pub use platform::acquire;
