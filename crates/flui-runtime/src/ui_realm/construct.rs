//! `UiRealm` construction, identity, and frame-failure reporting.

use super::commands::{UiCommandSender, WakeDebt};
use super::input::FocusCoordinator;
use super::presentation_lifecycle::HostLifecycle;
use super::{DEFAULT_COMMAND_CAPACITY, UiRealm, UiRealmError};
use crate::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use crate::presentation::{PresentationState, PresentationWindow, RealmCapabilities};
use crate::presentation_forest::PresentationForest;
use crate::realm_services::{RealmHostServices, RealmServices};
use crossbeam_channel::bounded;
use flui_foundation::{PresentationId, RealmId};
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

impl UiRealm {
    /// Construct the runtime with the default inbox capacity, over the
    /// services its `host` hands it (see [`RealmHostServices::new`]).
    ///
    /// `device_pixel_ratio` is applied to the freshly built pipeline BEFORE
    /// this constructor returns — the window's constraints are set later,
    /// but the scale must already agree so the first frame's `RenderView`
    /// configuration and layout do not disagree on it.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
    pub fn new(
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: f64,
        host: RealmHostServices<'_>,
    ) -> Result<Self, UiRealmError> {
        Self::with_capacity(DEFAULT_COMMAND_CAPACITY, window, device_pixel_ratio, host)
    }

