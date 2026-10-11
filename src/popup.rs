//! Native flyout windows drawn with Direct2D/DirectWrite.
//!
//! `Kind::Glance` is a non-activating hover card; `Kind::Panel` is a focused
//! flyout with tabs that hides itself when it loses activation, like the
//! Windows volume and network flyouts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_FLAT,
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetClientRect,
    GetCursorPos, GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, IDC_HAND, LoadCursorW,
    MA_NOACTIVATE, PostThreadMessageW, RegisterClassExW, SetCursor, WM_MOUSEWHEEL, WM_SETCURSOR,
    SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};
use windows_numerics::Vector2;

use crate::icon::level_rgb;
use crate::model::{Detail, ProviderId, Snapshot, Unit, View, dollars};
use crate::time;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Glance,
    Panel,
}

/// The tab to show: the user's choice if this provider has it, else a default.
fn effective_tab(details: &[Detail], chosen: Detail) -> Detail {
    if details.contains(&chosen) {
        chosen
    } else if details.contains(&Detail::Cycle) && details[0] == Detail::Today {
        Detail::Cycle
    } else {
        details.first().copied().unwrap_or(Detail::Cycle)
    }
}

fn brand_rgb(p: ProviderId) -> Option<u32> {
    match p {
        ProviderId::Cursor => None,
        ProviderId::Copilot => Some(0x8957E5),
        ProviderId::Claude => Some(0xD97757),
        ProviderId::Codex => Some(0x10A37F),
        ProviderId::OpenCode => Some(0x6E9EE8),
    }
}

const GLANCE_W: f32 = 300.0;
const GLANCE_H: f32 = 176.0;
const PANEL_W: f32 = 340.0;
const PANEL_H: f32 = 328.0;
const PAD: f32 = 14.0;
const BADGE_W: f32 = 76.0;

enum Click {
    None,
    Redraw,
    OpenUsagePage(ProviderId),
    CopyCode(String),
}

/// Column layout, in DIPs.
const COL_W: f32 = 124.0;
/// Bottom of the glance's columns; the combined-total bar sits below them.
const COLS_GLANCE_STRIP_H: f32 = 206.0;
const TOTAL_H: f32 = 40.0;
const COLS_GLANCE_FOOTER: f32 = COLS_GLANCE_STRIP_H + TOTAL_H;
const COLS_GLANCE_H: f32 = COLS_GLANCE_FOOTER + 30.0;
const COLS_PANEL_H: f32 = 378.0 + TOTAL_H;
const COLS_STRIP_H: f32 = 162.0;
/// Top of the tabs in the panel's single and column layouts.
const SINGLE_DETAIL_TOP: f32 = 122.0;
const COLS_DETAIL_TOP: f32 = 172.0 + TOTAL_H;

fn in_rect(x: f32, y: f32, (rx, ry, rw, rh): (f32, f32, f32, f32)) -> bool {
    x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
}
/// Gap between the popup and the taskbar, in DIPs.
const EDGE_GAP: f32 = 12.0;

/// Device-independent resources shared by every popup.
pub struct Gfx {
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    round: ID2D1StrokeStyle,
    dashed: ID2D1StrokeStyle,
}

impl Gfx {
    pub fn new() -> Result<Rc<Self>> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let round = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let dashed = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_FLAT,
                    endCap: D2D1_CAP_STYLE_FLAT,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    dashStyle: D2D1_DASH_STYLE_CUSTOM,
                    dashOffset: 0.0,
                },
                Some(&[3.0, 3.0]),
            )?;
            Ok(Rc::new(Gfx { d2d, dw, round, dashed }))
        }
    }
}

#[derive(Clone, Copy)]
struct Color(u32, f32);

impl Color {
    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: ((self.0 >> 16) & 0xff) as f32 / 255.0,
            g: ((self.0 >> 8) & 0xff) as f32 / 255.0,
            b: (self.0 & 0xff) as f32 / 255.0,
            a: self.1,
        }
    }
}

struct Palette {
    bg: Color,
    fg: Color,
    fg2: Color,
    fg3: Color,
    card: Color,
    seg_on: Color,
    track: Color,
    sep: Color,
    accent: Color,
}

impl Palette {
    fn new(light: bool) -> Self {
        if light {
            Palette {
                bg: Color(0xF9F9F9, 1.0),
                fg: Color(0x1A1A1A, 1.0),
                fg2: Color(0x000000, 0.62),
                fg3: Color(0x000000, 0.45),
                card: Color(0x000000, 0.04),
                seg_on: Color(0xFFFFFF, 1.0),
                track: Color(0x000000, 0.10),
                sep: Color(0x000000, 0.08),
                accent: Color(0x005FB8, 1.0),
            }
        } else {
            Palette {
                bg: Color(0x2B2B2B, 1.0),
                fg: Color(0xFFFFFF, 1.0),
                fg2: Color(0xFFFFFF, 0.66),
                fg3: Color(0xFFFFFF, 0.42),
                card: Color(0xFFFFFF, 0.05),
                seg_on: Color(0xFFFFFF, 0.10),
                track: Color(0xFFFFFF, 0.12),
                sep: Color(0xFFFFFF, 0.08),
                accent: Color(0x60CDFF, 1.0),
            }
        }
    }
}

struct Inner {
    kind: Kind,
    hwnd: HWND,
    gfx: Rc<Gfx>,
    rt: Option<(ID2D1HwndRenderTarget, ID2D1SolidColorBrush)>,
    formats: HashMap<(u32, i32, i32), IDWriteTextFormat>,
    entries: Vec<(ProviderId, View)>,
    /// The provider the tray icon shows.
    active: ProviderId,
    /// The panel column whose detail is shown, if the user picked one.
    selected: Option<ProviderId>,
    light: bool,
    dpi: u32,
    tab: Detail,
    visible: bool,
    hidden_at: Option<Instant>,
    /// Physical-pixel window rect while visible.
    rect: RECT,
    anchor: RECT,
}

