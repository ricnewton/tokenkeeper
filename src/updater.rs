//! User-triggered, signed GitHub updates. Network work never runs on the UI thread.
use self_update::update::Releases;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const VERSION: &str = env!("CARGO_PKG_VERSION");
const PUBLIC_KEY: [u8; 32] = *include_bytes!("../update-public-key.bin");
pub const RESTART_ARGUMENT: &str = "--updated-restart";
#[cfg(windows)]
const BINARY: &str = "tokenkeeper.exe";
#[cfg(target_os = "linux")]
const BINARY: &str = "tokenkeeper";

#[derive(Debug)]
enum State {
    Idle,
    Checking,
    Available(String),
    Installing,
    Current,
    Failed(String),
    Restart,
    RestartFailed,
}

enum Outcome {
    Checked(Option<String>),
    Installed,
}

pub struct Updater {
    state: State,
    pending: Option<Receiver<Result<Outcome, String>>>,
    next_check: Instant,
}

impl Updater {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            pending: None,
            next_check: Instant::now(),
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub fn without_automatic_check() -> Self {
        Self {
            next_check: Instant::now() + CHECK_INTERVAL,
            ..Self::new()
        }
    }

    pub fn label(&self) -> String {
        match self.state {
            State::Idle | State::Current | State::Failed(_) => "Check for update",
            State::Checking => "Checking…",
            State::Available(_) => "Update now",
            State::Installing => "Updating…",
            State::Restart => "Restarting…",
            State::RestartFailed => "Restart now",
        }
        .into()
    }

    pub fn status(&self) -> String {
        match &self.state {
            State::Idle => format!("TokenKeeper {VERSION}"),
            State::Checking => "Checking for updates…".into(),
            State::Available(version) => format!("Update to {version} and restart"),
            State::Installing => "Downloading and verifying update…".into(),
            State::Current => format!("TokenKeeper {VERSION} is up to date"),
            State::Failed(error) => format!("Update failed: {error}"),
            State::Restart => "Finishing requests before restart…".into(),
            State::RestartFailed => "Update installed — retry restart".into(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.pending.is_none() && !self.ready_to_restart()
    }

    pub fn installing(&self) -> bool {
        matches!(self.state, State::Installing | State::Restart)
    }

    /// Called by each UI's existing timer. Returns true if its menu needs refreshing.
    pub fn poll(&mut self) -> bool {
        if let Some(rx) = &self.pending {
            let result = match rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => Err("Update worker stopped".into()),
            };
            self.pending = None;
            self.state = match result {
                Ok(Outcome::Checked(Some(version))) => State::Available(version),
                Ok(Outcome::Checked(None)) => State::Current,
                Ok(Outcome::Installed) => State::Restart,
                Err(error) => {
                    crate::store::log(&format!("Update: {error}"));
                    State::Failed(short_error(&error))
                }
            };
            return true;
        }
        if Instant::now() >= self.next_check
            && !matches!(
                self.state,
                State::Available(_) | State::Restart | State::RestartFailed
            )
        {
            self.check();
            return true;
        }
        false
    }

    pub fn activate(&mut self) {
        if !self.enabled() {
            return;
        }
        match &self.state {
            State::Available(version) => {
                let version = version.clone();
                self.state = State::Installing;
                self.start(move || install(&version).map(|()| Outcome::Installed));
            }
            State::RestartFailed => self.state = State::Restart,
            _ => self.check(),
        }
    }

    fn check(&mut self) {
        self.state = State::Checking;
        self.next_check = Instant::now() + CHECK_INTERVAL;
        self.start(|| {
            let releases = builder()
                .build()
                .and_then(|u| u.get_latest_release())
                .map_err(|e| e.to_string())?;
            available(&releases).map(Outcome::Checked)
        });
    }

    fn start(&mut self, work: impl FnOnce() -> Result<Outcome, String> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
    }

    pub fn ready_to_restart(&self) -> bool {
        matches!(self.state, State::Restart)
    }

    /// Only call after all provider workers have finished their current saves.
    pub fn restart(&mut self) {
        // The child waits for our single-instance lock before initializing providers.
        // On Unix exec closes the CLOEXEC lock fd; on Windows the parent exits.
        let Err(error) = self_update::restart::restart_with([RESTART_ARGUMENT]);
        crate::store::log(&format!("Update installed, restart failed: {error}"));
        self.state = State::RestartFailed;
    }
}

fn short_error(error: &str) -> String {
    error
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect()
}

fn archive_name(version: &str) -> String {
    if cfg!(windows) {
        format!("tokenkeeper-win-{version}.zip")
    } else {
        format!("tokenkeeper-linux-{version}.tar.gz")
    }
}

fn available(releases: &Releases) -> Result<Option<String>, String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("No updates are published for this architecture".into());
    }
    if !releases.is_update_available().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let release = releases.latest().ok_or("No release found")?;
    let version = release.version();
    let expected = archive_name(version);
    if !release
        .assets()
        .iter()
        .any(|asset| asset.name() == expected)
    {
        return Err("Release has no download for this platform".into());
    }
    Ok(Some(version.to_owned()))
}

