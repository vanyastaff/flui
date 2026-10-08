use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use crate::ui_runtime::UiRuntime;
use flui_foundation::{
    PresentationAddress,
    geometry::{EdgeInsets, Size},
};
use flui_platform_api::{Brightness, PlatformInput, WindowExecutionState};

/// Normalized observations of one native window, without backend-specific types.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum WindowObservation {
    /// Logical content size and device-pixel ratio changed together.
    Metrics {
        /// Logical content size.
        size: Size<f64>,
        /// Device pixels per logical pixel.
        scale_factor: f64,
    },
    /// Keyboard focus changed.
    Focus(bool),
    /// Visibility or occlusion changed.
    Visibility(bool),
    /// Reversible native execution eligibility changed.
    Execution(WindowExecutionState),
    /// Atomic window state sampled after registering native callbacks.
    Snapshot {
        /// Native execution eligibility.
        execution: WindowExecutionState,
        /// Keyboard focus.
        focused: bool,
        /// Window visibility.
        visible: bool,
    },
    /// Pointer entered or left the window.
    Hover(bool),
    /// Logical content-view safe area changed.
    SafeArea(EdgeInsets),
    /// Normalized system appearance changed.
    Brightness(Brightness),
}

impl WindowObservation {
    pub(super) fn apply(
        self,
        runtime: &UiRuntime,
        address: PresentationAddress,
        effects: &dyn OwnerEffects,
    ) {
        let id = address.presentation_id;
        match self {
            Self::Metrics { size, scale_factor } => PendingWindowState {
                metrics: Some((size, scale_factor)),
                ..Default::default()
            }
            .apply(runtime, address, effects),
            Self::Focus(focused) => runtime.update_window_focus(id, focused),
            Self::Visibility(visible) => runtime.update_window_visibility(id, visible),
            Self::Execution(execution) => runtime.update_window_execution(id, execution),
            Self::Snapshot {
                execution,
                focused,
                visible,
            } => {
                runtime.synchronize_window_snapshot(id, execution, focused, visible);
            }
            Self::Hover(inside) => runtime.handle_window_hover_addressed(id, inside),
            Self::SafeArea(insets) => PendingWindowState {
                safe_area: Some(insets),
                ..Default::default()
            }
            .apply(runtime, address, effects),
            Self::Brightness(brightness) => PendingWindowState {
                brightness: Some(brightness),
                ..Default::default()
            }
            .apply(runtime, address, effects),
        }
    }
}

/// State whose intermediate values are unobservable until the next queued
/// operation. Never merged across another window or an ordered event.
#[derive(Default)]
pub(super) struct PendingWindowState {
    metrics: Option<(Size<f64>, f64)>,
    safe_area: Option<EdgeInsets>,
    brightness: Option<Brightness>,
}

impl PendingWindowState {
    pub(super) fn from_observation(
        observation: WindowObservation,
    ) -> Result<Self, WindowObservation> {
        match observation {
            WindowObservation::Metrics { size, scale_factor } => Ok(Self {
                metrics: Some((size, scale_factor)),
                ..Self::default()
            }),
            WindowObservation::SafeArea(insets) => Ok(Self {
                safe_area: Some(insets),
                ..Self::default()
            }),
            WindowObservation::Brightness(brightness) => Ok(Self {
                brightness: Some(brightness),
                ..Self::default()
            }),
            other => Err(other),
        }
    }

    pub(super) fn merge(&mut self, newer: &Self) {
        self.metrics = newer.metrics.or(self.metrics);
        self.safe_area = newer.safe_area.or(self.safe_area);
        self.brightness = newer.brightness.or(self.brightness);
    }

