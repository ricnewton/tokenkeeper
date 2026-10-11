//! "Start with Windows" via the per-user Run key (no admin rights needed).

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
// Preserve the existing opt-in setting across the TokenKeeper rename.
const VALUE: windows::core::PCWSTR = w!("ai-usage");

fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

/// The command line in the Run entry, if there is one.
fn registered() -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .ok()
    .ok()?;
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// True only if the Run entry points at this exe.
pub fn is_enabled() -> bool {
    registered().is_some_and(|current| command().is_some_and(|c| c.eq_ignore_ascii_case(&current)))
}

/// Points an existing Run entry at this exe, so autostart follows the app when
/// it is moved or a deployed copy replaces a dev build. A user who turned
/// autostart off has no entry, and that choice is kept.
pub fn follow_exe() {
    if registered().is_some() && !is_enabled() {
        set_enabled(true);
    }
}

pub fn set_enabled(enable: bool) {
    unsafe {
        if enable {
            let Some(cmd) = command() else { return };
            let wide: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE,
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE);
        }
    }
}
