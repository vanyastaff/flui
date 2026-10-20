use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use crate::ui_runtime::UiRuntime;
use flui_foundation::{
    PresentationAddress,
    geometry::{EdgeInsets, Size},
};
use flui_interaction::InputSurface;
use flui_platform_api::{Brightness, PlatformInput, WindowDisplay, WindowExecutionState};

use crate::HostInput;

/// The display a window is on, as the host derived it from the platform's
/// snapshot: a value to *set*, including "unknown". It is a type of its own
/// because a coalesced batch must let a newer "unknown" replace an older known
/// display; a bare `Option<WindowDisplay>` would be merged as "keep the older
/// when the newer has nothing".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayUpdate(Option<WindowDisplay>);

impl DisplayUpdate {
    /// An update that sets the window's display to `display`.
    #[must_use]
    pub const fn new(display: Option<WindowDisplay>) -> Self {
        Self(display)
    }

    /// The display this update sets.
    #[must_use]
    pub const fn get(self) -> Option<WindowDisplay> {
        self.0
    }
}

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
    /// The window moved and may now be on another display. Gesture geometry
    /// is re-read for new contacts when its display changed (ADR-0212 §2), and
    /// the window is handed its input regions again (ADR-0217 §5).
    Moved,
    /// Logical content-view safe area changed.
    SafeArea(EdgeInsets),
    /// Normalized system appearance changed.
    Brightness(Brightness),
    /// The display the window is on, or that display's refresh period, changed.
    Display(DisplayUpdate),
    /// The window's published state may have changed. Carries nothing: the
    /// owner reads the state when it delivers this.
    State,
}

impl WindowObservation {
    pub(super) fn apply(
        self,
        runtime: &UiRuntime,
        address: PresentationAddress,
        effects: &dyn OwnerEffects,
    ) {
        runtime.apply_window_observation(address, self, &|size, scale| {
            effects.resize_surface(address, size, scale);
        });
    }
}

/// A window observation applied on its own, in arrival order: every
/// [`WindowObservation`] that never merges into [`PendingWindowState`].
#[derive(Clone, Copy, Debug)]
pub(super) enum OrderedObservation {
    Focus(bool),
    Visibility(bool),
    Execution(WindowExecutionState),
    Snapshot {
        execution: WindowExecutionState,
        focused: bool,
        visible: bool,
    },
    Hover(bool),
    Moved,
    State,
}

impl From<OrderedObservation> for WindowObservation {
    fn from(observation: OrderedObservation) -> Self {
        match observation {
            OrderedObservation::Focus(focused) => Self::Focus(focused),
            OrderedObservation::Visibility(visible) => Self::Visibility(visible),
            OrderedObservation::Execution(execution) => Self::Execution(execution),
            OrderedObservation::Snapshot {
                execution,
                focused,
                visible,
            } => Self::Snapshot {
                execution,
                focused,
                visible,
            },
            OrderedObservation::Hover(inside) => Self::Hover(inside),
            OrderedObservation::Moved => Self::Moved,
            OrderedObservation::State => Self::State,
        }
    }
}

impl UiRuntime {
    /// Apply one observation of `address`'s window, as
    /// [`HostInput::Window`] does: `resize_surface` takes accepted surface
    /// metrics before the logical metrics change. While a tap records, the
    /// observation is recorded after it applied (ADR-0215 §1).
    pub fn apply_window_observation(
        &self,
        address: PresentationAddress,
        observation: WindowObservation,
        resize_surface: &dyn Fn(Size<f64>, f64),
    ) {
        match PendingWindowState::from_observation(observation) {
            Ok(pending) => pending.apply(self, address, resize_surface),
            Err(ordered) => self.apply_ordered_observation(address, ordered),
        }
    }

