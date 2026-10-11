use crate::{autostart, claude, clipboard, codex, copilot, cursor, icon, opencode, sheet, time};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::model::{ProviderId, Snapshot, View};
use crate::popup::{Gfx, Kind, Popup};
use crate::provider::Provider;
use crate::worker::{Cmd, Event, WM_WORKER_EVENT, Worker};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetSystemMetricsForDpi,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, HC_ACTION, HHOOK, KillTimer, MSG,
    MSLLHOOKSTRUCT, PostThreadMessageW, SM_CXSMICON, SW_SHOWNORMAL, SetTimer, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_MOUSEWHEEL, WM_TIMER,
};
use windows::core::{HSTRING, w};

/// Posted to the UI thread with the wheel delta in `wParam` (as i16).
pub const WM_WHEEL_SWITCH: u32 = WM_APP + 2;
const WHEEL_STEP: i32 = 120;
const HOUSEKEEPING_MS: u32 = 2000;
const HOVER_POLL_MS: u32 = 50;
const HOVER_SHOW_DELAY: Duration = Duration::from_millis(350);
const HOVER_HIDE_DELAY: Duration = Duration::from_millis(300);
/// A click that arrives this soon after the panel hid itself (because that
/// same click deactivated it) must not reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(400);
/// Opening a popup refreshes data older than this.
const STALE_AFTER_MS: i64 = 60_000;

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--icon-sheet") => {
            let path = args.get(2).map(String::as_str).unwrap_or("icon-sheet.png");
            sheet::write(path).expect("failed to write icon sheet");
            return;
        }
        Some("--popup-sheet") => {
            let path = args.get(2).map(String::as_str).unwrap_or("popup-sheet.png");
            let used = args
                .get(3)
                .and_then(|u| u.parse::<f32>().ok())
                .unwrap_or(42.0)
                / 100.0;
            sheet::write_popups(path, used).expect("failed to write popup sheet");
            return;
        }
        _ => {}
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    App::new().run();
}

pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

thread_local! {
    /// The tray icon's rect, for the low-level wheel hook (which runs on the UI thread).
    static WHEEL_AREA: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
}

