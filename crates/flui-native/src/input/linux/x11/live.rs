//! The loop's input over the platform's connection.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cursor_icon::CursorIcon;
use flui_platform::input::capabilities::InputCapabilities;
use flui_platform::input::keyboard::{KeyPress, KeyState};
use flui_platform::input::pointer::MotionDelta;
use flui_platform::{
    Cursor, CursorError, CursorGrab, CursorImage, EventTime, PlatformFailure, PlatformInput,
    Unsupported, UnsupportedReason,
};

use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::render;
use x11rb::protocol::xinput::{
    self, ConnectionExt as _, DeviceClassData, DeviceType, Fp1616, Fp3232, XIEventMask,
};
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    self, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, GrabMode, GrabStatus,
    NotifyMode,
};

use super::decode::{
    Axis, AxisLabel, DeviceChanges, DeviceDescription, HierarchyInfo, ScrollAxis, X11Pointers,
    XiAction, XiEvent, XiMaster, XiSlave, XiTime, valuators,
};
use crate::input::assembly::{NativeCancel, NativeClock, NativeSurface};
use crate::input::availability;
use crate::input::event_timestamp_ns;
use crate::input::linux::xkb::{Keyboard, KeymapSource};
use crate::owner::layout_callback::LayoutCallback;
use crate::owner::linux::x11::{
    InputTarget, LoopInput, LoopText, PointerRun, WindowRef, X11Connection, X11Key,
};

use crate::owner::window_delivery::WindowDelivery;

/// The XInput 2 version the input asks for: 2.4 brought touchpad
/// gestures.
const XI_ASKED: (u16, u16) = (2, 4);
/// The least it works with: 2.2 brought touch.
const XI_LEAST: (u16, u16) = (2, 2);

/// What one turn delivers to a window, once no borrow is held.
enum Delivery {
    Input(PlatformInput),
    Hover(bool),
    Present(InputCapabilities),
}

/// The label atoms a pen's valuators are named by.
#[derive(Debug, Clone, Copy)]
struct LabelAtoms {
    pressure: xproto::Atom,
    tilt_x: xproto::Atom,
    tilt_y: xproto::Atom,
}

impl LabelAtoms {
    fn label(&self, atom: xproto::Atom) -> AxisLabel {
        if atom == 0 {
            AxisLabel::Other
        } else if atom == self.pressure {
            AxisLabel::Pressure
        } else if atom == self.tilt_x {
            AxisLabel::TiltX
        } else if atom == self.tilt_y {
            AxisLabel::TiltY
        } else {
            AxisLabel::Other
        }
    }
}

/// The core keyboard: its keymap and state, where they come from, and
/// the names of its layouts.
struct CoreKeyboard {
    keyboard: Keyboard,
    source: KeymapSource,
    /// The layouts' names, by index, as XKB names the groups.
    layouts: Vec<String>,
}

/// What the input keeps on the owner thread.
struct State {
    pointers: X11Pointers,
    /// The open windows, by their X id.
    windows: HashMap<xproto::Window, InputTarget>,
    keyboard: Option<CoreKeyboard>,
    cursors: Option<x11rb::cursor::Handle>,
    /// The theme's cursors loaded so far.
    loaded: HashMap<CursorIcon, xproto::Cursor>,
    /// The cursor of no pixels, made at its first use: a hidden cursor's
    /// and a locked pointer's.
    blank: Option<xproto::Cursor>,
    /// The last application image made a cursor, and that cursor: a
    /// repeated image is the cursor already made.
    image: Option<(CursorImage, xproto::Cursor)>,
    /// The Render format an image is made a cursor in (`ARGB32`), asked
    /// for at the first image.
    argb32: Option<render::Pictformat>,
}

/// The pointer a window holds (`CursorGrab`), and for a lock, the point
/// of the window's content, in device pixels, it is held at (a point of
/// the content, not of the screen, so it moves with the window) and the
/// master pointer
/// the grab took (the client's pointer), whose cursor the raw motion is.
#[derive(Debug, Clone, Copy)]
struct Held {
    window: WindowRef,
    grab: CursorGrab,
    at: (i16, i16),
    master: Option<XiMaster>,
}

/// The X11 loop's input: the windows' pointers, keys and cursors on the
/// platform's connection.
pub(crate) struct X11Input {
    connection: Arc<X11Connection>,
    /// XInput 2 answers, at 2.2 or later.
    xinput: bool,
    /// XInput 2 answers at 2.4: touchpad gestures.
    gestures: bool,
    labels: LabelAtoms,
    state: Rc<RefCell<State>>,
    /// The name of the layout the keyboard is in.
    layout: RefCell<String>,
    /// The application's keyboard layout callback.
    layout_callback: LayoutCallback,
    /// The layout's name moved this turn: the callback hears it at the
    /// turn's settle.
    layout_moved: Cell<bool>,
    /// The server's keymap changed since it was last read: it is read
    /// again before the next event that reads it, or once the turn's
    /// events are heard, so a burst of changes costs one read.
    keymap_stale: Cell<bool>,
    /// The windows' text input, which every key goes through: the
    /// input method of the focused field takes it or lets it through.
    text: Rc<dyn LoopText>,
    /// The pointer a window holds: one at most, as the server grabs it
    /// for one window of the client.
    held: Cell<Option<Held>>,
}

impl std::fmt::Debug for X11Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Input")
            .field("xinput", &self.xinput)
            .field("gestures", &self.gestures)
            .finish_non_exhaustive()
    }
}

/// An X11 fixed-point 16.16 number.
fn fp1616(value: Fp1616) -> f64 {
    f64::from(value) / 65536.0
}

/// An X11 fixed-point 32.32 number.
fn fp3232(value: Fp3232) -> f64 {
    f64::from(value.integral) + f64::from(value.frac) / 4_294_967_296.0
}

