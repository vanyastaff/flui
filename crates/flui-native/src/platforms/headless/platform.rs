//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments. Its loop and owner
//! turns are in [`crate::owner::headless`] and its windows in
//! `crate::window::headless`; this file holds the text-store host a window
//! offers and the simulated system's drag and drop. The displays, the input,
//! the preferences, the host events and the doubles a window hands out are
//! their features' (`display::headless`, `input::headless`,
//! `preferences::headless`, `lifecycle::headless`, `text::headless`,
//! `accessibility::headless`, `haptics::headless`).

use std::{rc::Rc, sync::Arc};

use flui_platform::text_store::TextStoreHost;

use crate::host::Host;
use crate::owner::headless::HeadlessLoop;
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
}
