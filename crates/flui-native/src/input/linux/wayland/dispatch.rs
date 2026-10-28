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
use flui_platform::keyboard::{KeyEvent, KeyRepeat as Repeat, KeyState};
use flui_platform::pointer::{PenTool, PointerEvent, PointerKind};
use flui_platform::{
    CursorError, EventTime, PlatformInput, Unsupported, UnsupportedReason, WindowId,
};
use wayland_client::protocol::wl_keyboard::{self, KeymapFormat, WlKeyboard};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, Capability, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::protocol::wl_touch::{self, WlTouch};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child};
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
use super::repeat::KeyRepeat;
use super::wire::{
    Axis, AxisSource, PinchWire, PointerWire, Routed, SeatAssembly, SeatEvent, SeatWire, ToolWire,
    TouchWire, WireTime,
};
use crate::input::assembly::{NativeClock, NativeDevice, NativeSurface};
use crate::input::availability::{self, WaylandInputGlobals};
use crate::input::event_timestamp_ns;
use crate::input::keys::location_for_code;
use crate::input::linux::xkb::{Keymap, code_of, key_of};
use crate::owner::linux::wayland::{SeatWindow, WaylandInput, WaylandState};
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

/// What a dispatch records for the next delivery.
#[derive(Debug)]
enum Wire {
    /// A pointer, touch, tool or pinch event, or a device that came or went.
    Seat(SeatWire),
    /// The keyboard's keymap, as text.
    Keymap(String),
    /// The keyboard entered the window of a surface, or left (`None`).
    KeyFocus(Option<NativeSurface>),
    /// A key changed.
    Key {
        time: WireTime,
        code: u32,
        pressed: bool,
    },
    /// The modifier and layout masks.
    Modifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
        time: WireTime,
    },
    /// The compositor's key repeat settings.
    RepeatInfo { rate: i32, delay: i32 },
}

/// A window the input delivers to.
struct Attached {
    id: WindowId,
    surface: WlSurface,
    delivery: Arc<WindowDelivery>,
    cursor: CursorIcon,
}

/// A pointer's objects.
struct PointerObjects {
    pointer: WlPointer,
    device: NativeDevice,
    pinch: Option<ZwpPointerGesturePinchV1>,
    shape: Option<WpCursorShapeDeviceV1>,
    /// The window surface it is over and the serial of its enter.
    focus: Option<(NativeSurface, u32)>,
}

/// One seat's objects.
struct SeatObjects {
    name: u32,
    seat: WlSeat,
    pointer: Option<PointerObjects>,
    keyboard: Option<WlKeyboard>,
    touch: Option<(WlTouch, NativeDevice)>,
    tablet: Option<ZwpTabletSeatV2>,
}

/// The protocol objects the input made.
#[derive(Default)]
struct Objects {
    seats: Vec<SeatObjects>,
    tablet: Option<(u32, ZwpTabletManagerV2)>,
    gestures: Option<(u32, ZwpPointerGesturesV1)>,
    shapes: Option<(u32, WpCursorShapeManagerV1)>,
}

/// The keyboard: the compositor's keymap and the window it types into.
#[derive(Default)]
struct Keyboard {
    keymap: Option<Keymap>,
    focus: Option<NativeSurface>,
    repeat: KeyRepeat,
    /// The layout's name as last reported.
    layout: String,
    /// The layout changed since the callbacks last heard.
    layout_changed: bool,
}

/// What the delivery side keeps: the assembly, the keyboard, and the
/// capabilities the windows were last given.
struct Delivering {
    assembly: SeatAssembly,
    keyboard: Keyboard,
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
}

impl Shared {
    fn record(&self, wire: Wire) {
        self.wires.with(|wires| wires.push(wire));
    }

    fn record_seat(&self, time: WireTime, event: SeatEvent) {
        self.record(Wire::Seat(SeatWire { time, event }));
    }