    /// Apply an observation that is never merged; while a tap records, it is
    /// recorded after it applied.
    pub(super) fn apply_ordered_observation(
        &self,
        address: PresentationAddress,
        observation: OrderedObservation,
    ) {
        let id = address.presentation_id;
        let recording = self.begin_recorded_input();
        // What the recording keeps of the observation. A state notification
        // carries nothing a replay could honour: the owner read this host's
        // live window. What it did to the presentation is recorded instead, as
        // the move it was when the content went somewhere else.
        let mut recorded = Some(WindowObservation::from(observation));
        let applied = catch_unwind(AssertUnwindSafe(|| match observation {
            OrderedObservation::Focus(focused) => self.update_window_focus(id, focused),
            OrderedObservation::Visibility(visible) => self.update_window_visibility(id, visible),
            OrderedObservation::Execution(execution) => {
                self.update_window_execution(id, execution);
            }
            OrderedObservation::Snapshot {
                execution,
                focused,
                visible,
            } => {
                self.synchronize_window_snapshot(id, execution, focused, visible);
            }
            OrderedObservation::Hover(inside) => self.handle_window_hover_addressed(id, inside),
            OrderedObservation::Moved => self.window_moved(id),
            OrderedObservation::State => {
                recorded = None;
                self.publish_window_state(id, || recorded = Some(WindowObservation::Moved));
            }
        }));
        if recording && let Some(recorded) = recorded {
            self.record_applied(id, HostInput::Window(recorded), InputOutcome::Handled);
        }
        if let Err(payload) = applied {
            std::panic::resume_unwind(payload);
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
    /// The newest display update; an update to "unknown" is still an update.
    display: Option<DisplayUpdate>,
}

impl PendingWindowState {
    fn metrics(size: Size<f64>, scale: f64) -> Self {
        Self {
            // Reject before coalescing so a bad observation cannot replace
            // an accepted resize still awaiting delivery.
            metrics: (scale.is_finite() && scale > 0.0).then_some((size, scale)),
            ..Self::default()
        }
    }

    pub(super) fn from_observation(
        observation: WindowObservation,
    ) -> Result<Self, OrderedObservation> {
        match observation {
            WindowObservation::Focus(focused) => Err(OrderedObservation::Focus(focused)),
            WindowObservation::Visibility(visible) => Err(OrderedObservation::Visibility(visible)),
            WindowObservation::Execution(execution) => {
                Err(OrderedObservation::Execution(execution))
            }
            WindowObservation::Snapshot {
                execution,
                focused,
                visible,
            } => Err(OrderedObservation::Snapshot {
                execution,
                focused,
                visible,
            }),
            WindowObservation::Hover(inside) => Err(OrderedObservation::Hover(inside)),
            WindowObservation::Moved => Err(OrderedObservation::Moved),
            WindowObservation::State => Err(OrderedObservation::State),
            WindowObservation::Metrics { size, scale_factor } => {
                Ok(Self::metrics(size, scale_factor))
            }
            WindowObservation::SafeArea(insets) => Ok(Self {
                safe_area: Some(insets),
                ..Self::default()
            }),
            WindowObservation::Brightness(brightness) => Ok(Self {
                brightness: Some(brightness),
                ..Self::default()
            }),
            WindowObservation::Display(update) => Ok(Self {
                display: Some(update),
                ..Self::default()
            }),
        }
    }

    pub(super) fn merge(&mut self, newer: &Self) {
        self.metrics = newer.metrics.or(self.metrics);
        self.safe_area = newer.safe_area.or(self.safe_area);
        self.brightness = newer.brightness.or(self.brightness);
        self.display = newer.display.or(self.display);
    }

    /// Apply the pending values together; while a tap records, each applied
    /// value is recorded as its own observation (ADR-0215 §1).
    pub(super) fn apply(
        mut self,
        runtime: &UiRuntime,
        address: PresentationAddress,
        resize_surface: &dyn Fn(Size<f64>, f64),
    ) {
        let recording = runtime.begin_recorded_input();
        let id = address.presentation_id;
        let mut first = None;
        // A display update asks for its own redraw, and only when it changes the
        // display the presentation holds (see `UiRuntime::apply_window_display`).
        let redraw =
            self.metrics.is_some() || self.safe_area.is_some() || self.brightness.is_some();
        if let Some((size, scale)) = self.metrics {
            first = catch_unwind(AssertUnwindSafe(|| {
                resize_surface(size, scale);
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
        if self.metrics.is_some() {
            let failure =
                catch_unwind(AssertUnwindSafe(|| runtime.refresh_gesture_context_for(id))).err();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                failure,
                "accepted pixel ratio gesture projection",
            );
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
        if let Some(update) = self.display {
            let failure = catch_unwind(AssertUnwindSafe(|| {
                runtime.apply_window_display(id, update.get());
            }))
            .err();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                failure,
                "window display publication",
            );
        }
        if redraw {
            let failure = catch_unwind(AssertUnwindSafe(|| runtime.request_redraw_of(id))).err();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                failure,
                "window state redraw",
            );
        }
        if recording {
            let applied = [
                self.metrics
                    .map(|(size, scale_factor)| WindowObservation::Metrics { size, scale_factor }),
                self.safe_area.map(WindowObservation::SafeArea),
                self.brightness.map(WindowObservation::Brightness),
                // A display update is not recorded: its `DisplayId` names a
                // display of this host's platform instance (ADR-0236 §3),
                // which a replaying host cannot be handed.
            ];
            for observation in applied.into_iter().flatten() {
                runtime.record_applied(id, HostInput::Window(observation), InputOutcome::Handled);
            }
        }
        super::finish_retirement(first);
    }
}

use super::{Delivery, DispatchError, OwnerCore, OwnerEffects, OwnerHost, RuntimeWork};

/// Input disposition. Only `Unhandled` permits native default handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOutcome {
    /// The framework handled the input.
    Handled,
    /// Keyboard input ran synchronously and no framework handler consumed it,
    /// or an assistive request reached no handler.
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
    /// Install the native producer's read policy for this exact presentation.
    ///
    /// # Errors
    /// Refuses an expired or closing target, closed host or active publication.
    pub fn install_captured_text_sizing(
        &self,
        source: flui_painting::TextSizingSource,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(
            RuntimeWork::TextSizingInstall(self.address, source),
            effects,
        )
    }

    /// Offer retained requests after an actual native readiness opportunity.
    ///
    /// # Errors
    /// Refuses an expired or closing target, closed host or active publication.
    pub fn service_text_sizing_source(
        &self,
        source: &flui_painting::TextSizingSource,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(
            RuntimeWork::TextSizingService(self.address, source.clone()),
            effects,
        )
    }

    /// Withdraw only this producer's numeric source and pending work.
    ///
    /// # Errors
    /// Refuses an expired or closing target, closed host or active publication.
    pub fn withdraw_captured_text_sizing(
        &self,
        source: &flui_painting::TextSizingSource,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(
            RuntimeWork::TextSizingWithdraw(self.address, source.clone()),
            effects,
        )
    }

    /// Inject a fault or reentrant operation through the real delivery machinery.
    /// Available only to test consumers; production work uses closed operations.
    ///
    /// # Errors
    /// Refuses an expired host or presentation, or an active publication.
    #[cfg(feature = "testing")]
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

    /// Admit one unit of host input to the owner FIFO — the runtime's only
    /// input path (ADR-0207 §1) — and deliver within the remaining budget.
    ///
    /// Older accepted work always precedes it, including across native
    /// callbacks; a native event that arrives while a callback runs joins the
    /// queue behind it, never dropped. The runtime unwraps the value: platform
    /// input goes to the presentation's gesture, focus and text-input owners,
    /// a window observation updates its metrics and lifecycle, a frame paces
    /// its gesture sampling at the vsync and then renders.
    ///
    /// An AT request takes the path the window's accessibility bridge sends
    /// it on (ADR-0207, amendment): it is queued in the runtime's command
    /// inbox and reaches its semantics owner at the next owner turn's Idle
    /// drain, before that turn's frame begins, and the answer is
    /// [`InputOutcome::Queued`].
    ///
    /// Adjacent pending metrics, safe-area and brightness observations for
    /// this exact window keep their latest values together; any other input
    /// ends that segment, and observations already executing are never
    /// replaced.
    ///
    /// Only a keyboard event that ran synchronously and found no handler is
    /// [`InputOutcome::Unhandled`], which permits the native default. Queued
    /// input suppresses it because the framework cannot answer synchronously.
    ///
    /// # Errors
    /// Refuses an expired target or host, or an active publication;
    /// [`DispatchError::InboxFull`] for an AT request the runtime's inbox
    /// has no room for.
    pub fn dispatch(
        &self,
        input: HostInput,
        effects: &dyn OwnerEffects,
    ) -> Result<InputOutcome, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        let delivered = match input {
            HostInput::Platform(input) => {
                return self.platform_input(&core, InputSurface::Window, input, effects);
            }
            HostInput::Window(observation) => {
                let work = match PendingWindowState::from_observation(observation) {
                    Ok(state) => RuntimeWork::WindowState(self.address, state),
                    Err(event) => RuntimeWork::Observation(self.address, event),
                };
                core.deliver(work, effects)?
            }
            HostInput::Frame { vsync } => {
                core.deliver(RuntimeWork::Frame(self.address, vsync), effects)?
            }
            HostInput::Accessibility(request) => {
                return self.queue_accessibility(&core, request);
            }
        };
        Ok(match delivered {
            Delivery::Driven => InputOutcome::Handled,
            Delivery::Queued => InputOutcome::Queued,
        })
    }

    /// [`dispatch`](Self::dispatch) platform input a surface of this
    /// presentation's window reported: the window itself, or a surface
    /// embedded in it that its gesture binding minted, whose contacts, hover
    /// and latches stay its own (ADR-0207 §5). The same owner FIFO admits it.
    ///
    /// # Errors
    /// Refuses an expired target or host, or an active publication.
    pub fn dispatch_on(
        &self,
        surface: InputSurface,
        input: PlatformInput,
        effects: &dyn OwnerEffects,
    ) -> Result<InputOutcome, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        self.platform_input(&core, surface, input, effects)
    }

    /// Queue an AT request in the addressed runtime's command inbox, stamped
    /// for this presentation, as its accessibility bridge does.
    fn queue_accessibility(
        &self,
        core: &OwnerCore,
        request: accesskit::ActionRequest,
    ) -> Result<InputOutcome, DispatchError> {
        let sender = {
            let state = core.state.try_borrow().map_err(|_| DispatchError::Busy)?;
            let index = state.authorizer(self.address)?;
            state.runtimes[index]
                .factory
                .command_sender_for(self.address.presentation_id)
        };
        // No owner borrow is held: the send wakes the host.
        match sender.send_accessibility(request) {
            Ok(()) => Ok(InputOutcome::Queued),
            Err(crate::ui_runtime::CommandSendError::ChannelFull { .. }) => {
                Err(DispatchError::InboxFull)
            }
            Err(crate::ui_runtime::CommandSendError::OwnerGone { .. }) => {
                Err(DispatchError::UnknownRuntime)
            }
        }
    }

    fn platform_input(
        &self,
        core: &Rc<OwnerCore>,
        surface: InputSurface,
        input: PlatformInput,
        effects: &dyn OwnerEffects,
    ) -> Result<InputOutcome, DispatchError> {
        let keyboard = matches!(input, PlatformInput::Keyboard(_));
        let reply = Rc::new(Cell::new(None));
        core.deliver(
            RuntimeWork::Input {
                address: self.address,
                surface,
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

#[cfg(test)]
mod tests {
    use flui_foundation::PresentationAddress;
    use flui_platform_api::Brightness;

    use super::{PendingWindowState, WindowObservation};
    use crate::ui_runtime::UiRuntime;

    /// A secondary window's appearance batch reschedules that window, not the
    /// primary.
    fn a_window_state_batch_redraws_the_window_it_came_from() {
        let mut runtime = UiRuntime::for_test();
        let primary = runtime.presentation_id();
        let secondary = runtime.install_second_presentation_for_test();
        for id in [primary, secondary] {
            let _ = runtime.take_redraw_pending_for_test(id);
        }

        PendingWindowState::from_observation(WindowObservation::Brightness(Brightness::Dark))
            .expect("appearance is a pending window state")
            .apply(
                &runtime,
                PresentationAddress {
                    ui_runtime_id: runtime.id(),
                    presentation_id: secondary,
                },
                &|_, _| {},
            );

        assert_eq!(runtime.take_redraw_pending_for_test(secondary), Some(true));
        assert_eq!(runtime.take_redraw_pending_for_test(primary), Some(false));
    }

    #[test]
    fn window_state_batches() {
        crate::table_test::run_table(
            "window_state_batches",
            &[(
                "a_window_state_batch_redraws_the_window_it_came_from",
                a_window_state_batch_redraws_the_window_it_came_from as fn(),
            )],
        );
    }
}
