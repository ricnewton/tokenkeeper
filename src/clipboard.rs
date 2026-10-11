//! Puts text on the Windows clipboard (for device sign-in codes).

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;

/// `owner` must be a real window: with no owner, `SetClipboardData` fails
/// after `EmptyClipboard`.
pub fn set_text(owner: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return false;
        }
        let ok = (|| -> Option<()> {
            EmptyClipboard().ok()?;
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(mem);
            // On success the clipboard owns the memory.
            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_err() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            Some(())
        })();
        let _ = CloseClipboard();
        ok.is_some()
    }
}