/// The monotonic clock, which a local X server stamps its events on.
fn monotonic() -> Duration {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    Duration::new(
        u64::try_from(now.tv_sec).unwrap_or(0),
        u32::try_from(now.tv_nsec).unwrap_or(0),
    )
}

/// An event's time: the server's, with the clocks read as it arrived.
fn xi_time(server_ms: xproto::Timestamp) -> XiTime {
    XiTime {
        server_ms,
        observed_ms: u64::try_from(monotonic().as_millis()).unwrap_or(u64::MAX),
        arrival: EventTime::from_nanos(event_timestamp_ns()),
    }
}

/// The fields every XInput 2 pointer, touch and crossing event shares.
struct XiFields<'a> {
    window: xproto::Window,
    master: u16,
    source: u16,
    detail: u32,
    time: xproto::Timestamp,
    position: (Fp1616, Fp1616),
    modifiers: u32,
    mask: &'a [u32],
    values: &'a [Fp3232],
    emulated: bool,
}

/// The event `fields` make, as `action`, and the window it names.
fn xi_event(action: XiAction, fields: &XiFields<'_>) -> (xproto::Window, XiEvent) {
    let values: Vec<f64> = fields.values.iter().copied().map(fp3232).collect();
    (
        fields.window,
        XiEvent {
            action,
            master: XiMaster(fields.master),
            source: XiSlave(fields.source),
            detail: fields.detail,
            position: (fp1616(fields.position.0), fp1616(fields.position.1)),
            emulated: fields.emulated,
            modifiers: fields.modifiers,
            valuators: valuators(fields.mask, &values),
            time: xi_time(fields.time),
        },
    )
}

/// The XInput 2 event `event` carries, and the window it names.
fn copied_event(event: &Event) -> Option<(xproto::Window, XiEvent)> {
    let pointer = |action, e: &xinput::ButtonPressEvent| {
        let emulated =
            u32::from(e.flags) & u32::from(xinput::PointerEventFlags::POINTER_EMULATED) != 0;
        xi_event(
            action,
            &XiFields {
                window: e.event,
                master: e.deviceid,
                source: e.sourceid,
                detail: e.detail,
                time: e.time,
                position: (e.event_x, e.event_y),
                modifiers: e.mods.effective,
                mask: &e.valuator_mask,
                values: &e.axisvalues,
                emulated,
            },
        )
    };
    let touch = |action, e: &xinput::TouchBeginEvent| {
        xi_event(
            action,
            &XiFields {
                window: e.event,
                master: e.deviceid,
                source: e.sourceid,
                detail: e.detail,
                time: e.time,
                position: (e.event_x, e.event_y),
                modifiers: e.mods.effective,
                mask: &e.valuator_mask,
                values: &e.axisvalues,
                emulated: false,
            },
        )
    };
    let crossing = |action, e: &xinput::EnterEvent| {
        xi_event(
            action,
            &XiFields {
                window: e.event,
                master: e.deviceid,
                source: e.sourceid,
                detail: 0,
                time: e.time,
                position: (e.event_x, e.event_y),
                modifiers: e.mods.effective,
                mask: &[],
                values: &[],
                emulated: false,
            },
        )
    };
    let pinch = |action, e: &xinput::GesturePinchBeginEvent| {
        xi_event(
            action,
            &XiFields {
                window: e.event,
                master: e.deviceid,
                source: e.sourceid,
                detail: e.detail,
                time: e.time,
                position: (e.event_x, e.event_y),
                modifiers: e.mods.effective,
                mask: &[],
                values: &[],
                emulated: false,
            },
        )
    };
    let swipe = |action, e: &xinput::GestureSwipeBeginEvent| {
        xi_event(
            action,
            &XiFields {
                window: e.event,
                master: e.deviceid,
                source: e.sourceid,
                detail: e.detail,
                time: e.time,
                position: (e.event_x, e.event_y),
                modifiers: e.mods.effective,
                mask: &[],
                values: &[],
                emulated: false,
            },
        )
    };
    Some(match event {
        Event::XinputMotion(e) => pointer(XiAction::Motion, e),
        Event::XinputButtonPress(e) => pointer(XiAction::ButtonPress, e),
        Event::XinputButtonRelease(e) => pointer(XiAction::ButtonRelease, e),
        Event::XinputTouchBegin(e) => touch(XiAction::TouchBegin, e),
        Event::XinputTouchUpdate(e) => touch(XiAction::TouchUpdate, e),
        Event::XinputTouchEnd(e) => touch(XiAction::TouchEnd, e),
        Event::XinputEnter(e) => crossing(XiAction::Enter, e),
        Event::XinputLeave(e) => crossing(
            XiAction::Leave {
                grabbed: e.mode == xinput::NotifyMode::GRAB,
            },
            e,
        ),
        Event::XinputGesturePinchBegin(e) => pinch(XiAction::PinchBegin, e),
        Event::XinputGesturePinchUpdate(e) => pinch(
            XiAction::PinchUpdate {
                scale: fp1616(e.scale),
                rotation: fp1616(e.delta_angle),
            },
            e,
        ),
        Event::XinputGesturePinchEnd(e) => pinch(
            XiAction::PinchEnd {
                cancelled: u32::from(e.flags)
                    & u32::from(xinput::GesturePinchEventFlags::GESTURE_PINCH_CANCELLED)
                    != 0,
            },
            e,
        ),
        Event::XinputGestureSwipeBegin(e) => swipe(XiAction::SwipeBegin, e),
        Event::XinputGestureSwipeUpdate(e) => swipe(
            XiAction::SwipeUpdate {
                dx: fp1616(e.delta_x),
                dy: fp1616(e.delta_y),
            },
            e,
        ),
        Event::XinputGestureSwipeEnd(e) => swipe(
            XiAction::SwipeEnd {
                cancelled: u32::from(e.flags)
                    & u32::from(xinput::GestureSwipeEventFlags::GESTURE_SWIPE_CANCELLED)
                    != 0,
            },
            e,
        ),
        _ => return None,
    })
}