/// One provider in full, or a column per signed-in provider (entry indices).
enum Layout {
    Single(usize),
    Columns(Vec<usize>),
}

fn layout(entries: &[(ProviderId, View)], active: ProviderId) -> Layout {
    let active_i = entries.iter().position(|(id, _)| *id == active).unwrap_or(0);
    // A device code needs the whole card.
    if matches!(entries.get(active_i), Some((_, View::SigningIn { code: Some(_) }))) {
        return Layout::Single(active_i);
    }
    let shown: Vec<usize> = (0..entries.len()).filter(|&i| !matches!(entries[i].1, View::SignedOut(_))).collect();
    match shown.len() {
        0 => Layout::Single(active_i),
        1 => Layout::Single(shown[0]),
        _ => Layout::Columns(shown),
    }
}

fn size(kind: Kind, layout: &Layout) -> (f32, f32) {
    match (kind, layout) {
        (Kind::Glance, Layout::Single(_)) => (GLANCE_W, GLANCE_H),
        (Kind::Panel, Layout::Single(_)) => (PANEL_W, PANEL_H),
        (Kind::Glance, Layout::Columns(c)) => (2.0 * PAD + c.len() as f32 * COL_W, COLS_GLANCE_H),
        (Kind::Panel, Layout::Columns(c)) => ((2.0 * PAD + c.len() as f32 * COL_W).max(PANEL_W), COLS_PANEL_H),
    }
}

/// The column whose detail the panel shows.
fn selected_column(entries: &[(ProviderId, View)], cols: &[usize], selected: Option<ProviderId>, active: ProviderId) -> usize {
    let find = |id: Option<ProviderId>| cols.iter().copied().find(|&i| Some(entries[i].0) == id);
    find(selected).or(find(Some(active))).unwrap_or(cols[0])
}

pub struct Popup {
    inner: Rc<RefCell<Inner>>,
    hwnd: HWND,
}

impl Popup {
    pub fn new(kind: Kind, gfx: Rc<Gfx>) -> Result<Self> {
        unsafe {
            let hinstance = GetModuleHandleW(None)?;
            let class = w!("tokenkeeper-popup");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            };
            // Fails harmlessly with "already registered" for the second popup.
            RegisterClassExW(&wc);

            let mut ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
            if kind == Kind::Glance {
                ex |= WS_EX_NOACTIVATE;
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(ex.0),
                class,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?;
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&pref as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                std::mem::size_of_val(&pref) as u32,
            );

            let inner = Rc::new(RefCell::new(Inner {
                kind,
                hwnd,
                gfx,
                rt: None,
                formats: HashMap::new(),
                entries: Vec::new(),
                active: ProviderId::Cursor,
                selected: None,
                light: false,
                dpi: 96,
                tab: Detail::Cycle,
                visible: false,
                hidden_at: None,
                rect: RECT::default(),
                anchor: RECT::default(),
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&inner) as isize);
            Ok(Popup { inner, hwnd })
        }
    }

