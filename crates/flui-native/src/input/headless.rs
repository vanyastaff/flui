//! The headless host's input: the availability a test makes the backend
//! report, the simulated devices it attaches and removes, and the keyboard
//! layout the loop keeps. A test injects pointer and key input already
//! assembled, through a window's callbacks (`MockWindow`).

use std::num::NonZeroU64;
use std::sync::{Arc, Weak};

use flui_foundation::IdMint;
use flui_platform::input_capabilities::PointerKindSet;
use flui_platform::pointer::{DeviceId, PointerDeviceChange, PointerKind};
use flui_platform::{EventTime, PlatformInput};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::input::assembly::{DeviceRegistry, NativeDevice};
use crate::owner::call_point::{Field, install};
use crate::owner::headless::{HeadlessLoop, HeadlessState, SimulatedDevices};
use crate::platform::HostPlatform;

/// The headless platform's input controls: builders and handles taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Report `availability` as what this backend implements (its device
    /// presence is ignored: attached devices come from
    /// [`input_devices`](Self::input_devices)). Without it the headless
    /// backend reports pen, precise scrolling, trackpad gestures and input
    /// regions `Native`, since a test can synthesize or record all of them,
    /// and outgoing drags `Unavailable`. A window takes input regions exactly
    /// while this says they are available.
    #[must_use]
    pub fn with_input_availability(
        self,
        availability: flui_platform::input_capabilities::InputCapabilities,
    ) -> Self {
        self.owner()
            .with_state(|state| state.input_availability = availability);
        self
    }

    /// A handle that attaches and removes simulated input devices, valid
    /// across [`Platform::run`](crate::Platform::run). Each change reaches every open window as a
    /// device event and a capability change, in order with its other input;
    /// a window opened later has the devices in its initial snapshot.
    #[must_use]
    pub fn input_devices(&self) -> HeadlessInputDevices {
        HeadlessInputDevices {
            state: Arc::downgrade(&self.owner().state),
        }
    }
}

impl host::Input<HeadlessLoop> for HeadlessInputDevices {
    fn keyboard_layout(owner: &HeadlessLoop) -> String {
        owner.with_state(|state| state.keyboard_layout.clone())
    }

    fn on_keyboard_layout_change(owner: &HeadlessLoop, callback: Box<dyn FnMut() + Send>) {
        install(
            &Field::new(&*owner.state, |state| {
                &mut state.keyboard_layout_changed
            }),
            callback,
        );
    }
}

/// Attaches and removes simulated input devices on a [`HeadlessPlatform`](crate::HeadlessPlatform)
/// ([`HeadlessPlatform::input_devices`](crate::HeadlessPlatform::input_devices)).
#[derive(Clone)]
pub struct HeadlessInputDevices {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessInputDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessInputDevices")
            .finish_non_exhaustive()
    }
}

impl HeadlessInputDevices {
    /// Attach a device of `kind`: every open window receives
    /// [`DeviceAdded`](flui_platform::pointer::PointerEvent::DeviceAdded),
    /// then the capability change. The id its events carry; `None` once the
    /// platform is gone or device ids are exhausted.
    pub fn attach(&self, kind: PointerKind) -> Option<DeviceId> {
        use flui_platform::pointer::PointerEvent;
        let state = self.state.upgrade()?;
        let (change, windows, present) = {
            let mut state = state.lock();
            let time = EventTime::from_nanos(crate::input::event_timestamp_ns());
            let change = state.devices.attach(kind, time)?;
            let windows = state.windows.deliveries_of_all();
            (change, windows, state.devices.present())
        };
        for callbacks in windows {
            callbacks.dispatch_input(PlatformInput::Pointer(PointerEvent::DeviceAdded(change)));
            callbacks.set_input_capabilities(callbacks.input_capabilities().with_present(present));
        }
        Some(change.device)
    }

    /// Remove the device `id` named: every open window receives
    /// [`DeviceRemoved`](flui_platform::pointer::PointerEvent::DeviceRemoved),
    /// then the capability change. A simulated device has no contacts of its
    /// own to cancel: a test injects its pointer input already assembled.
    /// `false` when it was not attached.
    pub fn remove(&self, id: DeviceId) -> bool {
        use flui_platform::pointer::PointerEvent;
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let (removed, windows, present) = {
            let mut state = state.lock();
            let time = EventTime::from_nanos(crate::input::event_timestamp_ns());
            let Some(removed) = state.devices.remove(id, time) else {
                return false;
            };
            let windows = state.windows.deliveries_of_all();
            (removed, windows, state.devices.present())
        };
        for callbacks in windows {
            callbacks.dispatch_input(PlatformInput::Pointer(PointerEvent::DeviceRemoved(removed)));
            callbacks.set_input_capabilities(callbacks.input_capabilities().with_present(present));
        }
        true
    }
}

/// The simulated system's pointer devices, as the headless loop holds them
/// ([`SimulatedDevices`]): presence and device ids from one registry, and the
/// native label each device is minted under, never one twice.
#[derive(Default)]
pub(crate) struct AttachedDevices {
    registry: DeviceRegistry,
    labels: IdMint<NonZeroU64>,
}

impl SimulatedDevices for AttachedDevices {
    fn attach(&mut self, kind: PointerKind, time: EventTime) -> Option<PointerDeviceChange> {
        let label = self.labels.next().ok()?;
        self.registry
            .add_device(NativeDevice(label.get()), kind, time)
    }

    fn remove(&mut self, id: DeviceId, time: EventTime) -> Option<PointerDeviceChange> {
        let native = self.registry.native(id)?;
        self.registry.remove_device(native, time)
    }

    fn present(&self) -> PointerKindSet {
        self.registry.present()
    }
}