impl X11Input {
    /// The input of `connection`'s windows: XInput 2 set up on it, the
    /// devices listed, the keyboard read, and the device hierarchy and
    /// the keyboard's changes heard from here on.
    pub(crate) fn new(connection: &Arc<X11Connection>, text: Rc<dyn LoopText>) -> Rc<Self> {
        let xcb = connection.xcb();
        let version = xcb
            .xinput_xi_query_version(XI_ASKED.0, XI_ASKED.1)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| (reply.major_version, reply.minor_version));
        let xinput = version.is_some_and(|version| version >= XI_LEAST);
        let gestures = version.is_some_and(|version| version >= XI_ASKED);
        if !xinput {
            tracing::warn!("the X server has no XInput 2.2: X11 windows get no pointer input");
        }
        let atom = |name: &[u8]| {
            xcb.intern_atom(true, name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map_or(0, |reply| reply.atom)
        };
        let labels = LabelAtoms {
            pressure: atom(b"Abs Pressure"),
            tilt_x: atom(b"Abs Tilt X"),
            tilt_y: atom(b"Abs Tilt Y"),
        };
        let keyboard = Self::keyboard(connection);
        let cursors = x11rb::resource_manager::new_from_default(xcb)
            .ok()
            .and_then(|database| {
                x11rb::cursor::Handle::new(xcb, connection.screen_number(), &database)
                    .ok()?
                    .reply()
                    .ok()
            });
        // The server's time is milliseconds of the monotonic clock: the
        // registry places it on the event timeline from the two read
        // together now.
        let clock = NativeClock::new(
            EventTime::from_nanos(event_timestamp_ns()),
            monotonic(),
            None,
        );
        let layout = keyboard.as_ref().map_or_default(CoreKeyboard::layout_name);
        let state = State {
            pointers: X11Pointers::new(clock),
            windows: HashMap::new(),
            keyboard,
            cursors,
            loaded: HashMap::new(),
            blank: None,
            image: None,
            argb32: None,
        };
        let input = Rc::new(Self {
            connection: Arc::clone(connection),
            xinput,
            gestures,
            labels,
            state: Rc::new(RefCell::new(state)),
            layout: RefCell::new(layout),
            layout_callback: LayoutCallback::default(),
            layout_moved: Cell::new(false),
            keymap_stale: Cell::new(false),
            text,
            held: Cell::new(None),
        });
        if xinput {
            input.select_root_events(false);
            input.read_devices();
        }
        input
    }

    /// The core keyboard, its XKB events selected.
    fn keyboard(connection: &Arc<X11Connection>) -> Option<CoreKeyboard> {
        let Some(source) = KeymapSource::x11_core_keyboard(connection) else {
            tracing::warn!("libxkbcommon-x11 or XKB is missing: X11 windows get no keys");
            return None;
        };
        let keyboard = Keyboard::try_new(&source)?;
        let device = u16::try_from(source.x11_device()?).ok()?;
        let events = xkb::EventType::NEW_KEYBOARD_NOTIFY
            | xkb::EventType::MAP_NOTIFY
            | xkb::EventType::STATE_NOTIFY;
        let selected = connection.xcb().xkb_select_events(
            device,
            xkb::EventType::from(0_u16),
            events,
            xkb::MapPart::from(0xff_u16),
            xkb::MapPart::from(0xff_u16),
            &xkb::SelectEventsAux::new(),
        );
        if let Err(error) = selected {
            tracing::debug!(%error, "the X11 keyboard's changes go unheard");
        }
        let layouts = group_names(connection, device);
        Some(CoreKeyboard {
            keyboard,
            source,
            layouts,
        })
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }

    /// The devices the server lists for `device` (one id, or
    /// `Device::ALL`), as the pure decoder reads them: the enabled ones
    /// and the master pointers.
    fn query(&self, device: impl Into<xinput::DeviceId>) -> Vec<DeviceDescription> {
        let Some(reply) = self
            .connection
            .xcb()
            .xinput_xi_query_device(device)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return Vec::new();
        };
        reply
            .infos
            .into_iter()
            .filter(|info| info.enabled || info.type_ == DeviceType::MASTER_POINTER)
            .map(|info| {
                let mut description = DeviceDescription {
                    id: info.deviceid,
                    master: info.type_ == DeviceType::MASTER_POINTER,
                    pointer: info.type_ == DeviceType::SLAVE_POINTER,
                    name: String::from_utf8_lossy(&info.name).into_owned(),
                    direct_touch: false,
                    axes: Vec::new(),
                    scrolls: Vec::new(),
                };
                self.read_classes(&info.classes, &mut description);
                description
            })
            .collect()
    }

    /// Read `classes` (of a query's answer, or an `XIDeviceChanged`) into
    /// `description`.
    fn read_classes(&self, classes: &[xinput::DeviceClass], description: &mut DeviceDescription) {
        for class in classes {
            match &class.data {
                DeviceClassData::Valuator(valuator) => description.axes.push(Axis {
                    number: valuator.number,
                    label: self.labels.label(valuator.label),
                    min: fp3232(valuator.min),
                    max: fp3232(valuator.max),
                }),
                DeviceClassData::Scroll(scroll) => {
                    description.scrolls.push(ScrollAxis {
                        number: scroll.number,
                        horizontal: scroll.scroll_type == xinput::ScrollType::HORIZONTAL,
                        increment: fp3232(scroll.increment),
                    });
                }
                DeviceClassData::Touch(touch) => {
                    description.direct_touch = touch.mode == xinput::TouchMode::DIRECT;
                }
                _ => {}
            }
        }
    }

    /// The devices present before any window opens, from one listing of
    /// them all: their announcements reach no window, only the presence.
    fn read_devices(&self) {
        let devices = self.query(xinput::Device::ALL);
        drop(self.device_changes(|pointers, time| pointers.devices(&devices, time)));
    }

    /// A hierarchy change: what it removes goes, and each device it adds,
    /// enables or attaches is asked of the server by its own id (as GTK's
    /// `handle_hierarchy_changed` and winit's `xinput2_hierarchy_changed`
    /// do), so a change costs one query per device it names, up to the
    /// devices the decoder knows, and a device's events after it in the
    /// same batch find it known.
    fn hierarchy(&self, change: &xinput::HierarchyEvent) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        let change: Vec<HierarchyInfo> = change
            .infos
            .iter()
            .map(|info| HierarchyInfo {
                id: info.deviceid,
                device_use: u16::from(info.type_),
                flags: u32::from(info.flags),
            })
            .collect();
        self.device_changes(|pointers, time| {
            pointers.hierarchy(
                &change,
                |id| self.query(id).into_iter().find(|device| device.id == id),
                time,
            )
        })
    }

    /// What `change` makes of the devices goes to every window, with the
    /// kinds present.
    fn device_changes(
        &self,
        change: impl FnOnce(&mut X11Pointers, EventTime) -> DeviceChanges,
    ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        let time = EventTime::from_nanos(event_timestamp_ns());
        let capabilities = self.capabilities_now();
        self.with_state(|state| {
            let changes = change(&mut state.pointers, time);
            let present = capabilities.with_present(state.pointers.present());
            let mut out = Vec::new();
            for (surface, input) in changes {
                for target in state.windows.values() {
                    if surface.is_none_or(|surface| surface == surface_of(target.window)) {
                        out.push((Arc::clone(&target.delivery), Delivery::Input(input.clone())));
                    }
                }
            }
            for target in state.windows.values() {
                out.push((Arc::clone(&target.delivery), Delivery::Present(present)));
            }
            out
        })
    }

    /// What a window opening now can be given: the table of what this
    /// server's XInput 2 delivers.
    fn capabilities_now(&self) -> InputCapabilities {
        availability::x11(self.gestures)
    }

    /// The layout's name now, kept; whether it changed.
    fn read_layout(&self) -> bool {
        let name = self.with_state(|state| {
            state
                .keyboard
                .as_ref()
                .map_or_default(CoreKeyboard::layout_name)
        });
        let mut layout = self.layout.borrow_mut();
        let changed = *layout != name;
        *layout = name;
        changed
    }

    /// Note that the layout's name moved: the application's callback hears
    /// it once the turn's events are delivered (`settle`), so its
    /// panic cannot cost the event that noticed the change.
    fn layout_changed(&self) {
        self.layout_moved.set(true);
    }

    /// What a pointer, touch or gesture event delivers.
    fn pointer_event(
        &self,
        window: xproto::Window,
        event: &XiEvent,
    ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        self.with_state(|state| {
            let Some(target) = state.windows.get(&window) else {
                return Vec::new();
            };
            let delivery = Arc::clone(&target.delivery);
            let decoded =
                state
                    .pointers
                    .event(surface_of(target.window), target.scale.get(), event);
            // An enter is heard before the input it starts, a leave after
            // what ends the contact.
            let mut out = Vec::new();
            if decoded.hover == Some(true) {
                out.push((Arc::clone(&delivery), Delivery::Hover(true)));
            }
            out.extend(
                decoded
                    .inputs
                    .into_iter()
                    .map(|input| (Arc::clone(&delivery), Delivery::Input(input))),
            );
            if decoded.hover == Some(false) {
                out.push((delivery, Delivery::Hover(false)));
            }
            out
        })
    }

    /// What a pointer's motion over `window` delivers: `last`, and the
    /// motions `passed` it came through in the same turn (the rest of
    /// its [`PointerRun`]), as one move.
    fn pointer_motions(
        &self,
        window: xproto::Window,
        passed: &[Event],
        last: XiEvent,
    ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        let motions: Vec<XiEvent> = passed
            .iter()
            .filter_map(copied_event)
            .map(|(_, motion)| motion)
            .chain(std::iter::once(last))
            .collect();
        self.with_state(|state| {
            let Some(target) = state.windows.get(&window) else {
                return Vec::new();
            };
            let delivery = Arc::clone(&target.delivery);
            state
                .pointers
                .motions(surface_of(target.window), target.scale.get(), &motions)
                .into_iter()
                .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                .collect()
        })
    }

    /// What a key event delivers.
    fn key_event(&self, event: &xinput::KeyPressEvent, state: KeyState) -> Option<X11Key> {
        let repeat = u32::from(event.flags) & u32::from(xinput::KeyEventFlags::KEY_REPEAT) != 0;
        let state = match state {
            KeyState::Down(_) if repeat => KeyState::Down(KeyPress::AutoRepeat),
            state => state,
        };
        let time = EventTime::from_nanos(event_timestamp_ns());
        let shared = Rc::downgrade(&self.state);
        self.with_state(|input| {
            let target = input.windows.get(&event.event)?;
            let window = target.window;
            let delivery = Arc::clone(&target.delivery);
            let core = input.keyboard.as_mut()?;
            let keysym = core.keyboard.keysym(event.detail);
            let keycode = event.detail;
            let (held, held_keysym) = core.keyboard.key_held(keycode, state, time)?;
            // The core protocol's state: the modifier mask, and the
            // group in bits 13 and 14.
            let group = u32::from(event.group.effective) & 0b11;
            let mask = (event.mods.effective & 0xff) | (group << 13);
            Some(X11Key {
                window,
                keysym,
                keycode: u8::try_from(event.detail).unwrap_or(u8::MAX),
                state: u16::try_from(mask).unwrap_or(u16::MAX),
                release: state == KeyState::Up,
                time: event.time,
                // Only a key typed as it is feeds compose: one the input
                // method takes is its own sequence's.
                deliver: Box::new(move || {
                    let key = shared
                        .upgrade()
                        .and_then(|state| {
                            let mut state = state.try_borrow_mut().ok()?;
                            let core = state.keyboard.as_mut()?;
                            Some(core.keyboard.key_typed(keycode, held_keysym, held.clone()))
                        })
                        .unwrap_or(held);
                    delivery.dispatch_input(PlatformInput::Keyboard(key));
                }),
            })
        })
    }

    /// The server's keyboard state changed: whether its layout did.
    fn keyboard_state(&self, event: &xkb::StateNotifyEvent) -> bool {
        self.with_state(|state| {
            let Some(core) = state.keyboard.as_mut() else {
                return false;
            };
            core.keyboard.update_mask(
                u32::from(event.base_mods),
                u32::from(event.latched_mods),
                u32::from(event.locked_mods),
                [
                    u32::from(event.base_group.cast_unsigned()),
                    u32::from(event.latched_group.cast_unsigned()),
                    u32::from(u8::from(event.locked_group)),
                ],
            )
        })
    }

    /// Read the keymap again if the server changed it since the last
    /// read, and tell the listeners if the layout's name moved. An X
    /// keymap is bounded by the protocol (key codes 8 to 255, four
    /// groups), so one read is bounded work; however many changes the
    /// server reported since, they cost this one.
    fn read_stale_keymap(&self) {
        if !self.keymap_stale.take() {
            return;
        }
        self.keymap_changed();
        if self.read_layout() {
            self.layout_changed();
        }
    }

    /// The server's keymap changed: read it again, with its layouts'
    /// names. The keymap is compiled under the input's borrow; the names
    /// are asked for, and a failure reported to the `tracing`
    /// subscriber, with it released.
    fn keymap_changed(&self) {
        let reloaded = self.with_state(|state| {
            let core = state.keyboard.as_mut()?;
            let compiled = core.keyboard.reload(&core.source);
            let device = core
                .source
                .x11_device()
                .and_then(|device| u16::try_from(device).ok());
            Some((compiled, device))
        });
        let Some((compiled, device)) = reloaded else {
            return;
        };
        if !compiled {
            tracing::debug!("the X11 keyboard's new keymap cannot be read");
        }
        let Some(device) = device else {
            return;
        };
        let layouts = group_names(&self.connection, device);
        self.with_state(|state| {
            if let Some(core) = state.keyboard.as_mut() {
                core.layouts = layouts;
            }
        });
    }

    /// The focus left `window`: its contacts end, the compose sequence in
    /// progress ends and each key still held is released there.
    fn focus_out(&self, window: xproto::Window) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        let time = EventTime::from_nanos(event_timestamp_ns());
        self.with_state(|state| {
            let Some(target) = state.windows.get(&window) else {
                return Vec::new();
            };
            let delivery = Arc::clone(&target.delivery);
            let mut inputs = state.pointers.cancel_surface(
                surface_of(target.window),
                NativeCancel::FocusLost,
                time,
            );
            if let Some(core) = state.keyboard.as_mut() {
                inputs.extend(
                    core.keyboard
                        .leave(time)
                        .into_iter()
                        .map(PlatformInput::Keyboard),
                );
            }
            inputs
                .into_iter()
                .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                .collect()
        })
    }

    /// Let go of the pointer a window holds, if one does: the grab ends,
    /// and a lock's raw motion is no longer heard.
    fn let_go(&self) {
        let Some(held) = self.held.take() else {
            return;
        };
        if let Err(error) = self.connection.xcb().ungrab_pointer(x11rb::CURRENT_TIME) {
            tracing::debug!(%error, "cannot let go of the X11 pointer");
        }
        if held.grab == CursorGrab::Locked {
            self.select_root_events(false);
        }
    }

    /// Let go of the pointer if the window of X id `xid` holds it: the
    /// window lost the focus or was unmapped, and the server ended the
    /// grab or will.
    fn let_go_of(&self, xid: xproto::Window) {
        if self.held.get().is_some_and(|held| held.window.xid == xid) {
            self.let_go();
        }
    }

    /// Select what the input hears of every device on the root window:
    /// the device hierarchy and the devices' changes, and while a window
    /// holds the pointer in place, the raw motion (`XI_RawMotion`) XInput
    /// 2 sends there whoever grabs the pointer.
    fn select_root_events(&self, raw_motion: bool) {
        let mut mask = XIEventMask::HIERARCHY | XIEventMask::DEVICE_CHANGED;
        if raw_motion {
            mask |= XIEventMask::RAW_MOTION;
        }
        let selected = self.connection.xcb().xinput_xi_select_events(
            self.connection.root(),
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL.into(),
                mask: vec![mask],
            }],
        );
        if let Err(error) = selected {
            tracing::debug!(%error, "the X11 device hierarchy goes unheard");
        }
    }

    /// The point of `window` a lock holds the pointer at, in device
    /// pixels: where the pointer is when it is over the content, the
    /// content's centre otherwise.
    fn lock_point(&self, window: WindowRef) -> (i16, i16) {
        let xcb = self.connection.xcb();
        let pointer = xcb.query_pointer(window.xid).ok();
        let geometry = xcb.get_geometry(window.xid).ok();
        let pointer = pointer.and_then(|cookie| cookie.reply().ok());
        let Some(geometry) = geometry.and_then(|cookie| cookie.reply().ok()) else {
            return (0, 0);
        };
        let inside = |at: i16, extent: u16| (0..i32::from(extent)).contains(&i32::from(at));
        match pointer {
            Some(pointer)
                if pointer.same_screen
                    && inside(pointer.win_x, geometry.width)
                    && inside(pointer.win_y, geometry.height) =>
            {
                (pointer.win_x, pointer.win_y)
            }
            _ => {
                let half = |extent: u16| i16::try_from(extent / 2).unwrap_or(i16::MAX);
                (half(geometry.width), half(geometry.height))
            }
        }
    }

    /// Put the pointer at `at` of the window of X id `xid`.
    fn warp_to(&self, xid: xproto::Window, at: (i16, i16)) {
        let warped = self
            .connection
            .xcb()
            .warp_pointer(x11rb::NONE, xid, 0, 0, 0, 0, at.0, at.1);
        if let Err(error) = warped {
            tracing::debug!(%error, "cannot hold the X11 pointer in place");
        }
    }

    /// The window of X id `xid` was moved, resized or rescaled to a
    /// content of `width` by `height` device pixels (`ConfigureNotify`).
    /// A lock's point is the content's, so it moves with the window; it
    /// is held inside the content as it shrinks, and the pointer, which
    /// the server left where it was on the screen, is put back there. A
    /// confinement follows the window by itself: the grab's `confine_to`
    /// is the window, and the server keeps the pointer inside it as it is
    /// reconfigured.
    fn window_reconfigured(&self, xid: xproto::Window, width: u16, height: u16) {
        let Some(mut held) = self
            .held
            .get()
            .filter(|held| held.grab == CursorGrab::Locked && held.window.xid == xid)
        else {
            return;
        };
        let inside = |at: i16, extent: u16| {
            let last = i16::try_from(extent.saturating_sub(1)).unwrap_or(i16::MAX);
            at.clamp(0, last)
        };
        held.at = (inside(held.at.0, width), inside(held.at.1, height));
        self.held.set(Some(held));
        self.warp_to(xid, held.at);
    }

    /// Whether `event`, over the window of X id `xid`, is a motion of the
    /// cursor that window holds in place: it is no move, and the pointer
    /// it took away from where the lock holds it is put back there.
    fn held_in_place(&self, xid: xproto::Window, event: &XiEvent) -> bool {
        let Some(held) = self
            .held
            .get()
            .filter(|held| held.grab == CursorGrab::Locked && held.window.xid == xid)
        else {
            return false;
        };
        if !self.with_state(|state| state.pointers.moves_cursor(event)) {
            return false;
        }
        let at = (f64::from(held.at.0), f64::from(held.at.1));
        if (event.position.0.round(), event.position.1.round()) != at {
            self.warp_to(xid, held.at);
        }
        true
    }

    /// `XI_RawMotion` while a window holds the pointer in place: a
    /// physical device's unaccelerated motion along valuators 0 and 1 as
    /// that window's raw motion, of the master pointer the grab took. A
    /// motion along neither delivers nothing.
    ///
    /// Only a slave device's own event is read: the server sends a
    /// grabbed master's raw event to the grabbing client twice, through
    /// its grab and through its root selection, and its slave's once.
    fn raw_motion(&self, raw: &xinput::RawMotionEvent) -> Vec<(Arc<WindowDelivery>, Delivery)> {
        let Some(held) = self
            .held
            .get()
            .filter(|held| held.grab == CursorGrab::Locked)
        else {
            return Vec::new();
        };
        if raw.deviceid != raw.sourceid {
            return Vec::new();
        }
        let values: Vec<f64> = raw.axisvalues_raw.iter().copied().map(fp3232).collect();
        let moved = valuators(&raw.valuator_mask, &values);
        let along = |axis: u16| {
            moved
                .iter()
                .find(|(number, _)| *number == axis)
                .map_or(0.0, |(_, value)| *value)
        };
        let (dx, dy) = (along(0), along(1));
        if dx == 0.0 && dy == 0.0 {
            return Vec::new();
        }
        let Ok(delta) = MotionDelta::try_new(dx, dy) else {
            return Vec::new();
        };
        self.with_state(|state| {
            let Some(target) = state
                .windows
                .get(&held.window.xid)
                .filter(|target| target.window == held.window)
            else {
                return Vec::new();
            };
            state
                .pointers
                .raw_motion(
                    surface_of(target.window),
                    held.master,
                    XiSlave(raw.sourceid),
                    xi_time(raw.time),
                    delta,
                )
                .map(|input| (Arc::clone(&target.delivery), Delivery::Input(input)))
                .into_iter()
                .collect()
        })
    }

    /// Select a window's XInput 2 events: every master device's pointer,
    /// touch, key and, on XInput 2.4, pinch events.
    fn select(&self, window: xproto::Window) {
        if !self.xinput {
            return;
        }
        let mut events = vec![
            xinput::MOTION_EVENT,
            xinput::BUTTON_PRESS_EVENT,
            xinput::BUTTON_RELEASE_EVENT,
            xinput::ENTER_EVENT,
            xinput::LEAVE_EVENT,
            xinput::KEY_PRESS_EVENT,
            xinput::KEY_RELEASE_EVENT,
            xinput::TOUCH_BEGIN_EVENT,
            xinput::TOUCH_UPDATE_EVENT,
            xinput::TOUCH_END_EVENT,
        ];
        if self.gestures {
            events.extend([
                xinput::GESTURE_PINCH_BEGIN_EVENT,
                xinput::GESTURE_PINCH_UPDATE_EVENT,
                xinput::GESTURE_PINCH_END_EVENT,
                xinput::GESTURE_SWIPE_BEGIN_EVENT,
                xinput::GESTURE_SWIPE_UPDATE_EVENT,
                xinput::GESTURE_SWIPE_END_EVENT,
            ]);
        }
        let selected = self.connection.xcb().xinput_xi_select_events(
            window,
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL_MASTER.into(),
                mask: event_mask(&events),
            }],
        );
        if let Err(error) = selected {
            tracing::debug!(%error, "an X11 window's input goes unheard");
        }
    }
}