fn builder() -> self_update::backends::github::UpdateBuilder {
    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner("ricnewton")
        .repo_name("tokenkeeper")
        .current_version(VERSION)
        .bin_name(BINARY)
        .bin_path_in_archive(BINARY)
        .show_output(false)
        .show_download_progress(false)
        .no_confirm(true)
        .timeout(Duration::from_secs(120))
        .check_install_path_writable(true)
        .verifying_keys([PUBLIC_KEY])
        .verify_release_digest(true);
    builder
}

fn install(version: &str) -> Result<(), String> {
    if !self_update::version::bump_is_greater(VERSION, version).map_err(|e| e.to_string())? {
        return Err("Refusing to install an older or equal version".into());
    }
    let expected = archive_name(version);
    builder()
        .release_tag(format!("v{version}"))
        .asset_matcher(move |assets| assets.iter().find(|a| a.name() == expected).cloned())
        .build()
        .and_then(|u| u.update())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use self_update::update::{Release, ReleaseAsset, UpdateConfig};

    fn releases(version: &str, asset: &str) -> Releases {
        Releases::from_releases(
            vec![
                Release::builder()
                    .version(version)
                    .name("test")
                    .asset(ReleaseAsset::new(asset, "https://example.test/archive"))
                    .build()
                    .unwrap(),
            ],
            VERSION,
        )
    }

    #[test]
    fn only_newer_releases_with_the_exact_platform_asset_are_offered() {
        assert!(
            available(&releases(VERSION, &archive_name(VERSION)))
                .unwrap()
                .is_none()
        );
        assert!(
            available(&releases("0.1.0", &archive_name("0.1.0")))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            available(&releases("99.0.0", &archive_name("99.0.0"))).unwrap(),
            Some("99.0.0".into())
        );
        assert!(available(&releases("99.0.0", "wrong-platform.zip")).is_err());
        assert!(install(VERSION).is_err());
    }

    #[test]
    fn updater_requires_a_signature_and_has_no_terminal_prompt() {
        let update = builder().build().unwrap();
        assert!(!update.show_output());
        assert!(update.no_confirm());
        assert_eq!(update.bin_path_in_archive(), BINARY);
        assert_ne!(PUBLIC_KEY, [0; 32]);
    }

    #[test]
    fn an_available_update_stays_offered_when_the_daily_check_is_due() {
        let mut updater = Updater::new();
        updater.state = State::Available("99.0.0".into());
        updater.next_check = Instant::now() - CHECK_INTERVAL;

        assert!(!updater.poll());
        assert!(updater.pending.is_none());
        assert_eq!(updater.label(), "Update now");
        assert!(updater.enabled());
        assert!(updater.status().contains("99.0.0"));
    }

    #[test]
    fn busy_state_prevents_a_second_operation_and_reports_worker_failure() {
        let mut updater = Updater::new();
        let (tx, rx) = mpsc::channel();
        updater.pending = Some(rx);
        updater.state = State::Installing;
        updater.activate();
        assert!(updater.installing());
        assert!(!updater.enabled());
        assert!(!updater.poll());
        drop(tx);
        assert!(updater.poll());
        assert!(updater.enabled());
        assert!(updater.status().contains("worker stopped"));
    }
}

#[cfg(test)]
#[path = "updater_tests.rs"]
mod integration_tests;
