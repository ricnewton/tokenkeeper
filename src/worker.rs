//! One background thread per provider that owns its session: sign-in, token
//! refresh and polling. Results are posted back to the UI thread, which never
//! blocks on the network.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

use crate::model::{ProviderId, Snapshot};
use crate::provider::{FetchError, PollResult, Provider};
use crate::store;

#[cfg(windows)]
pub const WM_WORKER_EVENT: u32 = WM_APP + 1;

/// Manual refreshes closer together than this are ignored.
const MIN_GAP: Duration = Duration::from_secs(15);
const BACKOFF_START: Duration = Duration::from_secs(30);

pub enum Cmd {
    RefreshNow,
    SignIn,
    SignOut,
}

pub enum Event {
    /// With the reason, if it wasn't the user's choice (a failed sign-in or a
    /// session the server rejected).
    SignedOut(Option<String>),
    SigningIn(Option<String>),
    Usage(Snapshot),
    Error(String),
}

pub struct Worker {
    tx: Sender<Cmd>,
}

impl Worker {
    pub fn spawn(
        #[cfg(windows)] ui_thread: u32,
        mut provider: Box<dyn Provider>,
        events: Sender<(ProviderId, Event)>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            #[cfg(windows)]
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            }
            let id = provider.id();
            let send = |e: Event| {
                match &e {
                    Event::SignedOut(Some(why)) => {
                        store::log(&format!("{}: signed out: {why}", id.name()))
                    }
                    Event::SigningIn(_) => store::log(&format!("{}: sign-in started", id.name())),
                    Event::Error(e) => store::log(&format!("{}: {e}", id.name())),
                    _ => {}
                }
                let _ = events.send((id, e));
                #[cfg(windows)]
                unsafe {
                    let _ = PostThreadMessageW(ui_thread, WM_WORKER_EVENT, WPARAM(0), LPARAM(0));
                }
            };
            run(provider.as_mut(), rx, send);
        });
        Worker { tx }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

fn run(p: &mut dyn Provider, rx: Receiver<Cmd>, send: impl Fn(Event)) {
    store::log(&format!("{}: loading saved session", p.id().name()));
    let mut signed_in = p.load();
    store::log(&format!(
        "{}: saved session {}",
        p.id().name(),
        if signed_in { "loaded" } else { "absent" }
    ));
    if !signed_in {
        send(Event::SignedOut(None));
    }
    let mut next_due = Instant::now();
    let mut last_fetch: Option<Instant> = None;
    let mut backoff = BACKOFF_START;

    loop {
        let wait = if signed_in {
            next_due.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Cmd::RefreshNow) => {
                if !signed_in {
                    // Pick up an account that was signed in outside this app.
                    // Provider::load keeps explicit local opt-outs (Codex) off.
                    signed_in = p.load();
                    if !signed_in {
                        continue;
                    }
                    backoff = BACKOFF_START;
                } else if last_fetch.is_some_and(|t| t.elapsed() < MIN_GAP) {
                    continue;
                }
            }
            Ok(Cmd::SignOut) => {
                p.sign_out();
                signed_in = false;
                send(Event::SignedOut(None));
                continue;
            }
            Ok(Cmd::SignIn) => match sign_in(p, &rx, &send) {
                Ok(()) => {
                    signed_in = true;
                    backoff = BACKOFF_START;
                    store::log(&format!("{}: signed in", p.id().name()));
                }
                // A failed attempt doesn't end an existing session.
                Err(why) if signed_in => {
                    if let Some(e) = why {
                        send(Event::Error(e));
                    }
                }
                Err(why) => {
                    send(Event::SignedOut(why));
                    continue;
                }
            },
        }
        if !signed_in {
            continue;
        }

        last_fetch = Some(Instant::now());
        store::log(&format!("{}: fetching usage", p.id().name()));
        match p.fetch(&mut |s| send(Event::Usage(s))) {
            Ok(()) => {
                store::log(&format!("{}: usage updated", p.id().name()));
                backoff = BACKOFF_START;
                next_due = Instant::now() + p.poll_every();
            }
            Err(FetchError::SignedOut) => {
                p.sign_out();
                signed_in = false;
                send(Event::SignedOut(Some(
                    "The session was rejected; sign in again".into(),
                )));
            }
            Err(FetchError::Other(e)) => {
                next_due = Instant::now() + backoff;
                backoff = (backoff * 2).min(p.poll_every());
                send(Event::Error(e));
            }
        }
    }
}

/// Opens the browser and waits for the user to finish. `Err(None)` means
/// cancelled with SignOut; `Err(Some(reason))` means it failed or timed out.
fn sign_in(
    p: &mut dyn Provider,
    rx: &Receiver<Cmd>,
    send: &impl Fn(Event),
) -> Result<(), Option<String>> {
    let mut attempt = p.begin_login().map_err(Some)?;
    send(Event::SigningIn(attempt.user_code().map(String::from)));
    crate::open_url(attempt.url());
    loop {
        match rx.recv_timeout(attempt.poll_interval()) {
            Ok(Cmd::SignOut) | Err(RecvTimeoutError::Disconnected) => return Err(None),
            Ok(Cmd::SignIn) => crate::open_url(attempt.url()),
            Ok(Cmd::RefreshNow) | Err(RecvTimeoutError::Timeout) => {}
        }
        match attempt.poll() {
            PollResult::Pending => {}
            PollResult::Done(creds) => {
                return p
                    .accept_login(creds)
                    .map_err(|e| Some(format!("Couldn't save sign-in: {e}")));
            }
            PollResult::Failed(e) => return Err(Some(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Unit;
    use crate::provider::Login;
    use serde_json::Value;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct Fake {
        available: Arc<AtomicBool>,
        enabled: Arc<AtomicBool>,
    }
    impl Provider for Fake {
        fn id(&self) -> ProviderId {
            ProviderId::Cursor
        }
        fn load(&mut self) -> bool {
            self.available.load(Ordering::SeqCst) && self.enabled.load(Ordering::SeqCst)
        }
        fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
            Err("unused".into())
        }
        fn accept_login(&mut self, _creds: Value) -> Result<(), String> {
            Ok(())
        }
        fn sign_out(&mut self) {
            self.enabled.store(false, Ordering::SeqCst);
        }
        fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError> {
            send(Snapshot::new(ProviderId::Cursor, Unit::Cents));
            Ok(())
        }
    }

    #[test]
    fn refresh_discovers_external_sign_in_but_respects_opt_out() {
        let available = Arc::new(AtomicBool::new(false));
        let enabled = Arc::new(AtomicBool::new(true));
        let fake = Fake {
            available: available.clone(),
            enabled: enabled.clone(),
        };
        let (commands, rx) = mpsc::channel();
        let (events, output) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut fake = fake;
            run(&mut fake, rx, |e| {
                let _ = events.send(e);
            })
        });
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::SignedOut(None)
        ));
        available.store(true, Ordering::SeqCst);
        enabled.store(false, Ordering::SeqCst);
        commands.send(Cmd::RefreshNow).unwrap();
        assert!(output.recv_timeout(Duration::from_millis(100)).is_err());
        enabled.store(true, Ordering::SeqCst);
        commands.send(Cmd::RefreshNow).unwrap();
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Usage(_)
        ));
        drop(commands);
        thread.join().unwrap();
    }
}