    pub fn set_data(&self, entries: Vec<(ProviderId, View)>, active: ProviderId, light: bool) {
        let resize = {
            let mut inner = self.inner.borrow_mut();
            let before = size(inner.kind, &layout(&inner.entries, inner.active));
            inner.entries = entries;
            inner.active = active;
            inner.light = light;
            inner.visible && size(inner.kind, &layout(&inner.entries, inner.active)) != before
        };
        if resize {
            let anchor = self.inner.borrow().anchor;
            self.show(anchor);
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn is_visible(&self) -> bool {
        self.inner.borrow().visible
    }

    /// True if the popup hid itself within `window`, e.g. because the click
    /// that is now being handled deactivated it.
    pub fn hidden_within(&self, window: Duration) -> bool {
        self.inner.borrow().hidden_at.is_some_and(|t| t.elapsed() < window)
    }

    pub fn contains(&self, p: POINT) -> bool {
        let inner = self.inner.borrow();
        let r = inner.rect;
        inner.visible && p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
    }

    /// Shows the popup next to the tray icon at `anchor` (physical pixels).
    pub fn show(&self, anchor: RECT) {
        let (kind, (w_dip, h_dip)) = {
            let i = self.inner.borrow();
            (i.kind, size(i.kind, &layout(&i.entries, i.active)))
        };
        let (rect, dpi) = place(anchor, w_dip, h_dip);
        {
            let mut inner = self.inner.borrow_mut();
            inner.dpi = dpi;
            inner.rect = rect;
            inner.anchor = anchor;
            inner.visible = true;
            if let Some((rt, _)) = &inner.rt {
                unsafe {
                    rt.SetDpi(dpi as f32, dpi as f32);
                    let _ = rt.Resize(&D2D_SIZE_U {
                        width: (rect.right - rect.left) as u32,
                        height: (rect.bottom - rect.top) as u32,
                    });
                }
            }
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            if kind == Kind::Panel {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub fn hide(&self) {
        hide_window(&self.inner, self.hwnd);
    }
}

fn hide_window(inner: &RefCell<Inner>, hwnd: HWND) {
    if let Ok(mut i) = inner.try_borrow_mut() {
        if !i.visible {
            return;
        }
        i.visible = false;
        i.hidden_at = Some(Instant::now());
        i.rt = None;
        i.selected = None;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Picks a rect for a `w`x`h` DIP popup next to the tray icon, on whichever
/// edge the taskbar occupies, clamped to the monitor's work area.
fn place(anchor: RECT, w: f32, h: f32) -> (RECT, u32) {
    let centre = POINT { x: (anchor.left + anchor.right) / 2, y: (anchor.top + anchor.bottom) / 2 };
    unsafe {
        let mon = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
        let gap = (EDGE_GAP * scale).round() as i32;
        let work = info.rcWork;

        let (x, y) = if anchor.top >= work.bottom {
            (centre.x - pw / 2, work.bottom - gap - ph)
        } else if anchor.bottom <= work.top {
            (centre.x - pw / 2, work.top + gap)
        } else if anchor.left >= work.right {
            (work.right - gap - pw, centre.y - ph / 2)
        } else if anchor.right <= work.left {
            (work.left + gap, centre.y - ph / 2)
        } else {
            // Icon is inside the work area, e.g. in the overflow flyout.
            (centre.x - pw / 2, anchor.top - gap - ph)
        };
        let x = x.clamp(work.left + gap, (work.right - gap - pw).max(work.left));
        let y = y.clamp(work.top + gap, (work.bottom - gap - ph).max(work.top));
        (RECT { left: x, top: y, right: x + pw, bottom: y + ph }, dx)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<Inner>;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer comes from an Rc owned by the Popup, which lives as
    // long as the window (the app never destroys popups before exit).
    let cell = unsafe { &*ptr };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            if let Ok(mut inner) = cell.try_borrow_mut() {
                if let Err(e) = inner.paint() {
                    // Device loss: drop the target and repaint from scratch.
                    let _ = e;
                    inner.rt = None;
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE if cell.borrow().kind == Kind::Glance => LRESULT(MA_NOACTIVATE as isize),
        WM_ACTIVATE => {
            if (wparam.0 & 0xffff) as u32 == WA_INACTIVE && cell.borrow().kind == Kind::Panel {
                hide_window(cell, hwnd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            hide_window(cell, hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xffff) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
            let action = cell.try_borrow_mut().map(|mut i| i.click(x, y)).unwrap_or(Click::None);
            match action {
                Click::None => {}
                Click::Redraw => unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                },
                Click::OpenUsagePage(provider) => {
                    hide_window(cell, hwnd);
                    crate::open_url(provider.usage_url());
                }
                Click::CopyCode(code) => {
                    crate::clipboard::set_text(hwnd, &code);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xffff) as usize;
            unsafe {
                let _ = PostThreadMessageW(GetCurrentThreadId(), crate::WM_WHEEL_SWITCH, WPARAM(delta), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut p = POINT::default();
            let over_link = unsafe { GetCursorPos(&mut p).is_ok() && ScreenToClient(hwnd, &mut p).as_bool() }
                && cell.try_borrow().is_ok_and(|i| i.hot(p.x as f32, p.y as f32));
            if over_link {
                unsafe {
                    if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                        SetCursor(Some(hand));
                    }
                }
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

impl Inner {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// What a click at DIP coords would do, without the tab-switch side effect.
    fn hit(&self, x: f32, y: f32) -> Click {
        let lay = layout(&self.entries, self.active);
        let (w, _) = size(self.kind, &lay);
        match (&lay, self.kind) {
            (Layout::Single(i), _) => {
                let (id, view) = &self.entries[*i];
                if let View::SigningIn { code: Some(code) } = view {
                    return Click::CopyCode(code.clone());
                }
                if view.snapshot().is_some() && in_rect(x, y, link_rect(w)) {
                    return Click::OpenUsagePage(*id);
                }
            }
            (Layout::Columns(cols), Kind::Glance) => {
                if y < COLS_GLANCE_STRIP_H {
                    let i = (((x - PAD) / COL_W).max(0.0) as usize).min(cols.len() - 1);
                    return Click::OpenUsagePage(self.entries[cols[i]].0);
                }
            }
            (Layout::Columns(cols), Kind::Panel) => {
                let sel = selected_column(&self.entries, cols, self.selected, self.active);
                if y < COLS_STRIP_H {
                    let cw = (w - 2.0 * PAD) / cols.len() as f32;
                    let i = (((x - PAD) / cw).max(0.0) as usize).min(cols.len() - 1);
                    if in_rect(x, y, column_link_rect(PAD + i as f32 * cw, cw)) {
                        return Click::OpenUsagePage(self.entries[cols[i]].0);
                    }
                    return if cols[i] == sel { Click::None } else { Click::Redraw };
                }
            }
        }
        Click::None
    }

    /// True over something clickable, for the hand cursor (physical coords).
    fn hot(&self, x: f32, y: f32) -> bool {
        let (x, y) = (x / self.scale(), y / self.scale());
        matches!(self.hit(x, y), Click::OpenUsagePage(_) | Click::Redraw)
    }

    /// Handles a click at physical client coords.
    fn click(&mut self, x: f32, y: f32) -> Click {
        let (x, y) = (x / self.scale(), y / self.scale());
        let lay = layout(&self.entries, self.active);
        let (w, _) = size(self.kind, &lay);
        let action = self.hit(x, y);
        if let (Click::Redraw, Layout::Columns(cols)) = (&action, &lay) {
            let cw = (w - 2.0 * PAD) / cols.len() as f32;
            let i = (((x - PAD) / cw).max(0.0) as usize).min(cols.len() - 1);
            self.selected = Some(self.entries[cols[i]].0);
            return action;
        }
        if !matches!(action, Click::None) || self.kind != Kind::Panel {
            return action;
        }
        // Tabs of the provider whose detail is showing.
        let (shown, top) = match &lay {
            Layout::Single(i) => (*i, SINGLE_DETAIL_TOP),
            Layout::Columns(cols) => (selected_column(&self.entries, cols, self.selected, self.active), COLS_DETAIL_TOP),
        };
        let Some(s) = self.entries[shown].1.snapshot() else { return Click::None };
        let r = tabs_rect(top, w);
        if s.details.is_empty() || !in_rect(x, y, r) {
            return Click::None;
        }
        let n = s.details.len();
        let picked = s.details[(((x - r.0) / (r.2 / n as f32)) as usize).min(n - 1)];
        let changed = effective_tab(&s.details, self.tab) != picked;
        self.tab = picked;
        if changed { Click::Redraw } else { Click::None }
    }

    fn paint(&mut self) -> Result<()> {
        if self.rt.is_none() {
            let mut rc = RECT::default();
            unsafe { GetClientRect(self.hwnd, &mut rc)? };
            let rt = unsafe {
                self.gfx.d2d.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        // The hardware path loads the whole D3D/GPU driver stack
                        // (~50 MB) for two small, rarely redrawn cards.
                        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: D2D_SIZE_U {
                            width: (rc.right - rc.left).max(1) as u32,
                            height: (rc.bottom - rc.top).max(1) as u32,
                        },
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    },
                )?
            };
            unsafe { rt.SetDpi(self.dpi as f32, self.dpi as f32) };
            let brush = unsafe { rt.CreateSolidColorBrush(&Color(0, 1.0).d2d(), None)? };
            self.rt = Some((rt, brush));
        }
        let (rt, brush) = self.rt.clone().unwrap();
        let pal = Palette::new(self.light);
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&pal.bg.d2d()));
        }
        let entries = self.entries.clone();
        let mut p = Painter { gfx: &self.gfx, rt: &rt, brush: &brush, formats: &mut self.formats };
        draw(&mut p, &pal, &entries, self.active, self.selected, self.kind, self.tab);
        unsafe { rt.EndDraw(None, None) }
    }
}

fn draw(
    p: &mut Painter,
    pal: &Palette,
    entries: &[(ProviderId, View)],
    active: ProviderId,
    selected: Option<ProviderId>,
    kind: Kind,
    tab: Detail,
) {
    if entries.is_empty() {
        return;
    }
    let lay = layout(entries, active);
    let (w, _) = size(kind, &lay);
    match &lay {
        Layout::Single(i) => draw_single(p, pal, entries[*i].0, &entries[*i].1, kind, tab),
        Layout::Columns(cols) => {
            let cw = (w - 2.0 * PAD) / cols.len() as f32;
            let sel = selected_column(entries, cols, selected, active);
            for (n, &i) in cols.iter().enumerate() {
                let x = PAD + n as f32 * cw;
                let (id, view) = &entries[i];
                let highlight = kind == Kind::Panel && i == sel;
                draw_column(p, pal, x, cw, *id, view, kind, highlight, *id == active);
                if n > 0 && kind == Kind::Glance {
                    p.line(x, 16.0, x, COLS_GLANCE_STRIP_H - 14.0, 1.0, pal.sep, false);
                }
            }
            let total_y = if kind == Kind::Glance { COLS_GLANCE_STRIP_H } else { COLS_STRIP_H + 4.0 };
            draw_total(p, pal, entries, cols, total_y, w);
            match kind {
                Kind::Glance => draw_columns_footer(p, pal, entries, cols, w),
                Kind::Panel => {
                    let (_, view) = &entries[sel];
                    match view {
                        View::Ready { snap, error } => {
                            draw_detail(p, pal, snap, error.as_deref(), effective_tab(&snap.details, tab), COLS_DETAIL_TOP, w)
                        }
                        _ => p.message("No details yet", "Usage appears here once it has loaded", COLS_DETAIL_TOP + 60.0, w, pal),
                    }
                }
            }
        }
    }
}

fn draw_single(p: &mut Painter, pal: &Palette, provider: ProviderId, view: &View, kind: Kind, tab: Detail) {
    let (w, h) = match kind {
        Kind::Glance => (GLANCE_W, GLANCE_H),
        Kind::Panel => (PANEL_W, PANEL_H),
    };
    p.header(provider, view.snapshot().map(|s| s.plan.as_str()), w, pal);
    let site = provider.sign_in_site();
    let (title, sub) = match view {
        View::Ready { snap, error } => {
            match kind {
                Kind::Glance => draw_glance(p, pal, snap, error.as_deref()),
                Kind::Panel => draw_panel(p, pal, snap, error.as_deref(), effective_tab(&snap.details, tab)),
            }
            return;
        }
        View::Loading => ("Loading usage…".to_string(), format!("Contacting {site}")),
        View::SignedOut(None) => ("Not signed in".into(), "Click the tray icon to sign in with your browser".into()),
        View::SignedOut(Some(e)) => ("Sign-in failed".into(), truncate(e, 48)),
        View::SigningIn { code: Some(code) } => {
            let y = h / 2.0 - 44.0;
            p.text("Enter this code on the GitHub page", PAD, y, w - 2.0 * PAD, 18.0, 12.0, false, Align::Centre, pal.fg2);
            p.round_rect(PAD + 30.0, y + 24.0, w - 2.0 * PAD - 60.0, 44.0, 8.0, pal.card);
            p.text(code, PAD, y + 24.0, w - 2.0 * PAD, 44.0, 26.0, true, Align::Centre, pal.fg);
            p.text("Copied to clipboard · click to copy again", PAD, y + 74.0, w - 2.0 * PAD, 18.0, 11.0, false, Align::Centre, pal.fg3);
            return;
        }
        View::SigningIn { code: None } => ("Waiting for browser sign-in…".into(), format!("Finish signing in on {site}")),
        View::Failed(e) => ("Couldn't load usage".into(), truncate(e, 48)),
    };
    p.message(&title, &sub, h / 2.0 - 22.0, w, pal);
}

/// Dev tool: renders a popup offscreen and returns (width, height, RGBA8).
/// `tab` indexes the shown snapshot's detail tabs for the panel.
pub fn render_offscreen(
    gfx: &Gfx,
    kind: Kind,
    tab: usize,
    entries: &[(ProviderId, View)],
    active: ProviderId,
    light: bool,
    dpi: f32,
) -> Result<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapCacheOnLoad,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    let lay = layout(entries, active);
    let (w, h) = size(kind, &lay);
    let shown = match &lay {
        Layout::Single(i) => *i,
        Layout::Columns(cols) => selected_column(entries, cols, None, active),
    };
    let (pw, ph) = ((w * dpi / 96.0).round() as u32, (h * dpi / 96.0).round() as u32);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(pw, ph, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        let rt = gfx.d2d.CreateWicBitmapRenderTarget(
            &bmp,
            &D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi,
                dpiY: dpi,
                ..Default::default()
            },
        )?;
        let brush = rt.CreateSolidColorBrush(&Color(0, 1.0).d2d(), None)?;
        let pal = Palette::new(light);
        let mut formats = HashMap::new();
        rt.BeginDraw();
        rt.Clear(Some(&pal.bg.d2d()));
        let mut p = Painter { gfx, rt: &rt, brush: &brush, formats: &mut formats };
        let details = entries[shown].1.snapshot().map(|s| s.details.clone()).unwrap_or_default();
        let tab = details.get(tab).copied().unwrap_or(Detail::Cycle);
        draw(&mut p, &pal, entries, active, None, kind, tab);
        rt.EndDraw(None, None)?;
        let mut buf = vec![0u8; (pw * ph * 4) as usize];
        bmp.CopyPixels(std::ptr::null(), pw * 4, &mut buf)?;
        for px in buf.chunks_mut(4) {
            px.swap(0, 2);
        }
        Ok((pw, ph, buf))
    }
}

struct Painter<'a> {
    gfx: &'a Gfx,
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    formats: &'a mut HashMap<(u32, i32, i32), IDWriteTextFormat>,
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Centre,
    Right,
}

impl Painter<'_> {
    fn color(&self, c: Color) {
        unsafe { self.brush.SetColor(&c.d2d()) };
    }

    fn round_rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        self.color(c);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
            radiusX: r,
            radiusY: r,
        };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.brush) };
    }

    fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Color, dashed: bool) {
        self.color(c);
        let style = if dashed { &self.gfx.dashed } else { &self.gfx.round };
        unsafe {
            self.rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, self.brush, width, style)
        };
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, bold: bool, align: Align, c: Color) {
        let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        let al = match align {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Centre => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let Some(fmt) = self.format(size, weight, al) else { return };
        self.color(c);
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(
                &wide,
                &fmt,
                &D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h },
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    fn format(&mut self, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Option<IDWriteTextFormat> {
        let key = (size.to_bits(), weight.0, align.0);
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        let fmt = unsafe {
            let f = self
                .gfx
                .dw
                .CreateTextFormat(
                    w!("Segoe UI Variable Text"),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-gb"),
                )
                .ok()?;
            let _ = f.SetTextAlignment(align);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            f
        };
        self.formats.insert(key, fmt.clone());
        Some(fmt)
    }

    /// Progress ring centred at (cx, cy), filled clockwise from 12 o'clock,
    /// with a tick at the elapsed fraction of the billing cycle.
    #[allow(clippy::too_many_arguments)]
    fn ring(&mut self, cx: f32, cy: f32, r: f32, stroke: f32, used: f32, elapsed: f32, pal: &Palette) {
        self.color(pal.track);
        let ellipse = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&ellipse, self.brush, stroke, None) };

        let col = Color(level_rgb(used), 1.0);
        let frac = used.clamp(0.0, 1.0);
        if frac >= 0.999 {
            self.color(col);
            unsafe { self.rt.DrawEllipse(&ellipse, self.brush, stroke, None) };
        } else if frac > 0.002 {
            let a = frac * std::f32::consts::TAU;
            let _ = (|| -> Result<()> {
                unsafe {
                    let geo = self.gfx.d2d.CreatePathGeometry()?;
                    let sink = geo.Open()?;
                    sink.BeginFigure(Vector2 { X: cx, Y: cy - r }, D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: Vector2 { X: cx + r * a.sin(), Y: cy - r * a.cos() },
                        size: D2D_SIZE_F { width: r, height: r },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                        arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                    });
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    sink.Close()?;
                    self.color(col);
                    self.rt.DrawGeometry(&geo, self.brush, stroke, &self.gfx.round);
                }
                Ok(())
            })();
        }

        let t = elapsed.clamp(0.0, 1.0) * std::f32::consts::TAU;
        let (r0, r1) = (r - stroke / 2.0 - 2.0, r + stroke / 2.0 + 1.0);
        self.line(cx + r0 * t.sin(), cy - r0 * t.cos(), cx + r1 * t.sin(), cy - r1 * t.cos(), 2.0, pal.fg, false);
        let pct = format!("{}%", (used * 100.0).round());
        self.text(&pct, cx - r, cy - r / 2.0, r * 2.0, r, r * 0.45, true, Align::Centre, pal.fg);
    }

    fn ring_track(&mut self, cx: f32, cy: f32, r: f32, stroke: f32, pal: &Palette) {
        self.color(pal.track);
        let ellipse = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&ellipse, self.brush, stroke, None) };
    }

    /// Bar chart. `line` draws a dashed reference (budget or limit).
    #[allow(clippy::too_many_arguments)]
    fn bars(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        values: &[Option<u32>],
        labels: &[String],
        label_every: usize,
        line: Option<u32>,
        color_for: impl Fn(usize, u32) -> Color,
        pal: &Palette,
    ) {
        let label_h = 14.0;
        let chart_h = h - label_h;
        let n = values.len().max(1) as f32;
        let gap = if n > 12.0 { 2.0 } else { 6.0 };
        let bw = (w - gap * (n - 1.0)) / n;
        let max = values
            .iter()
            .flatten()
            .copied()
            .chain(line)
            .max()
            .unwrap_or(1)
            .max(1) as f32
            * 1.1;
        let last = values.iter().rposition(|v| v.is_some());
        for (i, v) in values.iter().enumerate() {
            let bx = x + i as f32 * (bw + gap);
            match v {
                None => self.round_rect(bx, y + chart_h - 2.0, bw, 2.0, 1.0, pal.track),
                Some(v) => {
                    let bh = (*v as f32 / max * chart_h).max(2.0);
                    let mut c = color_for(i, *v);
                    if Some(i) != last {
                        c.1 *= 0.7;
                    }
                    self.round_rect(bx, y + chart_h - bh, bw, bh, 1.5_f32.min(bw / 2.0), c);
                }
            }
            if i % label_every == 0 || i + 1 == values.len() {
                if let Some(l) = labels.get(i) {
                    let cx = bx + bw / 2.0;
                    self.text(l, cx - 24.0, y + chart_h + 1.0, 48.0, label_h, 9.5, false, Align::Centre, pal.fg3);
                }
            }
        }
        if let Some(l) = line {
            let ly = y + chart_h - l as f32 / max * chart_h;
            self.line(x, ly, x + w, ly, 1.0, pal.fg3, true);
        }
    }

    fn header(&mut self, provider: ProviderId, plan: Option<&str>, width: f32, pal: &Palette) {
        let logo = brand_rgb(provider).map_or(Color(pal.fg.0, 0.85), |c| Color(c, 1.0));
        self.round_rect(PAD, 14.0, 16.0, 16.0, 4.0, logo);
        self.text(provider.name(), PAD + 24.0, 12.0, 150.0, 20.0, 13.0, true, Align::Left, pal.fg);
        if let Some(plan) = plan {
            let bw = BADGE_W;
            let bx = width - PAD - bw;
            self.round_rect(bx, 13.0, bw, 18.0, 9.0, pal.card);
            self.text(plan, bx, 13.0, bw, 18.0, 10.5, false, Align::Centre, pal.fg2);
            let (lx, ly, lw, lh) = link_rect(width);
            self.text("Usage ↗", lx, ly, lw, lh, 11.5, false, Align::Right, pal.accent);
        }
    }

    fn footer(&mut self, s: &Snapshot, error: Option<&str>, y: f32, width: f32, pal: &Palette) {
        self.line(0.0, y, width, y, 1.0, pal.sep, false);
        self.text(&s.resets_text(), PAD, y + 4.0, width / 2.0, 24.0, 11.0, false, Align::Left, pal.fg3);
        let (right, color) = match error {
            Some(_) => ("⚠ Update failed, retrying".to_string(), Color(level_rgb(0.85), 1.0)),
            None => (format!("Updated {}", time::ago(s.fetched_ms)), pal.fg3),
        };
        self.text(&right, width / 2.0, y + 4.0, width / 2.0 - PAD, 24.0, 11.0, false, Align::Right, color);
    }

    /// One labelled bar per rate-limit window.
    fn meters(&mut self, x: f32, y: f32, w: f32, h: f32, s: &Snapshot, pal: &Palette) {
        let row_h = (h / s.meters.len().max(1) as f32).min(30.0);
        for (i, m) in s.meters.iter().take((h / 20.0) as usize).enumerate() {
            let ry = y + i as f32 * row_h;
            let pct = (m.fraction * 100.0).round();
            let right = match m.resets_ms {
                Some(r) => format!("{pct}% · resets {}", short_reset(r)),
                None => format!("{pct}%"),
            };
            self.text(&m.label, x, ry, w, 14.0, 11.0, false, Align::Left, pal.fg);
            self.text(&right, x, ry, w, 14.0, 11.0, false, Align::Right, pal.fg2);
            let bar_y = ry + 16.0;
            self.round_rect(x, bar_y, w, 4.0, 2.0, pal.track);
            let fw = w * m.fraction.clamp(0.0, 1.0);
            if fw > 0.5 {
                self.round_rect(x, bar_y, fw.max(4.0), 4.0, 2.0, Color(level_rgb(m.fraction), 1.0));
            }
        }
    }

    /// Centred two-line message, for states without usage data.
    fn message(&mut self, title: &str, sub: &str, y: f32, width: f32, pal: &Palette) {
        self.text(title, PAD, y, width - 2.0 * PAD, 24.0, 15.0, true, Align::Centre, pal.fg);
        self.text(sub, PAD, y + 26.0, width - 2.0 * PAD, 18.0, 11.5, false, Align::Centre, pal.fg2);
    }
}

