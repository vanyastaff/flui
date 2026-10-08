//! `UiRuntime` construction, identity, and frame-failure reporting.

use super::commands::{UiCommandSender, WakeDebt};
use super::input::FocusCoordinator;
use super::presentation_lifecycle::HostLifecycle;
use super::{DEFAULT_COMMAND_CAPACITY, UiRuntime, UiRuntimeError};
use crate::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use crate::presentation::{PresentationState, PresentationWindow, RuntimeCapabilities};
use crate::presentation_forest::PresentationForest;
use crate::runtime_services::{RuntimeHostServices, RuntimeServices};
use crossbeam_channel::bounded;
use flui_foundation::{PresentationId, UiRuntimeId};
use flui_interaction::InteractionLane;
#[cfg(any(test, feature = "test-support"))]
use flui_painting::FontCollection;
#[cfg(any(test, feature = "test-support"))]
use flui_platform_api::PlatformTextInput;
#[cfg(test)]
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::AppLifecycleState;
#[cfg(any(test, feature = "test-support"))]
use flui_scheduler::ClockSource;
use flui_view::GlobalKeyScope;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::AtomicU64;
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::Ordering;

impl UiRuntime {
    /// Construct the runtime with the default inbox capacity, over the
    /// services its `host` hands it (see [`RuntimeHostServices::new`]).
    ///
    /// `device_pixel_ratio` is applied to the freshly built pipeline BEFORE
    /// this constructor returns — the window's constraints are set later,
    /// but the scale must already agree so the first frame's `RenderView`
    /// configuration and layout do not disagree on it.
    ///
    /// # Errors
    ///
    /// [`UiRuntimeError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
    pub fn new(
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: f64,
        host: RuntimeHostServices<'_>,
    ) -> Result<Self, UiRuntimeError> {
        Self::with_capacity(DEFAULT_COMMAND_CAPACITY, window, device_pixel_ratio, host)
    }

    /// [`Self::new`] with an explicit inbox capacity.
    ///
    /// # Errors
    ///
    /// [`UiRuntimeError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` (a zero-capacity inbox could never accept
    /// a command; every sender would spuriously report backpressure).
    pub(crate) fn with_capacity(
        capacity: usize,
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: f64,
        host: RuntimeHostServices<'_>,
    ) -> Result<Self, UiRuntimeError> {
        assert!(capacity > 0, "UiRuntime inbox capacity must be non-zero");
        let identity = crate::runtime_services::next_identity();
        Self::construct(
            capacity,
            identity,
            window,
            Some(device_pixel_ratio),
            RuntimeServices::construct(host),
        )
    }

    /// Builds the UI runtime from already-resolved pieces: identity, the
    /// presentation's window, and `services: RuntimeServices` — the host's
    /// services plus a fresh `UpdateScheduler` and the `OwnerFrame` made for
    /// it, built by `RuntimeServices::construct`, which is what makes `UiRuntime`
    /// perform zero `::instance()` calls and gives every UI runtime its own
    /// scheduler strong root instead of sharing a process-global one.
    ///
    /// `device_pixel_ratio` is `None` only for the `#[cfg(test)]`
    /// constructors, which never touched it before this function existed
    /// (`PipelineOwner::new`'s own default applies) — preserved exactly,
    /// not silently changed to an explicit `1.0`.
    ///
    /// Assembles this UI runtime's INITIAL presentation via
    /// [`PresentationState::new`], which installs the scope, the UI runtime's
    /// shared dispatch handles, and this presentation's own focus/IME
    /// before anything attaches or mounts (ADR-0043 §1). Production
    /// topology is no longer limited to this one presentation:
    /// `PresentationForest`'s former `len()<=1` ratchet is lifted (issue
    /// #555's addressed-routing slice) — [`Self::install_presentation`]
    /// (paired with [`Self::assemble_presentation`]) is how a UI runtime grows
    /// PAST this initial presentation, once the UI runtime itself already
    /// exists.
    pub(super) fn construct(
        capacity: usize,
        (ui_runtime_id, presentation_id): (UiRuntimeId, PresentationId),
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: Option<f64>,
        services: RuntimeServices,
    ) -> Result<Self, UiRuntimeError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let command_wake_debt = Arc::new(WakeDebt::default());
        let RuntimeServices {
            preferences,
            owner_frame,
            scheduler,
            wake,
            needs_redraw,
            clipboard,
            storage,
            clock,
            text,
        } = services;