/// The XInput 2 mask selecting `events`, by type: the bit of an event is
/// its type, and the mask is as many 32-bit words as the highest type
/// needs (`XI_GestureSwipeEnd`, type 32, is the second word's first bit;
/// x11rb names no mask bit past `XI_BarrierLeave`).
pub(super) fn event_mask(events: &[u16]) -> Vec<XIEventMask> {
    let words = events
        .iter()
        .map(|event| usize::from(*event) / 32 + 1)
        .max()
        .unwrap_or(0);
    let mut mask = vec![0_u32; words];
    for event in events {
        mask[usize::from(*event) / 32] |= 1 << (event % 32);
    }
    mask.into_iter().map(XIEventMask::from).collect()
}

impl CoreKeyboard {
    /// The name of the layout in effect; empty when XKB names none.
    fn layout_name(&self) -> String {
        usize::try_from(self.keyboard.layout())
            .ok()
            .and_then(|index| self.layouts.get(index))
            .cloned()
            .unwrap_or_default()
    }
}

/// The names XKB gives the core keyboard's layouts (its groups), in
/// order: `English (US)`, `Russian`.
fn group_names(connection: &X11Connection, device: u16) -> Vec<String> {
    let xcb = connection.xcb();
    let Some(reply) = xcb
        .xkb_get_names(device, xkb::NameDetail::GROUP_NAMES)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return Vec::new();
    };
    reply
        .value_list
        .groups
        .unwrap_or_default()
        .into_iter()
        .map(|atom| {
            xcb.get_atom_name(atom)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map_or_default(|reply| String::from_utf8_lossy(&reply.name).into_owned())
        })
        .collect()
}