/// "in 3 h" within a day, else "20 Oct".
fn short_reset(ms: i64) -> String {
    let left = ms - time::now_ms();
    if left < time::DAY_MS {
        let mins = (left.max(0) / 60_000) as u32;
        if mins < 60 { format!("in {mins} min") } else { format!("in {} h", (mins + 30) / 60) }
    } else {
        time::day_month(ms)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max - 1).chain(Some('…')).collect() }
}

fn draw_glance(p: &mut Painter, pal: &Palette, s: &Snapshot, error: Option<&str>) {
    let w = GLANCE_W;
    p.ring(PAD + 44.0, 44.0 + 44.0, 39.0, 10.0, s.used_fraction(), s.elapsed_fraction(), pal);
    let x = 118.0;
    let cw = w - x - PAD;
    p.text(&s.fmt(s.used), x, 46.0, cw, 26.0, 20.0, true, Align::Left, pal.fg);
    p.text(&s.of_text(), x, 72.0, cw, 16.0, 11.5, false, Align::Left, pal.fg3);
    let mut y = 96.0;
    let secondary = match (s.today, s.meters.first()) {
        (Some(today), _) => Some(format!("{} today", s.fmt(today))),
        (None, Some(m)) => Some(format!("{}: {}%", m.label, (m.fraction * 100.0).round())),
        _ => None,
    };
    if let Some(line) = secondary {
        p.text(&line, x, y, cw, 18.0, 13.0, false, Align::Left, pal.fg);
        y += 20.0;
    }
    p.text(&s.pace_text(), x, y, cw, 18.0, 12.0, false, Align::Left, pal.fg2);
    p.footer(s, error, 146.0, w, pal);
}

