//! `UiRealm` construction, identity, and frame-failure reporting.

use super::commands::UiCommandSender;
use super::input::FocusCoordinator;
use super::presentation_lifecycle::HostLifecycle;
use super::{DEFAULT_COMMAND_CAPACITY, UiRealm, UiRealmError};
use crate::app::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use crate::app::presentation::{PresentationState, PresentationWindow, RealmCapabilities};
use crate::app::presentation_forest::PresentationForest;
use crate::app::runtime::RealmServices;
use crossbeam_channel::bounded;
use flui_foundation::{PresentationId, RealmId};
use flui_interaction::InteractionLane;
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_scheduler::AppLifecycleState;
use flui_view::GlobalKeyScope;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
#[cfg(test)]
use std::sync::atomic::Ordering;

impl UiRealm {
    /// Construct the runtime with the default inbox capacity.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread — in production this is
    /// `AppRuntime::frame_wake_callback()`. `needs_redraw` is a clone of
    /// that same runtime's flag (see [`Self::needs_redraw`]'s field doc).
    /// `device_pixel_ratio` is applied to the freshly built pipeline BEFORE
    /// this constructor returns — the window's constraints are set later,
    /// but the scale must already agree so the first frame's `RenderView`
    /// configuration and layout do not disagree on it.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
    pub(crate) fn new(
        wake: Arc<dyn Fn() + Send + Sync>,
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: f32,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        Self::with_capacity(
            DEFAULT_COMMAND_CAPACITY,
            wake,
            window,
            device_pixel_ratio,
            needs_redraw,
        )
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
        wake: Arc<dyn Fn() + Send + Sync>,
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: f32,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        assert!(capacity > 0, "UiRealm inbox capacity must be non-zero");
        let identity = crate::app::runtime::next_identity();
        let services = RealmServices::construct();
        Self::construct(
            capacity,
            wake,
            identity,
            window,
            Some(device_pixel_ratio),
            services,
            needs_redraw,
        )
    }

    /// Builds the realm from already-resolved pieces: identity, the
    /// presentation's window, and `services: RealmServices` — a fresh
    /// `UpdateScheduler` plus the `local_post_frame_lane()`/`async_driver()`
    /// handles derived from it, built by the caller (`RealmServices::
    /// construct`, in `runtime.rs`), which is what makes `UiRealm` perform
    /// zero `::instance()` calls and gives every realm its own scheduler
    /// strong root instead of sharing a process-global one.
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
        wake: Arc<dyn Fn() + Send + Sync>,
        (realm_id, presentation_id): (RealmId, PresentationId),
        window: impl Into<PresentationWindow>,
        device_pixel_ratio: Option<f32>,
        services: RealmServices,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let RealmServices {
            local_post_frame,
            async_driver,
            scheduler,
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

        let pipeline = PipelineCell::new(PipelineOwner::new());
        if let Some(device_pixel_ratio) = device_pixel_ratio {
            pipeline.with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
        }

        let presentation = PresentationState::new(
            presentation_id,
            pipeline,
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
                    presentation_id,
                    wake: Arc::clone(&wake),
                },
            },
        );

        Ok(Self {
            realm_id,
            local_post_frame,
            interaction_lane,
            global_key_scope,
            presentations: PresentationForest::single(presentation),
            focus_coordinator: FocusCoordinator::new(presentation_id),
            host_lifecycle: Cell::new(HostLifecycle::Observed(AppLifecycleState::Resumed)),
            start: web_time::Instant::now(),
            needs_redraw,
            wake: Arc::clone(&wake),
            #[cfg(test)]
            now_secs_override: AtomicU64::new(0),
            rx,
            sender_prototype: UiCommandSender {
                tx,
                capacity,
                redraw_pending: Arc::clone(&redraw_pending),
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

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::for_test_with_text_input(None)
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_text_input(
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let identity = crate::app::runtime::next_identity();
        let window = crate::app::presentation::test_platform_window(platform_text_input);
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
            wake,
            identity,
            window,
            None,
            RealmServices::construct(),
            needs_redraw,
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

    /// This incarnation's generational realm identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// Install (or clear) the embedder's typed frame-failure callback.
    /// Called by each backend's bootstrap with
    /// `AppConfig::frame_failure_handler` right after realm construction.
    pub(crate) fn set_frame_failure_handler(&self, handler: Option<FrameFailureHandler>) {
        let _prev = std::mem::replace(&mut *self.frame_failure_handler.borrow_mut(), handler);
    }

    /// Install the realm-scoped frame-failure text-retention policy.
    pub(crate) fn set_frame_failure_detail(&self, detail: FrameFailureDetail) {
        self.frame_failure_detail.set(detail);
    }

    #[cfg(test)]
    pub(crate) fn frame_failure_detail_for_test(&self) -> FrameFailureDetail {
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
        match &report.kind {
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
        }
        let handler = self.frame_failure_handler.borrow().clone();
        if let Some(handler) = handler {
            // The handler is embedder code running INSIDE the frame pump —
            // for a segment-panic report, OUTSIDE the per-presentation
            // `catch_unwind` (the boundary's `Err` arm already returned
            // from it). An uncontained handler panic would therefore
            // reopen exactly the process-fatal path this boundary exists
            // to close: unwind through the remaining siblings' segments
            // and into the runner's `resume_unwind`. On the
            // pipeline-error path (reported from inside the segment) it
            // was subtly worse: the boundary caught the HANDLER's panic
            // as a segment panic and re-reported it — invoking the same
            // panicking handler a second time, now uncontained.
            //
            // So the delivery itself is contained. A panicking handler is
            // an EMBEDDER bug: it is logged at error level (no `BUG:`
            // classification — that prefix asserts a FLUI invariant) and
            // the report it was given is already fully traced above, so
            // no diagnostics are lost. The handler stays registered — each
            // future delivery is individually contained (one call per
            // report, never a retry loop), and a transiently-broken
            // handler keeps receiving reports once it stops panicking.
            // Automatic disarming would silently cut off the embedder's
            // failure feed on the strength of a heuristic, which is the
            // "silent skip" shape this route exists to avoid.
            if catch_unwind(AssertUnwindSafe(|| handler.call(&report))).is_err() {
                tracing::error!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        report.address.presentation_id.as_u64(),
                    realm_id = report.address.realm_id.as_u64(),
                    "the registered FrameFailureHandler panicked while receiving this \
                     report — embedder bug; the panic was contained and the report was \
                     already traced above"
                );
            }
        }
    }
}