/// The surface a window's contacts belong to: its identity, which no
/// other window of the platform has.
fn surface_of(window: WindowRef) -> NativeSurface {
    NativeSurface::of_window(window.id)
}

/// Deliver `deliveries` in order, each in a boundary of its own: one
/// handler's panic leaves the rest delivered, and the first is raised
/// again after.
fn deliver(deliveries: Vec<(Arc<WindowDelivery>, Delivery)>) {
    let mut failure = flui_foundation::panic::FirstPanic::new();
    for (delivery, item) in deliveries {
        failure.contain(|| match item {
            Delivery::Input(input) => {
                delivery.dispatch_input(input);
            }
            Delivery::Hover(hovered) => delivery.report_hover(hovered),
            Delivery::Present(capabilities) => delivery.set_input_capabilities(capabilities),
        });
    }
    failure.resume();
}

impl LoopInput for X11Input {
    fn capabilities(&self) -> InputCapabilities {
        let present = self.with_state(|state| state.pointers.present());
        self.capabilities_now().with_present(present)
    }

    fn window_opened(&self, target: InputTarget) {
        let xid = target.window.xid;
        self.select(xid);
        let replaced = self.with_state(|state| state.windows.insert(xid, target));
        // A stale entry of the X id goes outside the borrow: it holds a
        // delivery, and with it a handler.
        drop(replaced);
    }

