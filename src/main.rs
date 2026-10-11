#![cfg_attr(windows, windows_subsystem = "windows")]

mod claude;
mod codex;
mod copilot;
mod cursor;
mod icon;
mod instance;
mod mock;
mod model;
mod opencode;
mod provider;
mod tracker;
mod worker;

#[cfg(windows)]
#[path = "autostart_windows.rs"]
mod autostart;
#[cfg(target_os = "linux")]
#[path = "autostart_linux.rs"]
mod autostart;
#[cfg(windows)]
#[path = "crypto_windows.rs"]
mod crypto;
#[cfg(target_os = "linux")]
#[path = "crypto_linux.rs"]
mod crypto;
#[cfg(windows)]
#[path = "http_windows.rs"]
mod http;
#[cfg(target_os = "linux")]
#[path = "http_linux.rs"]
mod http;
#[cfg(windows)]
#[path = "store_windows.rs"]
mod store;
#[cfg(target_os = "linux")]
#[path = "store_linux.rs"]
mod store;
#[cfg(windows)]
#[path = "time_windows.rs"]
mod time;
#[cfg(target_os = "linux")]
#[path = "time_linux.rs"]
mod time;

#[cfg(windows)]
mod clipboard;
#[cfg(target_os = "linux")]
mod desktop_linux;
#[cfg(target_os = "linux")]
mod linux_app;
#[cfg(target_os = "linux")]
mod linux_panel;
#[cfg(target_os = "linux")]
mod linux_window;
#[cfg(target_os = "linux")]
mod linux_tray;
#[cfg(windows)]
mod popup;
#[cfg(windows)]
mod sheet;
#[cfg(windows)]
#[path = "main_windows.rs"]
mod windows_app;

#[cfg(target_os = "linux")]
pub use desktop_linux::open_url;
#[cfg(windows)]
pub use windows_app::{WM_WHEEL_SWITCH, open_url};

fn main() {
    let preview = std::env::args().any(|arg| {
        matches!(
            arg.as_str(),
            "--demo" | "--demo-sheet" | "--icon-sheet" | "--popup-sheet"
        )
    });
    let _instance = if preview {
        None
    } else {
        match instance::acquire() {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => {
                eprintln!(
                    "TokenKeeper is already running; use its tray menu or quit it before running a diagnostic."
                );
                return;
            }
            Err(error) => {
                eprintln!("TokenKeeper could not acquire its instance lock: {error}");
                std::process::exit(1);
            }
        }
    };
    if std::env::args().any(|arg| arg == "--check-codex" || arg == "--check-opencode") {
        use crate::provider::Provider;
        let mut provider: Box<dyn Provider> = if std::env::args().any(|arg| arg == "--check-opencode") {
            Box::new(opencode::OpenCode::default())
        } else {
            Box::new(codex::Codex::default())
        };
        let name = provider.id().name();
        let mut usage = None;
        if !provider.load() {
            eprintln!("Sign in to {name} from the TokenKeeper tray menu first.");
            std::process::exit(1);
        }
        match provider.fetch(&mut |snap| usage = Some(snap)) {
            Ok(()) => {
                let snap = usage.expect("Provider returned no usage");
                println!(
                    "{name}: {} · {:.0}% used",
                    snap.plan,
                    snap.used_fraction() * 100.0
                );
                for meter in snap.meters {
                    println!("{}: {:.0}% used", meter.label, meter.fraction * 100.0);
                }
            }
            Err(crate::provider::FetchError::Other(message)) => {
                eprintln!("{name} usage unavailable: {message}");
                std::process::exit(1);
            }
            Err(crate::provider::FetchError::SignedOut) => {
                eprintln!("{name} session was rejected. Sign in again from the TokenKeeper tray menu.");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(windows)]
    windows_app::main();
    #[cfg(target_os = "linux")]
    linux_app::main();
}
