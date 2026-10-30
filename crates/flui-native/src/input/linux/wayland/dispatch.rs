//! `SeatInput`: the Wayland loop's input hook over `wayland-client`.
//!
//! It binds `wl_seat`, `zwp_tablet_manager_v2`, `zwp_pointer_gestures_v1`
//! and `wp_cursor_shape_manager_v1` as the compositor announces them, makes
//! each seat's devices as its capabilities say, and implements the dispatch
//! of their objects on the owner's queue. A dispatch only records: it copies
//! the event into a wire event ([`super::wire`]) or a keyboard event. On each
//! turn the loop asks it to deliver ([`WaylandInput::deliver`]): the wire
//! events become owned input through the one assembly, the keys through the
//! compositor's keymap, and each reaches its window's delivery with no cell
//! of the input borrowed, so a handler that opens or closes a window, sets a
//! cursor or quits finds the input as it was.
//!
//! What the input holds is reached on the owner thread only (`OwnerCell`);
//! the objects' user data hold it weakly, so a proxy outliving the input
//! records nothing.

use std::os::fd::OwnedFd;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use cursor_icon::CursorIcon;
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::keyboard::{KeyEvent, KeyState};
use flui_platform::pointer::{PenTool, PointerEvent, PointerKind};
use flui_platform::{
    CursorError, EventTime, PlatformInput, Unsupported, UnsupportedReason, WindowId,
};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_keyboard::{self, KeymapFormat, WlKeyboard};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, Capability, WlSeat};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child};
use wayland_cursor::CursorTheme;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    self, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::{
    self, WpCursorShapeManagerV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{
    self, ZwpPointerGesturePinchV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::{
    self, ZwpPointerGesturesV1,
};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_manager_v2::{self, ZwpTabletManagerV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_group_v2::{
    self, ZwpTabletPadGroupV2,
};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_ring_v2::{
    self, ZwpTabletPadRingV2,
};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_strip_v2::{
    self, ZwpTabletPadStripV2,
};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_v2::{self, ZwpTabletPadV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::{self, ZwpTabletSeatV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::{
    self, ButtonState, Type as ToolType, ZwpTabletToolV2,
};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_v2::{self, ZwpTabletV2};
use web_time::Instant;

use super::cursor;
use super::keyboards::Keyboards;
use super::wire::{
    Axis, AxisSource, PinchWire, PointerWire, Routed, SeatAssembly, SeatEvent, SeatKey, SeatWire,
    ToolWire, TouchWire, WireTime,
};
use crate::input::assembly::{NativeClock, NativeDevice, NativeSurface};
use crate::input::availability::{self, WaylandInputGlobals};
use crate::input::event_timestamp_ns;
use crate::input::linux::xkb::{EVDEV_OFFSET, Keyboard, KeymapSource, keymap_length};
#[cfg(any(test, feature = "testing"))]
use crate::owner::linux::wayland::ShownCursor;
use crate::owner::linux::wayland::activation::UserInput;
use crate::owner::linux::wayland::{
    FramePointer, LastInput, SeatWindow, WaylandInput, WaylandState,
};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::slot::OwnerCell;
use crate::owner::window_delivery::WindowDelivery;

/// The newest `wl_seat` the input understands: version 9 says which way a
/// scroll turned relative to the fingers.
const SEAT_VERSION: u32 = 9;
/// `zwp_tablet_manager_v2` version 1: no pad dials.
const TABLET_VERSION: u32 = 1;
/// `zwp_pointer_gestures_v1` version 1: pinches (and swipes, unasked).
const GESTURES_VERSION: u32 = 1;
/// `wp_cursor_shape_manager_v1` version 1.
const CURSOR_SHAPE_VERSION: u32 = 1;
/// The `wl_pointer` version from which events come in frames.
const POINTER_FRAMES: u32 = 5;
/// `wl_compositor` version 3: a cursor surface takes a buffer scale.
const COMPOSITOR_VERSION: u32 = 3;
/// The tablet tools a seat keeps: a pen, its eraser, an airbrush and a lens
/// for each of a few tablets. A compositor announcing tools it never removes
/// is refused past this rather than held forever.
const MAX_TOOLS: usize = 16;
/// The seats the input binds. A seat is a user at the machine, and a
/// compositor with more than a few is unheard of; a compositor announcing
/// seats without end is not bound past this.
const MAX_SEATS: usize = 16;

/// `CLOCK_MONOTONIC`, the clock compositors stamp input on.
fn monotonic() -> Duration {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    Duration::new(
        u64::try_from(now.tv_sec).unwrap_or(0),
        u32::try_from(now.tv_nsec).unwrap_or(0),
    )
}

/// When an event arrives now, with the compositor's `stamp` if it carries
/// one.
fn arrival(stamp: Option<u32>) -> WireTime {
    let monotonic_ms = u64::try_from(monotonic().as_millis()).unwrap_or(u64::MAX);
    let arrival = EventTime::from_nanos(event_timestamp_ns());
    WireTime {
        stamp,
        monotonic_ms,
        arrival,
    }
}

/// What a dispatch records for the next delivery. Each keyboard event names
/// its seat: every seat's keyboard has its own keymap, window and repeat.
#[derive(Debug)]
enum Wire {
    /// A pointer, touch, tool or pinch event, or a device or seat that came
    /// or went.
    Seat(SeatWire),
    /// A keyboard's keymap: the compositor's file and its size, read when it
    /// is delivered.
    Keymap {
        seat: SeatKey,
        fd: OwnedFd,
        size: u32,
    },
    /// A keyboard entered the window of a surface, or left (`None`).
    KeyFocus {
        seat: SeatKey,
        focus: Option<NativeSurface>,
    },
    /// A key changed.
    Key {
        seat: SeatKey,
        time: WireTime,
        code: u32,
        pressed: bool,
    },
    /// A keyboard's modifier and layout masks.
    Modifiers {
        seat: SeatKey,
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
        time: WireTime,
    },
    /// A keyboard's repeat settings.
    RepeatInfo {
        seat: SeatKey,
        rate: i32,
        delay: i32,
    },
}

/// Record `wire` for the next delivery. A keymap replaces one its seat's
/// keyboard has queued and not read, whose file it closes unread: each is up
/// to 8 MiB read and compiled on delivery, and a newer one supersedes it, so a
/// seat queues one at most and only the newest is read.
fn queue(wires: &mut Vec<Wire>, wire: Wire) {
    if let Wire::Keymap { seat, .. } = wire
        && let Some(queued) = wires
            .iter_mut()
            .find(|queued| matches!(queued, Wire::Keymap { seat: held, .. } if *held == seat))
    {
        *queued = wire;
        return;
    }
    wires.push(wire);
}

/// A window's device pixels per logical pixel, read when a theme cursor
/// over it is drawn.
type WindowScale = Arc<dyn Fn() -> f64 + Send + Sync>;

/// A window the input delivers to.
struct Attached {
    id: WindowId,
    surface: WlSurface,
    delivery: Arc<WindowDelivery>,
    cursor: CursorIcon,
    scale: WindowScale,
    /// The frame it draws itself, whose surfaces the pointer's events go to.
    frame: Arc<dyn FramePointer>,
}

/// A pointer's objects.
struct PointerObjects {
    pointer: WlPointer,
    device: NativeDevice,
    pinch: Option<ZwpPointerGesturePinchV1>,
    shape: Option<WpCursorShapeDeviceV1>,
    /// The surface a theme cursor is drawn on, made at its first.
    cursor_surface: Option<WlSurface>,
    /// The window surface it is over and the serial of its enter.
    focus: Option<(NativeSurface, u32)>,
    /// That surface, as the protocol names it: what a button names on an
    /// activation token.
    focus_surface: Option<WlSurface>,
    /// The window frame it is over instead, which its events go to.
    frame_focus: Option<FrameFocus>,
}

/// A pointer over a surface of a window's own frame.
#[derive(Clone)]
struct FrameFocus {
    /// The frame.
    frame: Arc<dyn FramePointer>,
    /// The frame's surface the pointer entered.
    surface: WlSurface,
    /// The window's own surface, which a button names on an activation
    /// token.
    window: WlSurface,
    /// The serial of its enter, which a cursor is set with.
    serial: u32,
    /// The cursor last shown there.
    cursor: CursorIcon,
    /// The window's scale, which a theme cursor is drawn at.
    scale: WindowScale,
}

/// One seat's objects.
struct SeatObjects {
    name: u32,
    key: SeatKey,
    seat: WlSeat,
    pointer: Option<PointerObjects>,
    keyboard: Option<WlKeyboard>,
    /// The surface its keyboard is in: what a key names on an activation
    /// token.
    keyboard_focus: Option<WlSurface>,
    touch: Option<(WlTouch, NativeDevice)>,
    tablet: Option<ZwpTabletSeatV2>,
    /// The tablet tools the tablet seat announced and has not removed.
    tools: Vec<ZwpTabletToolV2>,
}

/// The protocol objects the input made.
#[derive(Default)]
struct Objects {
    seats: Vec<SeatObjects>,
    tablet: Option<(u32, ZwpTabletManagerV2)>,
    gestures: Option<(u32, ZwpPointerGesturesV1)>,
    shapes: Option<(u32, WpCursorShapeManagerV1)>,
    /// `wl_shm`, which theme cursors are drawn with.
    shm: Option<(u32, WlShm)>,
    /// `wl_compositor`, which makes the cursor surfaces.
    compositor: Option<WlCompositor>,
    /// The connection and queue the globals came on.
    connection: Option<(Connection, QueueHandle<WaylandState>)>,
    /// What theme cursors are drawn from, once all of it is bound.
    cursor_source: Option<CursorSource>,
}

impl Objects {
    /// Whether a global of `interface`, of which the input binds one, is
    /// bound already.
    fn holds(&self, interface: &str) -> bool {
        if interface == WlShm::interface().name {
            self.shm.is_some()
        } else if interface == WlCompositor::interface().name {
            self.compositor.is_some()
        } else if interface == ZwpTabletManagerV2::interface().name {
            self.tablet.is_some()
        } else if interface == ZwpPointerGesturesV1::interface().name {
            self.gestures.is_some()
        } else if interface == WpCursorShapeManagerV1::interface().name {
            self.shapes.is_some()
        } else {
            false
        }
    }

    /// Draw theme cursors once `wl_shm` and `wl_compositor` are both bound.
    fn settle_cursor_source(&mut self) {
        self.cursor_source = match (&self.shm, &self.compositor, &self.connection) {
            (Some((_, shm)), Some(compositor), Some((connection, queue))) => Some(CursorSource {
                connection: connection.clone(),
                shm: shm.clone(),
                compositor: compositor.clone(),
                queue: queue.clone(),
            }),
            _ => None,
        };
    }
}

/// One seat's keymap: where it came from, which names its layouts, and the
/// keyboard (state, compose and the keys held) read through it.
struct SeatKeymap {
    source: KeymapSource,
    keyboard: Keyboard,
}

/// What the delivery side keeps: the assembly, each seat's keymap and
/// keyboard, the layout last named, and the capabilities the windows were
/// last given.
struct Delivering {
    assembly: SeatAssembly,
    keymaps: Vec<(SeatKey, SeatKeymap)>,
    keyboards: Keyboards,
    /// The layout's name as last reported, of the keyboard whose keymap or
    /// group changed last.
    layout: String,
    /// The layout changed since the callbacks last heard.
    layout_changed: bool,
    capabilities: InputCapabilities,
}

/// What the input holds, reached on the owner thread.
struct Shared {
    wires: OwnerCell<Vec<Wire>>,
    objects: OwnerCell<Objects>,
    windows: OwnerCell<Vec<Attached>>,
    delivering: OwnerCell<Option<Delivering>>,
    layout_callbacks: OwnerCell<Vec<Box<dyn FnMut() + Send>>>,
    /// The last device label minted.
    labels: AtomicU64,
    /// The last seat key minted.
    seat_keys: AtomicU64,
    /// The cursor theme loaded at each buffer scale a cursor was drawn at.
    themes: OwnerCell<Vec<(i32, CursorTheme)>>,
    /// The cursor last shown, for a test.
    #[cfg(any(test, feature = "testing"))]
    shown: OwnerCell<Option<ShownCursor>>,
    /// The last user input, which a window's activation token names.
    last_input: LastInput,
}

impl Shared {
    /// `seat` reports a user input, `serial`, that went to `surface`: what
    /// the next activation token names. Called by a dispatch with no cell of
    /// the input borrowed.
    fn record_input(&self, seat: SeatKey, serial: u32, surface: WlSurface) {
        let seat = self
            .objects
            .with(|objects| {
                objects
                    .seats
                    .iter()
                    .find(|held| held.key == seat)
                    .map(|held| held.seat.clone())
            })
            .flatten();
        if let Some(seat) = seat {
            self.last_input.record(UserInput {
                seat,
                serial,
                surface,
            });
        }
    }

    /// `seat`'s keyboard entered `surface`, a window's, or left (`None`).
    fn set_keyboard_focus(&self, seat: SeatKey, surface: Option<WlSurface>) {
        self.objects.with(|objects| {
            if let Some(held) = objects.seats.iter_mut().find(|held| held.key == seat) {
                held.keyboard_focus = surface;
            }
        });
    }

    /// The window surface `seat`'s keyboard is in.
    fn keyboard_focus(&self, seat: SeatKey) -> Option<WlSurface> {
        self.objects
            .with(|objects| {
                objects
                    .seats
                    .iter()
                    .find(|held| held.key == seat)
                    .and_then(|held| held.keyboard_focus.clone())
            })
            .flatten()
    }

    /// A seat key no seat had.
    fn seat_key(&self) -> SeatKey {
        SeatKey(self.seat_keys.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn record(&self, wire: Wire) {
        self.wires.with(|wires| queue(wires, wire));
    }

    fn record_seat(&self, time: WireTime, event: SeatEvent) {
        self.record(Wire::Seat(SeatWire { time, event }));
    }

    /// A device label no object had.
    fn label(&self) -> NativeDevice {
        NativeDevice(self.labels.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The frame `surface` is a surface of, with its window's own surface
    /// and scale, when it is one of a window's frame.
    fn frame_of(
        &self,
        surface: &WlSurface,
    ) -> Option<(Arc<dyn FramePointer>, WlSurface, WindowScale)> {
        self.windows
            .with(|windows| {
                windows
                    .iter()
                    .find(|window| window.frame.holds(surface))
                    .map(|window| {
                        (
                            Arc::clone(&window.frame),
                            window.surface.clone(),
                            Arc::clone(&window.scale),
                        )
                    })
            })
            .flatten()
    }

    /// The window surface `surface` is, when it is a window's.
    fn surface_of(&self, surface: &WlSurface) -> Option<NativeSurface> {
        self.windows
            .with(|windows| {
                windows
                    .iter()
                    .find(|window| window.surface == *surface)
                    .map(|window| surface_key(window.id))
            })
            .flatten()
    }

    fn globals(&self) -> WaylandInputGlobals {
        self.objects
            .with(|objects| WaylandInputGlobals {
                tablet: objects.tablet.is_some(),
                pointer_gestures: objects.gestures.is_some(),
            })
            .unwrap_or_default()
    }
}

/// The surface key of window `id`.
fn surface_key(id: WindowId) -> NativeSurface {
    NativeSurface(id.as_u64())
}

/// The Wayland seats' input: the loop's input hook. See the module docs.
pub(crate) struct SeatInput {
    shared: Arc<Shared>,
}

impl SeatInput {
    /// The input of a loop on the calling thread, with no global bound yet,
    /// recording each user input in `last_input`.
    pub(crate) fn new(last_input: LastInput) -> Self {
        let clock = NativeClock::new(
            EventTime::from_nanos(event_timestamp_ns()),
            monotonic(),
            None,
        );
        let shared = Arc::new(Shared {
            wires: OwnerCell::new(),
            objects: OwnerCell::new(),
            windows: OwnerCell::new(),
            delivering: OwnerCell::new(),
            layout_callbacks: OwnerCell::new(),
            labels: AtomicU64::new(0),
            seat_keys: AtomicU64::new(0),
            themes: OwnerCell::new(),
            #[cfg(any(test, feature = "testing"))]
            shown: OwnerCell::new(),
            last_input,
        });
        shared.delivering.with(|delivering| {
            *delivering = Some(Delivering::new(clock));
        });
        Self { shared }
    }

    /// Bind seat `name` and make what its capabilities and the managers
    /// already bound give it.
    fn bind_seat(
        &self,
        registry: &WlRegistry,
        name: u32,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        let seats = self
            .shared
            .objects
            .with(|objects| objects.seats.len())
            .unwrap_or_default();
        if seats >= MAX_SEATS {
            tracing::debug!(name, "a Wayland seat past {MAX_SEATS} was not bound");
            return;
        }
        let key = self.shared.seat_key();
        let seat = registry.bind::<WlSeat, SeatData, WaylandState>(
            name,
            version.min(SEAT_VERSION),
            queue,
            SeatData {
                shared: Arc::downgrade(&self.shared),
                seat: key,
            },
        );
        self.shared.objects.with(|objects| {
            let tablet = objects
                .tablet
                .as_ref()
                .map(|(_, manager)| tablet_seat(manager, &seat, key, queue, &self.shared));
            objects.seats.push(SeatObjects {
                name,
                key,
                seat,
                pointer: None,
                keyboard: None,
                keyboard_focus: None,
                touch: None,
                tablet,
                tools: Vec::new(),
            });
        });
    }

    /// The input's answer to `wire`, each input with the deliveries it goes
    /// to; the keyboard's layout change noted for the callbacks.
    fn assemble(&self, wire: Wire) -> Vec<(Target, PlatformInput)> {
        self.shared
            .delivering
            .with(|delivering| {
                let delivering = delivering
                    .as_mut()
                    .expect("BUG: the input holds its assembly from construction");
                match wire {
                    Wire::Seat(wire) => {
                        if let SeatEvent::SeatRemoved(seat) = wire.event {
                            delivering.keyboards.remove(seat);
                            delivering.keymaps.retain(|(held, _)| *held != seat);
                        }
                        delivering
                            .assembly
                            .assemble(wire)
                            .into_iter()
                            .map(|routed| match routed {
                                Routed::To(surface, input) => (Target::Surface(surface), input),
                                Routed::Everywhere(input) => (Target::Everywhere, input),
                            })
                            .collect()
                    }
                    other => delivering.keyboard_wire(other),
                }
            })
            .unwrap_or_default()
    }

    /// Hand `inputs` to their windows, with no cell borrowed.
    fn hand_out(&self, inputs: Vec<(Target, PlatformInput)>) {
        for (target, input) in inputs {
            let deliveries: Vec<Arc<WindowDelivery>> = self
                .shared
                .windows
                .with(|windows| {
                    windows
                        .iter()
                        .filter(|window| match target {
                            Target::Surface(surface) => surface_key(window.id) == surface,
                            Target::Everywhere => true,
                        })
                        .map(|window| Arc::clone(&window.delivery))
                        .collect()
                })
                .unwrap_or_default();
            let hover = match &input {
                PlatformInput::Pointer(PointerEvent::Enter(signal))
                    if signal.pointer.kind == PointerKind::Mouse =>
                {
                    Some(true)
                }
                PlatformInput::Pointer(PointerEvent::Leave(signal))
                    if signal.pointer.kind == PointerKind::Mouse =>
                {
                    Some(false)
                }
                _ => None,
            };
            for delivery in deliveries {
                delivery.dispatch_input(input.clone());
                if let Some(hovered) = hover {
                    delivery.report_hover(hovered);
                }
            }
        }
    }

    /// Give every window the capabilities the devices present and the
    /// globals bound make now, when they changed.
    fn refresh_capabilities(&self) {
        let globals = self.shared.globals();
        let changed = self
            .shared
            .delivering
            .with(|delivering| {
                let delivering = delivering.as_mut()?;
                let capabilities =
                    availability::wayland(globals).with_present(delivering.assembly.present());
                (capabilities != delivering.capabilities).then(|| {
                    delivering.capabilities = capabilities;
                    capabilities
                })
            })
            .flatten();
        let Some(capabilities) = changed else {
            return;
        };
        let deliveries = self.deliveries();
        for delivery in deliveries {
            delivery.set_input_capabilities(capabilities);
        }
    }

    fn deliveries(&self) -> Vec<Arc<WindowDelivery>> {
        self.shared
            .windows
            .with(|windows| {
                windows
                    .iter()
                    .map(|window| Arc::clone(&window.delivery))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The repeat of the held key, when it came due.
    fn repeat_due(&self) -> Vec<(Target, PlatformInput)> {
        self.shared
            .delivering
            .with(|delivering| {
                let Some(delivering) = delivering.as_mut() else {
                    return Vec::new();
                };
                let time = EventTime::from_nanos(event_timestamp_ns());
                let due = delivering.keyboards.take_due(Instant::now());
                due.into_iter()
                    .filter_map(|(seat, code, focus)| {
                        let event = delivering.key_event(seat, code, KeyState::Down, true, time)?;
                        Some((Target::Surface(focus), PlatformInput::Keyboard(event)))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tell the layout callbacks, outside every cell, when the layout
    /// changed.
    fn report_layout(&self) {
        let changed = self
            .shared
            .delivering
            .with(|delivering| {
                delivering
                    .as_mut()
                    .is_some_and(|delivering| std::mem::take(&mut delivering.layout_changed))
            })
            .unwrap_or(false);
        if !changed {
            return;
        }
        let mut callbacks = self
            .shared
            .layout_callbacks
            .with(std::mem::take)
            .unwrap_or_default();
        for callback in &mut callbacks {
            contain_owner_callback(&mut *callback);
        }
        // A callback registered while these ran is kept after them.
        self.shared.layout_callbacks.with(|held| {
            callbacks.append(held);
            *held = callbacks;
        });
    }
}

/// Where an input goes.
#[derive(Debug, Clone, Copy)]
enum Target {
    Surface(NativeSurface),
    Everywhere,
}

impl Delivering {
    /// Nothing delivered yet, readings stamped on `clock`.
    fn new(clock: NativeClock) -> Self {
        Self {
            assembly: SeatAssembly::new(clock),
            keymaps: Vec::new(),
            keyboards: Keyboards::default(),
            layout: String::new(),
            layout_changed: false,
            capabilities: availability::wayland(WaylandInputGlobals::default()),
        }
    }

    fn keymap(&self, seat: SeatKey) -> Option<&SeatKeymap> {
        self.keymaps
            .iter()
            .find(|(held, _)| *held == seat)
            .map(|(_, keymap)| keymap)
    }

    fn keymap_mut(&mut self, seat: SeatKey) -> Option<&mut SeatKeymap> {
        self.keymaps
            .iter_mut()
            .find(|(held, _)| *held == seat)
            .map(|(_, keymap)| keymap)
    }

    /// The key event of `seat`'s key `code` (the kernel's) going `state` at
    /// `time`, as its keyboard reads it now, compose included; `None` before
    /// a keymap came.
    fn key_event(
        &mut self,
        seat: SeatKey,
        code: u32,
        state: KeyState,
        repeat: bool,
        time: EventTime,
    ) -> Option<KeyEvent> {
        let keyboard = &mut self.keymap_mut(seat)?.keyboard;
        keyboard.key(code.saturating_add(EVDEV_OFFSET), state, repeat, time)
    }

    fn keyboard_wire(&mut self, wire: Wire) -> Vec<(Target, PlatformInput)> {
        match wire {
            Wire::Keymap { seat, fd, size } => {
                let source = match read_keymap(fd, size) {
                    Ok(source) => source,
                    Err(error) => {
                        tracing::warn!(%error, "the compositor's keymap could not be read");
                        return Vec::new();
                    }
                };
                self.keyboards.stop(seat);
                // A new keymap for a keyboard that has one is read into it:
                // the keys it holds stay held, and compose starts over.
                // Said once, when the keymap is taken, rather than at each
                // group change.
                if source.layout_name(0).is_none() {
                    let head = source.head(400);
                    tracing::info!(?seat, %head, "the compositor's keymap names no layout");
                }
                let read = match self.keymap_mut(seat) {
                    Some(held) => {
                        let read = held.keyboard.reload(&source);
                        if read {
                            held.source = source;
                        }
                        read
                    }
                    None => match Keyboard::try_new(&source) {
                        Some(keyboard) => {
                            self.keymaps.push((seat, SeatKeymap { source, keyboard }));
                            true
                        }
                        None => false,
                    },
                };
                if !read {
                    tracing::warn!("the compositor's keymap was not compiled: keys are unread");
                }
                self.note_layout(seat);
                if self.keyboards.focused(seat).is_some() {
                    self.send_modifiers(seat, arrival(None));
                }
                Vec::new()
            }
            Wire::KeyFocus { seat, focus } => {
                let left = self.keyboards.focused(seat);
                self.keyboards.focus(seat, focus);
                if focus.is_some() {
                    return Vec::new();
                }
                // What the keyboard held stays with it: the window's next
                // readings carry no modifier it no longer hears of, and the
                // keys still held are released in the window they were
                // pressed in, where compose ends too.
                self.assembly.assemble(SeatWire {
                    time: arrival(None),
                    event: SeatEvent::KeyboardLeft(seat),
                });
                let time = EventTime::from_nanos(event_timestamp_ns());
                let released = self
                    .keymap_mut(seat)
                    .map(|keymap| keymap.keyboard.leave(time))
                    .unwrap_or_default();
                let Some(left) = left else {
                    return Vec::new();
                };
                released
                    .into_iter()
                    .map(|event| (Target::Surface(left), PlatformInput::Keyboard(event)))
                    .collect()
            }
            Wire::Key {
                seat,
                time,
                code,
                pressed,
            } => {
                let stamp = self.assembly.stamp(time);
                let state = if pressed {
                    KeyState::Down
                } else {
                    KeyState::Up
                };
                // The keyboard reads every key, so a key pressed outside a
                // window is not left held in it.
                let event = self.key_event(seat, code, state, false, stamp);
                let focus = if !pressed {
                    self.keyboards.release(seat, code)
                } else if event.is_some() {
                    let repeats = self.keymap(seat).is_some_and(|keymap| {
                        keymap.keyboard.repeats(code.saturating_add(EVDEV_OFFSET))
                    });
                    self.keyboards.press(seat, code, repeats, Instant::now())
                } else {
                    // A press the keyboard refused does not repeat either.
                    None
                };
                let Some(focus) = focus else {
                    return Vec::new();
                };
                event
                    .map(|event| (Target::Surface(focus), PlatformInput::Keyboard(event)))
                    .into_iter()
                    .collect()
            }
            Wire::Modifiers {
                seat,
                depressed,
                latched,
                locked,
                group,
                time,
            } => {
                // The compositor sends the layout in effect; it is the locked
                // one as XKB counts them.
                let layout_moved = self.keymap_mut(seat).is_some_and(|keymap| {
                    keymap
                        .keyboard
                        .update_mask(depressed, latched, locked, [0, 0, group])
                });
                if layout_moved {
                    self.note_layout(seat);
                }
                if self.keyboards.focused(seat).is_some() {
                    self.send_modifiers(seat, time);
                }
                Vec::new()
            }
            Wire::RepeatInfo { seat, rate, delay } => {
                self.keyboards.configure(seat, rate, delay);
                Vec::new()
            }
            Wire::Seat(_) => Vec::new(),
        }
    }

    /// Tell the assembly what `seat`'s keyboard holds now.
    fn send_modifiers(&mut self, seat: SeatKey, time: WireTime) {
        let modifiers = self
            .keymap(seat)
            .map_or(flui_platform::keyboard::Modifiers::NONE, |keymap| {
                keymap.keyboard.modifiers()
            });
        self.assembly.assemble(SeatWire {
            time,
            event: SeatEvent::Modifiers { seat, modifiers },
        });
    }

    /// Note the name of `seat`'s layout, as its keymap names the XKB group in
    /// effect, and that the layout changed when it did.
    fn note_layout(&mut self, seat: SeatKey) {
        let layout = self
            .keymap(seat)
            .and_then(|keymap| keymap.source.layout_name(keymap.keyboard.layout()))
            .map(str::to_owned)
            .unwrap_or_default();
        if layout != self.layout {
            self.layout = layout;
            self.layout_changed = true;
        }
    }
}

impl WaylandInput for SeatInput {
    fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        self.shared.objects.with(|objects| {
            if objects.connection.is_none()
                && let Some(backend) = registry.backend().upgrade()
            {
                objects.connection = Some((Connection::from_backend(backend), queue.clone()));
            }
        });
        // One of each manager is bound: another announcement of one already
        // held would bind an object nothing destroys.
        if self
            .shared
            .objects
            .with(|objects| objects.holds(interface))
            .unwrap_or_default()
        {
            tracing::debug!(
                interface,
                "a second Wayland global of one kind was not bound"
            );
            return;
        }
        if interface == WlShm::interface().name {
            let shm = registry.bind::<WlShm, CursorShm, WaylandState>(name, 1, queue, CursorShm);
            self.shared.objects.with(|objects| {
                objects.shm = Some((name, shm));
                objects.settle_cursor_source();
            });
        } else if interface == WlCompositor::interface().name {
            let compositor = registry.bind::<WlCompositor, (), WaylandState>(
                name,
                version.min(COMPOSITOR_VERSION),
                queue,
                (),
            );
            self.shared.objects.with(|objects| {
                objects.compositor = Some(compositor);
                objects.settle_cursor_source();
            });
        } else if interface == WlSeat::interface().name {
            self.bind_seat(registry, name, version, queue);
        } else if interface == ZwpTabletManagerV2::interface().name {
            let manager = registry.bind::<ZwpTabletManagerV2, (), WaylandState>(
                name,
                version.min(TABLET_VERSION),
                queue,
                (),
            );
            self.shared.objects.with(|objects| {
                for seat in &mut objects.seats {
                    if seat.tablet.is_none() {
                        seat.tablet = Some(tablet_seat(
                            &manager,
                            &seat.seat,
                            seat.key,
                            queue,
                            &self.shared,
                        ));
                    }
                }
                objects.tablet = Some((name, manager));
            });
        } else if interface == ZwpPointerGesturesV1::interface().name {
            let manager = registry.bind::<ZwpPointerGesturesV1, (), WaylandState>(
                name,
                version.min(GESTURES_VERSION),
                queue,
                (),
            );
            self.shared.objects.with(|objects| {
                for pointer in objects
                    .seats
                    .iter_mut()
                    .filter_map(|seat| seat.pointer.as_mut())
                {
                    if pointer.pinch.is_none() {
                        pointer.pinch = Some(pinch(&manager, pointer, queue, &self.shared));
                    }
                }
                objects.gestures = Some((name, manager));
            });
        } else if interface == WpCursorShapeManagerV1::interface().name {
            let manager = registry.bind::<WpCursorShapeManagerV1, (), WaylandState>(
                name,
                version.min(CURSOR_SHAPE_VERSION),
                queue,
                (),
            );
            self.shared.objects.with(|objects| {
                for pointer in objects
                    .seats
                    .iter_mut()
                    .filter_map(|seat| seat.pointer.as_mut())
                {
                    if pointer.shape.is_none() {
                        pointer.shape = Some(manager.get_pointer(&pointer.pointer, queue, ()));
                    }
                }
                objects.shapes = Some((name, manager));
            });
        }
    }

    fn global_removed(&self, name: u32) {
        let removed = self
            .shared
            .objects
            .with(|objects| {
                if objects
                    .tablet
                    .as_ref()
                    .is_some_and(|(held, _)| *held == name)
                {
                    objects.tablet = None;
                }
                if objects
                    .gestures
                    .as_ref()
                    .is_some_and(|(held, _)| *held == name)
                {
                    objects.gestures = None;
                }
                if objects
                    .shapes
                    .as_ref()
                    .is_some_and(|(held, _)| *held == name)
                {
                    objects.shapes = None;
                }
                if objects.shm.as_ref().is_some_and(|(held, _)| *held == name) {
                    objects.shm = None;
                    objects.settle_cursor_source();
                }
                let index = objects.seats.iter().position(|seat| seat.name == name)?;
                Some(objects.seats.remove(index))
            })
            .flatten();
        if let Some(seat) = removed {
            release_seat(seat, &self.shared, arrival(None));
        }
    }

    fn deliver(&self) {
        // A handler that opens a window dispatches the queue meanwhile, which
        // records more: it is delivered in this turn too.
        loop {
            let wires = self.shared.wires.with(std::mem::take).unwrap_or_default();
            if wires.is_empty() {
                break;
            }
            for wire in wires {
                let inputs = self.assemble(wire);
                self.refresh_capabilities();
                self.hand_out(inputs);
            }
        }
        let repeated = self.repeat_due();
        self.hand_out(repeated);
        self.report_layout();
    }

    fn next_due(&self) -> Option<Instant> {
        self.shared
            .delivering
            .with(|delivering| delivering.as_ref()?.keyboards.due())
            .flatten()
    }

    fn attach(&self, window: SeatWindow) {
        let capabilities = self
            .shared
            .delivering
            .with(|delivering| delivering.as_ref().map(|held| held.capabilities))
            .flatten()
            .unwrap_or_else(|| availability::wayland(self.shared.globals()));
        window.delivery.set_input_capabilities(capabilities);
        self.shared.windows.with(|windows| {
            windows.retain(|held| held.id != window.id);
            windows.push(Attached {
                id: window.id,
                surface: window.surface,
                delivery: window.delivery,
                cursor: CursorIcon::Default,
                scale: window.scale,
                frame: window.frame,
            });
        });
    }

    fn detach(&self, id: WindowId) {
        let removed = self
            .shared
            .windows
            .with(|windows| {
                let index = windows.iter().position(|held| held.id == id)?;
                Some(windows.remove(index))
            })
            .flatten();
        if removed.is_none() {
            return;
        }
        let surface = surface_key(id);
        self.shared.objects.with(|objects| {
            for pointer in objects
                .seats
                .iter_mut()
                .filter_map(|seat| seat.pointer.as_mut())
            {
                if pointer.focus.is_some_and(|(focus, _)| focus == surface) {
                    pointer.focus = None;
                }
                if let Some(removed) = &removed
                    && pointer
                        .frame_focus
                        .as_ref()
                        .is_some_and(|focus| Arc::ptr_eq(&focus.frame, &removed.frame))
                {
                    pointer.frame_focus = None;
                }
            }
        });
        self.shared.delivering.with(|delivering| {
            if let Some(delivering) = delivering {
                delivering.keyboards.forget_surface(surface);
            }
        });
        self.shared
            .record_seat(arrival(None), SeatEvent::SurfaceGone(surface));
        // The window's own drop may release what its handler held.
        contain_owner_callback(|| drop(removed));
    }

    fn set_cursor(&self, id: WindowId, cursor: CursorIcon) -> Result<(), CursorError> {
        let known = self
            .shared
            .windows
            .with(|windows| {
                windows
                    .iter_mut()
                    .find(|held| held.id == id)
                    .map(|held| held.cursor = cursor)
                    .is_some()
            })
            .unwrap_or(false);
        if !known {
            return Err(CursorError::Backend(
                "the window is not open on this Wayland loop".to_owned(),
            ));
        }
        let scale = self
            .shared
            .windows
            .with(|windows| {
                windows
                    .iter()
                    .find(|held| held.id == id)
                    .map(|held| (held.scale)())
            })
            .flatten()
            .unwrap_or(1.0);
        let surface = surface_key(id);
        let showable = self
            .shared
            .objects
            .with(|objects| {
                show_cursor(&self.shared, objects, surface, cursor, scale);
                objects.shapes.is_some() || objects.cursor_source.is_some()
            })
            .unwrap_or(false);
        if showable {
            Ok(())
        } else {
            // Neither `cursor-shape` nor `wl_shm` for a theme cursor.
            Err(CursorError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisDevice,
            )))
        }
    }

    fn keyboard_layout(&self) -> String {
        self.shared
            .delivering
            .with(|delivering| {
                delivering
                    .as_ref()
                    .map(|delivering| delivering.layout.clone())
            })
            .flatten()
            .unwrap_or_default()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        let held = self
            .shared
            .layout_callbacks
            .with(|callbacks| callbacks.push(callback));
        if held.is_none() {
            tracing::warn!(
                "a keyboard layout callback is registered on the Wayland owner thread only"
            );
        }
    }

    fn release(&self) {
        let windows = self.shared.windows.with(std::mem::take).unwrap_or_default();
        let callbacks = self
            .shared
            .layout_callbacks
            .with(std::mem::take)
            .unwrap_or_default();
        contain_owner_callback(|| drop(windows));
        contain_owner_callback(|| drop(callbacks));
    }

    #[cfg(any(test, feature = "testing"))]
    fn shown_cursor(&self) -> Option<ShownCursor> {
        self.shared.shown.with(|shown| shown.clone()).flatten()
    }
}

/// `wl_shm` as the input binds it for theme cursors.
pub(crate) struct CursorShm;

/// A pointer's theme-cursor surface.
pub(crate) struct CursorSurface;

impl Dispatch<WlShm, CursorShm> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: wl_shm::Event,
        _: &CursorShm,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The formats it announces include the one cursor themes use.
    }
}

impl Dispatch<WlSurface, CursorSurface> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: wl_surface::Event,
        _: &CursorSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A cursor surface's outputs and scale are its window's concern.
    }
}

/// Show `cursor` on every pointer over the window of `surface`, since that
/// pointer's enter: by its `cursor-shape` value where the compositor has
/// `cursor-shape`, else as the cursor theme draws it at `scale`.
fn show_cursor(
    shared: &Shared,
    objects: &mut Objects,
    surface: NativeSurface,
    cursor: CursorIcon,
    scale: f64,
) {
    let shapes = objects
        .shapes
        .as_ref()
        .map(|(_, manager)| manager.version());
    let source = objects.cursor_source.clone();
    for pointer in objects
        .seats
        .iter_mut()
        .filter_map(|seat| seat.pointer.as_mut())
    {
        let Some((focus, serial)) = pointer.focus else {
            continue;
        };
        if focus != surface {
            continue;
        }
        show_pointer_cursor(
            shared,
            shapes,
            source.as_ref(),
            pointer,
            serial,
            cursor,
            scale,
        );
    }
}

/// Show `cursor` on `pointer` since its enter `serial`: by its
/// `cursor-shape` value where the compositor (`shapes`, its version) has
/// `cursor-shape`, else as the cursor theme `source` draws it at `scale`.
fn show_pointer_cursor(
    shared: &Shared,
    shapes: Option<u32>,
    source: Option<&CursorSource>,
    pointer: &mut PointerObjects,
    serial: u32,
    cursor: CursorIcon,
    scale: f64,
) {
    match (&pointer.shape, shapes, source) {
        (Some(shape), Some(version), _) => {
            let value = cursor::shape(cursor, version);
            if let Ok(shape_value) = wp_cursor_shape_device_v1::Shape::try_from(value) {
                shape.set_shape(serial, shape_value);
                #[cfg(any(test, feature = "testing"))]
                shared.shown.with(|shown| {
                    *shown = Some(ShownCursor {
                        name: value.to_string(),
                        scale: 0,
                    });
                });
            }
        }
        (_, _, Some(source)) => {
            if let Some(drawn) = draw_theme_cursor(shared, source, pointer, serial, cursor, scale) {
                #[cfg(any(test, feature = "testing"))]
                shared.shown.with(|shown| {
                    *shown = Some(ShownCursor {
                        name: drawn.0.to_owned(),
                        scale: drawn.1,
                    });
                });
                #[cfg(not(any(test, feature = "testing")))]
                let _ = drawn;
            }
        }
        _ => {}
    }
}

/// Show `cursor` on `pointer`, which is over a window's own frame since the
/// enter `serial`, drawn at `scale`.
fn show_frame_cursor(
    shared: &Shared,
    objects: &mut Objects,
    pointer: &WlPointer,
    serial: u32,
    cursor: CursorIcon,
    scale: f64,
) {
    let shapes = objects
        .shapes
        .as_ref()
        .map(|(_, manager)| manager.version());
    let source = objects.cursor_source.clone();
    if let Some(held) = objects
        .seats
        .iter_mut()
        .filter_map(|seat| seat.pointer.as_mut())
        .find(|held| held.pointer == *pointer)
    {
        show_pointer_cursor(shared, shapes, source.as_ref(), held, serial, cursor, scale);
    }
}

/// What `pointer`'s `event` does over a window's own frame: an enter on one
/// of a frame's surfaces makes the frame the pointer's focus, and while it is
/// every event goes to the frame and none to a window's input. `false` for an
/// event that is not the frame's. Calls the frame with no cell of the input
/// borrowed.
fn frame_event(
    shared: &Shared,
    pointer: &WlPointer,
    data: &PointerData,
    event: &wl_pointer::Event,
) -> bool {
    if let wl_pointer::Event::Enter {
        serial,
        surface,
        surface_x,
        surface_y,
    } = event
    {
        let Some((frame, window, scale)) = shared.frame_of(surface) else {
            return false;
        };
        let cursor = frame.moved(surface, *surface_x, *surface_y);
        let shown = scale();
        shared.objects.with(|objects| {
            if let Some(held) = objects
                .seats
                .iter_mut()
                .filter_map(|seat| seat.pointer.as_mut())
                .find(|held| held.pointer == *pointer)
            {
                held.focus = None;
                held.focus_surface = None;
                held.frame_focus = Some(FrameFocus {
                    frame,
                    surface: surface.clone(),
                    window,
                    serial: *serial,
                    cursor,
                    scale,
                });
            }
            show_frame_cursor(shared, objects, pointer, *serial, cursor, shown);
        });
        return true;
    }
    let focus = shared
        .objects
        .with(|objects| {
            objects
                .seats
                .iter()
                .filter_map(|seat| seat.pointer.as_ref())
                .find(|held| held.pointer == *pointer)
                .and_then(|held| held.frame_focus.clone())
        })
        .flatten();
    let Some(focus) = focus else {
        return false;
    };
    match event {
        wl_pointer::Event::Motion {
            surface_x,
            surface_y,
            ..
        } => {
            let cursor = focus.frame.moved(&focus.surface, *surface_x, *surface_y);
            if cursor != focus.cursor {
                let scale = (focus.scale)();
                shared.objects.with(|objects| {
                    if let Some(held) = objects
                        .seats
                        .iter_mut()
                        .filter_map(|seat| seat.pointer.as_mut())
                        .find(|held| held.pointer == *pointer)
                        && let Some(frame_focus) = held.frame_focus.as_mut()
                    {
                        frame_focus.cursor = cursor;
                    }
                    show_frame_cursor(shared, objects, pointer, focus.serial, cursor, scale);
                });
            }
        }
        wl_pointer::Event::Button {
            serial,
            time,
            button,
            state,
        } => {
            let pressed = *state == WEnum::Value(wl_pointer::ButtonState::Pressed);
            if pressed {
                shared.record_input(data.seat, *serial, focus.window.clone());
            }
            let seat = shared
                .objects
                .with(|objects| {
                    objects
                        .seats
                        .iter()
                        .find(|held| held.key == data.seat)
                        .map(|held| held.seat.clone())
                })
                .flatten();
            if let Some(seat) = seat {
                focus.frame.button(&seat, *serial, *time, *button, pressed);
            }
        }
        wl_pointer::Event::Leave { .. } => {
            focus.frame.left();
            shared.objects.with(|objects| {
                if let Some(held) = objects
                    .seats
                    .iter_mut()
                    .filter_map(|seat| seat.pointer.as_mut())
                    .find(|held| held.pointer == *pointer)
                {
                    held.frame_focus = None;
                }
            });
        }
        // A scroll or a frame over the frame reaches no window.
        _ => {}
    }
    true
}

/// What a theme cursor is drawn from: the connection, `wl_shm` and
/// `wl_compositor` the input bound, and the owner's queue.
#[derive(Clone)]
struct CursorSource {
    connection: Connection,
    shm: WlShm,
    compositor: WlCompositor,
    queue: QueueHandle<WaylandState>,
}

/// The cursor theme's name and its size at scale one: `XCURSOR_THEME` and
/// `XCURSOR_SIZE`, else the default theme at [`cursor::THEME_SIZE`].
fn theme_setting() -> (String, u32) {
    let name = std::env::var("XCURSOR_THEME")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "default".to_owned());
    let size = std::env::var("XCURSOR_SIZE")
        .ok()
        .and_then(|size| size.parse::<u32>().ok())
        .filter(|size| (1..=256).contains(size))
        .unwrap_or(cursor::THEME_SIZE);
    (name, size)
}

/// Draw `cursor` from the theme on `pointer`'s cursor surface at the buffer
/// scale `scale` rounds up to, and set it since the enter `serial`. What was
/// drawn, or `None` where the theme has no image of it.
fn draw_theme_cursor(
    shared: &Shared,
    source: &CursorSource,
    pointer: &mut PointerObjects,
    serial: u32,
    cursor: CursorIcon,
    scale: f64,
) -> Option<(&'static str, i32)> {
    let scale = cursor::theme_scale(scale);
    let surface = pointer
        .cursor_surface
        .get_or_insert_with(|| {
            source
                .compositor
                .create_surface(&source.queue, CursorSurface)
        })
        .clone();
    let drawn = shared
        .themes
        .with(|themes| {
            let index = if let Some(index) = themes.iter().position(|(held, _)| *held == scale) {
                index
            } else {
                let (name, size) = theme_setting();
                let theme = CursorTheme::load_from_name(
                    &source.connection,
                    source.shm.clone(),
                    &name,
                    size.saturating_mul(scale.unsigned_abs()),
                )
                .ok()?;
                themes.push((scale, theme));
                themes.len() - 1
            };
            let theme = &mut themes[index].1;
            cursor::theme_names(cursor).into_iter().find_map(|name| {
                let image = &theme.get_cursor(name)?[0];
                let (width, height) = image.dimensions();
                let (x, y) = image.hotspot();
                let buffer: &WlBuffer = image;
                if surface.version() >= 3 {
                    surface.set_buffer_scale(scale);
                }
                surface.attach(Some(buffer), 0, 0);
                surface.damage(
                    0,
                    0,
                    i32::try_from(width).unwrap_or(i32::MAX),
                    i32::try_from(height).unwrap_or(i32::MAX),
                );
                surface.commit();
                let hotspot = |at: u32| i32::try_from(at).unwrap_or(0) / scale;
                pointer
                    .pointer
                    .set_cursor(serial, Some(&surface), hotspot(x), hotspot(y));
                Some(name)
            })
        })
        .flatten();
    if drawn.is_none() {
        tracing::debug!(?cursor, "the cursor theme has no image of the cursor");
    }
    drawn.map(|name| (name, scale))
}

/// The tablet seat of `seat`, whose key is `key`.
fn tablet_seat(
    manager: &ZwpTabletManagerV2,
    seat: &WlSeat,
    key: SeatKey,
    queue: &QueueHandle<WaylandState>,
    shared: &Arc<Shared>,
) -> ZwpTabletSeatV2 {
    manager.get_tablet_seat(
        seat,
        queue,
        SeatData {
            shared: Arc::downgrade(shared),
            seat: key,
        },
    )
}

/// The pinch of `pointer`.
fn pinch(
    manager: &ZwpPointerGesturesV1,
    pointer: &PointerObjects,
    queue: &QueueHandle<WaylandState>,
    shared: &Arc<Shared>,
) -> ZwpPointerGesturePinchV1 {
    manager.get_pinch_gesture(
        &pointer.pointer,
        queue,
        PinchData {
            shared: Arc::downgrade(shared),
            device: shared.label(),
            pointer: pointer.device,
        },
    )
}

/// Let go of a removed seat's objects. Every device of it, its tablet tools
/// among them, goes with the seat (`SeatEvent::SeatRemoved`), ending what
/// they held open.
fn release_seat(seat: SeatObjects, shared: &Shared, time: WireTime) {
    if let Some(pointer) = seat.pointer {
        if let Some(pinch) = pointer.pinch {
            pinch.destroy();
        }
        if let Some(surface) = pointer.cursor_surface {
            surface.destroy();
        }
        if let Some(shape) = pointer.shape {
            shape.destroy();
        }
        if pointer.pointer.version() >= 3 {
            pointer.pointer.release();
        }
    }
    if let Some(keyboard) = seat.keyboard
        && keyboard.version() >= 3
    {
        keyboard.release();
    }
    if let Some((touch, _)) = seat.touch
        && touch.version() >= 3
    {
        touch.release();
    }
    for tool in seat.tools {
        tool.destroy();
    }
    if let Some(tablet) = seat.tablet {
        tablet.destroy();
    }
    if seat.seat.version() >= 5 {
        seat.seat.release();
    }
    shared.record_seat(time, SeatEvent::SeatRemoved(seat.key));
}

fn release_pointer(pointer: PointerObjects, shared: &Shared, time: WireTime) {
    if let Some(pinch) = pointer.pinch {
        if let Some(data) = pinch.data::<PinchData>() {
            shared.record_seat(
                time,
                SeatEvent::DeviceRemoved {
                    device: data.device,
                },
            );
        }
        pinch.destroy();
    }
    if let Some(surface) = pointer.cursor_surface {
        surface.destroy();
    }
    if let Some(shape) = pointer.shape {
        shape.destroy();
    }
    if pointer.pointer.version() >= 3 {
        pointer.pointer.release();
    }
    shared.record_seat(
        time,
        SeatEvent::DeviceRemoved {
            device: pointer.device,
        },
    );
}

fn release_keyboard(keyboard: &WlKeyboard, seat: SeatKey, shared: &Shared) {
    if keyboard.version() >= 3 {
        keyboard.release();
    }
    shared.record(Wire::KeyFocus { seat, focus: None });
}

// ---- The dispatch of the input's objects on the owner's queue: each records
// what the compositor said, and runs no handler.

/// A seat's (and its tablet seat's) user data.
pub(crate) struct SeatData {
    shared: Weak<Shared>,
    seat: SeatKey,
}

/// A pointer's user data.
pub(crate) struct PointerData {
    shared: Weak<Shared>,
    device: NativeDevice,
    seat: SeatKey,
}

/// A keyboard's user data.
pub(crate) struct KeyboardData {
    shared: Weak<Shared>,
    seat: SeatKey,
}

/// A touchscreen's user data.
pub(crate) struct TouchData {
    shared: Weak<Shared>,
    device: NativeDevice,
    seat: SeatKey,
}

/// A pinch's user data.
pub(crate) struct PinchData {
    shared: Weak<Shared>,
    device: NativeDevice,
    pointer: NativeDevice,
}

/// A tablet tool's user data, set when its seat announces it: the tool is
/// made by the protocol, before the input sees it.
pub(crate) struct ToolData {
    held: OnceLock<(Weak<Shared>, NativeDevice, SeatKey)>,
    /// The kind its `type` event named, read on the owner thread alone.
    kind: OwnerCell<Option<PointerKind>>,
}

impl ToolData {
    /// A tool's data, made by the dispatch on the owner thread.
    fn new() -> Self {
        Self {
            held: OnceLock::new(),
            kind: OwnerCell::new(),
        }
    }

    fn shared(&self) -> Option<(Arc<Shared>, NativeDevice, SeatKey)> {
        let (shared, device, seat) = self.held.get()?;
        Some((shared.upgrade()?, *device, *seat))
    }

    fn kind(&self) -> PointerKind {
        self.kind
            .with(|kind| *kind)
            .flatten()
            .unwrap_or(PointerKind::Pen { tool: PenTool::Tip })
    }
}

impl Dispatch<WlSeat, SeatData> for WaylandState {
    fn event(
        _: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        data: &SeatData,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities { capabilities } = event else {
            return;
        };
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let capabilities = match capabilities {
            WEnum::Value(capabilities) => capabilities,
            WEnum::Unknown(bits) => Capability::from_bits_truncate(bits),
        };
        let time = arrival(None);
        let key = data.seat;
        let mut gone_pointer = None;
        let mut gone_keyboard = None;
        let mut gone_touch = None;
        shared.objects.with(|objects| {
            let gestures = objects
                .gestures
                .as_ref()
                .map(|(_, manager)| manager.clone());
            let shapes = objects.shapes.as_ref().map(|(_, manager)| manager.clone());
            let Some(held) = objects.seats.iter_mut().find(|held| held.key == key) else {
                return;
            };
            if capabilities.contains(Capability::Pointer) {
                if held.pointer.is_none() {
                    let device = shared.label();
                    let pointer = seat.get_pointer(
                        queue,
                        PointerData {
                            shared: Arc::downgrade(&shared),
                            device,
                            seat: key,
                        },
                    );
                    let mut objects = PointerObjects {
                        shape: shapes
                            .as_ref()
                            .map(|manager| manager.get_pointer(&pointer, queue, ())),
                        pointer,
                        device,
                        pinch: None,
                        cursor_surface: None,
                        focus: None,
                        focus_surface: None,
                        frame_focus: None,
                    };
                    objects.pinch = gestures
                        .as_ref()
                        .map(|manager| pinch(manager, &objects, queue, &shared));
                    held.pointer = Some(objects);
                    shared.record_seat(
                        time,
                        SeatEvent::DeviceAdded {
                            device,
                            kind: PointerKind::Mouse,
                            seat: key,
                        },
                    );
                }
            } else {
                gone_pointer = held.pointer.take();
            }
            if capabilities.contains(Capability::Keyboard) {
                if held.keyboard.is_none() {
                    held.keyboard = Some(seat.get_keyboard(
                        queue,
                        KeyboardData {
                            shared: Arc::downgrade(&shared),
                            seat: key,
                        },
                    ));
                }
            } else {
                gone_keyboard = held.keyboard.take();
            }
            if capabilities.contains(Capability::Touch) {
                if held.touch.is_none() {
                    let device = shared.label();
                    held.touch = Some((
                        seat.get_touch(
                            queue,
                            TouchData {
                                shared: Arc::downgrade(&shared),
                                device,
                                seat: key,
                            },
                        ),
                        device,
                    ));
                    shared.record_seat(
                        time,
                        SeatEvent::DeviceAdded {
                            device,
                            kind: PointerKind::Touch,
                            seat: key,
                        },
                    );
                }
            } else {
                gone_touch = held.touch.take();
            }
        });
        // The seat itself stays: only the devices it lost go.
        if let Some(pointer) = gone_pointer {
            release_pointer(pointer, &shared, time);
        }
        if let Some(keyboard) = gone_keyboard {
            release_keyboard(&keyboard, key, &shared);
        }
        if let Some((touch, device)) = gone_touch {
            if touch.version() >= 3 {
                touch.release();
            }
            shared.record_seat(time, SeatEvent::DeviceRemoved { device });
        }
    }
}

impl Dispatch<WlPointer, PointerData> for WaylandState {
    fn event(
        _: &mut Self,
        pointer: &WlPointer,
        event: wl_pointer::Event,
        data: &PointerData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        // Over a window's own frame the pointer is the frame's.
        if frame_event(&shared, pointer, data, &event) {
            return;
        }
        let axis = |axis: WEnum<wl_pointer::Axis>| match axis {
            WEnum::Value(wl_pointer::Axis::HorizontalScroll) => Some(Axis::Horizontal),
            WEnum::Value(wl_pointer::Axis::VerticalScroll) => Some(Axis::Vertical),
            _ => None,
        };
        let (time, wire) = match event {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                let Some(key) = shared.surface_of(&surface) else {
                    return;
                };
                let (cursor, scale) = shared
                    .windows
                    .with(|windows| {
                        windows
                            .iter()
                            .find(|window| surface_key(window.id) == key)
                            .map(|window| (window.cursor, (window.scale)()))
                    })
                    .flatten()
                    .unwrap_or((CursorIcon::Default, 1.0));
                shared.objects.with(|objects| {
                    for held in objects
                        .seats
                        .iter_mut()
                        .filter_map(|seat| seat.pointer.as_mut())
                    {
                        if held.pointer == *pointer {
                            held.focus = Some((key, serial));
                            held.focus_surface = Some(surface.clone());
                        }
                    }
                    show_cursor(&shared, objects, key, cursor, scale);
                });
                (
                    arrival(None),
                    PointerWire::Enter {
                        surface: key,
                        x: surface_x,
                        y: surface_y,
                    },
                )
            }
            wl_pointer::Event::Leave { .. } => {
                shared.objects.with(|objects| {
                    if let Some(held) = objects
                        .seats
                        .iter_mut()
                        .filter_map(|seat| seat.pointer.as_mut())
                        .find(|held| held.pointer == *pointer)
                    {
                        held.focus = None;
                        held.focus_surface = None;
                    }
                });
                (arrival(None), PointerWire::Leave)
            }
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => (
                arrival(Some(time)),
                PointerWire::Motion {
                    x: surface_x,
                    y: surface_y,
                },
            ),
            wl_pointer::Event::Button {
                serial,
                time,
                button,
                state,
            } => {
                let pressed = state == WEnum::Value(wl_pointer::ButtonState::Pressed);
                if pressed {
                    let surface = shared
                        .objects
                        .with(|objects| {
                            objects
                                .seats
                                .iter()
                                .filter_map(|seat| seat.pointer.as_ref())
                                .find(|held| held.pointer == *pointer)
                                .and_then(|held| held.focus_surface.clone())
                        })
                        .flatten();
                    if let Some(surface) = surface {
                        shared.record_input(data.seat, serial, surface);
                    }
                }
                (
                    arrival(Some(time)),
                    PointerWire::Button {
                        code: button,
                        pressed,
                    },
                )
            }
            wl_pointer::Event::Axis {
                time,
                axis: which,
                value,
            } => {
                let Some(which) = axis(which) else {
                    return;
                };
                (
                    arrival(Some(time)),
                    PointerWire::Axis { axis: which, value },
                )
            }
            wl_pointer::Event::Frame => (arrival(None), PointerWire::Frame),
            wl_pointer::Event::AxisSource { axis_source } => {
                let source = match axis_source {
                    WEnum::Value(wl_pointer::AxisSource::Wheel) => AxisSource::Wheel,
                    WEnum::Value(wl_pointer::AxisSource::Finger) => AxisSource::Finger,
                    WEnum::Value(wl_pointer::AxisSource::Continuous) => AxisSource::Continuous,
                    WEnum::Value(wl_pointer::AxisSource::WheelTilt) => AxisSource::WheelTilt,
                    _ => return,
                };
                (arrival(None), PointerWire::AxisSource(source))
            }
            wl_pointer::Event::AxisStop { time, axis: which } => {
                let Some(which) = axis(which) else {
                    return;
                };
                (arrival(Some(time)), PointerWire::AxisStop(which))
            }
            wl_pointer::Event::AxisDiscrete {
                axis: which,
                discrete,
            } => {
                let Some(which) = axis(which) else {
                    return;
                };
                (
                    arrival(None),
                    PointerWire::AxisDiscrete {
                        axis: which,
                        discrete,
                    },
                )
            }
            wl_pointer::Event::AxisValue120 {
                axis: which,
                value120,
            } => {
                let Some(which) = axis(which) else {
                    return;
                };
                (
                    arrival(None),
                    PointerWire::AxisValue120 {
                        axis: which,
                        value120,
                    },
                )
            }
            _ => return,
        };
        let framed = pointer.version() >= POINTER_FRAMES;
        shared.record_seat(
            time,
            SeatEvent::Pointer {
                device: data.device,
                event: wire,
            },
        );
        // Before frames, each event stands alone.
        if !framed && wire != PointerWire::Frame {
            shared.record_seat(
                time,
                SeatEvent::Pointer {
                    device: data.device,
                    event: PointerWire::Frame,
                },
            );
        }
    }
}

impl Dispatch<WlKeyboard, KeyboardData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        data: &KeyboardData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let seat = data.seat;
        let wire = match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if format != WEnum::Value(KeymapFormat::XkbV1) {
                    tracing::warn!("the compositor's keymap is not XKB's: keys are unread");
                    return;
                }
                Wire::Keymap { seat, fd, size }
            }
            wl_keyboard::Event::Enter {
                serial, surface, ..
            } => {
                let focus = shared.surface_of(&surface);
                let entered = focus.map(|_| surface);
                shared.set_keyboard_focus(seat, entered.clone());
                // The keyboard entering a window is input a token may name.
                if let Some(surface) = entered {
                    shared.record_input(seat, serial, surface);
                }
                Wire::KeyFocus { seat, focus }
            }
            wl_keyboard::Event::Leave { .. } => {
                shared.set_keyboard_focus(seat, None);
                Wire::KeyFocus { seat, focus: None }
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                key,
                state,
            } => {
                let pressed = state == WEnum::Value(wl_keyboard::KeyState::Pressed);
                if pressed && let Some(surface) = shared.keyboard_focus(seat) {
                    shared.record_input(seat, serial, surface);
                }
                Wire::Key {
                    seat,
                    time: arrival(Some(time)),
                    code: key,
                    pressed,
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => Wire::Modifiers {
                seat,
                depressed: mods_depressed,
                latched: mods_latched,
                locked: mods_locked,
                group,
                time: arrival(None),
            },
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                Wire::RepeatInfo { seat, rate, delay }
            }
            _ => return,
        };
        shared.record(wire);
    }
}

/// The keymap the compositor shared through `fd`, `size` bytes with its
/// terminating NUL, within the keymap limit (`xkb::KEYMAP_LIMIT`).
fn read_keymap(fd: OwnedFd, size: u32) -> std::io::Result<KeymapSource> {
    let length = keymap_length(size).ok_or_else(|| {
        std::io::Error::other(format!("a keymap of {size} bytes is past the limit"))
    })?;
    let file = std::fs::File::from(fd);
    let mut bytes = vec![0_u8; length];
    file.read_exact_at(&mut bytes, 0)?;
    KeymapSource::compositor(bytes).ok_or_else(|| std::io::Error::other("the keymap is not UTF-8"))
}

impl Dispatch<WlTouch, TouchData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlTouch,
        event: wl_touch::Event,
        data: &TouchData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let (time, wire) = match event {
            wl_touch::Event::Down {
                serial,
                time,
                surface,
                id,
                x,
                y,
            } => {
                let Some(key) = shared.surface_of(&surface) else {
                    return;
                };
                shared.record_input(data.seat, serial, surface);
                (
                    arrival(Some(time)),
                    TouchWire::Down {
                        surface: key,
                        id,
                        x,
                        y,
                    },
                )
            }
            wl_touch::Event::Up { time, id, .. } => (arrival(Some(time)), TouchWire::Up { id }),
            wl_touch::Event::Motion { time, id, x, y } => {
                (arrival(Some(time)), TouchWire::Motion { id, x, y })
            }
            wl_touch::Event::Shape { id, major, minor } => {
                (arrival(None), TouchWire::Shape { id, major, minor })
            }
            wl_touch::Event::Frame => (arrival(None), TouchWire::Frame),
            wl_touch::Event::Cancel => (arrival(None), TouchWire::Cancel),
            _ => return,
        };
        shared.record_seat(
            time,
            SeatEvent::Touch {
                device: data.device,
                event: wire,
            },
        );
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, PinchData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        data: &PinchData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let (time, wire) = match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { time, surface, .. } => {
                let Some(surface) = shared.surface_of(&surface) else {
                    return;
                };
                (arrival(Some(time)), PinchWire::Begin { surface })
            }
            zwp_pointer_gesture_pinch_v1::Event::Update {
                time,
                dx,
                dy,
                scale,
                rotation,
            } => (
                arrival(Some(time)),
                PinchWire::Update {
                    dx,
                    dy,
                    scale,
                    rotation,
                },
            ),
            zwp_pointer_gesture_pinch_v1::Event::End {
                time, cancelled, ..
            } => (
                arrival(Some(time)),
                PinchWire::End {
                    cancelled: cancelled != 0,
                },
            ),
            _ => return,
        };
        shared.record_seat(
            time,
            SeatEvent::Pinch {
                device: data.device,
                pointer: data.pointer,
                event: wire,
            },
        );
    }
}

impl Dispatch<ZwpTabletSeatV2, SeatData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletSeatV2,
        event: zwp_tablet_seat_v2::Event,
        data: &SeatData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // FLUI reads neither a tablet (its tools carry the input) nor a pad
        // (its buttons, rings and strips stay the compositor's shortcuts):
        // each goes as it comes, so a compositor adding them without end
        // leaves nothing live.
        match &event {
            zwp_tablet_seat_v2::Event::TabletAdded { id } => {
                id.destroy();
                return;
            }
            zwp_tablet_seat_v2::Event::PadAdded { id } => {
                id.destroy();
                return;
            }
            _ => {}
        }
        if let zwp_tablet_seat_v2::Event::ToolAdded { id } = event
            && let Some(shared) = data.shared.upgrade()
            && let Some(tool) = id.data::<ToolData>()
        {
            // The seat keeps its tools, which go with it.
            let kept = shared.objects.with(|objects| {
                let seat = objects
                    .seats
                    .iter_mut()
                    .find(|seat| seat.key == data.seat)?;
                (seat.tools.len() < MAX_TOOLS).then(|| seat.tools.push(id.clone()))
            });
            if kept.flatten().is_none() {
                // A tool it does not keep has no device: its events are
                // dropped until it goes.
                tracing::debug!("a Wayland tablet tool past {MAX_TOOLS} on its seat was refused");
                id.destroy();
                return;
            }
            let _ = tool
                .held
                .set((Arc::downgrade(&shared), shared.label(), data.seat));
        }
    }

    event_created_child!(WaylandState, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ToolData::new()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

/// The pointer kind a tablet tool of `kind` is.
fn tool_kind(kind: ToolType) -> PointerKind {
    match kind {
        ToolType::Eraser => PointerKind::Pen {
            tool: PenTool::Eraser,
        },
        ToolType::Mouse | ToolType::Lens => PointerKind::Mouse,
        _ => PointerKind::Pen { tool: PenTool::Tip },
    }
}

impl Dispatch<ZwpTabletToolV2, ToolData> for WaylandState {
    fn event(
        _: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        data: &ToolData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((shared, device, seat)) = data.shared() else {
            return;
        };
        let kind = || data.kind();
        let wire = match event {
            zwp_tablet_tool_v2::Event::Type { tool_type } => {
                if let WEnum::Value(tool_type) = tool_type {
                    data.kind.with(|kind| *kind = Some(tool_kind(tool_type)));
                }
                return;
            }
            zwp_tablet_tool_v2::Event::Done => {
                shared.record_seat(
                    arrival(None),
                    SeatEvent::DeviceAdded {
                        device,
                        kind: kind(),
                        seat,
                    },
                );
                return;
            }
            zwp_tablet_tool_v2::Event::Removed => {
                shared.objects.with(|objects| {
                    for held in &mut objects.seats {
                        held.tools.retain(|held| held != tool);
                    }
                });
                shared.record_seat(arrival(None), SeatEvent::DeviceRemoved { device });
                tool.destroy();
                return;
            }
            zwp_tablet_tool_v2::Event::ProximityIn { surface, .. } => {
                let Some(surface) = shared.surface_of(&surface) else {
                    return;
                };
                ToolWire::ProximityIn { surface }
            }
            zwp_tablet_tool_v2::Event::ProximityOut => ToolWire::ProximityOut,
            zwp_tablet_tool_v2::Event::Down { .. } => ToolWire::Down,
            zwp_tablet_tool_v2::Event::Up => ToolWire::Up,
            zwp_tablet_tool_v2::Event::Motion { x, y } => ToolWire::Motion { x, y },
            zwp_tablet_tool_v2::Event::Pressure { pressure } => ToolWire::Pressure(pressure),
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => ToolWire::Tilt {
                x: tilt_x,
                y: tilt_y,
            },
            zwp_tablet_tool_v2::Event::Rotation { degrees } => ToolWire::Rotation(degrees),
            zwp_tablet_tool_v2::Event::Slider { position } => ToolWire::Slider(position),
            zwp_tablet_tool_v2::Event::Button { button, state, .. } => ToolWire::Button {
                code: button,
                pressed: state == WEnum::Value(ButtonState::Pressed),
            },
            zwp_tablet_tool_v2::Event::Frame { time } => {
                shared.record_seat(
                    arrival(Some(time)),
                    SeatEvent::Tool {
                        device,
                        kind: kind(),
                        event: ToolWire::Frame,
                    },
                );
                return;
            }
            _ => return,
        };
        // A tool's events carry no time: its frame does.
        shared.record_seat(
            arrival(None),
            SeatEvent::Tool {
                device,
                kind: kind(),
                event: wire,
            },
        );
    }
}

impl Dispatch<ZwpTabletV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        tablet: &ZwpTabletV2,
        _: zwp_tablet_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Destroyed as it was added: should an event still reach it,
        // destroying it again sends nothing.
        tablet.destroy();
    }
}