    /// A device label no object had.
    fn label(&self) -> NativeDevice {
        NativeDevice(self.labels.fetch_add(1, Ordering::Relaxed) + 1)
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
    /// The input of a loop on the calling thread, with no global bound yet.
    pub(crate) fn new() -> Self {
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
        });
        shared.delivering.with(|delivering| {
            *delivering = Some(Delivering {
                assembly: SeatAssembly::new(clock),
                keyboard: Keyboard::default(),
                capabilities: availability::wayland(WaylandInputGlobals::default()),
            });
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
        let seat = registry.bind::<WlSeat, SeatData, WaylandState>(
            name,
            version.min(SEAT_VERSION),
            queue,
            SeatData {
                shared: Arc::downgrade(&self.shared),
            },
        );
        self.shared.objects.with(|objects| {
            let tablet = objects
                .tablet
                .as_ref()
                .map(|(_, manager)| tablet_seat(manager, &seat, queue, &self.shared));
            objects.seats.push(SeatObjects {
                name,
                seat,
                pointer: None,
                keyboard: None,
                touch: None,
                tablet,
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
                    Wire::Seat(wire) => delivering
                        .assembly
                        .assemble(wire)
                        .into_iter()
                        .map(|routed| match routed {
                            Routed::To(surface, input) => (Target::Surface(surface), input),
                            Routed::Everywhere(input) => (Target::Everywhere, input),
                        })
                        .collect(),
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
                let delivering = delivering.as_mut()?;
                let code = delivering.keyboard.repeat.take_due(Instant::now())?;
                let time = EventTime::from_nanos(event_timestamp_ns());
                let event = delivering.key_event(code, KeyState::Down, time)?;
                let focus = delivering.keyboard.focus?;
                Some(vec![(
                    Target::Surface(focus),
                    PlatformInput::Keyboard(event.with_repeat(Repeat::AutoRepeat)),
                )])
            })
            .flatten()
            .unwrap_or_default()
    }

    /// Tell the layout callbacks, outside every cell, when the layout
    /// changed.
    fn report_layout(&self) {
        let changed = self
            .shared
            .delivering
            .with(|delivering| {
                delivering.as_mut().is_some_and(|delivering| {
                    std::mem::take(&mut delivering.keyboard.layout_changed)
                })
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
    /// The key event of key `code` going `state` at `time`, as the keymap
    /// reads it now; `None` before a keymap came.
    fn key_event(&self, code: u32, state: KeyState, time: EventTime) -> Option<KeyEvent> {
        let keymap = self.keyboard.keymap.as_ref()?;
        let facts = keymap.key(code);
        let physical = code_of(code);
        Some(
            KeyEvent::new(state, key_of(facts.keysym, &facts.text), physical, time)
                .with_location(location_for_code(physical))
                .with_modifiers(keymap.modifiers()),
        )
    }

    fn keyboard_wire(&mut self, wire: Wire) -> Vec<(Target, PlatformInput)> {
        match wire {
            Wire::Keymap(text) => {
                self.keyboard.repeat.stop();
                self.keyboard.keymap = Keymap::compile(&text);
                if self.keyboard.keymap.is_none() {
                    tracing::warn!("the compositor's keymap was not compiled: keys are unread");
                }
                self.note_layout();
                self.assembly.assemble(SeatWire {
                    time: arrival(None),
                    event: SeatEvent::Modifiers(self.modifiers()),
                });
                Vec::new()
            }
            Wire::KeyFocus(focus) => {
                self.keyboard.focus = focus;
                self.keyboard.repeat.stop();
                Vec::new()
            }
            Wire::Key {
                time,
                code,
                pressed,
            } => {
                let stamp = self
                    .assembly
                    .contacts()
                    .stamp(time.reading_time())
                    .unwrap_or(time.arrival);
                let state = if pressed {
                    KeyState::Down
                } else {
                    KeyState::Up
                };
                if pressed {
                    let repeats = self
                        .keyboard
                        .keymap
                        .as_ref()
                        .is_some_and(|keymap| keymap.key(code).repeats);
                    self.keyboard.repeat.press(code, repeats, Instant::now());
                } else {
                    self.keyboard.repeat.release(code);
                }
                let Some(focus) = self.keyboard.focus else {
                    return Vec::new();
                };
                self.key_event(code, state, stamp)
                    .map(|event| (Target::Surface(focus), PlatformInput::Keyboard(event)))
                    .into_iter()
                    .collect()
            }
            Wire::Modifiers {
                depressed,
                latched,
                locked,
                group,
                time,
            } => {
                let layout_moved = self
                    .keyboard
                    .keymap
                    .as_mut()
                    .is_some_and(|keymap| keymap.update(depressed, latched, locked, group));
                if layout_moved {
                    self.note_layout();
                }
                self.assembly.assemble(SeatWire {
                    time,
                    event: SeatEvent::Modifiers(self.modifiers()),
                });
                Vec::new()
            }
            Wire::RepeatInfo { rate, delay } => {
                self.keyboard.repeat.configure(rate, delay);
                Vec::new()
            }
            Wire::Seat(_) => Vec::new(),
        }
    }

    fn modifiers(&self) -> flui_platform::keyboard::Modifiers {
        self.keyboard
            .keymap
            .as_ref()
            .map_or(flui_platform::keyboard::Modifiers::NONE, Keymap::modifiers)
    }

    /// Note the layout's name, and that it changed when it did.
    fn note_layout(&mut self) {
        let layout = self
            .keyboard
            .keymap
            .as_ref()
            .map(Keymap::layout)
            .unwrap_or_default();
        if layout != self.keyboard.layout {
            self.keyboard.layout = layout;
            self.keyboard.layout_changed = true;
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
        if interface == WlSeat::interface().name {
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
                        seat.tablet = Some(tablet_seat(&manager, &seat.seat, queue, &self.shared));
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
            .with(|delivering| delivering.as_ref()?.keyboard.repeat.due())
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
            }
        });
        self.shared.delivering.with(|delivering| {
            if let Some(delivering) = delivering
                && delivering.keyboard.focus == Some(surface)
            {
                delivering.keyboard.focus = None;
                delivering.keyboard.repeat.stop();
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
        let surface = surface_key(id);
        let shown = self
            .shared
            .objects
            .with(|objects| {
                let (_, manager) = objects.shapes.as_ref()?;
                let version = manager.version();
                for pointer in objects
                    .seats
                    .iter()
                    .filter_map(|seat| seat.pointer.as_ref())
                {
                    if let (Some(shape), Some((focus, serial))) = (&pointer.shape, pointer.focus)
                        && focus == surface
                    {
                        set_shape(shape, serial, cursor, version);
                    }
                }
                Some(())
            })
            .flatten();
        shown.ok_or(CursorError::Unsupported(Unsupported::new(
            UnsupportedReason::NotYetOnThisHost,
        )))
    }

    fn keyboard_layout(&self) -> String {
        self.shared
            .delivering
            .with(|delivering| {
                delivering
                    .as_ref()
                    .map(|delivering| delivering.keyboard.layout.clone())
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
}

/// Show `cursor` on the pointer of `shape` since its enter `serial`.
fn set_shape(shape: &WpCursorShapeDeviceV1, serial: u32, cursor: CursorIcon, version: u32) {
    if let Ok(value) = wp_cursor_shape_device_v1::Shape::try_from(cursor::shape(cursor, version)) {
        shape.set_shape(serial, value);
    } else {
        tracing::debug!(?cursor, "the cursor has no shape on this compositor");
    }
}

/// The tablet seat of `seat`.
fn tablet_seat(
    manager: &ZwpTabletManagerV2,
    seat: &WlSeat,
    queue: &QueueHandle<WaylandState>,
    shared: &Arc<Shared>,
) -> ZwpTabletSeatV2 {
    manager.get_tablet_seat(
        seat,
        queue,
        SeatData {
            shared: Arc::downgrade(shared),
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

/// Let go of a seat's devices, each removed at `time`.
fn release_seat(seat: SeatObjects, shared: &Shared, time: WireTime) {
    if let Some(pointer) = seat.pointer {
        release_pointer(pointer, shared, time);
    }
    if let Some(keyboard) = seat.keyboard {
        release_keyboard(&keyboard, shared);
    }
    if let Some((touch, device)) = seat.touch {
        if touch.version() >= 3 {
            touch.release();
        }
        shared.record_seat(time, SeatEvent::DeviceRemoved { device });
    }
    if let Some(tablet) = seat.tablet {
        tablet.destroy();
    }
    if seat.seat.version() >= 5 {
        seat.seat.release();
    }
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

fn release_keyboard(keyboard: &WlKeyboard, shared: &Shared) {
    if keyboard.version() >= 3 {
        keyboard.release();
    }
    shared.record(Wire::KeyFocus(None));
}

// ---- The dispatch of the input's objects on the owner's queue: each records
// what the compositor said, and runs no handler.

/// A seat's (and its tablet seat's) user data.
pub(crate) struct SeatData {
    shared: Weak<Shared>,
}

/// A pointer's user data.
pub(crate) struct PointerData {
    shared: Weak<Shared>,
    device: NativeDevice,
}

/// A keyboard's user data.
pub(crate) struct KeyboardData {
    shared: Weak<Shared>,
}

/// A touchscreen's user data.
pub(crate) struct TouchData {
    shared: Weak<Shared>,
    device: NativeDevice,
}

/// A pinch's user data.
pub(crate) struct PinchData {
    shared: Weak<Shared>,
    device: NativeDevice,
    pointer: NativeDevice,
}

/// A tablet tool's user data, set when its seat announces it: the tool is
/// made by the protocol, before the input sees it.
#[derive(Default)]
pub(crate) struct ToolData {
    held: OnceLock<(Weak<Shared>, NativeDevice)>,
    kind: parking_lot::Mutex<Option<PointerKind>>,
}

impl ToolData {
    fn shared(&self) -> Option<(Arc<Shared>, NativeDevice)> {
        let (shared, device) = self.held.get()?;
        Some((shared.upgrade()?, *device))
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
        let mut released: Vec<SeatObjects> = Vec::new();
        shared.objects.with(|objects| {
            let gestures = objects
                .gestures
                .as_ref()
                .map(|(_, manager)| manager.clone());
            let shapes = objects.shapes.as_ref().map(|(_, manager)| manager.clone());
            let Some(held) = objects.seats.iter_mut().find(|held| held.seat == *seat) else {
                return;
            };
            let mut gone = SeatObjects {
                name: held.name,
                seat: held.seat.clone(),
                pointer: None,
                keyboard: None,
                touch: None,
                tablet: None,
            };
            if capabilities.contains(Capability::Pointer) {
                if held.pointer.is_none() {
                    let device = shared.label();
                    let pointer = seat.get_pointer(
                        queue,
                        PointerData {
                            shared: Arc::downgrade(&shared),
                            device,
                        },
                    );
                    let mut objects = PointerObjects {
                        shape: shapes
                            .as_ref()
                            .map(|manager| manager.get_pointer(&pointer, queue, ())),
                        pointer,
                        device,
                        pinch: None,
                        focus: None,
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
                        },
                    );
                }
            } else {
                gone.pointer = held.pointer.take();
            }
            if capabilities.contains(Capability::Keyboard) {
                if held.keyboard.is_none() {
                    held.keyboard = Some(seat.get_keyboard(
                        queue,
                        KeyboardData {
                            shared: Arc::downgrade(&shared),
                        },
                    ));
                }
            } else {
                gone.keyboard = held.keyboard.take();
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
                            },
                        ),
                        device,
                    ));
                    shared.record_seat(
                        time,
                        SeatEvent::DeviceAdded {
                            device,
                            kind: PointerKind::Touch,
                        },
                    );
                }
            } else {
                gone.touch = held.touch.take();
            }
            released.push(gone);
        });
        for gone in released {
            // The seat itself stays: only the devices it lost go.
            if let Some(pointer) = gone.pointer {
                release_pointer(pointer, &shared, time);
            }
            if let Some(keyboard) = gone.keyboard {
                release_keyboard(&keyboard, &shared);
            }
            if let Some((touch, device)) = gone.touch {
                if touch.version() >= 3 {
                    touch.release();
                }
                shared.record_seat(time, SeatEvent::DeviceRemoved { device });
            }
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
                let cursor = shared
                    .windows
                    .with(|windows| {
                        windows
                            .iter()
                            .find(|window| surface_key(window.id) == key)
                            .map(|window| window.cursor)
                    })
                    .flatten()
                    .unwrap_or_default();
                shared.objects.with(|objects| {
                    let version = objects
                        .shapes
                        .as_ref()
                        .map(|(_, manager)| manager.version());
                    if let Some(held) = objects
                        .seats
                        .iter_mut()
                        .filter_map(|seat| seat.pointer.as_mut())
                        .find(|held| held.pointer == *pointer)
                    {
                        held.focus = Some((key, serial));
                        if let (Some(shape), Some(version)) = (&held.shape, version) {
                            set_shape(shape, serial, cursor, version);
                        }
                    }
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
                time,
                button,
                state,
                ..
            } => (
                arrival(Some(time)),
                PointerWire::Button {
                    code: button,
                    pressed: state == WEnum::Value(wl_pointer::ButtonState::Pressed),
                },
            ),
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
        let wire = match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if format != WEnum::Value(KeymapFormat::XkbV1) {
                    tracing::warn!("the compositor's keymap is not XKB's: keys are unread");
                    return;
                }
                match read_keymap(fd, size) {
                    Ok(text) => Wire::Keymap(text),
                    Err(error) => {
                        tracing::warn!(%error, "the compositor's keymap could not be read");
                        return;
                    }
                }
            }
            wl_keyboard::Event::Enter { surface, .. } => {
                Wire::KeyFocus(shared.surface_of(&surface))
            }
            wl_keyboard::Event::Leave { .. } => Wire::KeyFocus(None),
            wl_keyboard::Event::Key {
                time, key, state, ..
            } => Wire::Key {
                time: arrival(Some(time)),
                code: key,
                pressed: state == WEnum::Value(wl_keyboard::KeyState::Pressed),
            },
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => Wire::Modifiers {
                depressed: mods_depressed,
                latched: mods_latched,
                locked: mods_locked,
                group,
                time: arrival(None),
            },
            wl_keyboard::Event::RepeatInfo { rate, delay } => Wire::RepeatInfo { rate, delay },
            _ => return,
        };
        shared.record(wire);
    }
}

/// The keymap text the compositor shared through `fd`, `size` bytes with its
/// terminating NUL.
fn read_keymap(fd: OwnedFd, size: u32) -> std::io::Result<String> {
    let file = std::fs::File::from(fd);
    let length = usize::try_from(size).map_err(std::io::Error::other)?;
    let mut text = vec![0_u8; length];
    file.read_exact_at(&mut text, 0)?;
    if let Some(end) = text.iter().position(|byte| *byte == 0) {
        text.truncate(end);
    }
    String::from_utf8(text).map_err(std::io::Error::other)
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
                time,
                surface,
                id,
                x,
                y,
                ..
            } => {
                let Some(surface) = shared.surface_of(&surface) else {
                    return;
                };
                (arrival(Some(time)), TouchWire::Down { surface, id, x, y })
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
        if let zwp_tablet_seat_v2::Event::ToolAdded { id } = event
            && let Some(shared) = data.shared.upgrade()
            && let Some(tool) = id.data::<ToolData>()
        {
            let _ = tool.held.set((Arc::downgrade(&shared), shared.label()));
        }
    }

    event_created_child!(WaylandState, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ToolData::default()),
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
        let Some((shared, device)) = data.shared() else {
            return;
        };
        let kind = || (*data.kind.lock()).unwrap_or(PointerKind::Pen { tool: PenTool::Tip });
        let wire = match event {
            zwp_tablet_tool_v2::Event::Type { tool_type } => {
                if let WEnum::Value(tool_type) = tool_type {
                    *data.kind.lock() = Some(tool_kind(tool_type));
                }
                return;
            }
            zwp_tablet_tool_v2::Event::Done => {
                shared.record_seat(
                    arrival(None),
                    SeatEvent::DeviceAdded {
                        device,
                        kind: kind(),
                    },
                );
                return;
            }
            zwp_tablet_tool_v2::Event::Removed => {
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
        event: zwp_tablet_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The tablet itself names its hardware; its tools carry the input.
        if let zwp_tablet_v2::Event::Removed = event {
            tablet.destroy();
        }
    }
}

impl Dispatch<ZwpTabletPadV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A pad's buttons, rings and strips are the compositor's shortcuts
        // until FLUI reads them.
        if let zwp_tablet_pad_v2::Event::Removed = event {
            pad.destroy();
        }
    }

    event_created_child!(WaylandState, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(WaylandState, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadRingV2,
        _: zwp_tablet_pad_ring_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadStripV2,
        _: zwp_tablet_pad_strip_v2::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