/// The header link that opens the Cursor usage page (per-model detail lives there).
fn link_rect(width: f32) -> (f32, f32, f32, f32) {
    let w = 64.0;
    (width - PAD - BADGE_W - 8.0 - w, 12.0, w, 20.0)
}

fn tabs_rect(top: f32, w: f32) -> (f32, f32, f32, f32) {
    (PAD, top, w - 2.0 * PAD, 30.0)
}

/// The panel's chart card sits 40 DIPs below the tabs and the footer 176 below.
fn footer_y(top: f32) -> f32 {
    top + 176.0
}

/// The usage-page link in a column's header.
fn column_link_rect(x: f32, cw: f32) -> (f32, f32, f32, f32) {
    (x + cw - 28.0, 10.0, 22.0, 24.0)
}

/// The short line under a column's amount.
fn column_lines(s: &Snapshot) -> (String, Option<String>) {
    let of = match s.unit {
        _ if s.limit == 0 => "no allowance".to_string(),
        Unit::Cents => format!("of {}", s.fmt(s.limit)),
        Unit::Basis if s.cycle_start_ms <= 0 => "of quota".into(),
        Unit::Basis => format!("of {}-day limit", s.cycle_days().round()),
    };
    let secondary = match (s.today, s.meters.first()) {
        (Some(today), _) => Some(format!("{} today", s.fmt(today))),
        (None, Some(m)) if m.label.starts_with("5-hour") => Some(format!("5h session {}%", (m.fraction * 100.0).round())),
        (None, Some(m)) => Some(format!("{} {}%", truncate(&m.label, 12), (m.fraction * 100.0).round())),
        _ => None,
    };
    (of, secondary)
}

