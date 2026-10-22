//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments. Its loop and owner
//! turns are in [`crate::owner::headless`] and its windows in
//! `crate::window::headless`; this file holds the input devices a test drives
//! and the text-store host a window offers. The displays, the preferences,
//! the host events and the doubles a window hands out are their features'
//! (`display::headless`, `preferences::headless`, `lifecycle::headless`,
//! `text::headless`, `accessibility::headless`, `haptics::headless`).

use std::{
    rc::Rc,
    sync::{Arc, Weak},
};

use flui_platform::PlatformInput;
use flui_platform::text_store::TextStoreHost;
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::call_point::{Field, install};
use crate::owner::headless::{HeadlessLoop, HeadlessState};
use crate::platform::HostPlatform;

/// Builds the text-store host a headless window offers
/// ([`HeadlessPlatform::with_text_store_host`](crate::HeadlessPlatform::with_text_store_host)). Shared with every window,
/// so `Send + Sync`; what it builds is owner-thread state.
pub(crate) type TextStoreHostFactory = Arc<dyn Fn() -> Rc<dyn TextStoreHost> + Send + Sync>;

/// The headless platform's test controls: builders and handles taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Make every window this platform opens pull-model: its
    /// [`HostWindow::text_store_host`](crate::HostWindow::text_store_host) answers a host that `factory` builds,
    /// on the owner thread, each time it is read (the runner reads it once
    /// per window). A test-support stand-in for the Win32 text services
    /// (ADR-0135); without it a headless window is push-model and offers
    /// [`PlatformWindow::text_input`](flui_platform::PlatformWindow::text_input) alone.
    #[must_use]
    pub fn with_text_store_host(
        self,
        factory: impl Fn() -> Rc<dyn TextStoreHost> + Send + Sync + 'static,
    ) -> Self {
        self.owner()
            .with_state(|state| state.text_store_host = Some(Arc::new(factory)));
        self
    }
    /// Report `availability` as what this backend implements (its device
    /// presence is ignored: attached devices come from
    /// [`input_devices`](Self::input_devices)). Without it the headless
    /// backend reports pen, precise scrolling and trackpad gestures `Native`,
    /// since a test can synthesize all of them, and input regions and outgoing
    /// drags `Unavailable`.
    #[must_use]
    pub fn with_input_availability(
        self,
        availability: flui_platform::input_capabilities::InputCapabilities,
    ) -> Self {
        self.owner()
            .with_state(|state| state.input_availability = availability);
        self
    }

    /// Let this backend's windows start OS drag sessions, starting them at
    /// `timing` (desktop: `OnLeavingWindow`; iOS and the web: `AtStart`), and
    /// report outgoing drags `Native`. Without it its windows have no drag
    /// source, as a Linux/X11 window has none. The test plays the OS through
    /// [`drag_and_drop`](Self::drag_and_drop).
    #[must_use]
    pub fn with_os_drag(self, timing: flui_platform::drag::OsDragStart) -> Self {
        self.owner().with_state(|state| {
            state.os_drag_start = Some(timing);
            state.input_availability = state
                .input_availability
                .to_builder()
                .outgoing_drag(flui_platform::input_capabilities::Availability::Native)
                .build();
        });
        self
    }

    /// The simulated OS side of drag-and-drop, valid across [`Platform::run`](crate::Platform::run).
    #[must_use]
    pub fn drag_and_drop(&self) -> crate::drag::headless::HeadlessDragAndDrop {
        crate::drag::headless::HeadlessDragAndDrop::of(&self.owner().state)
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
                &mut state.handlers.keyboard_layout_changed
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
    pub fn attach(
        &self,
        kind: flui_platform::pointer::PointerKind,
    ) -> Option<flui_platform::pointer::DeviceId> {
        use flui_platform::pointer::PointerEvent;
        let state = self.state.upgrade()?;
        let (change, windows, present) = {
            let mut state = state.lock();
            let label = state.device_labels.next().ok()?;
            let native = crate::shared::pointer_assembly::NativeDevice(label.get());
            let time =
                flui_platform::EventTime::from_nanos(crate::shared::events::event_timestamp_ns());
            let change = state.devices.add_device(native, kind, time)?;
            let windows = state.windows.callbacks_of_all();
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
    pub fn remove(&self, id: flui_platform::pointer::DeviceId) -> bool {
        use flui_platform::pointer::PointerEvent;
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let (removed, windows, present) = {
            let mut state = state.lock();
            let time =
                flui_platform::EventTime::from_nanos(crate::shared::events::event_timestamp_ns());
            let Some(removed) = state
                .devices
                .native(id)
                .and_then(|native| state.devices.remove_device(native, time))
            else {
                return false;
            };
            let windows = state.windows.callbacks_of_all();
            (removed, windows, state.devices.present())
        };
        for callbacks in windows {
            callbacks.dispatch_input(PlatformInput::Pointer(PointerEvent::DeviceRemoved(removed)));
            callbacks.set_input_capabilities(callbacks.input_capabilities().with_present(present));
        }
        true
    }
}