// A pad is destroyed as it is added; a group, ring or strip the compositor
// made for it before that goes the moment any event of its own, or of its
// parent, reaches it.
impl Dispatch<ZwpTabletPadV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_pad_v2::Event::Group { pad_group } = event {
            pad_group.destroy();
        }
        pad.destroy();
    }

    event_created_child!(WaylandState, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        group: &ZwpTabletPadGroupV2,
        event: zwp_tablet_pad_group_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_pad_group_v2::Event::Ring { ring } => ring.destroy(),
            zwp_tablet_pad_group_v2::Event::Strip { strip } => strip.destroy(),
            _ => {}
        }
        group.destroy();
    }

    event_created_child!(WaylandState, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        ring: &ZwpTabletPadRingV2,
        _: zwp_tablet_pad_ring_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        ring.destroy();
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        strip: &ZwpTabletPadStripV2,
        _: zwp_tablet_pad_strip_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        strip.destroy();
    }
}

impl Dispatch<ZwpTabletManagerV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletManagerV2,
        _: zwp_tablet_manager_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZwpPointerGesturesV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpPointerGesturesV1,
        _: zwp_pointer_gestures_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: wp_cursor_shape_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: wp_cursor_shape_device_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A cursor shape device has no events.
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::OwnedFd;

    use super::{Delivering, NativeClock, SeatKey, Wire, arrival, queue, read_keymap};
    use crate::input::linux::xkb::GROUP_NAME_READS;

    /// A compositor that sends keymap after keymap before the loop delivers
    /// leaves one queued per seat, the newest: the older files are closed
    /// unread, and the one delivery reads.
    ///
    /// Each older keymap is a pipe whose closing the test sees through its
    /// write end, so a descriptor another test opens at the same time can't
    /// move the count.
    #[test]
    fn a_seat_queues_one_keymap() {
        let mut wires = Vec::new();
        let mut older: Option<std::io::PipeWriter> = None;
        let keymap = |wires: &mut Vec<Wire>, fd: OwnedFd, size: u32| {
            queue(
                wires,
                Wire::Keymap {
                    seat: SeatKey(1),
                    fd,
                    size,
                },
            );
        };
        for _ in 0..1_000 {
            let (reader, writer) = std::io::pipe().expect("a pipe opens");
            keymap(&mut wires, OwnedFd::from(reader), 16);
            assert_replaced(older.replace(writer));
        }
        let text = b"xkb_keymap { };\0";
        let path = std::env::temp_dir().join(format!("flui-keymap-{}", std::process::id()));
        std::fs::write(&path, text).expect("the keymap is written");
        let file = std::fs::File::open(&path).expect("the keymap opens");
        std::fs::remove_file(&path).expect("the keymap is removed");
        let size = u32::try_from(text.len()).expect("a short keymap");
        keymap(&mut wires, OwnedFd::from(file), size);
        assert_replaced(older.take());
        queue(
            &mut wires,
            Wire::RepeatInfo {
                seat: SeatKey(1),
                rate: 25,
                delay: 600,
            },
        );
        assert_eq!(wires.len(), 2, "wires queued");
        let Some(Wire::Keymap { fd, size, .. }) = wires.into_iter().next() else {
            panic!("the keymap stays first");
        };
        let source = read_keymap(fd, size).expect("the queued keymap reads");
        assert!(format!("{source:?}").contains("xkb_keymap"), "{source:?}");
    }

    /// A replaced keymap's descriptor is closed: its pipe has no reader left.
    fn assert_replaced(writer: Option<std::io::PipeWriter>) {
        use std::io::Write as _;
        let Some(mut writer) = writer else {
            return;
        };
        let error = writer
            .write_all(b"x")
            .expect_err("a replaced keymap's pipe has no reader");
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }

    /// A keymap of two named layouts, padded with a comment to nearly the
    /// largest a host reads.
    fn wide_keymap() -> Vec<u8> {
        let mut text = String::from(
            "xkb_keymap {\n\
             xkb_keycodes \"flui\" { minimum = 8; maximum = 255; <AE01> = 10; };\n\
             xkb_types \"flui\" { type \"ONE_LEVEL\" { modifiers = none; \
             level_name[Level1] = \"Any\"; }; };\n\
             xkb_compatibility \"flui\" { };\n\
             xkb_symbols \"flui\" {\n\
             name[Group1] = \"First\"; name[Group2] = \"Second\";\n\
             key <AE01> { type = \"ONE_LEVEL\", symbols[Group1] = [ 1 ], \
             symbols[Group2] = [ 2 ] };\n\
             };\n};\n// ",
        );
        let padding = crate::input::linux::xkb::KEYMAP_LIMIT - text.len() - 2;
        text.extend(std::iter::repeat_n('x', padding));
        text.push('\n');
        let mut bytes = text.into_bytes();
        bytes.push(0);
        bytes
    }

    /// A compositor switching layouts without end has each switch named by a
    /// lookup: an 8 MiB keymap's layouts are read once, however many group
    /// changes follow.
    #[test]
    fn a_layout_switch_reads_no_keymap() {
        if xkbcommon_dl::xkbcommon_option().is_none() {
            eprintln!("skipped: libxkbcommon is not installed");
            return;
        }
        let bytes = wide_keymap();
        let path = std::env::temp_dir().join(format!("flui-wide-keymap-{}", std::process::id()));
        std::fs::write(&path, &bytes).expect("the keymap is written");
        let fd = OwnedFd::from(std::fs::File::open(&path).expect("the keymap opens"));
        std::fs::remove_file(&path).expect("the keymap is removed");
        GROUP_NAME_READS.with(|reads| reads.set(0));
        let mut delivering = Delivering::new(NativeClock::epoch_only());
        let seat = SeatKey(1);
        let size = u32::try_from(bytes.len()).expect("within the limit");
        let _ = delivering.keyboard_wire(Wire::Keymap { seat, fd, size });
        assert_eq!(delivering.layout, "First");
        for change in 0..10_000_u32 {
            let _ = delivering.keyboard_wire(Wire::Modifiers {
                seat,
                depressed: 0,
                latched: 0,
                locked: 0,
                group: change % 2,
                time: arrival(None),
            });
        }
        assert_eq!(
            delivering.layout, "Second",
            "the last change names its layout"
        );
        assert_eq!(
            GROUP_NAME_READS.with(std::cell::Cell::get),
            1,
            "keymap texts read for names"
        );
    }
}