#[allow(clippy::too_many_arguments)]
fn draw_column(p: &mut Painter, pal: &Palette, x: f32, cw: f32, id: ProviderId, view: &View, kind: Kind, highlight: bool, active: bool) {
    let panel = kind == Kind::Panel;
    if highlight {
        p.round_rect(x + 2.0, 6.0, cw - 4.0, COLS_STRIP_H - 6.0, 8.0, pal.card);
    }
    let logo = brand_rgb(id).map_or(Color(pal.fg.0, 0.85), |c| Color(c, 1.0));
    p.round_rect(x + 10.0, 16.0, 12.0, 12.0, 3.0, logo);
    p.text(id.short_name(), x + 28.0, 12.0, cw - 62.0, 20.0, 12.0, true, Align::Left, pal.fg);
    if active {
        // Marks the provider the tray icon is showing.
        p.round_rect(x + cw - 38.0, 19.0, 6.0, 6.0, 3.0, pal.accent);
    }
    let (lx, ly, lw, lh) = column_link_rect(x, cw);
    p.text("↗", lx, ly, lw, lh, 12.0, false, Align::Centre, pal.accent);
    let (r, stroke, cy) = if panel { (26.0, 7.0, 66.0) } else { (30.0, 8.0, 72.0) };
    let cx = x + cw / 2.0;
    let mut y = cy + r + 10.0;
    match view {
        View::Ready { snap: s, error } => {
            p.ring(cx, cy, r, stroke, s.used_fraction(), s.elapsed_fraction(), pal);
            p.text(&s.fmt(s.used), x, y, cw, 20.0, if panel { 14.0 } else { 15.0 }, true, Align::Centre, pal.fg);
            y += 20.0;
            let (of, secondary) = column_lines(s);
            p.text(&of, x, y, cw, 16.0, 10.5, false, Align::Centre, pal.fg3);
            y += 16.0;
            if let Some(line) = secondary {
                p.text(&line, x, y, cw, 16.0, 11.0, false, Align::Centre, pal.fg);
            }
            y += 18.0;
            if !panel {
                let pace = if s.cycle_start_ms > 0 {
                    format!("Pace ~{}%", (s.projected_fraction() * 100.0).round())
                } else { "Pace unavailable".into() };
                p.text(&pace, x, y, cw, 16.0, 10.5, false, Align::Centre, pal.fg2);
                y += 16.0;
                let reset = if s.cycle_end_ms > 0 { format!("Resets {}", short_reset(s.cycle_end_ms)) }
                    else { "Reset unknown".into() };
                let (text, color) = match error {
                    Some(_) => ("⚠ Update failed".to_string(), Color(level_rgb(0.85), 1.0)),
                    None => (reset, pal.fg3),
                };
                p.text(&text, x, y, cw, 16.0, 10.5, false, Align::Centre, color);
            }
        }
        other => {
            p.ring_track(cx, cy, r, stroke, pal);
            let (title, sub) = match other {
                View::Loading => ("Loading…".to_string(), String::new()),
                View::SigningIn { .. } => ("Signing in…".into(), "Finish in the browser".into()),
                View::Failed(e) => ("Couldn't load".into(), truncate(e, 20)),
                _ => (String::new(), String::new()),
            };
            p.text(&title, x, y, cw, 20.0, 12.0, true, Align::Centre, pal.fg2);
            p.text(&sub, x + 4.0, y + 20.0, cw - 8.0, 16.0, 10.5, false, Align::Centre, pal.fg3);
        }
    }
}

