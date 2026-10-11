//! XDG per-user desktop autostart entry.
//! Keep the legacy filename so the TokenKeeper rename preserves opt-in state.
use std::path::PathBuf;
fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(std::env::temp_dir)
        .join("autostart/ai-usage.desktop")
}
fn entry() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let quoted = exe
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    Some(format!(
        "[Desktop Entry]\nType=Application\nName=TokenKeeper\nExec=\"{quoted}\"\nIcon=applications-utilities\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    ))
}
pub fn is_enabled() -> bool {
    path().exists()
}
pub fn follow_exe() {
    if is_enabled() {
        set_enabled(true);
    }
}
pub fn set_enabled(enable: bool) {
    if enable {
        if let Some(e) = entry() {
            if let Some(parent) = path().parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path(), e);
        }
    } else {
        let _ = std::fs::remove_file(path());
    }
}
