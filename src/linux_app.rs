//! GTK3/Cairo flyout with native KDE StatusNotifier integration.
use crate::model::{Detail, ProviderId, Snapshot, Unit, View};
use crate::provider::Provider;
use crate::worker::{Cmd, Event, Worker};
use crate::{autostart, claude, codex, copilot, cursor, icon, opencode, time};
use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

type P = *mut c_void;
#[link(name = ":libgtk-3.so.0")]
unsafe extern "C" {
    fn gtk_init(argc: *mut c_int, argv: *mut P);
    fn gtk_main();
    fn gtk_main_quit();
    fn gtk_window_new(kind: c_int) -> P;
    fn gtk_settings_get_default() -> P;
    fn gtk_window_set_title(window: P, title: *const c_char);
    fn gtk_window_set_default_size(window: P, w: c_int, h: c_int);
    fn gtk_window_set_decorated(window: P, decorated: c_int);
    fn gtk_window_set_skip_taskbar_hint(window: P, setting: c_int);
    fn gtk_window_set_keep_above(window: P, setting: c_int);
    fn gtk_window_set_resizable(window: P, resizable: c_int);
    fn gtk_window_resize(window: P, width: c_int, height: c_int);
    fn gtk_window_move(window: P, x: c_int, y: c_int);
    fn gtk_window_present(window: P);
    fn gtk_window_present_with_time(window: P, timestamp: u32);
    fn gtk_widget_realize(widget: P);
    fn gtk_widget_get_window(widget: P) -> P;
    fn gtk_container_add(container: P, child: P);
    fn gtk_drawing_area_new() -> P;
    fn gtk_scrolled_window_new(hadj: P, vadj: P) -> P;
    fn gtk_scrolled_window_set_policy(window: P, hpolicy: c_int, vpolicy: c_int);
    fn gtk_widget_set_size_request(widget: P, w: c_int, h: c_int);
    fn gtk_widget_show_all(widget: P);
    fn gtk_widget_hide(widget: P);
    fn gtk_widget_get_visible(widget: P) -> c_int;
    fn gtk_widget_queue_draw(widget: P);
    fn gtk_widget_add_events(widget: P, events: c_int);
    fn gtk_menu_new() -> P;
    fn gtk_menu_popup(
        menu: P,
        parent_shell: P,
        parent_item: P,
        position: P,
        data: P,
        button: u32,
        time: u32,
    );
    fn gtk_menu_item_new_with_label(label: *const c_char) -> P;
    fn gtk_separator_menu_item_new() -> P;
    fn gtk_menu_shell_append(shell: P, child: P);
    fn gtk_menu_item_set_submenu(item: P, submenu: P);
    fn gtk_menu_item_set_label(item: P, label: *const c_char);
    fn gtk_widget_set_sensitive(widget: P, sensitive: c_int);
    fn gtk_clipboard_get(selection: P) -> P;
    fn gtk_clipboard_set_text(clipboard: P, text: *const c_char, len: c_int);
}
#[link(name = ":libgdk-3.so.0")]
unsafe extern "C" {
    fn g_object_set(object: P, name: *const c_char, ...);
    fn gdk_atom_intern_static_string(name: *const c_char) -> P;
    fn gdk_display_get_default() -> P;
    fn gdk_display_get_primary_monitor(display: P) -> P;
    fn gdk_display_get_monitor_at_point(display: P, x: c_int, y: c_int) -> P;
    fn gdk_display_get_monitor(display: P, monitor_num: c_int) -> P;
    fn gdk_monitor_get_workarea(monitor: P, workarea: *mut Workarea);
    fn gdk_x11_display_get_type() -> usize;
    fn gdk_x11_get_server_time(window: P) -> u32;
    fn gdk_window_get_events(window: P) -> c_int;
    fn gdk_window_set_events(window: P, events: c_int);
}
#[repr(C)]
struct Workarea {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
}
#[link(name = ":libgobject-2.0.so.0")]
unsafe extern "C" {
    fn g_type_check_instance_is_a(instance: P, kind: usize) -> c_int;
    fn g_signal_connect_data(
        instance: P,
        signal: *const c_char,
        callback: P,
        data: P,
        destroy: P,
        flags: c_int,
    ) -> u64;
}
#[link(name = ":libglib-2.0.so.0")]
unsafe extern "C" {
    fn g_timeout_add(interval: u32, callback: extern "C" fn(P) -> c_int, data: P) -> u32;
}
#[link(name = ":libcairo.so.2")]
unsafe extern "C" {
    fn cairo_set_source_rgb(cr: P, r: f64, g: f64, b: f64);
    fn cairo_rectangle(cr: P, x: f64, y: f64, w: f64, h: f64);
    fn cairo_fill(cr: P);
    fn cairo_set_line_width(cr: P, w: f64);
    fn cairo_arc(cr: P, x: f64, y: f64, r: f64, a: f64, b: f64);
    fn cairo_stroke(cr: P);
    fn cairo_move_to(cr: P, x: f64, y: f64);
    fn cairo_show_text(cr: P, text: *const c_char);
    fn cairo_select_font_face(cr: P, family: *const c_char, slant: c_int, weight: c_int);
    fn cairo_set_font_size(cr: P, size: f64);
    fn cairo_line_to(cr: P, x: f64, y: f64);
    fn cairo_close_path(cr: P);
    fn cairo_new_path(cr: P);
    fn cairo_set_dash(cr: P, dashes: *const f64, num: c_int, offset: f64);
    fn cairo_save(cr: P);
    fn cairo_restore(cr: P);
    fn cairo_translate(cr: P, tx: f64, ty: f64);
    fn cairo_image_surface_create(format: c_int, width: c_int, height: c_int) -> P;
    fn cairo_create(surface: P) -> P;
    fn cairo_surface_write_to_png(surface: P, path: *const c_char) -> c_int;
    fn cairo_destroy(cr: P);
    fn cairo_surface_destroy(surface: P);
}
thread_local! { static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) }; static LIGHT: Cell<bool> = const { Cell::new(false) }; }
struct Slot {
    id: ProviderId,
    worker: Option<Worker>,
    view: View,
    last_good: Option<Rc<Snapshot>>,
    sign: P,
    show: P,
}
struct App {
    window: P,
    area: P,
    _menu: P,
    slots: Vec<Slot>,
    events: Receiver<(ProviderId, Event)>,
    active: usize,
    tab: usize,
    autostart_item: P,
    demo: bool,
    light: bool,
    last_focus_hide: Option<Instant>,
}
#[derive(Clone, Copy)]
enum Action {
    Panel,
    Refresh,
    Quit,
    Autostart,
    Show(usize),
    Sign(usize),
}
struct MenuData(Action);
fn c(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).unwrap()
}
fn in_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Some(a) = a.borrow().as_ref() {
            f(&mut a.borrow_mut())
        }
    });
}
unsafe fn signal(widget: P, name: &[u8], cb: P, data: P) {
    unsafe {
        g_signal_connect_data(widget, name.as_ptr().cast(), cb, data, ptr::null_mut(), 0);
    }
}
unsafe fn menu_item(menu: P, label: &str, action: Action) -> P {
    let item = unsafe { gtk_menu_item_new_with_label(c(label).as_ptr()) };
    unsafe {
        gtk_menu_shell_append(menu, item);
        signal(
            item,
            b"activate\0",
            on_menu as P,
            Box::into_raw(Box::new(MenuData(action))).cast(),
        )
    };
    item
}
unsafe fn separator(menu: P) {
    unsafe { gtk_menu_shell_append(menu, gtk_separator_menu_item_new()) };
}
unsafe fn submenu(menu: P, label: &str) -> P {
    let item = unsafe { gtk_menu_item_new_with_label(c(label).as_ptr()) };
    let child = unsafe { gtk_menu_new() };
    unsafe {
        gtk_menu_item_set_submenu(item, child);
        gtk_menu_shell_append(menu, item);
    }
    child
}
extern "C" fn on_menu(_widget: P, data: P) {
    let action = unsafe { (*(data as *const MenuData)).0 };
    in_app(|a| a.action(action));
}
fn on_tray(action: crate::linux_tray::Action) {
    in_app(|a| match action {
        crate::linux_tray::Action::Activate(x, y) => a.toggle_panel(x, y),
        crate::linux_tray::Action::ContextMenu => unsafe {
            // KDE normally renders the exported DBusMenu itself. This is the
            // fallback for hosts that invoke ContextMenu instead.
            gtk_menu_popup(
                a._menu,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                3,
                0,
            );
        },
        crate::linux_tray::Action::Scroll(delta) => {
            let ready = a
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, slot)| slot.view.snapshot().map(|_| i))
                .collect::<Vec<_>>();
            if let Some(next) = scroll_target(a.active, delta, &ready) {
                if next == a.active {
                    return;
                }
                a.active = next;
                a.tab = 0;
                a.refresh();
            }
        }
    });
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
extern "C" fn on_draw(_widget: P, cr: P, _data: P) -> c_int {
    in_app(|a| a.draw(cr));
    0
}
#[repr(C)]
struct ButtonEvent {
    _kind: i32,
    _window: P,
    _send: i8,
    _time: u32,
    x: f64,
    y: f64,
}
extern "C" fn on_click(_widget: P, ev: *const ButtonEvent, _data: P) -> c_int {
    let (x, y) = unsafe { ((*ev).x, (*ev).y) };
    in_app(|a| a.click(x, y));
    1
}
extern "C" fn on_tick(_data: P) -> c_int {
    in_app(|a| a.tick());
    1
}
extern "C" fn on_delete(_widget: P, _event: P, _data: P) -> c_int {
    in_app(|a| unsafe { gtk_widget_hide(a.window) });
    1
}
extern "C" fn on_focus_out(widget: P, _event: P, _data: P) -> c_int {
    in_app(|a| {
        a.last_focus_hide = Some(Instant::now());
        unsafe { gtk_widget_hide(widget) };
    });
    0
}
#[repr(C)]
struct KeyEvent {
    _kind: c_int,
    _window: P,
    _send: i8,
    _time: u32,
    _state: u32,
    keyval: u32,
}
extern "C" fn on_key_press(widget: P, event: *const KeyEvent, _data: P) -> c_int {
    if unsafe { (*event).keyval } == 0xff1b {
        unsafe { gtk_widget_hide(widget) };
        1
    } else {
        0
    }
}
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|x| x == "--demo");
    if let Some(i) = args.iter().position(|x| x == "--demo-sheet") {
        let path = args.get(i + 1).expect("--demo-sheet needs a PNG path");
        demo_sheet(path, &args);
        return;
    }
    unsafe { gtk_init(ptr::null_mut(), ptr::null_mut()) };
    let app = Rc::new(RefCell::new(App::new(demo)));
    APP.with(|a| *a.borrow_mut() = Some(app));
    unsafe {
        g_timeout_add(100, on_tick, ptr::null_mut());
        gtk_main()
    };
}
impl App {
    fn new(demo: bool) -> Self {
        if !demo {
            autostart::follow_exe();
        }
        let window = unsafe { gtk_window_new(0) };
        unsafe {
            g_object_set(
                gtk_settings_get_default(),
                c("gtk-application-prefer-dark-theme").as_ptr(),
                (!light_theme()) as c_int,
                ptr::null::<c_char>(),
            );
            gtk_window_set_title(window, c("TokenKeeper").as_ptr());
            gtk_window_set_default_size(window, 440, 1);
            gtk_window_set_decorated(window, 0);
            gtk_window_set_skip_taskbar_hint(window, 1);
            gtk_window_set_keep_above(window, 1);
            gtk_window_set_resizable(window, 0);
            signal(
                window,
                b"focus-out-event\0",
                on_focus_out as P,
                ptr::null_mut(),
            );
            signal(
                window,
                b"key-press-event\0",
                on_key_press as P,
                ptr::null_mut(),
            );
        };
        let area = unsafe { gtk_drawing_area_new() };
        let scroll = unsafe { gtk_scrolled_window_new(ptr::null_mut(), ptr::null_mut()) };
        unsafe {
            gtk_scrolled_window_set_policy(scroll, 2, 1); // never horizontal, automatic vertical
            gtk_widget_set_size_request(area, 440, 610);
            gtk_container_add(window, scroll);
            gtk_container_add(scroll, area);
            gtk_widget_add_events(area, 1 << 8);
            signal(area, b"draw\0", on_draw as P, ptr::null_mut());
            signal(
                area,
                b"button-press-event\0",
                on_click as P,
                ptr::null_mut(),
            );
            signal(window, b"delete-event\0", on_delete as P, ptr::null_mut());
        }
        let menu = unsafe { gtk_menu_new() };
        unsafe {
            menu_item(menu, "Show usage panel", Action::Panel);
            separator(menu)
        };
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
                    sign: ptr::null_mut(),
                    show: ptr::null_mut(),
                }
            })
            .collect::<Vec<_>>();
        let accounts_menu = unsafe { submenu(menu, "Accounts") };
        let mut slots = slots;
        for (i, s) in slots.iter_mut().enumerate() {
            s.sign = unsafe {
                menu_item(
                    accounts_menu,
                    &format!("Sign in to {}…", s.id.name()),
                    Action::Sign(i),
                )
            };
        }
        let icon_menu = unsafe { submenu(menu, "Switch tray icon") };
        for (i, s) in slots.iter_mut().enumerate() {
            unsafe {
                s.show = menu_item(icon_menu, s.id.name(), Action::Show(i));
            }
        }
        unsafe {
            separator(menu);
            menu_item(menu, "Refresh now", Action::Refresh)
        };
        let autostart_item = unsafe { menu_item(menu, "Autostart - Off", Action::Autostart) };
        unsafe {
            menu_item(menu, "Quit", Action::Quit);
            gtk_widget_show_all(menu)
        };
        if !demo {
            if let Err(error) = crate::linux_tray::start(menu, on_tray) {
                eprintln!("{error}");
            }
        }
        let mut a = Self {
            window,
            area,
            _menu: menu,
            slots,
            events,
            active: 0,
            tab: 0,
            autostart_item,
            demo,
            light: light_theme(),
            last_focus_hide: None,
        };
        a.refresh();
        if demo {
            a.show_panel();
        }
        a
    }
    fn show_panel(&self) {
        self.show_panel_at(None);
    }
    fn show_panel_at(&self, anchor: Option<(i32, i32)>) {
        unsafe {
            let display = gdk_display_get_default();
            // SNI activation has no input timestamp. Obtain a fresh X11 server
            // time so KWin can focus the flyout instead of merely raising it.
            // PropertyChangeMask is required by gdk_x11_get_server_time.
            gtk_widget_realize(self.window);
            let native = gtk_widget_get_window(self.window);
            let timestamp = if !display.is_null()
                && !native.is_null()
                && g_type_check_instance_is_a(display, gdk_x11_display_get_type()) != 0
            {
                gdk_window_set_events(native, gdk_window_get_events(native) | (1 << 16));
                Some(gdk_x11_get_server_time(native))
            } else {
                None
            };
            if !display.is_null() {
                let mut monitor = anchor
                    .map(|(x, y)| gdk_display_get_monitor_at_point(display, x, y))
                    .unwrap_or_else(|| gdk_display_get_primary_monitor(display));
                if monitor.is_null() {
                    monitor = gdk_display_get_monitor(display, 0);
                }
                if !monitor.is_null() {
                    let mut workarea = Workarea {
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    };
                    gdk_monitor_get_workarea(monitor, &mut workarea);
                    let panel_height = (self.panel_height() as c_int).min(610);
                    let fallback_x = workarea.x + (workarea.width - 440 - 12).max(0);
                    let fallback_y = workarea.y + (workarea.height - panel_height - 12).max(0);
                    let (x, y) = anchor
                        .map(|(x, y)| {
                            (
                                (x - 440).clamp(
                                    workarea.x,
                                    (workarea.x + workarea.width - 440).max(workarea.x),
                                ),
                                (y - panel_height - 12).clamp(
                                    workarea.y,
                                    (workarea.y + workarea.height - panel_height).max(workarea.y),
                                ),
                            )
                        })
                        .unwrap_or((fallback_x, fallback_y));
                    gtk_window_move(self.window, x, y);
                }
            }
            let height = (self.panel_height() as c_int).min(610);
            gtk_window_set_default_size(self.window, 440, height);
            gtk_window_resize(self.window, 440, height);
            gtk_widget_show_all(self.window);
            if let Some(timestamp) = timestamp {
                gtk_window_present_with_time(self.window, timestamp);
            } else {
                gtk_window_present(self.window);
            }
        }
    }
    fn toggle_panel(&mut self, x: i32, y: i32) {
        if unsafe { gtk_widget_get_visible(self.window) } != 0 {
            unsafe { gtk_widget_hide(self.window) };
        } else if !self
            .last_focus_hide
            .is_some_and(|hidden| hidden.elapsed() < Duration::from_millis(400))
        {
            self.show_panel_at(Some((x, y)));
        }
    }
    fn action(&mut self, a: Action) {
        match a {
            Action::Panel => self.show_panel(),
            Action::Refresh => {
                for s in &self.slots {
                    if let Some(w) = &s.worker {
                        w.send(Cmd::RefreshNow)
                    }
                }
            }
            Action::Quit => unsafe { gtk_main_quit() },
            Action::Autostart => {
                if !self.demo {
                    autostart::set_enabled(!autostart::is_enabled());
                    self.refresh()
                }
            }
            Action::Show(i) => {
                self.active = i;
                self.tab = 0;
                self.refresh()
            }
            Action::Sign(i) => {
                if let Some(w) = &self.slots[i].worker {
                    if matches!(self.slots[i].view, View::SignedOut(_)) {
                        w.send(Cmd::SignIn)
                    } else {
                        w.send(Cmd::SignOut)
                    }
                }
            }
        }
    }
    fn tick(&mut self) {
        let mut dirty = false;
        while let Ok((id, ev)) = self.events.try_recv() {
            if let Some(i) = self.slots.iter().position(|s| s.id == id) {
                let mut show_panel = false;
                let slot = &mut self.slots[i];
                slot.view = match ev {
                    Event::SignedOut(e) => {
                        slot.last_good = None;
                        View::SignedOut(e)
                    }
                    Event::SigningIn(code) => {
                        self.active = i;
                        self.tab = 0;
                        if let Some(code) = &code {
                            let selection = unsafe {
                                gdk_atom_intern_static_string(b"CLIPBOARD\0".as_ptr().cast())
                            };
                            unsafe {
                                gtk_clipboard_set_text(
                                    gtk_clipboard_get(selection),
                                    c(code).as_ptr(),
                                    -1,
                                );
                            }
                            show_panel = true;
                        }
                        View::SigningIn { code }
                    }
                    Event::Usage(s) => {
                        let completed_login = matches!(slot.view, View::SigningIn { .. });
                        let snap = Rc::new(s);
                        slot.last_good = Some(snap.clone());
                        if completed_login {
                            self.active = i;
                            self.tab = 0;
                            show_panel = true;
                        }
                        View::Ready { snap, error: None }
                    }
                    Event::Error(e) => {
                        if let Some(snap) = &slot.last_good {
                            View::Ready {
                                snap: snap.clone(),
                                error: Some(e),
                            }
                        } else {
                            View::Failed(e)
                        }
                    }
                };
                if show_panel {
                    self.show_panel();
                }
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
            self.refresh()
        }
    }
    fn refresh(&mut self) {
        for (i, s) in self.slots.iter().enumerate() {
            unsafe {
                gtk_menu_item_set_label(
                    s.show,
                    c(&format!(
                        "{}{}",
                        s.id.name(),
                        if i == self.active { " (active)" } else { "" }
                    ))
                    .as_ptr(),
                );
                gtk_menu_item_set_label(
                    s.sign,
                    c(&s.view.account_menu_label(s.id.name())).as_ptr(),
                );
                gtk_widget_set_sensitive(s.sign, (!matches!(s.view, View::Loading)) as c_int);
            }
        }
        unsafe {
            gtk_menu_item_set_label(
                self.autostart_item,
                c(if autostart::is_enabled() {
                    "Autostart - On"
                } else {
                    "Autostart - Off"
                })
                .as_ptr(),
            )
        };
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
        );
        unsafe {
            gtk_widget_set_size_request(self.area, 440, self.panel_height() as c_int);
            gtk_widget_queue_draw(self.area)
        };
    }
    fn panel_height(&self) -> f64 {
        let height = self.slots[self.active]
            .view
            .snapshot()
            .map_or(610.0, |snap| 380.0 + detail_height(snap, self.tab) + 58.0);
        height
            - if self.visible_providers().len() == 1 {
                155.0
            } else {
                0.0
            }
    }

    fn click(&mut self, x: f64, y: f64) {
        let details = self.slots[self.active]
            .view
            .snapshot()
            .map_or(0, |s| s.details.len());
        let visible = self.visible_providers();
        match hit_test(x, y, &visible, details) {
            Hit::Provider(i) => {
                self.active = i;
                self.tab = 0;
                self.refresh();
            }
            Hit::Tab(i) => {
                self.tab = i;
                self.refresh();
            }
            Hit::Usage => open_url(self.slots[self.active].id.usage_url()),
            Hit::None => {}
        }
    }

    fn draw(&self, cr: P) {
        LIGHT.with(|theme| theme.set(self.light));
        background(cr, self.panel_height());
        let visible = self.visible_providers();
        let width = 440.0 / visible.len() as f64;
        if visible.len() > 1 {
            for (column, &i) in visible.iter().enumerate() {
                let slot = &self.slots[i];
                let x = column as f64 * width;
                rounded(
                    cr,
                    x + 4.0,
                    5.0,
                    width - 8.0,
                    92.0,
                    9.0,
                    if i == self.active { 0.25 } else { 0.20 },
                );
                text(
                    cr,
                    x + 12.0,
                    26.0,
                    slot.id.short_name(),
                    13.0,
                    if i == self.active { 0.98 } else { 0.75 },
                );
                if let Some(s) = slot.view.snapshot() {
                    ring(
                        cr,
                        x + 35.0,
                        61.0,
                        17.0,
                        s.used_fraction(),
                        s.elapsed_fraction(),
                    );
                    text(
                        cr,
                        x + 59.0,
                        65.0,
                        &format!("{:.0}%", s.used_fraction() * 100.0),
                        14.0,
                        0.95,
                    );
                } else {
                    text(cr, x + 12.0, 64.0, "Signed out", 11.0, 0.55);
                }
                if i == self.active {
                    rounded(cr, x + width / 2.0 - 7.0, 97.0, 14.0, 3.0, 2.0, 0.7);
                }
            }
        }
        unsafe {
            cairo_save(cr);
            if visible.len() == 1 {
                cairo_translate(cr, 0.0, -95.0);
            }
        }
        let slot = &self.slots[self.active];
        text(cr, 22.0, 137.0, slot.id.name(), 17.0, 1.0);
        text_accent(cr, 361.0, 137.0, "Usage >", 12.0);
        if let Some(s) = slot.view.snapshot() {
            rounded(cr, 252.0, 116.0, 100.0, 27.0, 13.0, 0.23);
            text(cr, 261.0, 134.0, &s.plan, 10.0, 0.75);
            ring(
                cr,
                75.0,
                205.0,
                43.0,
                s.used_fraction(),
                s.elapsed_fraction(),
            );
            text(
                cr,
                55.0,
                212.0,
                &format!("{:.0}%", s.used_fraction() * 100.0),
                17.0,
                1.0,
            );
            text(cr, 145.0, 184.0, &s.fmt(s.used), 28.0, 1.0);
            text(cr, 145.0, 207.0, &s.of_text(), 13.0, 0.62);
            text(cr, 145.0, 232.0, &s.pace_text(), 13.0, 0.8);
            self.draw_combined(cr);
            unsafe {
                cairo_save(cr);
                if visible.len() == 1 {
                    cairo_translate(cr, 0.0, -60.0);
                }
            }
            rounded(cr, 20.0, 330.0, 400.0, 38.0, 8.0, 0.18);
            for (i, d) in s.details.iter().enumerate() {
                let width = 400.0 / s.details.len() as f64;
                let x = 20.0 + i as f64 * width;
                if i == self.tab {
                    rounded(cr, x + 2.0, 332.0, width - 4.0, 34.0, 7.0, 0.27);
                }
                text(
                    cr,
                    x + 18.0,
                    355.0,
                    d.label(),
                    13.0,
                    if i == self.tab { 1.0 } else { 0.6 },
                );
            }
            let detail_height = detail_height(s, self.tab);
            rounded(cr, 20.0, 380.0, 400.0, detail_height, 9.0, 0.18);
            self.draw_detail(cr, s);
            let footer = 380.0 + detail_height + 34.0;
            text(cr, 22.0, footer, &s.resets_text(), 11.0, 0.56);
            if matches!(slot.view, View::Ready { error: Some(_), .. }) {
                text_warning(cr, 275.0, footer, "Update failed, retrying", 11.0);
            } else {
                text(
                    cr,
                    315.0,
                    footer,
                    &format!("Updated {}", time::ago(s.fetched_ms)),
                    11.0,
                    0.56,
                );
            }
            unsafe { cairo_restore(cr) };
        } else {
            let message = match &slot.view {
                View::Loading => "Loading usage…",
                View::SignedOut(_) => "Sign in from the tray menu",
                View::SigningIn { code: Some(_) } => "Enter the copied code in your browser",
                View::SigningIn { .. } => "Complete sign-in in your browser",
                View::Failed(_) => "Could not load usage",
                _ => "",
            };
            text(cr, 22.0, 200.0, message, 15.0, 0.75);
            if let View::SignedOut(Some(error)) | View::Failed(error) = &slot.view {
                text_warning(
                    cr,
                    22.0,
                    228.0,
                    &error.chars().take(65).collect::<String>(),
                    11.0,
                );
            }
            if let View::SigningIn { code: Some(code) } = &slot.view {
                text(cr, 22.0, 248.0, code, 26.0, 1.0);
            }
        }
        unsafe { cairo_restore(cr) };
    }

    fn visible_providers(&self) -> Vec<usize> {
        let mut visible = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.view.snapshot().is_some() || matches!(s.view, View::SigningIn { .. })
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if !visible.contains(&self.active) {
            visible.push(self.active);
            visible.sort_unstable();
        }
        visible
    }

    fn draw_combined(&self, cr: P) {
        let dollars: Vec<&Snapshot> = self
            .slots
            .iter()
            .filter_map(|s| s.view.snapshot())
            .filter(|s| s.unit == Unit::Cents && s.limit > 0)
            .collect();
        if dollars.len() < 2 {
            return;
        }
        let used: u32 = dollars.iter().map(|s| s.used).sum();
        let limit: u32 = dollars.iter().map(|s| s.limit).sum();
        text(cr, 22.0, 273.0, "All dollar tools", 12.0, 0.7);
        text(
            cr,
            255.0,
            273.0,
            &format!(
                "{} / {}",
                crate::model::dollars(used),
                crate::model::dollars(limit)
            ),
            11.0,
            0.8,
        );
        rounded(cr, 22.0, 284.0, 396.0, 11.0, 5.0, 0.19);
        let mut x = 22.0;
        for s in dollars {
            let width = 396.0 * s.used.min(s.limit) as f64 / limit as f64;
            if width > 0.0 {
                let rgb = match s.provider {
                    ProviderId::Cursor => 0x58c7eb,
                    ProviderId::Copilot => 0x8957e5,
                    ProviderId::Claude => 0xd97757,
                    ProviderId::Codex => 0x10a37f,
                    ProviderId::OpenCode => 0x6e9ee8,
                };
                rgb_color(cr, rgb);
                unsafe {
                    cairo_rectangle(cr, x, 284.0, width, 11.0);
                    cairo_fill(cr);
                }
                x += width;
            }
        }
    }

    fn draw_detail(&self, cr: P, s: &Snapshot) {
        let detail = s.details.get(self.tab).copied().unwrap_or(Detail::Today);
        match detail {
            Detail::Limits => {
                text(cr, 35.0, 407.0, "Rate limits", 13.0, 0.95);
                for (i, meter) in s.meters.iter().enumerate() {
                    let y = 432.0 + i as f64 * 38.0;
                    text(cr, 35.0, y, &meter.label, 12.0, 0.8);
                    if let Some(reset) = meter.resets_ms {
                        text(
                            cr,
                            250.0,
                            y,
                            &format!("resets {}", time::day_month(reset)),
                            10.0,
                            0.58,
                        );
                    }
                    text(
                        cr,
                        370.0,
                        y,
                        &format!("{:.0}%", meter.fraction * 100.0),
                        12.0,
                        0.9,
                    );
                    bar(cr, 35.0, y + 7.0, 365.0, meter.fraction);
                }
            }
            _ => {
                let (values, labels, title): (Vec<Option<u32>>, Vec<String>, &str) = match detail {
                    Detail::Today => (
                        s.hourly.clone(),
                        (0..s.hourly.len()).map(|i| format!("{i}:00")).collect(),
                        "Today by hour",
                    ),
                    Detail::Cycle => (s.daily.clone(), s.daily_labels.clone(), "Daily this cycle"),
                    Detail::History => (
                        s.history.iter().copied().map(Some).collect(),
                        s.history_labels.clone(),
                        "Monthly totals",
                    ),
                    _ => unreachable!(),
                };
                text(cr, 35.0, 408.0, title, 13.0, 0.95);
                if values.is_empty() {
                    text(cr, 85.0, 480.0, s.empty_note, 13.0, 0.55);
                    return;
                }
                let max = values.iter().filter_map(|v| *v).max().unwrap_or(1).max(1) as f64;
                let width = 365.0 / values.len() as f64;
                if detail == Detail::Cycle && s.limit > 0 {
                    let budget = s.limit as f64 / s.cycle_days() as f64;
                    let y = 524.0 - (budget / max * 97.0).min(100.0);
                    color(cr, 0.58);
                    unsafe {
                        cairo_set_line_width(cr, 1.0);
                        cairo_set_dash(cr, [3.0, 3.0].as_ptr(), 2, 0.0);
                        cairo_move_to(cr, 35.0, y);
                        cairo_line_to(cr, 400.0, y);
                        cairo_stroke(cr);
                        cairo_set_dash(cr, ptr::null(), 0, 0.0);
                    }
                    text(
                        cr,
                        310.0,
                        408.0,
                        &format!("budget {}/day", s.fmt(budget as u32)),
                        10.0,
                        0.62,
                    );
                }
                for (i, value) in values.iter().enumerate() {
                    if let Some(value) = value {
                        let h = *value as f64 / max * 97.0;
                        rgb_color(
                            cr,
                            if *value > s.limit && detail == Detail::History {
                                0xc65353
                            } else {
                                0x5bc4ed
                            },
                        );
                        unsafe {
                            cairo_rectangle(
                                cr,
                                35.0 + i as f64 * width,
                                524.0 - h,
                                (width - 2.0).max(1.0),
                                h,
                            );
                            cairo_fill(cr);
                        }
                    }
                }
                for i in [0, values.len() / 2, values.len() - 1] {
                    if let Some(label) = labels.get(i) {
                        text(cr, 35.0 + i as f64 * width, 544.0, label, 10.0, 0.55);
                    }
                }
            }
        }
    }
}
fn color(cr: P, v: f64) {
    LIGHT.with(|light| {
        let v = if light.get() { 1.0 - v } else { v };
        unsafe { cairo_set_source_rgb(cr, v, v, v) }
    })
}
fn rgb_color(cr: P, rgb: u32) {
    unsafe {
        cairo_new_path(cr);
        cairo_set_source_rgb(
            cr,
            ((rgb >> 16) & 255) as f64 / 255.0,
            ((rgb >> 8) & 255) as f64 / 255.0,
            (rgb & 255) as f64 / 255.0,
        )
    }
}
fn background(cr: P, height: f64) {
    LIGHT.with(|light| {
        let v = if light.get() { 0.976 } else { 0.169 };
        unsafe {
            cairo_set_source_rgb(cr, v, v, v);
            cairo_rectangle(cr, 0.0, 0.0, 440.0, height);
            cairo_fill(cr)
        }
    })
}
fn rounded(cr: P, x: f64, y: f64, w: f64, h: f64, r: f64, v: f64) {
    LIGHT.with(|light| {
        if light.get() && v <= 0.5 {
            let shade = 1.0 - v * 0.25;
            unsafe { cairo_set_source_rgb(cr, shade, shade, shade) };
        } else {
            color(cr, v);
        }
    });
    unsafe {
        cairo_new_path(cr);
        cairo_arc(cr, x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cairo_arc(
            cr,
            x + w - r,
            y + h - r,
            r,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        cairo_arc(
            cr,
            x + r,
            y + h - r,
            r,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        );
        cairo_arc(
            cr,
            x + r,
            y + r,
            r,
            std::f64::consts::PI,
            1.5 * std::f64::consts::PI,
        );
        cairo_close_path(cr);
        cairo_fill(cr)
    }
}
fn text_warning(cr: P, x: f64, y: f64, s: &str, size: f64) {
    rgb_color(cr, 0xf0b030);
    unsafe {
        cairo_select_font_face(cr, b"Sans\0".as_ptr().cast(), 0, 0);
        cairo_set_font_size(cr, size);
        cairo_move_to(cr, x, y);
        cairo_show_text(cr, c(s).as_ptr());
    }
}
fn detail_height(s: &Snapshot, tab: usize) -> f64 {
    if s.details.get(tab) == Some(&Detail::Limits) {
        (50.0 + s.meters.len() as f64 * 38.0).max(172.0)
    } else {
        172.0
    }
}
fn text_accent(cr: P, x: f64, y: f64, s: &str, size: f64) {
    rgb_color(cr, 0x58c7eb);
    unsafe {
        cairo_select_font_face(cr, b"Sans\0".as_ptr().cast(), 0, 0);
        cairo_set_font_size(cr, size);
        cairo_move_to(cr, x, y);
        cairo_show_text(cr, c(s).as_ptr())
    }
}
fn text(cr: P, x: f64, y: f64, s: &str, size: f64, v: f64) {
    let s = c(s);
    color(cr, v);
    unsafe {
        cairo_select_font_face(cr, b"Sans\0".as_ptr().cast(), 0, 0);
        cairo_set_font_size(cr, size);
        cairo_move_to(cr, x, y);
        cairo_show_text(cr, s.as_ptr())
    }
}
fn ring(cr: P, x: f64, y: f64, r: f64, used: f32, elapsed: f32) {
    use std::f64::consts::PI;
    color(cr, 0.22);
    unsafe {
        cairo_new_path(cr);
        cairo_set_line_width(cr, 5.0);
        cairo_arc(cr, x, y, r, -PI / 2.0, 3.0 * PI / 2.0);
        cairo_stroke(cr)
    };
    let rgb = icon::level_rgb(used);
    unsafe {
        cairo_new_path(cr);
        cairo_set_source_rgb(
            cr,
            ((rgb >> 16) & 255) as f64 / 255.0,
            ((rgb >> 8) & 255) as f64 / 255.0,
            (rgb & 255) as f64 / 255.0,
        );
        cairo_arc(
            cr,
            x,
            y,
            r,
            -PI / 2.0,
            -PI / 2.0 + 2.0 * PI * used.min(1.0) as f64,
        );
        cairo_stroke(cr)
    };
    color(cr, 0.9);
    let a = -PI / 2.0 + 2.0 * PI * elapsed as f64;
    unsafe {
        cairo_new_path(cr);
        cairo_set_line_width(cr, 2.0);
        cairo_move_to(cr, x + (r - 4.0) * a.cos(), y + (r - 4.0) * a.sin());
        cairo_line_to(cr, x + (r + 4.0) * a.cos(), y + (r + 4.0) * a.sin());
        cairo_stroke(cr)
    }
}
fn bar(cr: P, x: f64, y: f64, w: f64, f: f32) {
    rounded(cr, x, y, w, 8.0, 4.0, 0.3);
    rgb_color(cr, icon::level_rgb(f));
    unsafe {
        cairo_rectangle(cr, x, y, w * f.min(1.0) as f64, 8.0);
        cairo_fill(cr)
    }
}
fn light_theme() -> bool {
    std::env::var("TOKENKEEPER_THEME")
        .or_else(|_| std::env::var("AI_USAGE_THEME"))
        .is_ok_and(|v| v.eq_ignore_ascii_case("light"))
}

/// Render the real Cairo panel without a display server or live account access.
fn demo_sheet(path: &str, args: &[String]) {
    let slots = ProviderId::ALL
        .iter()
        .map(|&id| {
            let snap = match id {
                ProviderId::Cursor => crate::mock::simulated(0.42),
                ProviderId::Copilot => crate::mock::simulated_copilot(0.67),
                ProviderId::Claude => crate::mock::simulated_claude(0.27),
                ProviderId::Codex => crate::mock::simulated_codex(0.52),
                ProviderId::OpenCode => crate::mock::simulated_opencode(0.38),
            };
            Slot {
                id,
                worker: None,
                view: View::Ready {
                    snap: Rc::new(snap),
                    error: None,
                },
                last_good: None,
                sign: ptr::null_mut(),
                show: ptr::null_mut(),
            }
        })
        .collect();
    let (_, events) = mpsc::channel();
    let mut app = App {
        window: ptr::null_mut(),
        area: ptr::null_mut(),
        _menu: ptr::null_mut(),
        slots,
        events,
        active: args
            .iter()
            .position(|x| x == "--demo-provider")
            .and_then(|i| args.get(i + 1))
            .map_or(0, |name| match name.as_str() {
                "copilot" => 1,
                "claude" => 2,
                "codex" => 3,
                "opencode" => 4,
                _ => 0,
            }),
        tab: args
            .iter()
            .position(|x| x == "--demo-tab")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
        autostart_item: ptr::null_mut(),
        demo: true,
        light: light_theme(),
        last_focus_hide: None,
    };
    if args.iter().any(|arg| arg == "--demo-single") {
        for (i, slot) in app.slots.iter_mut().enumerate() {
            if i != app.active {
                slot.view = View::SignedOut(None);
            }
        }
    }
    unsafe {
        let surface = cairo_image_surface_create(0, 440, app.panel_height() as c_int);
        assert!(!surface.is_null(), "Cairo surface unavailable");
        let cr = cairo_create(surface);
        app.draw(cr);
        let result = cairo_surface_write_to_png(surface, c(path).as_ptr());
        cairo_destroy(cr);
        cairo_surface_destroy(surface);
        assert_eq!(result, 0, "failed to write demo PNG");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Hit {
    Provider(usize),
    Tab(usize),
    Usage,
    None,
}
fn hit_test(x: f64, y: f64, providers: &[usize], details: usize) -> Hit {
    if !(0.0..440.0).contains(&x) || !(0.0..610.0).contains(&y) {
        return Hit::None;
    }
    if y < 102.0 && providers.len() > 1 {
        return Hit::Provider(providers[(x / (440.0 / providers.len() as f64)) as usize]);
    }
    let single = providers.len() == 1;
    let detail_y = y + if single { 155.0 } else { 0.0 };
    if (330.0..370.0).contains(&detail_y) && (20.0..420.0).contains(&x) && details > 0 {
        return Hit::Tab(((x - 20.0) / (400.0 / details as f64)) as usize);
    }
    let header_y = y + if single { 95.0 } else { 0.0 };
    if (115.0..155.0).contains(&header_y) && x > 345.0 {
        return Hit::Usage;
    }
    Hit::None
}
#[cfg(test)]
mod hit_tests {
    use super::*;
    #[test]
    fn four_columns_and_offset_tabs() {
        assert_eq!(hit_test(385.0, 60.0, &[0, 1, 2, 3], 3), Hit::Provider(3));
        assert_eq!(hit_test(180.0, 345.0, &[0, 1, 2, 3], 3), Hit::Tab(1));
        assert_eq!(hit_test(400.0, 345.0, &[0, 1, 2, 3], 3), Hit::Tab(2));
        assert_eq!(hit_test(10.0, 345.0, &[0, 1, 2, 3], 3), Hit::None);
        assert_eq!(hit_test(220.0, 60.0, &[3], 1), Hit::None);
        assert_eq!(hit_test(180.0, 190.0, &[3], 1), Hit::Tab(0));
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;
    #[test]
    fn all_limit_rows_extend_scrollable_canvas() {
        let mut s = crate::mock::simulated_codex(0.5);
        s.meters = (0..8)
            .map(|i| crate::model::Meter {
                label: format!("window {i}"),
                fraction: 0.5,
                resets_ms: None,
            })
            .collect();
        let height = detail_height(&s, 0);
        assert!(height >= 50.0 + 8.0 * 38.0);
        assert!(380.0 + height + 58.0 > 610.0);
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::scroll_target;
    #[test]
    fn scrolling_skips_signed_out_providers_and_wraps() {
        assert_eq!(scroll_target(3, 120, &[0, 3]), Some(0));
        assert_eq!(scroll_target(0, -120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(0, 120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(3, -120, &[0, 3]), Some(0));
        assert_eq!(scroll_target(2, 120, &[0, 3]), Some(0));
        assert_eq!(scroll_target(2, -120, &[0, 3]), Some(3));
    }
    #[test]
    fn one_ready_provider_stays_selected_and_empty_is_idle() {
        assert_eq!(scroll_target(3, 120, &[3]), Some(3));
        assert_eq!(scroll_target(3, -120, &[3]), Some(3));
        assert_eq!(scroll_target(3, 120, &[]), None);
        assert_eq!(scroll_target(3, 0, &[3]), None);
    }
}
