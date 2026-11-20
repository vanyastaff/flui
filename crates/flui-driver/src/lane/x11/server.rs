//! The X server as the X11 lane talks to it: one connection, in pure Rust, on the lane's thread.
//!
//! Every request whose failure matters is followed by a round trip, so an error the server sends
//! back is the request's own and is answered before the next event.

use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
use x11rb::protocol::xproto::{
    self, AtomEnum, ConfigureWindowAux, ConnectionExt as _, ImageFormat, ImageOrder, InputFocus,
    MapState, StackMode, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::held::Keymap;
use super::image::{self, PixmapFormat};
use super::keys::{Keysym, NO_SYMBOL};
use super::safety::{Keycode, LANE};
use crate::coordinates::DevicePoint;
use crate::error::{Error, Result};
use crate::lane::OsCapture;

/// The most windows the search for the app's window visits.
const MAX_WINDOWS: usize = 4096;

/// How deep under the root the search looks: a window manager's frame and its parts sit between
/// the root and the app's window.
const MAX_DEPTH: usize = 4;

/// How much of a window's title a description reads, in the protocol's four-byte units.
const TITLE_WORDS: u32 = 32;

/// An error of the lane that is no refusal.
pub(super) fn lane_error(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}

fn request_failed(what: &str, error: impl std::fmt::Display) -> Error {
    lane_error(format!("the X server refused {what}: {error}"))
}

/// What the lane fakes through XTEST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fake {
    /// Press a key.
    KeyPress(Keycode),
    /// Release a key.
    KeyRelease(Keycode),
    /// Press a button, 1 to 9.
    ButtonPress(u8),
    /// Release a button.
    ButtonRelease(u8),
    /// Move the pointer to a point on the root.
    Motion(DevicePoint),
}