/// One bar for the combined dollar usage of the columns, a segment per provider.
fn draw_total(p: &mut Painter, pal: &Palette, entries: &[(ProviderId, View)], cols: &[usize], y: f32, w: f32) {
    let snaps: Vec<&Snapshot> = cols.iter().filter_map(|&i| entries[i].1.snapshot()).filter(|s| s.unit == Unit::Cents).collect();
    let used: u32 = snaps.iter().map(|s| s.used).sum();
    let limit: u32 = snaps.iter().map(|s| s.limit).sum();
    let (x, bw) = (PAD, w - 2.0 * PAD);
    // Name the providers counted when some columns can't be summed (rate-limit percentages, loading).
    let label = if snaps.len() == cols.len() {
        "All tools".to_string()
    } else {
        snaps.iter().map(|s| s.provider.short_name()).collect::<Vec<_>>().join(" + ")
    };
    let fraction = if limit == 0 { 0.0 } else { used as f32 / limit as f32 };
    let (right, color) = match (snaps.is_empty(), limit) {
        (true, _) => (String::new(), pal.fg3),
        (false, 0) => (format!("{} used", dollars(used)), pal.fg2),
        _ => {
            let text = format!("{} of {} · {}%", dollars(used), dollars(limit), (fraction * 100.0).round());
            (text, if fraction >= 0.8 { Color(level_rgb(fraction), 1.0) } else { pal.fg2 })
        }
    };
    p.text(&label, x, y, bw, 16.0, 12.0, true, Align::Left, pal.fg);
    p.text(&right, x, y, bw, 16.0, 11.0, false, Align::Right, color);
    let (by, bh) = (y + 21.0, 6.0);
    p.round_rect(x, by, bw, bh, bh / 2.0, pal.track);
    let scale = bw / limit.max(used).max(1) as f32;
    let mut sx = x;
    for s in &snaps {
        let sw = s.used as f32 * scale;
        if sw > 0.5 {
            let c = brand_rgb(s.provider).map_or(Color(pal.fg.0, 0.85), |c| Color(c, 1.0));
            // A hairline gap keeps adjacent segments distinct.
            p.round_rect(sx, by, (sw - 1.5).max(3.0), bh, bh / 2.0, c);
        }
        sx += sw;
    }
    if used > limit && limit > 0 {
        let lx = x + limit as f32 * scale;
        p.line(lx, by - 3.0, lx, by + bh + 3.0, 1.5, pal.fg, false);
    }
}