        // The ui_runtime's scheduler fires the SAME platform wake its presentation
        // and command sender use. This is the edge an async completion travels:
        // a background task's `Waker::wake()` reaches the owner frame's
        // `FrameWaker`, which sets the scheduler's frame latch, whose
        // `frame_scheduled` false->true transition fires this hook. Without it
        // that demand is a bare atomic store only an already-running pump can
        // observe, so an idle `ControlFlow::Wait` loop sleeps through it and
        // the future silently stops advancing until unrelated input arrives.
        //
        // Installed here rather than at each constructor because `construct`
        // is the single chokepoint every `UiRuntime` is built through — a ui_runtime
        // with an unwired scheduler wake is not constructible.
        scheduler.set_on_frame_scheduled(Some(Arc::clone(&wake)));

        let interaction_lane = InteractionLane::try_new()?;
        let global_key_scope = GlobalKeyScope::new();

        let presentation = PresentationState::new(
            presentation_id,
            device_pixel_ratio,
            window,
            RuntimeCapabilities {
                global_key_scope: global_key_scope.clone(),
                async_driver: owner_frame.async_driver(),
                local_post_frame_handle: owner_frame.local_post_frame_handle(),
                interaction_dispatch_handle: interaction_lane.dispatch_handle(),
                scheduler: &scheduler,
                wake: Arc::clone(&wake),
                command_sender: UiCommandSender {
                    tx: tx.clone(),
                    capacity,
                    redraw_pending: Arc::clone(&redraw_pending),
                    wake_debt: Arc::clone(&command_wake_debt),
                    presentation_id,
                    wake: Arc::clone(&wake),
                },
                clipboard: Arc::clone(&clipboard),
                storage: storage.clone(),
                clock: &clock,
                text: text.clone(),
            },
        );