/// A connection to the X server the app's window is on.
pub(super) struct Server {
    connection: RustConnection,
    root: Window,
    net_wm_pid: xproto::Atom,
    net_wm_name: xproto::Atom,
    utf8_string: xproto::Atom,
    /// Whether the server answers X-Resource's client ids (1.2 and later).
    resource_ids: bool,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Connect to the X server `display` names (`DISPLAY` when `None`).
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] with `unavailable` when no X server answers there, or it has no XTEST.
    pub(super) fn connect(display: Option<&str>) -> Result<Self> {
        let unavailable = |message: String| Error::Lane {
            lane: LANE.to_owned(),
            unavailable: true,
            message,
        };
        let (connection, screen) = RustConnection::connect(display).map_err(|error| {
            unavailable(format!(
                "no X server answers on {}: {error}",
                display.unwrap_or("DISPLAY")
            ))
        })?;
        if connection
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)
            .ok()
            .flatten()
            .is_none()
        {
            return Err(unavailable(
                "the X server has no XTEST extension to send input through".to_owned(),
            ));
        }
        let root = connection.setup().roots[screen].root;
        let atom = |name: &[u8]| -> Result<xproto::Atom> {
            Ok(connection
                .intern_atom(false, name)
                .map_err(|error| request_failed("an atom", error))?
                .reply()
                .map_err(|error| request_failed("an atom", error))?
                .atom)
        };
        let net_wm_pid = atom(b"_NET_WM_PID")?;
        let net_wm_name = atom(b"_NET_WM_NAME")?;
        let utf8_string = atom(b"UTF8_STRING")?;
        let resource_ids = connection
            .extension_information(x11rb::protocol::res::X11_EXTENSION_NAME)
            .ok()
            .flatten()
            .is_some()
            && connection
                .res_query_version(1, 2)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some_and(|version| (version.server_major, version.server_minor) >= (1, 2));
        Ok(Self {
            connection,
            root,
            net_wm_pid,
            net_wm_name,
            utf8_string,
            resource_ids,
        })
    }

    /// A round trip: every request before it has been carried out, and an error it caused has
    /// come back.
    fn sync(&self, what: &str) -> Result<()> {
        self.connection
            .get_input_focus()
            .map_err(|error| request_failed(what, error))?
            .reply()
            .map_err(|error| request_failed(what, error))?;
        Ok(())
    }

    /// The process that made `window`: what the X server knows of the client's connection
    /// (X-Resource's local client id), or else what the window says in `_NET_WM_PID`. A client
    /// in a PID namespace other than the first writes no `_NET_WM_PID`, which a server in the
    /// same namespace still answers for.
    fn pid_of(&self, window: Window) -> Option<u32> {
        self.server_pid_of(window).or_else(|| {
            let reply = self
                .connection
                .get_property(false, window, self.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()?;
            reply.value32()?.next()
        })
    }

    /// The process of the client that made `window`, as the X server knows it.
    fn server_pid_of(&self, window: Window) -> Option<u32> {
        if !self.resource_ids {
            return None;
        }
        let reply = self
            .connection
            .res_query_client_ids(&[ClientIdSpec {
                client: window,
                mask: ClientIdMask::LOCAL_CLIENT_PID,
            }])
            .ok()?
            .reply()
            .ok()?;
        reply
            .ids
            .iter()
            .find(|id| id.spec.mask.contains(ClientIdMask::LOCAL_CLIENT_PID))
            .and_then(|id| id.value.first().copied())
    }

    /// Whether `window` is mapped and every ancestor too.
    fn viewable(&self, window: Window) -> bool {
        self.connection
            .get_window_attributes(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE)
    }

    /// The viewable windows of the process `pid` (by `pid_of`: X-Resource's client process, else
    /// `_NET_WM_PID`), each with its width and
    /// height in device pixels, searched under the root within [`MAX_DEPTH`] levels and
    /// [`MAX_WINDOWS`] windows.
    pub(super) fn windows_of(&self, pid: u32) -> Result<Vec<(Window, u32, u32)>> {
        let mut found = Vec::new();
        let mut level = vec![self.root];
        let mut visited = 0_usize;
        for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for parent in level {
                let Ok(tree) = self
                    .connection
                    .query_tree(parent)
                    .map_err(|error| request_failed("the window tree", error))?
                    .reply()
                else {
                    continue;
                };
                for child in tree.children {
                    visited += 1;
                    if visited > MAX_WINDOWS {
                        return Ok(found);
                    }
                    if self.pid_of(child) == Some(pid) && self.viewable(child) {
                        if let Ok(geometry) = self
                            .connection
                            .get_geometry(child)
                            .map_err(|error| request_failed("a window's size", error))?
                            .reply()
                        {
                            found.push((
                                child,
                                u32::from(geometry.width),
                                u32::from(geometry.height),
                            ));
                        }
                    } else {
                        next.push(child);
                    }
                }
            }
            level = next;
        }
        Ok(found)
    }

    /// Where `window`'s top-left pixel is on the root.
    pub(super) fn origin(&self, window: Window) -> Result<DevicePoint> {
        let reply = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(|error| request_failed("a window's position", error))?
            .reply()
            .map_err(|error| request_failed("a window's position", error))?;
        Ok(DevicePoint::new(
            i32::from(reply.dst_x),
            i32::from(reply.dst_y),
        ))
    }

    /// Whether `window` is still the process `pid`'s and viewable.
    pub(super) fn alive(&self, window: Window, pid: u32) -> bool {
        self.pid_of(window) == Some(pid) && self.viewable(window)
    }

    /// The root's child that holds `window`: the window itself, or a window manager's frame.
    fn top_level_of(&self, window: Window) -> Option<Window> {
        let mut current = window;
        for _ in 0..=MAX_DEPTH {
            let tree = self.connection.query_tree(current).ok()?.reply().ok()?;
            if tree.parent == self.root || tree.parent == x11rb::NONE {
                return Some(current);
            }
            current = tree.parent;
        }
        None
    }

    /// Whether the keyboard focus is in `window`: on it, inside it, or following the pointer
    /// while the pointer is over it.
    pub(super) fn focused(&self, window: Window) -> (bool, Option<Window>) {
        let Some(focus) = self
            .connection
            .get_input_focus()
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.focus)
        else {
            return (false, None);
        };
        let pointer_root = u32::from(InputFocus::POINTER_ROOT);
        if focus == pointer_root {
            let under = self.pointer_window();
            return (under == self.top_level_of(window) && under.is_some(), under);
        }
        let mut current = focus;
        for _ in 0..=MAX_DEPTH {
            if current == window {
                return (true, Some(focus));
            }
            let Some(tree) = self
                .connection
                .query_tree(current)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            else {
                break;
            };
            if tree.parent == x11rb::NONE {
                break;
            }
            current = tree.parent;
        }
        (false, Some(focus))
    }

    /// The root's child under the pointer.
    fn pointer_window(&self) -> Option<Window> {
        let reply = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        (reply.child != x11rb::NONE).then_some(reply.child)
    }

    /// Give `window` the keyboard focus and raise it, as a window manager does on a click.
    pub(super) fn give_focus(&self, window: Window) {
        let top = self.top_level_of(window).unwrap_or(window);
        let _ = self
            .connection
            .configure_window(top, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = self
            .connection
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME);
        let _ = self.sync("the focus");
    }

    /// Whether the root's child at `point` holds `window`, and the window there.
    pub(super) fn lands_on(&self, window: Window, point: DevicePoint) -> (bool, Option<Window>) {
        let (Ok(x), Ok(y)) = (i16::try_from(point.x), i16::try_from(point.y)) else {
            return (false, None);
        };
        let Some(reply) = self
            .connection
            .translate_coordinates(self.root, self.root, x, y)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return (false, None);
        };
        let under = (reply.child != x11rb::NONE).then_some(reply.child);
        (under.is_some() && under == self.top_level_of(window), under)
    }

    /// `window` for people: its id and title.
    pub(super) fn describe(&self, window: Window) -> String {
        let title = self
            .connection
            .get_property(
                false,
                window,
                self.net_wm_name,
                self.utf8_string,
                0,
                TITLE_WORDS,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| String::from_utf8_lossy(&reply.value).into_owned())
            .filter(|title| !title.is_empty());
        match title {
            Some(title) => format!("window {window:#x} \"{title}\""),
            None => format!("window {window:#x}"),
        }
    }

    /// The mouse buttons down, button `n` at bit `n - 1`.
    pub(super) fn buttons_down(&self) -> u16 {
        self.connection
            .query_pointer(self.root)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map_or(0, |reply| (u16::from(reply.mask) >> 8) & 0x1F)
    }

    /// The keys down.
    pub(super) fn keys_down(&self) -> Vec<Keycode> {
        let Some(reply) = self
            .connection
            .query_keymap()
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        for (byte, bits) in reply.keys.iter().enumerate() {
            for bit in 0..8 {
                if bits & (1 << bit) != 0
                    && let Ok(code) = u8::try_from(byte * 8 + bit)
                {
                    keys.push(Keycode(code));
                }
            }
        }
        keys
    }

    /// The server's core keymap.
    pub(super) fn keymap(&self) -> Result<Keymap> {
        let setup = self.connection.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let count = max.saturating_sub(min).saturating_add(1);
        let reply = self
            .connection
            .get_keyboard_mapping(min, count)
            .map_err(|error| request_failed("the keymap", error))?
            .reply()
            .map_err(|error| request_failed("the keymap", error))?;
        Ok(Keymap::new(
            min,
            reply.keysyms_per_keycode,
            reply
                .keysyms
                .into_iter()
                .map(Keysym)
                .collect::<Vec<Keysym>>(),
        ))
    }

    /// Map `key` to `keysym` on each of its `per_keycode` levels; `NoSymbol` empties it.
    pub(super) fn map_key(&self, key: Keycode, keysym: Keysym, per_keycode: u8) -> Result<()> {
        let keysyms = vec![keysym.0; usize::from(per_keycode.max(1))];
        self.connection
            .change_keyboard_mapping(1, key.0, per_keycode.max(1), &keysyms)
            .map_err(|error| request_failed("a key's mapping", error))?;
        self.sync(if keysym == NO_SYMBOL {
            "emptying a key"
        } else {
            "a key's mapping"
        })
    }

    /// Fake `fake` through XTEST and wait for the server to have carried it out.
    pub(super) fn fake(&self, fake: Fake) -> Result<()> {
        let (kind, detail, x, y) = match fake {
            Fake::KeyPress(key) => (xproto::KEY_PRESS_EVENT, key.0, 0, 0),
            Fake::KeyRelease(key) => (xproto::KEY_RELEASE_EVENT, key.0, 0, 0),
            Fake::ButtonPress(button) => (xproto::BUTTON_PRESS_EVENT, button, 0, 0),
            Fake::ButtonRelease(button) => (xproto::BUTTON_RELEASE_EVENT, button, 0, 0),
            Fake::Motion(to) => {
                let x = i16::try_from(to.x)
                    .map_err(|_| Error::outside_target("the point is past the X screen's range"))?;
                let y = i16::try_from(to.y)
                    .map_err(|_| Error::outside_target("the point is past the X screen's range"))?;
                (xproto::MOTION_NOTIFY_EVENT, 0, x, y)
            }
        };
        self.connection
            .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0)
            .map_err(|error| request_failed("faked input", error))?;
        self.sync("faked input")
    }

    /// `window`'s pixels, through `GetImage`.
    pub(super) fn capture(&self, window: Window) -> Result<OsCapture> {
        let geometry = self
            .connection
            .get_geometry(window)
            .map_err(|error| request_failed("the window's size", error))?
            .reply()
            .map_err(|error| request_failed("the window's size", error))?;
        let attributes = self
            .connection
            .get_window_attributes(window)
            .map_err(|error| request_failed("the window's visual", error))?
            .reply()
            .map_err(|error| request_failed("the window's visual", error))?;
        let reply = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                window,
                0,
                0,
                geometry.width,
                geometry.height,
                u32::MAX,
            )
            .map_err(|error| request_failed("the window's image", error))?
            .reply()
            .map_err(|error| request_failed("the window's image", error))?;
        let setup = self.connection.setup();
        let pixmap = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == reply.depth)
            .ok_or_else(|| {
                lane_error(format!(
                    "the X server has no format of depth {}",
                    reply.depth
                ))
            })?;
        let visual = setup
            .roots
            .iter()
            .flat_map(|screen| &screen.allowed_depths)
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.visual_id == attributes.visual)
            .ok_or_else(|| lane_error("the window's visual is none the X server lists"))?;
        let format = PixmapFormat {
            width: u32::from(geometry.width),
            height: u32::from(geometry.height),
            bits_per_pixel: pixmap.bits_per_pixel,
            scanline_pad: pixmap.scanline_pad,
            lsb_first: setup.image_byte_order == ImageOrder::LSB_FIRST,
            red_mask: visual.red_mask,
            green_mask: visual.green_mask,
            blue_mask: visual.blue_mask,
        };
        let rgba = image::rgba(&format, &reply.data)?;
        OsCapture::try_new(format.width, format.height, rgba)
    }
}