    fn window_closed(&self, window: WindowRef) {
        self.let_pointer_go(window);
        let removed = self.with_state(|state| {
            // The X id may already name a later window: only this one's
            // entry goes.
            if state
                .windows
                .get(&window.xid)
                .is_some_and(|target| target.window.id == window.id)
            {
                let time = EventTime::from_nanos(event_timestamp_ns());
                // The window's handler hears nothing more.
                drop(state.pointers.cancel_surface(
                    surface_of(window),
                    NativeCancel::FocusLost,
                    time,
                ));
                return state.windows.remove(&window.xid);
            }
            None
        });
        // The entry goes outside the borrow: it holds the delivery.
        drop(removed);
    }

    fn pointer_taken(&self, window: xproto::Window) {
        let time = EventTime::from_nanos(event_timestamp_ns());
        let deliveries = self.with_state(|state| {
            let Some(target) = state.windows.get(&window) else {
                return Vec::new();
            };
            let delivery = Arc::clone(&target.delivery);
            state
                .pointers
                .cancel_surface(surface_of(target.window), NativeCancel::CaptureLost, time)
                .into_iter()
                .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                .collect()
        });
        deliver(deliveries);
    }

    fn event(&self, event: &Event) -> bool {
        let deliveries = match event {
            Event::XinputKeyPress(key) | Event::XinputKeyRelease(key) => {
                let state = if matches!(event, Event::XinputKeyPress(_)) {
                    KeyState::Down(KeyPress::First)
                } else {
                    KeyState::Up
                };
                // A key is read with the keymap the server has now.
                self.read_stale_keymap();
                // The text input delivers it, with no borrow of the input
                // held: now, or once the input method answered for it.
                if let Some(key) = self.key_event(key, state) {
                    self.text.key(key);
                }
                return true;
            }
            Event::XinputHierarchy(change) => self.hierarchy(change),
            Event::XinputDeviceChanged(changed) => {
                // The classes the event carries are the device's now, as
                // GTK's `handle_device_changed` reads them: no query.
                let mut classes = DeviceDescription {
                    id: changed.deviceid,
                    master: false,
                    pointer: true,
                    name: String::new(),
                    direct_touch: false,
                    axes: Vec::new(),
                    scrolls: Vec::new(),
                };
                if changed.reason == xinput::ChangeReason::DEVICE_CHANGE {
                    self.read_classes(&changed.classes, &mut classes);
                }
                self.with_state(|state| {
                    state.pointers.reset_scrolling(XiSlave(changed.sourceid));
                    if changed.reason == xinput::ChangeReason::DEVICE_CHANGE {
                        state.pointers.classes_changed(
                            XiSlave(changed.deviceid),
                            &classes.axes,
                            &classes.scrolls,
                        );
                    }
                });
                Vec::new()
            }
            Event::XkbStateNotify(state) => {
                // The state is the state of the keymap the server has now.
                self.read_stale_keymap();
                if self.keyboard_state(state) && self.read_layout() {
                    self.layout_changed();
                }
                Vec::new()
            }
            Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                self.keymap_stale.set(true);
                Vec::new()
            }
            Event::FocusOut(focus) => {
                // A grab of the keyboard (a window manager's switcher)
                // moves no focus; a focus that moved takes the window's
                // hold of the pointer with it.
                if focus.mode != NotifyMode::GRAB {
                    self.let_go_of(focus.event);
                }
                if focus.mode == NotifyMode::NORMAL {
                    deliver(self.focus_out(focus.event));
                }
                // The window follows its focus too; the loop hands it
                // the event even when a handler panicked here.
                return false;
            }
            Event::ConfigureNotify(configured) => {
                self.window_reconfigured(configured.window, configured.width, configured.height);
                // The window reads itself again too.
                return false;
            }
            Event::UnmapNotify(unmapped) => {
                // The server ends the grab of a window that is no longer
                // viewable; the window reads itself again too.
                self.let_go_of(unmapped.window);
                return false;
            }
            Event::XinputRawMotion(raw) => self.raw_motion(raw),
            _ => match copied_event(event) {
                Some((window, xi)) if self.held_in_place(window, &xi) => Vec::new(),
                Some((window, xi)) => {
                    if matches!(
                        event,
                        Event::XinputButtonPress(_) | Event::XinputTouchBegin(_)
                    ) {
                        // The keys the window holds are typed before
                        // the press that may move its caret.
                        self.text.pointer_pressed(window);
                    }
                    self.pointer_event(window, &xi)
                }
                None => return false,
            },
        };
        deliver(deliveries);
        true
    }

    fn motions(&self, run: &PointerRun) -> bool {
        let Some((window, last)) = copied_event(run.last()) else {
            return false;
        };
        // A run is one pointer's over one window: held in place, it is
        // put back once, after its last motion.
        if self.held_in_place(window, &last) {
            return true;
        }
        let deliveries = if run.passed().is_empty() {
            self.pointer_event(window, &last)
        } else {
            self.pointer_motions(window, run.passed(), last)
        };
        deliver(deliveries);
        true
    }

    fn settle(&self) {
        self.read_stale_keymap();
        // After the turn's events: the callback's panic is the turn's
        // failure, raised once every event was heard.
        if self.layout_moved.take() {
            self.layout_callback.changed().or_resume(());
        }
    }

    /// A system cursor from the theme, a hidden one as the cursor of no
    /// pixels, an image as a Render cursor made of it. The cursor is the
    /// window's attribute: a lock's grab shows its blank cursor over it
    /// until the grab ends.
    fn set_cursor(&self, window: WindowRef, cursor: &Cursor) -> Result<(), CursorError> {
        let native = self.with_state(|state| {
            if !state
                .windows
                .get(&window.xid)
                .is_some_and(|target| target.window.id == window.id)
            {
                return Err(CursorError::from(Unsupported::new(
                    UnsupportedReason::NoWindow,
                )));
            }
            match cursor {
                Cursor::Icon(icon) => state.theme_cursor(&self.connection, *icon),
                Cursor::Hidden => state.blank_cursor(&self.connection),
                Cursor::Image(image) => state.image_cursor(&self.connection, image),
            }
        })?;
        let xcb = self.connection.xcb();
        xcb.change_window_attributes(window.xid, &ChangeWindowAttributesAux::new().cursor(native))
            .map_err(|error| CursorError::failed(error.to_string()))?;
        xcb.flush()
            .map_err(|error| CursorError::failed(error.to_string()))
    }

    /// The server's pointer grab of `window`, confined to it. Confined,
    /// the pointer moves inside the window's content. Locked, the grab
    /// shows the blank cursor, the master pointers' raw motion is heard
    /// on the root window, and each motion of the cursor is put back
    /// where the lock holds it (`held_in_place`): a warp,
    /// rather than a confinement to a window of one pixel, whose
    /// crossing would read as the pointer leaving the window.
    fn hold_pointer(&self, window: WindowRef, grab: CursorGrab) -> Result<(), PlatformFailure> {
        self.let_go();
        let locked = match grab {
            CursorGrab::None => return Ok(()),
            CursorGrab::Confined => false,
            CursorGrab::Locked => true,
        };
        let failed = |message: String| PlatformFailure::Failed { message };
        let cursor = if locked {
            self.with_state(|state| state.blank_cursor(&self.connection))
                .map_err(|error| failed(error.to_string()))?
        } else {
            x11rb::NONE
        };
        let at = if locked {
            self.lock_point(window)
        } else {
            (0, 0)
        };
        let xcb = self.connection.xcb();
        let answer = xcb
            .grab_pointer(
                true,
                window.xid,
                EventMask::NO_EVENT,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                window.xid,
                cursor,
                x11rb::CURRENT_TIME,
            )
            .map_err(|error| failed(error.to_string()))?
            .reply()
            .map_err(|error| failed(error.to_string()))?;
        if answer.status != GrabStatus::SUCCESS {
            return Err(failed(format!(
                "the X server refused the pointer grab: {:?}",
                answer.status
            )));
        }
        // The core grab takes the client's pointer, a master.
        let master = locked
            .then(|| xcb.xinput_xi_get_client_pointer(window.xid).ok())
            .flatten()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| XiMaster(reply.deviceid))
            .filter(|master| master.0 != 0);
        self.held.set(Some(Held {
            window,
            grab,
            at,
            master,
        }));
        if locked {
            self.select_root_events(true);
            self.warp_to(window.xid, at);
        }
        Ok(())
    }

    fn let_pointer_go(&self, window: WindowRef) {
        if self.held.get().is_some_and(|held| held.window == window) {
            self.let_go();
        }
    }

    fn pointer_grab(&self, window: WindowRef) -> CursorGrab {
        self.held
            .get()
            .filter(|held| held.window == window)
            .map_or(CursorGrab::None, |held| held.grab)
    }

    fn keyboard_layout(&self) -> String {
        self.layout.borrow().clone()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.layout_callback.register(callback);
    }
}

mod cursors;
#[cfg(test)]
mod tests;