        // The frame-time origin reads the ui_runtime's clock, so a manual clock's
        // frame timestamps measure from the same timeline they advance on.
        let start = flui_foundation::MonotonicClock::now(&clock);
        if let Some(snapshot) = &preferences {
            super::preferences::publish(&presentation, snapshot);
        }
        Ok(Self {
            id: ui_runtime_id,
            preferences: RefCell::new(preferences),
            owner_frame,
            interaction_lane,
            global_key_scope,
            presentations: PresentationForest::single(presentation),
            focus_coordinator: FocusCoordinator::new(presentation_id),
            host_lifecycle: Cell::new(HostLifecycle::Observed(AppLifecycleState::Resumed)),
            start,
            clock,
            frame_time: Cell::new(None),
            needs_redraw,
            wake: Arc::clone(&wake),
            clipboard,
            storage,
            text,
            #[cfg(any(test, feature = "test-support"))]
            now_secs_override: AtomicU64::new(0),
            rx,
            sender_prototype: UiCommandSender {
                tx,
                capacity,
                redraw_pending: Arc::clone(&redraw_pending),
                wake_debt: command_wake_debt,
                presentation_id,
                wake,
            },
            redraw_pending,
            scheduler,
            frame_failure_handler: RefCell::new(None),
            frame_failure_detail: Cell::new(FrameFailureDetail::default()),
            _owner_affine: PhantomData,
        })
    }

    /// A UI runtime over a focused test window, with a wake that only sets the
    /// redraw flag.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn for_test() -> Self {
        Self::for_test_with_text_input(None)
    }

    /// [`Self::for_test`], its window offering `platform_text_input`.
    ///
    /// # Panics
    ///
    /// If the UI runtime's interaction lane cannot be created.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn for_test_with_text_input(
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let identity = crate::runtime_services::next_identity();
        let window = crate::presentation::test_platform_window(platform_text_input);
        // The no-op `wake` still must set THIS SAME `needs_redraw` flag —
        // in production the two are the same fact through AppRuntime's
        // `frame_wake_callback` (see `Self::needs_redraw`'s field doc); a
        // disconnected no-op here would silently break every test that
        // calls `wake_frame()`/relies on the vsync/gesture continuation
        // setting `needs_redraw` (there is no window to poke in a test, so
        // only the flag half applies).
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let wake_needs_redraw = Arc::clone(&needs_redraw);
        let wake: Arc<dyn Fn() + Send + Sync> =
            Arc::new(move || wake_needs_redraw.store(true, Ordering::Relaxed));
        Self::construct(
            DEFAULT_COMMAND_CAPACITY,
            identity,
            window,
            None,
            RuntimeServices::construct(RuntimeHostServices::new(
                wake,
                needs_redraw,
                crate::presentation::test_clipboard(),
                &FontCollection::new(),
                ClockSource::Platform,
            )),
        )
        .expect("test UiRuntime should create an interaction lane")
    }

    /// Test-only: a clone of this UI runtime's exact `PipelineCell` — the same
    /// one its primary presentation's `renderer` and `widgets` share (one
    /// fact, one place).
    #[cfg(test)]
    pub(crate) fn pipeline_for_test(&self) -> PipelineCell {
        self.presentations.primary().pipeline().clone()
    }

    /// Test-only: this UI runtime's text context, so a test can check which font
    /// collection it was built from and which pipelines lend it.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn text_context_for_test(&self) -> &flui_rendering::TextContextHandle {
        &self.text
    }

    /// This incarnation's generational UI runtime identity.
    #[must_use]
    pub fn id(&self) -> UiRuntimeId {
        self.id
    }

    /// Install (or clear) the embedder's typed frame-failure callback.
    /// Called by each backend's bootstrap with
    /// `AppConfig::frame_failure_handler` right after UI runtime construction.
    pub fn set_frame_failure_handler(&self, handler: Option<FrameFailureHandler>) {
        let _prev = std::mem::replace(&mut *self.frame_failure_handler.borrow_mut(), handler);
    }

    /// Install the UI runtime-scoped frame-failure text-retention policy.
    pub fn set_frame_failure_detail(&self, detail: FrameFailureDetail) {
        self.frame_failure_detail.set(detail);
    }

    /// The UI runtime's frame-failure text-retention policy.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn frame_failure_detail_for_test(&self) -> FrameFailureDetail {
        self.frame_failure_detail.get()
    }

    /// Report an application callback's panic that the host contained on
    /// this UI runtime's owner turn, outside any frame, as one
    /// [`FrameFailureKind::CallbackPanic`] addressed to `address`: for a
    /// UI runtime-level task, the UI runtime's primary presentation; for a
    /// presentation's close, the presentation closing, which may already be
    /// gone. The payload stays the caller's: it is read, never dropped here.
    ///
    /// Not yet delivered to the registered [`FrameFailureHandler`]: the
    /// panic is only traced.
    pub fn report_contained_panic(
        &self,
        address: flui_foundation::PresentationAddress,
        payload: &(dyn std::any::Any + Send),
    ) {
        let (message, internal_invariant) = self.frame_failure_detail.get().panic_text(payload);
        // Diagnostics are foreign code through tracing subscribers; a failed
        // diagnostic must not unwind into the host's containment boundary.
        if let Err(failure) = catch_unwind(AssertUnwindSafe(|| {
            tracing::error!(
                { flui_foundation::diagnostics::PRESENTATION_ID } =
                    address.presentation_id.as_u64(),
                ui_runtime_id = address.ui_runtime_id.as_u64(),
                internal_invariant,
                panic_message = %message,
                "application callback panic contained; the ui_runtime keeps running"
            );
        })) {
            flui_foundation::panic::retain_opaque_payload(failure);
        }
    }

    /// Surface one frame-failure report for `presentation` through tracing
    /// and the registered handler (if any).
    ///
    /// A dropped frame increments the presentation's consecutive-failure
    /// streak. An inner contained recovery exposes the current streak but
    /// leaves it unchanged.
    ///
    /// Both panic text and the text rendering of a pipeline error pass
    /// through this UI runtime's [`FrameFailureDetail`] policy before tracing.
    /// The handler receives the same policy-filtered panic text, but retains
    /// the typed [`flui_rendering::RenderError`] and can inspect its variants
    /// without formatting it.
    ///
    /// The handler is cloned out of its cell before the call so no UI runtime
    /// borrow is held while embedder code runs; see
    /// [`FrameFailureHandler`]'s doc for the re-entrancy contract it must
    /// still honor (it runs mid-frame, inside the pump).
    pub(super) fn report_frame_failure(
        &self,
        presentation: &PresentationState,
        kind: FrameFailureKind,
    ) {
        let disposition = kind.disposition();
        let consecutive_failures = match disposition {
            FailureDisposition::FrameDropped => presentation.note_frame_failure(),
            FailureDisposition::Contained => presentation.frame_failure_streak(),
        };
        let report = FrameFailureReport {
            address: flui_foundation::PresentationAddress {
                ui_runtime_id: self.id,
                presentation_id: presentation.id(),
            },
            kind,
            disposition,
            consecutive_failures,
        };
        // Diagnostics are foreign code through tracing subscribers. A failed
        // diagnostic must not prevent the report's callback or sibling frames.
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| match &report.kind {
            FrameFailureKind::SegmentPanic {
                message,
                phase,
                internal_invariant,
            } => {
                tracing::error!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        report.address.presentation_id.as_u64(),
                    ui_runtime_id = report.address.ui_runtime_id.as_u64(),
                    consecutive_failures,
                    internal_invariant,
                    phase = ?phase,
                    panic_message = %message,
                    "frame segment panicked; frame dropped for this presentation only — \
                     siblings keep framing, last presented frame is retained"
                );
            }
            FrameFailureKind::Pipeline { error } => {
                let error_text = self.frame_failure_detail.get().pipeline_text(error);
                tracing::error!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        report.address.presentation_id.as_u64(),
                    ui_runtime_id = report.address.ui_runtime_id.as_u64(),
                    consecutive_failures,
                    error = %error_text,
                    "frame pipeline failed; frame dropped for this presentation only — \
                     siblings keep framing, last presented frame is retained"
                );
            }
            FrameFailureKind::RecoveredPanic {
                at,
                view_type_id,
                hook,
                message,
                internal_invariant,
            } => {
                tracing::error!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        report.address.presentation_id.as_u64(),
                    ui_runtime_id = report.address.ui_runtime_id.as_u64(),
                    consecutive_failures,
                    internal_invariant,
                    ?at,
                    ?view_type_id,
                    ?hook,
                    panic_message = %message,
                    "lifecycle panic contained; frame continued for this presentation"
                );
            }
            FrameFailureKind::CallbackPanic {
                message,
                internal_invariant,
            } => {
                tracing::error!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        report.address.presentation_id.as_u64(),
                    ui_runtime_id = report.address.ui_runtime_id.as_u64(),
                    consecutive_failures,
                    internal_invariant,
                    panic_message = %message,
                    "application callback panic contained; the ui_runtime keeps running"
                );
            }
        })) {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
        let handler = self.frame_failure_handler.borrow().clone();
        if let Some(handler) = handler {
            // Keep the owning envelope outside the callback's unwind boundary.
            // It may be the final owner after callback-driven unregistration.
            let failed = match catch_unwind(AssertUnwindSafe(|| handler.call(&report))) {
                Ok(()) => match catch_unwind(AssertUnwindSafe(|| drop(handler))) {
                    Ok(()) => false,
                    Err(payload) => {
                        flui_foundation::panic::retain_opaque_payload(payload);
                        true
                    }
                },
                Err(payload) => {
                    // Aggregate capture destruction is unsafe while preserving
                    // another failure; retain the opaque callback envelope.
                    std::mem::forget(handler);
                    flui_foundation::panic::retain_opaque_payload(payload);
                    true
                }
            };
            if failed
                && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                    tracing::error!(
                        { flui_foundation::diagnostics::PRESENTATION_ID } =
                            report.address.presentation_id.as_u64(),
                        ui_runtime_id = report.address.ui_runtime_id.as_u64(),
                        "FrameFailureHandler delivery failed; the frame failure remains contained"
                    );
                }))
            {
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
    }
}