fn contains(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

/// Tray icons never receive wheel messages, so while the cursor is over the
/// icon a low-level hook catches the wheel and swallows it.
unsafe extern "system" fn wheel_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEWHEEL {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if WHEEL_AREA.with(|a| contains(&a.get(), info.pt)) {
            let delta = (info.mouseData >> 16) as u16;
            unsafe {
                let _ = PostThreadMessageW(
                    GetCurrentThreadId(),
                    WM_WHEEL_SWITCH,
                    WPARAM(delta as usize),
                    LPARAM(0),
                );
            }
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

struct Slot {
    id: ProviderId,
    worker: Worker,
    view: View,
    last_good: Option<Rc<Snapshot>>,
    sign_item: MenuItem,
    show_item: CheckMenuItem,
}

impl Slot {
    fn sign_text(&self) -> String {
        self.view.account_menu_label(self.id.name())
    }
}

struct App {
    tray: TrayIcon,
    slots: Vec<Slot>,
    active: usize,
    events: Receiver<(ProviderId, Event)>,
    light_taskbar: bool,
    icon_size: u32,
    refresh_item: MenuItem,
    autostart_item: CheckMenuItem,
    quit_item: MenuItem,
    glance: Popup,
    panel: Popup,
    icon_rect: Option<RECT>,
    hover_since: Option<Instant>,
    last_inside: Instant,
    hover_timer: usize,
    hook: Option<HHOOK>,
    wheel_acc: i32,
    /// The glance is showing a sign-in code and ignores hover until it's done.
    pinned: bool,
}

impl App {
    fn new() -> Self {
        autostart::follow_exe();

        let refresh_item = MenuItem::new("Refresh now", true, None);
        let autostart_item = CheckMenuItem::new("Autostart", true, autostart::is_enabled(), None);
        let quit_item = MenuItem::new("Quit", true, None);
        let accounts_menu = Submenu::new("Accounts", true);
        let show_menu = Submenu::new("Switch tray icon", true);

        let ui_thread = unsafe { GetCurrentThreadId() };
        let (etx, events) = mpsc::channel();
        let slots: Vec<Slot> = ProviderId::ALL
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let p: Box<dyn Provider> = match id {
                    ProviderId::Cursor => Box::new(cursor::Cursor::default()),
                    ProviderId::Copilot => Box::new(copilot::Copilot::default()),
                    ProviderId::Claude => Box::new(claude::Claude::default()),
                    ProviderId::Codex => Box::new(codex::Codex::default()),
                    ProviderId::OpenCode => Box::new(opencode::OpenCode::default()),
                };
                Slot {
                    id,
                    worker: Worker::spawn(ui_thread, p, etx.clone()),
                    view: View::Loading,
                    last_good: None,
                    sign_item: MenuItem::new(format!("Sign in to {}…", id.name()), true, None),
                    show_item: CheckMenuItem::new(id.name(), true, i == 0, None),
                }
            })
            .collect();

        let menu = Menu::new();
        for s in &slots {
            show_menu.append(&s.show_item).unwrap();
        }
        for s in &slots {
            accounts_menu.append(&s.sign_item).unwrap();
        }
        menu.append(&accounts_menu).unwrap();
        menu.append(&show_menu).unwrap();
        menu.append(&refresh_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&autostart_item).unwrap();
        menu.append(&quit_item).unwrap();

        let light_taskbar = taskbar_is_light();
        let icon_size = tray_icon_size();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(make_icon(icon_size, &View::Loading, light_taskbar))
            .build()
            .expect("failed to create tray icon");

        let gfx = Gfx::new().expect("failed to initialise Direct2D");
        let glance = Popup::new(Kind::Glance, gfx.clone()).expect("failed to create glance popup");
        let panel = Popup::new(Kind::Panel, gfx).expect("failed to create panel popup");

        let app = App {
            tray,
            slots,
            active: 0,
            events,
            light_taskbar,
            icon_size,
            refresh_item,
            autostart_item,
            quit_item,
            glance,
            panel,
            icon_rect: None,
            hover_since: None,
            last_inside: Instant::now(),
            hover_timer: 0,
            hook: None,
            wheel_acc: 0,
            pinned: false,
        };
        app.refresh();
        app
    }

    fn slot(&self) -> &Slot {
        &self.slots[self.active]
    }

    fn run(mut self) {
        let house_timer = unsafe { SetTimer(None, 0, HOUSEKEEPING_MS, None) };
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            if msg.hwnd.is_invalid() {
                match msg.message {
                    WM_WORKER_EVENT => {
                        while let Ok((id, ev)) = self.events.try_recv() {
                            self.on_worker(id, ev);
                        }
                        continue;
                    }
                    WM_WHEEL_SWITCH => {
                        self.on_wheel(msg.wParam.0 as u16 as i16 as i32);
                        continue;
                    }
                    WM_TIMER => {
                        let id = msg.wParam.0;
                        if id == house_timer {
                            let (light, size) = (taskbar_is_light(), tray_icon_size());
                            if light != self.light_taskbar || size != self.icon_size {
                                self.light_taskbar = light;
                                self.icon_size = size;
                                self.refresh();
                            }
                        } else if id == self.hover_timer && id != 0 {
                            self.hover_tick();
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // Tray and menu events are raised synchronously inside DispatchMessage
            // on this thread, so draining here never misses any.
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                self.on_tray(ev);
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if !self.on_menu(ev) {
                    return;
                }
            }
        }
    }

    fn on_worker(&mut self, id: ProviderId, ev: Event) {
        let Some(index) = self.slots.iter().position(|s| s.id == id) else {
            return;
        };
        let shows_code = matches!(ev, Event::SigningIn(Some(_)));
        let slot = &mut self.slots[index];
        slot.view = match ev {
            Event::SignedOut(why) => {
                slot.last_good = None;
                View::SignedOut(why)
            }
            Event::SigningIn(code) => View::SigningIn { code },
            Event::Usage(s) => {
                let snap = Rc::new(s);
                slot.last_good = Some(snap.clone());
                View::Ready { snap, error: None }
            }
            Event::Error(e) => match (&slot.view, &slot.last_good) {
                // A failed sign-in is followed by SignedOut from the worker.
                (View::SigningIn { .. }, _) => slot.view.clone(),
                (_, Some(snap)) => View::Ready {
                    snap: snap.clone(),
                    error: Some(e),
                },
                (_, None) => View::Failed(e),
            },
        };
        if shows_code {
            self.pin_code_card(index);
        }
        self.unpin_if_done();
        self.refresh();
    }

    /// Device sign-in codes must be typed into the browser, so the code stays
    /// on screen (the glance card, pinned) until the sign-in ends.
    fn pin_code_card(&mut self, index: usize) {
        self.active = index;
        self.refresh();
        self.panel.hide();
        if let View::SigningIn { code: Some(code) } = &self.slots[index].view {
            clipboard::set_text(self.glance.hwnd(), code);
        }
        let anchor = self.icon_rect.or_else(|| self.tray.rect().map(to_rect));
        if let Some(r) = anchor {
            self.glance.show(r);
            self.pinned = true;
        }
    }

    /// Unpins the code card once the active provider is no longer waiting for one.
    fn unpin_if_done(&mut self) {
        if self.pinned && !matches!(self.slot().view, View::SigningIn { code: Some(_) }) {
            self.pinned = false;
            self.glance.hide();
        }
    }

    /// Mouse wheel over the icon or a popup: step through providers.
    fn on_wheel(&mut self, delta: i32) {
        self.wheel_acc += delta;
        let n = self.slots.len() as i32;
        let mut step = 0;
        while self.wheel_acc.abs() >= WHEEL_STEP {
            // Wheel down (negative) moves to the next provider.
            step -= self.wheel_acc.signum();
            self.wheel_acc -= self.wheel_acc.signum() * WHEEL_STEP;
        }
        if step != 0 {
            self.select((self.active as i32 + step).rem_euclid(n) as usize);
        }
    }

    fn select(&mut self, i: usize) {
        self.active = i;
        self.unpin_if_done();
        self.refresh();
        if self.glance.is_visible() || self.panel.is_visible() {
            self.refresh_if_stale();
        }
    }

    fn on_tray(&mut self, ev: TrayIconEvent) {
        match ev {
            TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. } => {
                let r = to_rect(rect);
                self.icon_rect = Some(r);
                WHEEL_AREA.with(|a| a.set(r));
                if self.hover_since.is_none() {
                    self.hover_since = Some(Instant::now());
                }
                if self.hover_timer == 0 {
                    self.hover_timer = unsafe { SetTimer(None, 0, HOVER_POLL_MS, None) };
                }
                if self.hook.is_none() {
                    self.hook = unsafe {
                        GetModuleHandleW(None).ok().and_then(|m| {
                            SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_hook), Some(m.into()), 0).ok()
                        })
                    };
                }
            }
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } => {
                let anchor = to_rect(rect);
                self.icon_rect = Some(anchor);
                if self.pinned {
                    return;
                }
                self.glance.hide();
                if matches!(self.slot().view, View::SignedOut(_)) {
                    self.slot().worker.send(Cmd::SignIn);
                } else if self.panel.is_visible() {
                    self.panel.hide();
                } else if !self.panel.hidden_within(REOPEN_GUARD) {
                    self.refresh_if_stale();
                    self.panel.show(anchor);
                }
            }
            TrayIconEvent::Click {
                button: MouseButton::Right,
                ..
            } => self.glance.hide(),
            _ => {}
        }
    }

    fn refresh_if_stale(&self) {
        if let View::Ready { snap, .. } = &self.slot().view {
            if time::now_ms() - snap.fetched_ms > STALE_AFTER_MS {
                self.slot().worker.send(Cmd::RefreshNow);
            }
        }
    }

    /// Shows the glance card after the cursor rests on the icon, and hides it
    /// once the cursor has left both the icon and the card for a moment.
    fn hover_tick(&mut self) {
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let on_icon = self.icon_rect.is_some_and(|r| contains(&r, cursor));
        let on_glance = self.glance.contains(cursor);

        if self.panel.is_visible() {
            self.glance.hide();
        } else if on_icon {
            self.last_inside = Instant::now();
            let rested = self
                .hover_since
                .is_some_and(|t| t.elapsed() >= HOVER_SHOW_DELAY);
            if rested && !self.glance.is_visible() {
                if let Some(r) = self.icon_rect {
                    self.refresh_if_stale();
                    self.glance.show(r);
                }
            }
            return;
        } else if on_glance {
            self.last_inside = Instant::now();
            return;
        }

        self.hover_since = None;
        if self.glance.is_visible() && self.last_inside.elapsed() < HOVER_HIDE_DELAY {
            return;
        }
        if !self.pinned {
            self.glance.hide();
        }
        unsafe {
            let _ = KillTimer(None, self.hover_timer);
            if let Some(h) = self.hook.take() {
                let _ = UnhookWindowsHookEx(h);
            }
        }
        self.hover_timer = 0;
        self.wheel_acc = 0;
    }

    /// Returns false when the app should exit.
    fn on_menu(&mut self, ev: MenuEvent) -> bool {
        let id = &ev.id;
        if *id == *self.quit_item.id() {
            return false;
        } else if let Some(i) = self.slots.iter().position(|s| s.show_item.id() == id) {
            self.select(i);
        } else if let Some(s) = self.slots.iter().find(|s| s.sign_item.id() == id) {
            match s.view {
                View::SignedOut(_) => s.worker.send(Cmd::SignIn),
                _ => s.worker.send(Cmd::SignOut),
            }
        } else if *id == *self.refresh_item.id() {
            for s in &self.slots {
                if !matches!(s.view, View::SigningIn { .. }) {
                    s.worker.send(Cmd::RefreshNow);
                }
            }
        } else if *id == *self.autostart_item.id() {
            autostart::set_enabled(!autostart::is_enabled());
            self.autostart_item.set_checked(autostart::is_enabled());
        }
        true
    }

    fn refresh(&self) {
        for (i, s) in self.slots.iter().enumerate() {
            s.sign_item.set_text(s.sign_text());
            s.sign_item.set_enabled(!matches!(s.view, View::Loading));
            s.show_item.set_checked(i == self.active);
        }
        let can_refresh = self
            .slots
            .iter()
            .any(|s| !matches!(s.view, View::SigningIn { .. }));
        self.refresh_item.set_enabled(can_refresh);
        let slot = self.slot();
        let _ = self.tray.set_icon(Some(make_icon(
            self.icon_size,
            &slot.view,
            self.light_taskbar,
        )));
        let entries: Vec<(ProviderId, View)> =
            self.slots.iter().map(|s| (s.id, s.view.clone())).collect();
        self.glance
            .set_data(entries.clone(), slot.id, self.light_taskbar);
        self.panel.set_data(entries, slot.id, self.light_taskbar);
    }
}

fn to_rect(r: Rect) -> RECT {
    let (x, y) = (r.position.x as i32, r.position.y as i32);
    RECT {
        left: x,
        top: y,
        right: x + r.size.width as i32,
        bottom: y + r.size.height as i32,
    }
}

fn make_icon(size: u32, view: &View, light_taskbar: bool) -> Icon {
    let usage = view
        .snapshot()
        .map(|s| (s.used_fraction(), s.elapsed_fraction()));
    Icon::from_rgba(icon::render(size, usage, light_taskbar), size, size)
        .expect("invalid icon buffer")
}

fn tray_icon_size() -> u32 {
    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) as u32 }
}

/// Flyouts and the taskbar follow the system (not app) light/dark setting.
fn taskbar_is_light() -> bool {
    let mut value: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut len),
        )
    };
    rc.is_ok() && value != 0
}