fn draw_columns_footer(p: &mut Painter, pal: &Palette, entries: &[(ProviderId, View)], cols: &[usize], w: f32) {
    let y = COLS_GLANCE_FOOTER;
    p.line(0.0, y, w, y, 1.0, pal.sep, false);
    let oldest = cols.iter().filter_map(|&i| entries[i].1.snapshot().map(|s| s.fetched_ms)).min();
    if let Some(t) = oldest {
        p.text(&format!("Updated {}", time::ago(t)), PAD, y + 4.0, w / 2.0, 24.0, 11.0, false, Align::Left, pal.fg3);
    }
    p.text("Click for usage page", w / 2.0, y + 4.0, w / 2.0 - PAD, 24.0, 11.0, false, Align::Right, pal.fg3);
}

fn draw_panel(p: &mut Painter, pal: &Palette, s: &Snapshot, error: Option<&str>, tab: Detail) {
    let w = PANEL_W;
    p.ring(PAD + 32.0, 44.0 + 32.0, 28.0, 8.0, s.used_fraction(), s.elapsed_fraction(), pal);
    let x = 92.0;
    let cw = w - x - PAD;
    p.text(&s.fmt(s.used), x, 42.0, cw, 24.0, 18.0, true, Align::Left, pal.fg);
    let mut sub = s.of_text();
    if let Some(today) = s.today {
        sub += &format!(" · {} today", s.fmt(today));
    }
    p.text(&sub, x, 66.0, cw, 16.0, 11.5, false, Align::Left, pal.fg3);
    p.text(&s.pace_text(), x, 86.0, cw, 18.0, 12.0, false, Align::Left, pal.fg2);
    draw_detail(p, pal, s, error, tab, SINGLE_DETAIL_TOP, w);
}

/// Tabs, chart card and footer for one provider, starting at `top`.
#[allow(clippy::too_many_arguments)]
fn draw_detail(p: &mut Painter, pal: &Palette, s: &Snapshot, error: Option<&str>, tab: Detail, top: f32, w: f32) {
    let (tx, ty, tw, th) = tabs_rect(top, w);
    p.round_rect(tx, ty, tw, th, 6.0, pal.card);
    let sw = (tw - 4.0) / s.details.len().max(1) as f32;
    for (i, t) in s.details.iter().enumerate() {
        let sx = tx + 2.0 + i as f32 * sw;
        let on = *t == tab;
        if on {
            p.round_rect(sx, ty + 2.0, sw, th - 4.0, 4.0, pal.seg_on);
        }
        p.text(t.label(), sx, ty, sw, th, 12.0, on, Align::Centre, if on { pal.fg } else { pal.fg2 });
    }

    let (cx, cy, cw, ch) = (PAD, top + 40.0, w - 2.0 * PAD, 124.0);
    p.round_rect(cx, cy, cw, ch, 6.0, pal.card);
    let (ix, iw) = (cx + 10.0, cw - 20.0);
    let limit = s.limit;
    let budget = (limit as f32 / s.cycle_days()) as u32;
    let totals = if s.cycle_days() > 20.0 { "Monthly totals" } else { "Weekly totals" };
    let (title, right) = match tab {
        Detail::Today => ("Today by hour", s.today.map(|t| format!("{} so far", s.fmt(t))).unwrap_or_default()),
        Detail::Cycle => ("Daily this cycle", format!("budget {}/day", s.fmt(budget))),
        Detail::History => (totals, format!("limit {}", s.fmt(limit))),
        Detail::Limits => ("Rate limits", String::new()),
    };
    p.text(title, ix, cy + 6.0, iw, 18.0, 12.0, true, Align::Left, pal.fg);
    p.text(&right, ix, cy + 6.0, iw, 18.0, 11.0, false, Align::Right, pal.fg2);
    let (bx, by, bw, bh) = (ix, cy + 30.0, iw, ch - 38.0);
    let empty = match tab {
        Detail::Today => s.hourly.is_empty(),
        Detail::Cycle => s.daily.is_empty(),
        Detail::History => s.history.is_empty(),
        Detail::Limits => s.meters.is_empty(),
    };
    if empty {
        p.text(s.empty_note, bx, by, bw, bh, 11.5, false, Align::Centre, pal.fg3);
    } else {
        let over = Color(level_rgb(1.0), 1.0);
        match tab {
            Detail::Limits => p.meters(bx, by, bw, bh, s, pal),
            Detail::Today => {
                let labels: Vec<String> = (0..24).map(|h| format!("{h}:00")).collect();
                p.bars(bx, by, bw, bh, &s.hourly, &labels, 6, None, |_, _| pal.accent, pal);
            }
            Detail::Cycle => {
                let cumulative: Vec<u32> = s
                    .daily
                    .iter()
                    .scan(0u32, |acc, v| {
                        *acc += v.unwrap_or(0);
                        Some(*acc)
                    })
                    .collect();
                p.bars(bx, by, bw, bh, &s.daily, &s.daily_labels, 5, Some(budget), |i, _| {
                    if cumulative[i] > limit { over } else { pal.accent }
                }, pal);
            }
            Detail::History => {
                let values: Vec<Option<u32>> = s.history.iter().map(|v| Some(*v)).collect();
                p.bars(bx, by, bw, bh, &values, &s.history_labels, 1, Some(limit), |_, v| {
                    if v > limit { over } else { pal.accent }
                }, pal);
            }
        }
    }
    p.footer(s, error, footer_y(top), w, pal);
}
