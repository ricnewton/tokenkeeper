//! X11 window-manager hints and tray-anchor placement for the Iced flyout.
//!
//! Wayland deliberately remains compositor controlled: global coordinates and
//! EWMH activation are X11 facilities, not portable window operations.

use iced::window::raw_window_handle::RawWindowHandle;
use std::error::Error;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux,
    EventMask, PropMode, StackMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Rect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let width = (self.x + self.width).min(other.x + other.width) - x;
        let height = (self.y + self.height).min(other.y + other.height) - y;
        (width > 0 && height > 0).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn distance_squared(self, x: i32, y: i32) -> i64 {
        let dx = i64::from(x) - i64::from(x.clamp(self.x, self.x + self.width - 1));
        let dy = i64::from(y) - i64::from(y.clamp(self.y, self.y + self.height - 1));
        dx * dx + dy * dy
    }
}

fn placement(work: Rect, width: i32, height: i32, anchor: Option<(i32, i32)>) -> (i32, i32) {
    let max_x = work.x + (work.width - width).max(0);
    let max_y = work.y + (work.height - height).max(0);
    let (x, y) = anchor
        .map(|(x, y)| (x - width, y - height - 12))
        .unwrap_or((max_x - 12, max_y - 12));
    (x.clamp(work.x, max_x), y.clamp(work.y, max_y))
}

/// Configure only this app's X11 window, after Iced reports it opened.
/// Native Wayland is a successful no-op; its activation token and placement
/// must be supplied by the compositor rather than fabricated from coordinates.
pub fn configure(
    window: &dyn iced::window::Window,
    anchor: Option<(i32, i32)>,
) -> Result<(), String> {
    let handle = window.window_handle().map_err(|error| error.to_string())?;
    let id = match handle.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)
            .map_err(|_| "Xlib window identifier does not fit X11 protocol".to_owned())?,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => return Ok(()),
    };
    configure_x11(id, anchor).map_err(|error| error.to_string())
}

fn configure_x11(id: u32, anchor: Option<(i32, i32)>) -> Result<(), Box<dyn Error>> {
    let (connection, _) = x11rb::connect(None)?;
    let geometry = connection.get_geometry(id)?.reply()?;
    let root = geometry.root;
    let root_geometry = connection.get_geometry(root)?.reply()?;
    let screen = Rect {
        x: 0,
        y: 0,
        width: i32::from(root_geometry.width),
        height: i32::from(root_geometry.height),
    };
    let atom = |name: &str| -> Result<Atom, Box<dyn Error>> {
        Ok(connection
            .intern_atom(false, name.as_bytes())?
            .reply()?
            .atom)
    };
    let desktop = read_cardinals(&connection, root, atom("_NET_CURRENT_DESKTOP")?)?
        .first()
        .copied()
        .unwrap_or(0) as usize;
    let workareas = read_cardinals(&connection, root, atom("_NET_WORKAREA")?)?;
    let work = desktop
        .checked_mul(4)
        .and_then(|offset| workareas.get(offset..offset + 4))
        .and_then(|values| {
            Some(Rect {
                x: values[0] as i32,
                y: values[1] as i32,
                width: i32::try_from(values[2]).ok()?,
                height: i32::try_from(values[3]).ok()?,
            })
        })
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .unwrap_or(screen);
    let monitors = connection
        .randr_get_monitors(root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.monitors)
        .unwrap_or_default();
    let monitor = anchor
        .and_then(|(x, y)| {
            monitors
                .iter()
                .filter(|monitor| monitor.width > 0 && monitor.height > 0)
                .min_by_key(|monitor| {
                    let rect = Rect {
                        x: i32::from(monitor.x),
                        y: i32::from(monitor.y),
                        width: i32::from(monitor.width),
                        height: i32::from(monitor.height),
                    };
                    (!rect.contains(x, y), rect.distance_squared(x, y))
                })
        })
        .or_else(|| monitors.iter().find(|monitor| monitor.primary))
        .or_else(|| monitors.first());
    let monitor = monitor
        .map(|monitor| Rect {
            x: i32::from(monitor.x),
            y: i32::from(monitor.y),
            width: i32::from(monitor.width),
            height: i32::from(monitor.height),
        })
        .unwrap_or(screen);
    let area = monitor.intersect(work).unwrap_or(monitor);
    // X11 geometry is already in physical pixels, including Iced's scale factor.
    let (x, y) = placement(
        area,
        i32::from(geometry.width),
        i32::from(geometry.height),
        anchor,
    );

    let state = atom("_NET_WM_STATE")?;
    let skip_taskbar = atom("_NET_WM_STATE_SKIP_TASKBAR")?;
    let skip_pager = atom("_NET_WM_STATE_SKIP_PAGER")?;
    let above = atom("_NET_WM_STATE_ABOVE")?;
    let mut states: Vec<u32> = connection
        .get_property(false, id, state, AtomEnum::ATOM, 0, 128)?
        .reply()?
        .value32()
        .map(|values| values.collect())
        .unwrap_or_default();
    for requested in [skip_taskbar, skip_pager, above] {
        if !states.contains(&requested) {
            states.push(requested);
        }
    }
    connection
        .change_property32(PropMode::REPLACE, id, state, AtomEnum::ATOM, &states)?
        .check()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            id,
            atom("_NET_WM_WINDOW_TYPE")?,
            AtomEnum::ATOM,
            &[atom("_NET_WM_WINDOW_TYPE_UTILITY")?],
        )?
        .check()?;
    // Properties establish initial hints; messages cover an already mapped window.
    send_message(
        &connection,
        root,
        id,
        state,
        [1, skip_taskbar, skip_pager, 1, 0],
    )?;
    send_message(&connection, root, id, state, [1, above, 0, 1, 0])?;
    connection
        .configure_window(
            id,
            &ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .stack_mode(StackMode::ABOVE),
        )?
        .check()?;
    send_message(
        &connection,
        root,
        id,
        atom("_NET_MOVERESIZE_WINDOW")?,
        [
            1 | (1 << 8) | (1 << 9) | (1 << 12),
            x as u32,
            y as u32,
            0,
            0,
        ],
    )?;
    // SNI activation carries no input timestamp. Ask this server for a fresh
    // property-event time, as the GTK implementation did, without setting the
    // input focus directly or activating any unrelated application window.
    let timestamp = server_timestamp(&connection, root, atom("_TOKENKEEPER_ACTIVATION_TIME")?)?;
    connection
        .change_property32(
            PropMode::REPLACE,
            id,
            atom("_NET_WM_USER_TIME")?,
            AtomEnum::CARDINAL,
            &[timestamp],
        )?
        .check()?;
    send_message(
        &connection,
        root,
        id,
        atom("_NET_ACTIVE_WINDOW")?,
        [1, timestamp, 0, 0, 0],
    )?;
    connection.flush()?;
    Ok(())
}