    /// [`Self::new`] with an explicit inbox capacity.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::InteractionLane`] if the owner-local interaction lane
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
        host: RealmHostServices<'_>,
    ) -> Result<Self, UiRealmError> {
        assert!(capacity > 0, "UiRealm inbox capacity must be non-zero");
        let identity = crate::realm_services::next_identity();
        Self::construct(
            capacity,
            identity,
            window,
            Some(device_pixel_ratio),
            RealmServices::construct(host),
        )
    }

    /// Builds the realm from already-resolved pieces: identity, the
    /// presentation's window, and `services: RealmServices` — the host's
    /// services plus a fresh `UpdateScheduler` and the
    /// `local_post_frame_lane()`/`async_driver()` handles derived from it,
    /// built by `RealmServices::construct`, which is what makes `UiRealm`
    /// perform zero `::instance()` calls and gives every realm its own
    /// scheduler strong root instead of sharing a process-global one.
    ///
    /// `device_pixel_ratio` is `None` only for the `#[cfg(test)]`
    /// constructors, which never touched it before this function existed
    /// (`PipelineOwner::new`'s own default applies) — preserved exactly,
    /// not silently changed to an explicit `1.0`.
    ///
    /// Assembles this realm's INITIAL presentation via
    /// [`PresentationState::new`], which installs the scope, the realm's
    /// shared dispatch handles, and this presentation's own focus/IME
    /// before anything attaches or mounts (ADR-0043 §1). Production
    /// topology is no longer limited to this one presentation:
    /// `PresentationForest`'s former `len()<=1` ratchet is lifted (issue
    /// #555's addressed-routing slice) — [`Self::install_presentation`]
    /// (paired with [`Self::assemble_presentation`]) is how a realm grows
    /// PAST this initial presentation, once the realm itself already
    /// exists.
    pub(super) fn construct(
        capacity: usize,
        (realm_id, presentation_id): (RealmId, PresentationId),
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: Option<f64>,
        services: RealmServices,
    ) -> Result<Self, UiRealmError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let command_wake_debt = Arc::new(WakeDebt::default());
        let RealmServices {
            local_post_frame,
            async_driver,
            scheduler,
            wake,
            needs_redraw,
            clipboard,
            storage,
            clock,
            text,
        } = services;

        // The realm's scheduler fires the SAME platform wake its presentation
        // and command sender use. This is the edge an async completion travels:
        // a background task's `Waker::wake()` reaches `AsyncDriver`'s
        // request-frame, which reaches `UpdateScheduler::request_frame`, whose
        // `frame_scheduled` false->true transition fires this hook. Without it
        // that demand is a bare atomic store only an already-running pump can
        // observe, so an idle `ControlFlow::Wait` loop sleeps through it and
        // the future silently stops advancing until unrelated input arrives.
        //
        // Installed here rather than at each constructor because `construct`
        // is the single chokepoint every `UiRealm` is built through — a realm
        // with an unwired scheduler wake is not constructible.
        scheduler.set_on_frame_scheduled(Some(Arc::clone(&wake)));

        let interaction_lane = InteractionLane::try_new()?;
        let global_key_scope = GlobalKeyScope::new();

        let presentation = PresentationState::new(
            presentation_id,
            device_pixel_ratio,
            window,
            RealmCapabilities {
                global_key_scope: global_key_scope.clone(),
                async_driver,
                local_post_frame_handle: local_post_frame.local_handle(),
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

        // The frame-time origin reads the realm's clock, so a manual clock's
        // frame timestamps measure from the same timeline they advance on.
        let start = flui_foundation::MonotonicClock::now(&clock);
        Ok(Self {
            realm_id,
            local_post_frame,
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

    /// A realm over a focused test window, with a wake that only sets the
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
    /// If the realm's interaction lane cannot be created.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn for_test_with_text_input(
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let identity = crate::realm_services::next_identity();
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
            RealmServices::construct(RealmHostServices::new(
                wake,
                needs_redraw,
                crate::presentation::test_clipboard(),
                &FontCollection::new(),
                ClockSource::Platform,
            )),
        )
        .expect("test UiRealm should create an interaction lane")
    }

    /// Test-only: a clone of this realm's exact `PipelineCell` — the same
    /// one its primary presentation's `renderer` and `widgets` share (one
    /// fact, one place).
    #[cfg(test)]
    pub(crate) fn pipeline_for_test(&self) -> PipelineCell {
        self.presentations.primary().pipeline().clone()
    }

    /// Test-only: this realm's text context, so a test can check which font
    /// collection it was built from and which pipelines lend it.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn text_context_for_test(&self) -> &flui_rendering::TextContextHandle {
        &self.text
    }

    /// This incarnation's generational realm identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// Install (or clear) the embedder's typed frame-failure callback.
    /// Called by each backend's bootstrap with
    /// `AppConfig::frame_failure_handler` right after realm construction.
    pub fn set_frame_failure_handler(&self, handler: Option<FrameFailureHandler>) {
        let _prev = std::mem::replace(&mut *self.frame_failure_handler.borrow_mut(), handler);
    }

    /// Install the realm-scoped frame-failure text-retention policy.
    pub fn set_frame_failure_detail(&self, detail: FrameFailureDetail) {
        self.frame_failure_detail.set(detail);
    }

    /// The realm's frame-failure text-retention policy.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn frame_failure_detail_for_test(&self) -> FrameFailureDetail {
        self.frame_failure_detail.get()
    }

    /// Surface one frame-failure report for `presentation` through tracing
    /// and the registered handler (if any).
    ///
    /// A dropped frame increments the presentation's consecutive-failure
    /// streak. An inner contained recovery exposes the current streak but
    /// leaves it unchanged.
    ///
    /// Both panic text and the text rendering of a pipeline error pass
    /// through this realm's [`FrameFailureDetail`] policy before tracing.
    /// The handler receives the same policy-filtered panic text, but retains
    /// the typed [`flui_rendering::RenderError`] and can inspect its variants
    /// without formatting it.
    ///
    /// The handler is cloned out of its cell before the call so no realm
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
                realm_id: self.realm_id,
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
                    realm_id = report.address.realm_id.as_u64(),
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
                    realm_id = report.address.realm_id.as_u64(),
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
                    realm_id = report.address.realm_id.as_u64(),
                    consecutive_failures,
                    internal_invariant,
                    ?at,
                    ?view_type_id,
                    ?hook,
                    panic_message = %message,
                    "lifecycle panic contained; frame continued for this presentation"
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
                        realm_id = report.address.realm_id.as_u64(),
                        "FrameFailureHandler delivery failed; the frame failure remains contained"
                    );
                }))
            {
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
    }
}