    pub(super) fn apply(
        mut self,
        runtime: &UiRuntime,
        address: PresentationAddress,
        effects: &dyn OwnerEffects,
    ) {
        let id = address.presentation_id;
        let mut first = None;
        if let Some((size, scale)) = self.metrics {
            first = catch_unwind(AssertUnwindSafe(|| {
                effects.resize_surface(address, size, scale);
                runtime.set_pipeline_device_pixel_ratio_for(id, scale);
            }))
            .err();
            if first.is_some() {
                // A merged batch still owns independent appearance/safe-area
                // observations. A failed surface update must not discard them
                // or publish geometry the native adapter did not accept.
                self.metrics = None;
            }
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            if let Some(source) = runtime.media_query_for(id) {
                source.update(|data| {
                    if let Some((size, scale)) = self.metrics {
                        data.size = size;
                        data.device_pixel_ratio = scale;
                    }
                    if let Some(insets) = self.safe_area {
                        data.padding = insets;
                    }
                    if let Some(brightness) = self.brightness {
                        data.platform_brightness = brightness;
                    }
                });
            }
        }))
        .err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            failure,
            "window state publication",
        );
        let failure = catch_unwind(AssertUnwindSafe(|| runtime.request_redraw())).err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            failure,
            "window state redraw",
        );
        super::finish_retirement(first);
    }
}

use super::{Delivery, DispatchError, OwnerCore, OwnerEffects, OwnerHost, RuntimeWork};

/// Input disposition. Only `Unhandled` permits native default handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOutcome {
    /// The framework handled the input.
    Handled,
    /// Keyboard input ran synchronously and no framework handler consumed it.
    Unhandled,
    /// Input was accepted for later delivery; suppress native default handling.
    Queued,
}

/// Weak authority for one exact presentation incarnation.
#[derive(Clone)]
pub struct PresentationDispatcher {
    owner: Weak<OwnerCore>,
    address: PresentationAddress,
}

impl std::fmt::Debug for PresentationDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationDispatcher")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl PresentationDispatcher {
    /// Inject a fault or reentrant operation through the real delivery machinery.
    /// Available only to test consumers; production work uses closed operations.
    ///
    /// # Errors
    /// Refuses an expired host or presentation, or an active publication.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn test_callback(
        &self,
        run: Box<dyn FnOnce(&UiRuntime)>,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(RuntimeWork::TestCallback(self.address, run), effects)
    }

    /// Fence new presentation work immediately and close after already accepted work.
    ///
    /// # Errors
    /// Refuses an expired or closing target, closed host, or active publication.
    pub fn close(&self, effects: &dyn OwnerEffects) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(RuntimeWork::Close(self.address), effects)
    }

    /// Admit a window observation to the owner FIFO. Adjacent pending metrics,
    /// safe-area and brightness updates for this exact window keep their latest
    /// values together. Other windows, input, frames and lifecycle events end
    /// the segment; observations already executing are never replaced.
    ///
    /// # Errors
    /// Refuses an expired target or host, or an active publication.
    pub fn observe(
        &self,
        observation: WindowObservation,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        let work = match PendingWindowState::from_observation(observation) {
            Ok(state) => RuntimeWork::WindowState(self.address, state),
            Err(event) => RuntimeWork::Observation(self.address, event),
        };
        core.deliver(work, effects)
    }

    /// Append input to the owner FIFO and deliver within the remaining budget.
    /// Older accepted work always precedes this input, including across native
    /// callbacks. Queued or refused input suppresses the native default because
    /// the framework cannot answer it synchronously.
    ///
    /// # Errors
    /// Refuses an expired target or host, or an active publication.
    pub fn input(
        &self,
        input: PlatformInput,
        effects: &dyn OwnerEffects,
    ) -> Result<InputOutcome, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        let keyboard = matches!(input, PlatformInput::Keyboard(_));
        let reply = Rc::new(Cell::new(None));
        core.deliver(
            RuntimeWork::Input {
                address: self.address,
                input,
                reply: Rc::clone(&reply),
            },
            effects,
        )?;
        match reply.get() {
            Some(false) if keyboard => Ok(InputOutcome::Unhandled),
            Some(_) => Ok(InputOutcome::Handled),
            None => Ok(InputOutcome::Queued),
        }
    }
}

impl OwnerHost {
    /// Acquire input authority for an exact presentation.
    ///
    /// # Errors
    /// Refuses an expired target, a closed host, or an active publication.
    pub fn presentation_dispatcher(
        &self,
        address: PresentationAddress,
    ) -> Result<PresentationDispatcher, DispatchError> {
        self.core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?
            .authorizer(address)?;
        Ok(PresentationDispatcher {
            owner: Rc::downgrade(&self.core),
            address,
        })
    }
}
