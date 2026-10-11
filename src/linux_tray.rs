//! KDE StatusNotifierItem: real activation, pixel icons and native hover summaries.
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;
type P = *mut c_void;
const PATH: &str = "/StatusNotifierItem";
const MENU: &str = "/Menu";
const IFACE: &str = "org.kde.StatusNotifierItem";
#[derive(Clone, Copy)]
pub enum Action {
    Activate(i32, i32),
    ContextMenu,
    Scroll(i32),
}
#[repr(C)]
struct VTable {
    method: extern "C" fn(P, *const c_char, *const c_char, *const c_char, *const c_char, P, P, P),
    get: extern "C" fn(
        P,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *mut P,
        P,
    ) -> P,
    set: P,
    padding: [P; 8],
}
#[link(name = ":libgio-2.0.so.0")]
unsafe extern "C" {
    fn g_bus_get_sync(kind: c_int, cancel: P, error: *mut P) -> P;
    fn g_dbus_node_info_new_for_xml(xml: *const c_char, error: *mut P) -> P;
    fn g_dbus_node_info_lookup_interface(info: P, name: *const c_char) -> P;
    fn g_dbus_connection_register_object(
        conn: P,
        path: *const c_char,
        info: P,
        vtable: *const VTable,
        data: P,
        destroy: P,
        error: *mut P,
    ) -> u32;
    fn g_dbus_connection_call(
        conn: P,
        bus: *const c_char,
        path: *const c_char,
        iface: *const c_char,
        method: *const c_char,
        args: P,
        reply: P,
        flags: c_int,
        timeout: c_int,
        cancel: P,
        callback: P,
        data: P,
    );
    fn g_dbus_connection_emit_signal(
        conn: P,
        dest: *const c_char,
        path: *const c_char,
        iface: *const c_char,
        name: *const c_char,
        args: P,
        error: *mut P,
    ) -> c_int;
    fn g_dbus_method_invocation_return_value(invocation: P, args: P);
    fn g_bus_watch_name_on_connection(
        conn: P,
        name: *const c_char,
        flags: c_int,
        appeared: extern "C" fn(P, *const c_char, *const c_char, P),
        vanished: P,
        data: P,
        destroy: P,
    ) -> u32;
}
#[link(name = ":libglib-2.0.so.0")]
unsafe extern "C" {
    fn g_variant_parse(kind: P, text: *const c_char, limit: P, end: P, error: *mut P) -> P;
    fn g_variant_ref(v: P) -> P;
    fn g_variant_unref(v: P);
    fn g_variant_get_child_value(v: P, index: usize) -> P;
    fn g_variant_get_int32(v: P) -> i32;
    fn g_error_free(e: P);
}
#[link(name = ":libdbusmenu-gtk3.so.4")]
unsafe extern "C" {
    fn dbusmenu_gtk_parse_menu_structure(widget: P) -> P;
}
#[link(name = ":libdbusmenu-glib.so.4")]
unsafe extern "C" {
    fn dbusmenu_server_new(path: *const c_char) -> P;
    fn dbusmenu_server_set_root(server: P, root: P);
}
struct State {
    connection: P,
    properties: HashMap<String, P>,
    action: fn(Action),
    _server: P,
    _node: P,
    _registration: u32,
    _watch: u32,
}
thread_local! { static STATE:RefCell<Option<State>> = const {RefCell::new(None)}; }
fn c(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).unwrap()
}
fn quoted(s: &str) -> String {
    format!(
        "'{}'",
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\0', " ")
    )
}
fn variant(s: &str) -> P {
    let mut error = ptr::null_mut();
    let v = unsafe {
        g_variant_parse(
            ptr::null_mut(),
            c(s).as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut error,
        )
    };
    if !error.is_null() {
        unsafe { g_error_free(error) };
    }
    assert!(!v.is_null(), "invalid internal tray variant");
    v
}
fn put(state: &mut State, name: &str, text: &str) {
    let v = variant(text);
    if let Some(old) = state.properties.insert(name.into(), v) {
        unsafe { g_variant_unref(old) }
    }
}
extern "C" fn get(
    _conn: P,
    _sender: *const c_char,
    _path: *const c_char,
    _iface: *const c_char,
    name: *const c_char,
    _error: *mut P,
    _data: P,
) -> P {
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|s| s.properties.get(name.as_ref()))
            .map(|&v| unsafe { g_variant_ref(v) })
            .unwrap_or(ptr::null_mut())
    })
}
fn integer(args: P, i: usize) -> i32 {
    let v = unsafe { g_variant_get_child_value(args, i) };
    let n = unsafe { g_variant_get_int32(v) };
    unsafe { g_variant_unref(v) };
    n
}
extern "C" fn method(
    _conn: P,
    _sender: *const c_char,
    _path: *const c_char,
    _iface: *const c_char,
    name: *const c_char,
    args: P,
    invocation: P,
    _data: P,
) {
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let action = match name.as_ref() {
        "Activate" | "SecondaryActivate" => {
            Some(Action::Activate(integer(args, 0), integer(args, 1)))
        }
        "Scroll" => Some(Action::Scroll(integer(args, 0))),
        "ContextMenu" => Some(Action::ContextMenu),
        _ => None,
    };
    let callback = STATE.with(|s| s.borrow().as_ref().map(|s| s.action));
    // Release STATE's borrow before app activation can refresh the tray.
    if let (Some(action), Some(callback)) = (action, callback) {
        callback(action)
    }
    unsafe { g_dbus_method_invocation_return_value(invocation, ptr::null_mut()) };
}
extern "C" fn watcher(conn: P, _name: *const c_char, _owner: *const c_char, _data: P) {
    let args = variant("('/StatusNotifierItem',)");
    unsafe {
        g_dbus_connection_call(
            conn,
            c("org.kde.StatusNotifierWatcher").as_ptr(),
            c("/StatusNotifierWatcher").as_ptr(),
            c("org.kde.StatusNotifierWatcher").as_ptr(),
            c("RegisterStatusNotifierItem").as_ptr(),
            args,
            ptr::null_mut(),
            0,
            5000,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        g_variant_unref(args)
    };
}
const XML: &str = r#"<node><interface name="org.kde.StatusNotifierItem">
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="WindowId" type="u" access="read"/><property name="ItemIsMenu" type="b" access="read"/>
<property name="Menu" type="o" access="read"/><property name="IconName" type="s" access="read"/>
<property name="IconPixmap" type="a(iiay)" access="read"/><property name="IconThemePath" type="s" access="read"/>
<property name="OverlayIconName" type="s" access="read"/><property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
<property name="AttentionIconName" type="s" access="read"/><property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
<property name="AttentionMovieName" type="s" access="read"/>
<property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
<signal name="NewIcon"/><signal name="NewToolTip"/><signal name="NewTitle"/><signal name="NewStatus"><arg type="s"/></signal>
</interface></node>"#;
pub fn start(menu: P, action: fn(Action)) -> Result<(), String> {
    let mut error = ptr::null_mut();
    let connection = unsafe { g_bus_get_sync(2, ptr::null_mut(), &mut error) };
    if connection.is_null() {
        if !error.is_null() {
            unsafe { g_error_free(error) }
        };
        return Err("Could not connect to KDE session bus".into());
    }
    let node = unsafe { g_dbus_node_info_new_for_xml(c(XML).as_ptr(), &mut error) };
    if node.is_null() {
        return Err("Invalid StatusNotifier interface".into());
    }
    let table = Box::leak(Box::new(VTable {
        method,
        get,
        set: ptr::null_mut(),
        padding: [ptr::null_mut(); 8],
    }));
    let registration = unsafe {
        g_dbus_connection_register_object(
            connection,
            c(PATH).as_ptr(),
            g_dbus_node_info_lookup_interface(node, c(IFACE).as_ptr()),
            table,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut error,
        )
    };
    if registration == 0 {
        if !error.is_null() {
            unsafe { g_error_free(error) }
        };
        return Err("Could not publish KDE tray item".into());
    }
    let server = unsafe { dbusmenu_server_new(c(MENU).as_ptr()) };
    unsafe { dbusmenu_server_set_root(server, dbusmenu_gtk_parse_menu_structure(menu)) };
    let mut state = State {
        connection,
        properties: HashMap::new(),
        action,
        _server: server,
        _node: node,
        _registration: registration,
        _watch: 0,
    };
    for (name, value) in [
        ("Category", "'ApplicationStatus'"),
        ("Id", "'tokenkeeper'"),
        ("Title", "'TokenKeeper'"),
        ("Status", "'Active'"),
        ("WindowId", "uint32 0"),
        ("ItemIsMenu", "false"),
        ("Menu", "objectpath '/Menu'"),
        ("IconName", "''"),
        ("IconThemePath", "''"),
        ("OverlayIconName", "''"),
        ("OverlayIconPixmap", "@a(iiay) []"),
        ("AttentionIconName", "''"),
        ("AttentionIconPixmap", "@a(iiay) []"),
        ("AttentionMovieName", "''"),
        ("IconPixmap", "@a(iiay) []"),
        ("ToolTip", "('', @a(iiay) [], 'TokenKeeper', 'Loading usage…')"),
    ] {
        put(&mut state, name, value)
    }
    STATE.with(|s| *s.borrow_mut() = Some(state));
    let watch = unsafe {
        g_bus_watch_name_on_connection(
            connection,
            c("org.kde.StatusNotifierWatcher").as_ptr(),
            0,
            watcher,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    STATE.with(|s| s.borrow_mut().as_mut().unwrap()._watch = watch);
    Ok(())
}
fn pixels(rgba: &[u8]) -> String {
    let mut result = String::new();
    for p in rgba.chunks_exact(4) {
        for value in [p[3], p[0], p[1], p[2]] {
            if !result.is_empty() {
                result.push(',')
            };
            result.push_str(&value.to_string())
        }
    }
    result
}
pub fn update(usage: Option<(f32, f32)>, title: &str, summary: &str) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(state) = state.as_mut() else { return };
        let pixmaps = [16, 22, 32, 48]
            .map(|size| {
                format!(
                    "({size}, {size}, @ay [{}])",
                    pixels(&crate::icon::render(size, usage, false))
                )
            })
            .join(",");
        put(state, "IconPixmap", &format!("@a(iiay) [{pixmaps}]"));
        put(state, "Title", &quoted(title));
        put(
            state,
            "ToolTip",
            &format!("('', @a(iiay) [], {}, {})", quoted(title), quoted(summary)),
        );
        for name in ["NewIcon", "NewToolTip", "NewTitle"] {
            unsafe {
                g_dbus_connection_emit_signal(
                    state.connection,
                    ptr::null(),
                    c(PATH).as_ptr(),
                    c(IFACE).as_ptr(),
                    c(name).as_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_bytes_use_network_argb() {
        assert_eq!(pixels(&[1, 2, 3, 4]), "4,1,2,3");
    }
    #[test]
    fn tooltip_variant_escapes_text() {
        let v = variant(&format!(
            "('', @a(iiay) [], {}, {})",
            quoted("Worker's quota"),
            quoted("a\\b\nnext")
        ));
        unsafe { g_variant_unref(v) };
    }
}