fn read_cardinals(
    connection: &impl Connection,
    window: u32,
    atom: Atom,
) -> Result<Vec<u32>, Box<dyn Error>> {
    let reply = connection
        .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 4096)?
        .reply()?;
    Ok(reply
        .value32()
        .map(|values| values.collect())
        .unwrap_or_default())
}

fn send_message(
    connection: &impl Connection,
    root: u32,
    window: u32,
    atom: Atom,
    data: [u32; 5],
) -> Result<(), Box<dyn Error>> {
    connection
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ClientMessageEvent::new(32, window, atom, data),
        )?
        .check()?;
    Ok(())
}

fn server_timestamp(
    connection: &impl Connection,
    root: u32,
    atom: Atom,
) -> Result<u32, Box<dyn Error>> {
    let helper = connection.generate_id()?;
    connection
        .create_window(
            0,
            helper,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?
        .check()?;
    let result = (|| -> Result<u32, Box<dyn Error>> {
        connection
            .change_property32(PropMode::REPLACE, helper, atom, AtomEnum::CARDINAL, &[0])?
            .check()?;
        connection.flush()?;
        let deadline = Instant::now() + Duration::from_millis(150);
        loop {
            if let Some(Event::PropertyNotify(event)) = connection.poll_for_event()? {
                if event.window == helper && event.atom == atom {
                    return Ok(event.time);
                }
            }
            if Instant::now() >= deadline {
                return Err("X11 activation timestamp timed out".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    })();
    connection.destroy_window(helper)?.check()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_popup_clamps_to_each_edge_and_supports_negative_monitors() {
        let work = Rect {
            x: -1920,
            y: 40,
            width: 1920,
            height: 1040,
        };
        assert_eq!(placement(work, 440, 606, Some((-10, 1080))), (-450, 462));
        assert_eq!(placement(work, 440, 606, Some((-1900, 20))), (-1920, 40));
        assert_eq!(placement(work, 440, 606, Some((100, 2000))), (-440, 474));
    }

    #[test]
    fn fallback_and_scaled_window_use_actual_physical_size() {
        let work = Rect {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1400,
        };
        assert_eq!(placement(work, 880, 1212, None), (3588, 176));
        assert_eq!(placement(work, 880, 1212, Some((4470, 1400))), (3590, 176));
        assert_eq!(placement(work, 3000, 2000, None), (1920, 0));
    }

    #[test]
    fn workarea_intersection_respects_panel_space() {
        let monitor = Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let work = Rect {
            x: -1920,
            y: 0,
            width: 3840,
            height: 1040,
        };
        assert_eq!(
            monitor.intersect(work),
            Some(Rect {
                height: 1040,
                ..monitor
            })
        );
        assert!(monitor.contains(-1, 1079));
        assert!(!monitor.contains(0, 1079));
        assert_eq!(monitor.distance_squared(1, 100), 4);
    }
}
