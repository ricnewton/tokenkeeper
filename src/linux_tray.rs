//! KDE StatusNotifierItem and native D-Bus menus implemented in Rust.
//! D-Bus callbacks enqueue actions; only the GTK main thread touches windows.
use ksni::blocking::TrayMethods;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    Panel,
    Refresh,
    Quit,
    Autostart,
    Show(usize),
    Sign(usize),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Activate(i32, i32),
    Scroll(i32),
    Menu(MenuAction),
}
#[derive(Clone)]
pub enum MenuEntry {
    Item {
        label: String,
        enabled: bool,
        action: MenuAction,
    },
    Submenu {
        label: String,
        children: Vec<MenuEntry>,
    },
    Separator,
}
impl MenuEntry {
    pub fn item(label: impl Into<String>, action: MenuAction) -> Self {
        Self::Item {
            label: label.into(),
            enabled: true,
            action,
        }
    }
}
struct Tray {
    events: Sender<Action>,
    icons: Vec<ksni::Icon>,
    title: String,
    summary: String,
    menu: Vec<MenuEntry>,
}
fn menu_items(entries: &[MenuEntry]) -> Vec<ksni::MenuItem<Tray>> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item {
                label,
                enabled,
                action,
            } => {
                let action = *action;
                ksni::menu::StandardItem {
                    label: label.clone(),
                    enabled: *enabled,
                    activate: Box::new(move |tray: &mut Tray| {
                        let _ = tray.events.send(Action::Menu(action));
                    }),
                    ..Default::default()
                }
                .into()
            }
            MenuEntry::Submenu { label, children } => ksni::menu::SubMenu {
                label: label.clone(),
                submenu: menu_items(children),
                ..Default::default()
            }
            .into(),
            MenuEntry::Separator => ksni::MenuItem::Separator,
        })
        .collect()
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "tokenkeeper".into()
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title.clone(),
            description: self.summary.clone(),
            ..Default::default()
        }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        menu_items(&self.menu)
    }
    fn activate(&mut self, x: i32, y: i32) {
        let _ = self.events.send(Action::Activate(x, y));
    }
    fn secondary_activate(&mut self, x: i32, y: i32) {
        self.activate(x, y);
    }
    fn scroll(&mut self, delta: i32, _orientation: ksni::Orientation) {
        let _ = self.events.send(Action::Scroll(delta));
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        true
    }
}
struct State {
    handle: ksni::blocking::Handle<Tray>,
    events: Receiver<Action>,
    action: fn(Action),
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
pub fn start(action: fn(Action)) -> Result<(), String> {
    let (tx, events) = mpsc::channel();
    let handle = Tray {
        events: tx,
        icons: vec![],
        title: "TokenKeeper".into(),
        summary: "Sign in from Accounts in the tray menu".into(),
        menu: vec![],
    }
    .spawn()
    .map_err(|e| format!("Could not publish KDE tray item: {e}"))?;
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            handle,
            events,
            action,
        })
    });
    Ok(())
}
pub fn poll() {
    // Drop the state borrow before invoking app actions that can update this tray.
    let pending = STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|state| (state.action, state.events.try_iter().collect::<Vec<_>>()))
    });
    if let Some((callback, events)) = pending {
        for action in events {
            callback(action);
        }
    }
}
fn pixels(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| [p[3], p[0], p[1], p[2]])
        .collect()
}
pub fn update(usage: Option<(f32, f32)>, title: &str, summary: &str, menu: Vec<MenuEntry>) {
    STATE.with(|state| {
        if let Some(state) = state.borrow().as_ref() {
            state.handle.update(|tray| {
                tray.icons = [16, 22, 32, 48]
                    .into_iter()
                    .map(|size| ksni::Icon {
                        width: size as i32,
                        height: size as i32,
                        data: pixels(&crate::icon::render(size, usage, false)),
                    })
                    .collect();
                tray.title = title.into();
                tray.summary = summary.into();
                tray.menu = menu;
            });
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;
    #[test]
    fn icon_bytes_use_network_argb() {
        assert_eq!(pixels(&[1, 2, 3, 4]), vec![4, 1, 2, 3]);
    }
    #[test]
    fn menu_actions_are_queued_and_preserve_disabled_state() {
        let (events, received) = mpsc::channel();
        let mut tray = Tray {
            events,
            icons: vec![],
            title: "Worker's quota".into(),
            summary: "a\\b\nnext".into(),
            menu: vec![MenuEntry::Item {
                label: "OpenCode".into(),
                enabled: false,
                action: MenuAction::Sign(4),
            }],
        };
        assert_eq!(tray.tool_tip().description, "a\\b\nnext");
        let ksni::MenuItem::Standard(item) = tray.menu().remove(0) else {
            panic!("expected item")
        };
        assert!(!item.enabled);
        (item.activate)(&mut tray);
        assert_eq!(
            received.try_recv().unwrap(),
            Action::Menu(MenuAction::Sign(4))
        );
        tray.activate(100, 200);
        assert_eq!(received.try_recv().unwrap(), Action::Activate(100, 200));
    }
}
