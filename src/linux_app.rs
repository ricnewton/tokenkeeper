//! Borderless Iced/tiny-skia usage panel with native KDE StatusNotifier integration.
use crate::linux_panel::{self, Hit, Panel};
use crate::linux_tray::{Action as TrayAction, MenuAction as Action, MenuEntry};
use crate::model::{ProviderId, Snapshot, View};
use crate::provider::Provider;
use crate::worker::{Cmd, Event, Worker};
use crate::{autostart, claude, codex, copilot, cursor, opencode};
use iced::{Element, Length, Size, Subscription, Task, Theme, window};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

struct Slot {
    id: ProviderId,
    worker: Option<Worker>,
    view: View,
    last_good: Option<Rc<Snapshot>>,
}
struct App {
    window: Option<window::Id>,
    slots: Vec<Slot>,
    events: Receiver<(ProviderId, Event)>,
    active: usize,
    tab: usize,
    demo: bool,
    light: bool,
    render_epoch: u64,
    had_focus: bool,
    visible: bool,
    window_ready: bool,
    keep_window: bool,
    last_focus_hide: Option<Instant>,
    anchor: Option<(i32, i32)>,
    pending_clipboard: Option<String>,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    started: Instant,
    auto_close: Option<Duration>,
}
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Panel(linux_panel::Action),
    Opened(window::Id),
    Window(window::Id, window::Event),
    Escape,
    Screenshot(iced::window::Screenshot),
    Configured(Result<(), String>),
}
pub fn main() {
    // Choose the X11/XWayland backend before Iced or provider threads start.
    // It provides tray-relative placement and timestamp-based KWin activation.
    // Wayland-only sessions still use their compositor's toplevel behavior.
    let backend = std::env::var("TOKENKEEPER_BACKEND").unwrap_or_default();
    if backend == "x11" || (backend != "wayland" && std::env::var_os("DISPLAY").is_some()) {
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
    } else if backend == "wayland" {
        unsafe {
            std::env::remove_var("DISPLAY");
        }
    }

    if let Err(error) = iced::daemon(App::boot, App::update, App::view)
        .title("TokenKeeper")
        .theme(|app: &App, _| if app.light { Theme::Light } else { Theme::Dark })
        .subscription(App::subscription)
        .run()
    {
        eprintln!("Could not start TokenKeeper UI: {error}");
        std::process::exit(1);
    }
}
impl App {
    fn boot() -> (Self, Task<Message>) {
        let args: Vec<String> = std::env::args().collect();
        let screenshot = args
            .iter()
            .position(|arg| arg == "--demo-sheet")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--demo-sheet needs a PNG path")));
        let demo = screenshot.is_some() || args.iter().any(|arg| arg == "--demo");
        if !demo {
            autostart::follow_exe();
        }
        let (tx, events) = mpsc::channel();
        let slots = ProviderId::ALL
            .iter()
            .map(|&id| {
                let worker = if demo {
                    None
                } else {
                    let p: Box<dyn Provider> = match id {
                        ProviderId::Cursor => Box::new(cursor::Cursor::default()),
                        ProviderId::Copilot => Box::new(copilot::Copilot::default()),
                        ProviderId::Claude => Box::new(claude::Claude::default()),
                        ProviderId::Codex => Box::new(codex::Codex::default()),
                        ProviderId::OpenCode => Box::new(opencode::OpenCode::default()),
                    };
                    Some(Worker::spawn(p, tx.clone()))
                };
                Slot {
                    id,
                    worker,
                    view: if demo {
                        View::Ready {
                            snap: Rc::new(match id {
                                ProviderId::Cursor => crate::mock::simulated(0.42),
                                ProviderId::Copilot => crate::mock::simulated_copilot(0.67),
                                ProviderId::Claude => crate::mock::simulated_claude(0.27),
                                ProviderId::Codex => crate::mock::simulated_codex(0.52),
                                ProviderId::OpenCode => crate::mock::simulated_opencode(0.38),
                            }),
                            error: None,
                        }
                    } else {
                        View::Loading
                    },
                    last_good: None,
                }
            })
            .collect::<Vec<_>>();

        let active = if demo {
            args.iter()
                .position(|arg| arg == "--demo-provider")
                .and_then(|i| args.get(i + 1))
                .map_or(0, |name| match name.as_str() {
                    "copilot" => 1,
                    "claude" => 2,
                    "codex" => 3,
                    "opencode" => 4,
                    _ => 0,
                })
        } else {
            0
        };
        let tab = if demo {
            args.iter()
                .position(|arg| arg == "--demo-tab")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(1)
        } else {
            0
        };
        let auto_close = args
            .iter()
            .position(|arg| arg == "--auto-close")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .map(Duration::from_secs);
        let mut app = Self {
            window: None,
            slots,
            events,
            active,
            tab,
            demo,
            light: light_theme(),
            render_epoch: 0,
            had_focus: false,
            visible: false,
            window_ready: false,
            keep_window: std::env::var_os("DISPLAY").is_some(),
            last_focus_hide: None,
            anchor: None,
            pending_clipboard: None,
            screenshot,
            screenshot_requested: false,
            started: Instant::now(),
            auto_close,
        };
        if demo && args.iter().any(|arg| arg == "--demo-single") {
            for (i, slot) in app.slots.iter_mut().enumerate() {
                if i != active {
                    slot.view = View::SignedOut(None);
                }
            }
        }
        if !demo {
            if let Err(error) = crate::linux_tray::start() {
                eprintln!("{error}");
            }
        }
        app.refresh();
        let task = if demo {
            app.show_panel_at(None)
        } else {
            Task::none()
        };
        (app, task)
    }
    fn entries(&self) -> Vec<(ProviderId, View)> {
        self.slots.iter().map(|s| (s.id, s.view.clone())).collect()
    }
    fn panel_height(&self) -> f32 {
        linux_panel::content_height(&self.entries(), self.active, self.tab)
    }
    fn show_panel_at(&mut self, anchor: Option<(i32, i32)>) -> Task<Message> {
        if let Some(id) = self.window {
            if self.visible {
                return window::gain_focus(id);
            }
            self.visible = true;
            self.had_focus = false;
            self.anchor = anchor;
            return window::set_mode(id, window::Mode::Windowed).chain(self.configure_window(id));
        }
        self.visible = true;
        self.window_ready = false;
        self.anchor = anchor;
        self.had_focus = false;
        let (id, open) = window::open(window::Settings {
            size: Size::new(440.0, self.panel_height().min(610.0)),
            decorations: false,
            resizable: false,
            minimizable: false,
            level: window::Level::AlwaysOnTop,
            exit_on_close_request: false,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "tokenkeeper".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        self.window = Some(id);
        open.map(Message::Opened)
    }
    fn hide_panel(&mut self, focus: bool) -> Task<Message> {
        if focus {
            self.last_focus_hide = Some(Instant::now());
        }
        self.had_focus = false;
        self.visible = false;
        if self.keep_window {
            self.window
                .map_or_else(Task::none, |id| window::set_mode(id, window::Mode::Hidden))
        } else {
            self.window_ready = false;
            self.window.take().map_or_else(Task::none, window::close)
        }
    }
    fn configure_window(&self, id: window::Id) -> Task<Message> {
        let anchor = self.anchor;
        window::run(id, move |window| {
            crate::linux_window::configure(window, anchor)
        })
        .map(Message::Configured)
    }
    fn tray_action(&mut self, action: TrayAction) -> Task<Message> {
        match action {
            TrayAction::Activate(x, y) => {
                if self.visible {
                    self.hide_panel(false)
                } else if self
                    .last_focus_hide
                    .is_some_and(|time| time.elapsed() < Duration::from_millis(400))
                {
                    Task::none()
                } else {
                    self.show_panel_at(Some((x, y)))
                }
            }
            TrayAction::Scroll(delta) => {
                self.cycle(delta);
                Task::none()
            }
            TrayAction::Menu(action) => self.action(action),
        }
    }
    fn cycle(&mut self, delta: i32) {
        let ready: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.view.snapshot().map(|_| i))
            .collect();
        if let Some(next) = scroll_target(self.active, delta, &ready) {
            self.active = next;
            self.tab = 0;
            self.refresh();
        }
    }
    fn action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Panel => return self.show_panel_at(None),
            Action::Refresh => {
                for slot in &self.slots {
                    if let Some(worker) = &slot.worker {
                        worker.send(Cmd::RefreshNow);
                    }
                }
            }
            Action::Quit => return iced::exit(),
            Action::Autostart => {
                if !self.demo {
                    autostart::set_enabled(!autostart::is_enabled());
                    self.refresh();
                }
            }
            Action::Show(i) => {
                if i < self.slots.len() {
                    self.active = i;
                    self.tab = 0;
                    self.refresh();
                }
            }
            Action::Sign(i) => {
                if let Some(slot) = self.slots.get(i) {
                    if let Some(worker) = &slot.worker {
                        worker.send(if matches!(slot.view, View::SignedOut(_)) {
                            Cmd::SignIn
                        } else {
                            Cmd::SignOut
                        });
                    }
                }
            }
        }
        Task::none()
    }
    fn tick(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for action in crate::linux_tray::drain() {
            tasks.push(self.tray_action(action));
        }
        let mut dirty = false;
        let mut show = false;
        while let Ok((id, event)) = self.events.try_recv() {
            if let Some(i) = self.slots.iter().position(|slot| slot.id == id) {
                let slot = &mut self.slots[i];
                slot.view = match event {
                    Event::SignedOut(error) => {
                        slot.last_good = None;
                        View::SignedOut(error)
                    }
                    Event::SigningIn(code) => {
                        self.active = i;
                        self.tab = 0;
                        if let Some(code) = &code {
                            self.pending_clipboard = Some(code.clone());
                            show = true;
                        }
                        View::SigningIn { code }
                    }
                    Event::Usage(snapshot) => {
                        if matches!(slot.view, View::SigningIn { .. }) {
                            self.active = i;
                            self.tab = 0;
                            show = true;
                        }
                        let snap = Rc::new(snapshot);
                        slot.last_good = Some(snap.clone());
                        View::Ready { snap, error: None }
                    }
                    Event::Error(error) => match &slot.last_good {
                        Some(snap) => View::Ready {
                            snap: snap.clone(),
                            error: Some(error),
                        },
                        None => View::Failed(error),
                    },
                };
                dirty = true;
            }
        }
        if dirty {
            if matches!(self.slots[self.active].view, View::SignedOut(_)) {
                if let Some(i) = self
                    .slots
                    .iter()
                    .position(|s| matches!(s.view, View::Ready { .. }))
                {
                    self.active = i;
                    self.tab = 0;
                }
            }
            self.refresh();
        }
        if show {
            let already_open = self.window_ready;
            tasks.push(self.show_panel_at(None));
            if already_open {
                if let Some(code) = self.pending_clipboard.take() {
                    tasks.push(iced::clipboard::write(code));
                }
            }
        }
        if self
            .auto_close
            .is_some_and(|duration| self.started.elapsed() >= duration)
        {
            tasks.push(iced::exit());
        }
        if self.screenshot.is_some()
            && !self.screenshot_requested
            && self.started.elapsed() > Duration::from_millis(600)
        {
            if let Some(id) = self.window {
                self.screenshot_requested = true;
                tasks.push(window::screenshot(id).map(Message::Screenshot));
            }
        }
        Task::batch(tasks)
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        let before = self.panel_height();
        let task = match message {
            Message::Tick => self.tick(),
            Message::Panel(linux_panel::Action::Click(x, y)) => {
                let entries = self.entries();
                let visible = linux_panel::visible_providers(&entries, self.active);
                let details = self.slots[self.active]
                    .view
                    .snapshot()
                    .map_or(0, |s| s.details.len());
                match linux_panel::hit_test(x, y, &visible, details) {
                    Hit::Provider(i) => {
                        self.active = i;
                        self.tab = 0;
                        self.refresh();
                    }
                    Hit::Tab(i) => {
                        self.tab = i;
                        self.refresh();
                    }
                    Hit::Usage => crate::open_url(self.slots[self.active].id.usage_url()),
                    Hit::None => {}
                }
                Task::none()
            }
            Message::Panel(linux_panel::Action::Scroll(delta)) => {
                self.cycle(delta);
                Task::none()
            }
            Message::Opened(id) => {
                if self.window != Some(id) {
                    return window::close(id);
                }
                self.window_ready = true;
                let mut tasks = vec![if self.visible {
                    self.configure_window(id)
                } else {
                    window::set_mode(id, window::Mode::Hidden)
                }];
                if let Some(code) = self.pending_clipboard.take() {
                    tasks.push(iced::clipboard::write(code));
                }
                Task::batch(tasks)
            }
            Message::Window(id, event) => {
                if self.window != Some(id) {
                    return Task::none();
                }
                match event {
                    window::Event::Focused if self.visible => {
                        self.had_focus = true;
                        Task::none()
                    }
                    window::Event::Unfocused if self.visible && self.had_focus && !self.demo => {
                        self.hide_panel(true)
                    }
                    window::Event::CloseRequested => self.hide_panel(false),
                    window::Event::Resized(_) if self.visible => self.configure_window(id),
                    window::Event::Closed => {
                        self.visible = false;
                        self.window_ready = false;
                        self.window = None;
                        Task::none()
                    }
                    _ => Task::none(),
                }
            }
            Message::Escape => self.hide_panel(false),
            Message::Configured(result) => {
                if let Err(error) = result {
                    eprintln!("Popup integration: {error}");
                }
                Task::none()
            }
            Message::Screenshot(image) => {
                if let Some(path) = &self.screenshot {
                    if let Err(error) =
                        save_png(path, image.size.width, image.size.height, &image.rgba)
                    {
                        eprintln!("Could not save panel preview: {error}");
                        std::process::exit(1);
                    }
                }
                iced::exit()
            }
        };
        if self.window.is_some() && before != self.panel_height() {
            let id = self.window.unwrap();
            Task::batch([
                task,
                window::resize(id, Size::new(440.0, self.panel_height().min(610.0))),
            ])
        } else {
            task
        }
    }
    fn view(&self, _id: window::Id) -> Element<'_, Message> {
        let height = self.panel_height();
        let panel = iced::widget::canvas(Panel {
            slots: self.entries(),
            active: self.active,
            tab: self.tab,
            light: self.light,
            generation: self.render_epoch,
        })
        .width(440)
        .height(height);
        let content: Element<'_, linux_panel::Action> = if height > 610.0 {
            iced::widget::scrollable(panel)
                .direction(iced::widget::scrollable::Direction::Vertical(
                    iced::widget::scrollable::Scrollbar::hidden(),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            panel.into()
        };
        content.map(Message::Panel)
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick),
            iced::event::listen_with(|event, _, id| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::Escape),
                iced::Event::Window(event)
                    if !matches!(event, window::Event::RedrawRequested(_)) =>
                {
                    Some(Message::Window(id, event))
                }
                _ => None,
            }),
        ])
    }
    fn refresh(&mut self) {
        self.render_epoch = self.render_epoch.wrapping_add(1);
        let accounts = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| MenuEntry::Item {
                label: slot.view.account_menu_label(slot.id.name()),
                enabled: !matches!(slot.view, View::Loading),
                action: Action::Sign(i),
            })
            .collect();
        let icons = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                MenuEntry::item(
                    format!(
                        "{}{}",
                        slot.id.name(),
                        if i == self.active { " (active)" } else { "" }
                    ),
                    Action::Show(i),
                )
            })
            .collect();
        let menu = vec![
            MenuEntry::item("Show usage panel", Action::Panel),
            MenuEntry::Separator,
            MenuEntry::Submenu {
                label: "Accounts".into(),
                children: accounts,
            },
            MenuEntry::Submenu {
                label: "Switch tray icon".into(),
                children: icons,
            },
            MenuEntry::Separator,
            MenuEntry::item("Refresh now", Action::Refresh),
            MenuEntry::item(
                if autostart::is_enabled() {
                    "Autostart - On"
                } else {
                    "Autostart - Off"
                },
                Action::Autostart,
            ),
            MenuEntry::item("Quit", Action::Quit),
        ];
        let usage = self.slots[self.active]
            .view
            .snapshot()
            .map(|s| (s.used_fraction(), s.elapsed_fraction()));
        let active = &self.slots[self.active];
        let title = format!("TokenKeeper · {}", active.id.name());
        let summary = self
            .slots
            .iter()
            .filter_map(|slot| {
                slot.view.snapshot().map(|snap| {
                    format!(
                        "{}: {:.0}% used · {} · {}",
                        slot.id.name(),
                        snap.used_fraction() * 100.0,
                        snap.plan,
                        snap.resets_text()
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("\n");
        crate::linux_tray::update(
            usage,
            &title,
            if summary.is_empty() {
                "Sign in from Accounts in the tray menu"
            } else {
                &summary
            },
            menu,
        );
    }
}
fn scroll_target(active: usize, delta: i32, ready: &[usize]) -> Option<usize> {
    if delta == 0 || ready.is_empty() {
        return None;
    }
    if let Some(position) = ready.iter().position(|&i| i == active) {
        let next =
            (position as isize - delta.signum() as isize).rem_euclid(ready.len() as isize) as usize;
        Some(ready[next])
    } else if delta > 0 {
        ready
            .iter()
            .rev()
            .copied()
            .find(|&i| i < active)
            .or_else(|| ready.last().copied())
    } else {
        ready
            .iter()
            .copied()
            .find(|&i| i > active)
            .or_else(|| ready.first().copied())
    }
}

fn light_theme() -> bool {
    std::env::var("TOKENKEEPER_THEME")
        .or_else(|_| std::env::var("AI_USAGE_THEME"))
        .is_ok_and(|v| v.eq_ignore_ascii_case("light"))
}
fn save_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, mpsc::Sender<(ProviderId, Event)>) {
        let (tx, events) = mpsc::channel();
        let snap = Rc::new(crate::mock::simulated_codex(0.33));
        let app = App {
            window: None,
            slots: ProviderId::ALL
                .iter()
                .map(|&id| Slot {
                    id,
                    worker: None,
                    view: if id == ProviderId::Codex {
                        View::Ready {
                            snap: snap.clone(),
                            error: None,
                        }
                    } else {
                        View::SignedOut(None)
                    },
                    last_good: (id == ProviderId::Codex).then(|| snap.clone()),
                })
                .collect(),
            events,
            active: 3,
            tab: 0,
            demo: false,
            light: false,
            render_epoch: 0,
            had_focus: false,
            visible: false,
            window_ready: false,
            keep_window: true,
            last_focus_hide: None,
            anchor: None,
            pending_clipboard: None,
            screenshot: None,
            screenshot_requested: false,
            started: Instant::now(),
            auto_close: None,
        };
        (app, tx)
    }

    #[test]
    fn dismissal_keeps_daemon_alive_and_stale_window_events_do_not_close_reopened_panel() {
        let (mut app, _) = fixture();
        let _ = app.show_panel_at(Some((1000, 1000)));
        let first = app.window.unwrap();
        let _ = app.update(Message::Window(first, window::Event::Unfocused));
        assert_eq!(app.window, Some(first), "ignore initial unfocused creation");
        let _ = app.update(Message::Window(first, window::Event::Focused));
        let _ = app.update(Message::Window(first, window::Event::Unfocused));
        assert!(!app.visible);
        assert_eq!(
            app.window,
            Some(first),
            "retain the X11 clipboard connection"
        );
        assert!(app.last_focus_hide.is_some());
        let _ = app.tray_action(TrayAction::Activate(1000, 1000));
        assert!(!app.visible, "dismissing tray click must not reopen");
        app.last_focus_hide = Some(Instant::now() - Duration::from_secs(1));
        let _ = app.tray_action(TrayAction::Activate(1000, 1000));
        let second = app.window.unwrap();
        assert_eq!(first, second, "reuse the X11 window across popup cycles");
        let _ = app.update(Message::Window(window::Id::unique(), window::Event::Closed));
        assert_eq!(app.window, Some(second));
        let _ = app.update(Message::Escape);
        assert!(!app.visible);
        assert!(matches!(app.slots[3].view, View::Ready { .. }));
        app.keep_window = false;
        let _ = app.show_panel_at(None);
        let _ = app.hide_panel(false);
        assert!(
            app.window.is_none(),
            "native Wayland closes to hide its surface"
        );
    }

    #[test]
    fn device_code_waits_for_its_open_window_and_refresh_failure_keeps_last_usage() {
        let (mut app, tx) = fixture();
        tx.send((
            ProviderId::Copilot,
            Event::SigningIn(Some("TEST-CODE".into())),
        ))
        .unwrap();
        let _ = app.tick();
        assert_eq!(app.active, 1);
        assert_eq!(app.pending_clipboard.as_deref(), Some("TEST-CODE"));
        let id = app.window.unwrap();
        tx.send((
            ProviderId::Copilot,
            Event::SigningIn(Some("NEW-CODE".into())),
        ))
        .unwrap();
        let _ = app.tick();
        assert_eq!(
            app.pending_clipboard.as_deref(),
            Some("NEW-CODE"),
            "opening window isn't clipboard-ready"
        );
        let _ = app.update(Message::Opened(window::Id::unique()));
        assert!(
            app.pending_clipboard.is_some(),
            "stale opens must not consume the clipboard code"
        );
        let _ = app.update(Message::Opened(id));
        assert!(app.pending_clipboard.is_none());
        tx.send((ProviderId::Codex, Event::Error("temporary failure".into())))
            .unwrap();
        let _ = app.tick();
        let View::Ready { snap, error } = &app.slots[3].view else {
            panic!("lost cached usage");
        };
        assert_eq!(snap.used_fraction(), 0.33);
        assert_eq!(error.as_deref(), Some("temporary failure"));
    }

    #[test]
    fn signed_out_providers_are_skipped_when_scrolling() {
        assert_eq!(scroll_target(3, 120, &[0, 3]), Some(0));
        assert_eq!(scroll_target(0, -120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(0, 120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(2, -120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(3, 120, &[3]), Some(3));
        assert_eq!(scroll_target(3, 120, &[]), None);
        assert_eq!(scroll_target(3, 0, &[3]), None);
    }
}
