//! Input dispatch and focus coordination.

use super::UiRuntime;
use super::presentation_lifecycle::HostLifecycle;
use crate::epoch::FrameCommitState;
use crate::presentation::PresentationState;
use flui_foundation::{InputSequenceId, PresentationId};
use flui_interaction::{InputSurface, PointerEvent};
use flui_platform_api::{DragDropEvent, EventTime, PlatformInput};
use flui_rendering::binding::RendererBinding as _;
use flui_scheduler::AppLifecycleState;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

/// Runtime-level arbitration of which presentation currently owns OS keyboard
/// focus (issue #555's addressed-routing slice). One [`FocusManager`](flui_interaction::FocusManager) exists per presentation
/// (`PresentationState::focus_manager`), but only ONE presentation's tree
/// should ever receive a keyboard event at a time — the one whose native
/// window the platform most recently reported as focused.
/// [`UiRuntime::handle_input_addressed`]'s `Keyboard` arm routes there,
/// deliberately never to whichever presentation an individual event happens
/// to be stamped for: a stray or racing keyboard event addressed to a
/// presentation that is not (or no longer) the OS-focused one must still
/// land on the active one, matching a real OS's single-keyboard-focus model
/// rather than trusting the per-event address.
///
/// Defaults to the UI runtime's initial presentation. A UI runtime may host N resident
/// presentations, so focus starts on the primary and moves only once a
/// genuine `WindowFocus(true)` event names a different live forest member;
/// secondary widget content and frame submission remain unwired.
pub(super) struct FocusCoordinator {
    active: Cell<PresentationId>,
}

impl FocusCoordinator {
    pub(super) fn new(initial: PresentationId) -> Self {
        Self {
            active: Cell::new(initial),
        }
    }

    /// The presentation keyboard input currently routes to.
    pub(super) fn active(&self) -> PresentationId {
        self.active.get()
    }

    /// Record that `id` is now the active presentation. Two write sides:
    /// `PlatformToUi::WindowFocus(true)` handling (`runner.rs`) calls this
    /// with the event's own addressed presentation when its native window
    /// gains OS focus; [`UiRuntime::close_presentation_entered`] calls this
    /// with the surviving primary when `id` is the presentation being
    /// closed and it was the active one (never leave this pointing at a
    /// presentation the forest is about to drop).
    ///
    /// A `WindowFocus(false)` (focus LOST) never calls this: losing focus
    /// names no new owner, so the coordinator keeps pointing at whichever
    /// presentation held it most recently (losing OS focus does not un-focus
    /// the last-focused element; a *different* window gaining focus does).
    pub(super) fn note_focus_gained(&self, id: PresentationId) {
        self.active.set(id);
    }
}

/// What applying one platform input did: whether a handler consumed it (a
/// key a shortcut or focused node handled) and the input sequence its
/// dispatch placed it in.
#[derive(Debug, Clone, Copy, Default)]
struct Applied {
    handled: bool,
    sequence: Option<InputSequenceId>,
}

/// Keep the earliest input-phase panic when cleanup or wake work also
/// unwinds. Secondary panic payloads are intentionally leaked because their
/// destructors are arbitrary user code and must not replace or abort the
/// original failure while it is being resumed.
pub(super) fn preserve_first_input_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    candidate: Option<Box<dyn std::any::Any + Send>>,
    phase: &'static str,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if first.is_none() {
        *first = Some(candidate);
    } else {
        let telemetry_panic = catch_unwind(AssertUnwindSafe(|| {
            tracing::error!(
                phase,
                "input phase panicked after an earlier phase; only the first panic is resumed"
            );
        }))
        .err();
        // A panic payload may itself panic while being dropped. Leaking only
        // secondary exceptional payloads keeps the original failure stable,
        // including when the tracing subscriber itself panics.
        if let Some(payload) = telemetry_panic {
            std::mem::forget(payload);
        }
        std::mem::forget(candidate);
    }
}

/// Whether the UI runtime's input entry point must drop `input` outright,
/// before ever reaching the per-kind dispatch above, given the
/// presentation's current lifecycle. Moved here from the retired
/// `AppBinding`, unchanged — the match arms below are the per-lifecycle
/// rationale: a suspended presentation drops pointer/drag input (no gesture
/// arena to resolve into) but still accepts keyboard/IME (a background
/// window can keep text-input focus), while a not-yet-attached or closing
/// presentation drops everything.
///
/// Explicit and exhaustive over **both** axes — lifecycle and input kind —
/// with no wildcard on the input axis: adding a `PlatformInput` variant
/// breaks this match at compile time instead of silently falling through a
/// `_` arm.
pub(super) fn input_dropped_by_lifecycle(
    lifecycle: crate::presentation::PresentationLifecycle,
    input: &PlatformInput,
) -> bool {
    use crate::presentation::PresentationLifecycle;
    match lifecycle {
        PresentationLifecycle::Created
        | PresentationLifecycle::Closing
        | PresentationLifecycle::Closed => true,
        PresentationLifecycle::Suspended => match input {
            PlatformInput::Pointer(_) | PlatformInput::DragDrop(_) => true,
            // Keyboard/IME keep flowing; capabilities describe the session,
            // not an interaction, so a suspended window keeps them current.
            PlatformInput::Keyboard(_) | PlatformInput::Ime(_) | PlatformInput::Capabilities(_) => {
                false
            }
        },
        PresentationLifecycle::SurfaceAttached => false,
    }
}

/// A safe, content-free discriminator for a [`PlatformInput`] — never the
/// payload itself. Logging text-input/IME payloads and drag-and-drop payloads
/// is forbidden; this is the only thing about an input event that may reach a
/// trace/log line. Moved here from the retired `AppBinding`, unchanged.
pub(super) fn input_kind(input: &PlatformInput) -> &'static str {
    match input {
        PlatformInput::Pointer(_) => "pointer",
        PlatformInput::Keyboard(_) => "keyboard",
        PlatformInput::Ime(_) => "ime",
        PlatformInput::DragDrop(_) => "drag_drop",
        PlatformInput::Capabilities(_) => "capabilities",
    }
}

/// A safe, content-free discriminator for a [`DragDropEvent`] — never the
/// carried offer/payload. Moved here from the retired `AppBinding`, unchanged.
pub(super) fn drag_drop_kind(event: &DragDropEvent) -> &'static str {
    match event {
        DragDropEvent::Entered { .. } => "entered",
        DragDropEvent::Moved { .. } => "moved",
        DragDropEvent::Dropped { .. } => "dropped",
        DragDropEvent::Exited { .. } => "exited",
    }
}

impl UiRuntime {
    /// Finish an addressed input dispatch without letting a secondary platform
    /// wake panic replace the dispatch failure that caused the redraw request.
    ///
    /// The redraw flags are set before the wake runs, so even a panicking wake
    /// leaves durable demand behind. When both phases panic, the dispatch
    /// payload remains the first failure and the wake payload is discarded by
    /// [`preserve_first_input_panic`].
    pub(super) fn finish_addressed_input_dispatch(
        &self,
        presentation: &PresentationState,
        dispatch: Result<(), Box<dyn std::any::Any + Send>>,
    ) {
        let mut first_panic = dispatch.err();
        let wake_panic = catch_unwind(AssertUnwindSafe(|| {
            self.request_redraw_for(presentation);
        }))
        .err();
        preserve_first_input_panic(&mut first_panic, wake_panic, "redraw wake");
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    // ========================================================================
    // Input dispatch (moved from the retired `AppBinding`)
    // ========================================================================

    /// Handle a platform input event while this UI runtime is already entered,
    /// addressed to exactly one presentation this UI runtime hosts (issue #555
    /// the addressed-routing slice).
    ///
    /// `presentation_id` is the exact presentation `dispatch_platform_ui_runtime`
    /// (`runner.rs`) stamped this event with at enqueue time — the window
    /// that produced it, resolved through `WindowRegistry` at hop-1 — never
    /// assumed to be this UI runtime's primary. A `presentation_id` this UI runtime no
    /// longer hosts (closed between enqueue and drain) is a traced drop,
    /// never a panic or a silent fall-through to the forest's primary.
    ///
    /// Pointer/IME/drag-drop events go to the ADDRESSED presentation's own
    /// gesture/text-input state — never a sibling's, and never falling
    /// through to the primary when the addressed id is missing (pointer:
    /// `input_stamped_for_b_never_reaches_as_arena`). Keyboard is the one
    /// exception: it always goes to the focus coordinator's currently
    /// ACTIVE presentation instead of the stamped one — see
    /// `FocusCoordinator`'s own doc for why
    /// (`panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake`
    /// stamps a key for the primary and observes the active sibling's
    /// handler run).
    ///
    /// `input_dropped_by_lifecycle` gates every kind next, against the
    /// RESOLVED target's own lifecycle (not necessarily `presentation_id`'s,
    /// for `Keyboard`): `Closing`/`Closed` refuses all input; `Suspended`
    /// drops pointer/drag-drop only (keyboard/IME keep flowing, so a flaky
    /// or absent occlusion signal never becomes a keystroke blackout).
    ///
    /// Pointer events are coalesced by the target presentation's own
    /// `GestureBinding` — high-frequency move events are stored and flushed
    /// by the draw step of [`Self::pump`]. Keyboard and IME first deliver that
    /// target's pending motion without waiting for a frame or ending contacts.
    /// A motion callback failure does not erase the following input; both are
    /// contained independently and the first failure remains authoritative.
    ///
    /// Returns whether a handler consumed the event: a key event a global
    /// key handler (a shortcut) or a focused node handled. Every other kind,
    /// and any event dropped above, returns `false`, so the host may keep
    /// its own default for it (Alt+F4, the system menu).
    ///
    /// While a tap records, the input is recorded after its dispatch
    /// committed, with the sequence the dispatch placed it in and followed by
    /// the arena verdicts it produced (ADR-0215 §1); a dispatch failure is
    /// recorded too and then resumed.
    pub fn handle_input_addressed(
        &self,
        presentation_id: PresentationId,
        input: PlatformInput,
    ) -> bool {
        self.handle_input_on(presentation_id, InputSurface::Window, input)
    }

    /// [`handle_input_addressed`](Self::handle_input_addressed) for input a
    /// surface of `presentation_id`'s window reported: the window itself, or
    /// a surface embedded in it that the presentation's gesture binding
    /// minted. Pointer input on an embedded surface keeps that surface's own
    /// contacts, hover, coalescing and latches (ADR-0207 §5) and is held,
    /// like the window's, until the tree it hit-tests is committed. Keyboard,
    /// IME, drag-and-drop and capability input belong to the window whatever
    /// surface reported them.
    pub fn handle_input_on(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: PlatformInput,
    ) -> bool {
        if !self.begin_recorded_input() {
            return self
                .apply_platform_input(presentation_id, surface, input)
                .handled;
        }
        let keyboard = matches!(input, PlatformInput::Keyboard(_));
        let recorded = input.clone();
        let applied = catch_unwind(AssertUnwindSafe(|| {
            self.apply_platform_input(presentation_id, surface, input)
        }));
        let (outcome, sequence) = match &applied {
            Ok(applied) if keyboard && !applied.handled => {
                (crate::owner::InputOutcome::Unhandled, applied.sequence)
            }
            Ok(applied) => (crate::owner::InputOutcome::Handled, applied.sequence),
            Err(_) => (crate::owner::InputOutcome::Handled, None),
        };
        self.record_dispatched(
            presentation_id,
            surface,
            crate::HostInput::Platform(recorded),
            outcome,
            sequence,
        );
        applied.map_or_else(|payload| resume_unwind(payload), |applied| applied.handled)
    }

    fn apply_platform_input(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: PlatformInput,
    ) -> Applied {
        let target_id = match &input {
            PlatformInput::Keyboard(_) => self.focus_coordinator.active(),
            PlatformInput::Pointer(_)
            | PlatformInput::Ime(_)
            | PlatformInput::DragDrop(_)
            | PlatformInput::Capabilities(_) => presentation_id,
        };
        let Some(presentation) = self.presentations.get(target_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = target_id.as_u64(),
                stamped_presentation_id = presentation_id.as_u64(),
                input_kind = input_kind(&input),
                "dropping input addressed to a presentation this ui_runtime no longer hosts"
            );
            return Applied::default();
        };
        if presentation.closing_requested.get()
            || self.host_lifecycle.get() == HostLifecycle::Observed(AppLifecycleState::Detached)
            || input_dropped_by_lifecycle(presentation.lifecycle(), &input)
        {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation.id().as_u64(),
                lifecycle = ?presentation.lifecycle(),
                input_kind = input_kind(&input),
                "dropping input due to presentation lifecycle"
            );
            return Applied::default();
        }
        // Telemetry: stamp this event's arrival on the RESOLVED target's own
        // clock (never a wall-clock read) before dispatch, so a consumer can
        // later attribute a produced frame's latency back to this specific
        // input — see `FrameClock::stamp_input_epoch`'s own doc. Stamped
        // INSIDE each arm below, only for a kind this method actually
        // routes (every arm that stamps also calls `request_redraw_for`,
        // its own contribution to demand) — never for `DragDrop`, whose own
        // arm traces the event as dropped without routing it anywhere or
        // requesting a frame. Stamping an event this method is about to
        // drop would leave a phantom epoch sitting in the ring until
        // eviction, attributed to whichever unrelated frame happens to
        // produce next.
        match input {
            PlatformInput::Ime(ime_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                let dispatch = Self::dispatch_after_pending_motion(presentation, || {
                    presentation.text_input().dispatch(&ime_event);
                });
                self.finish_addressed_input_dispatch(presentation, dispatch);
                Applied::default()
            }
            PlatformInput::Pointer(pointer_event) => {
                let should_hold_pointer = presentation.frame_commit_state()
                    != FrameCommitState::Committed
                    || !presentation.held_pointer_input().borrow().is_empty();
                if should_hold_pointer {
                    let pointer_id = flui_interaction::PointerEventExt::pointer_id(&pointer_event);
                    let has_active_contact_sequence =
                        pointer_id.is_some_and(|id| presentation.gestures().has_hit_test(id));
                    presentation.held_pointer_input().borrow_mut().append_on(
                        surface,
                        pointer_event,
                        has_active_contact_sequence,
                    );
                    return Applied::default();
                }
                let mut sequence = None;
                let dispatch =
                    Self::dispatch_pointer_event_entered(presentation, surface, &pointer_event)
                        .map(|dispatched| sequence = dispatched);
                self.finish_addressed_input_dispatch(presentation, dispatch);
                Applied {
                    handled: false,
                    sequence,
                }
            }
            PlatformInput::Keyboard(keyboard_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                let mut handled = false;
                let dispatch = Self::dispatch_after_pending_motion(presentation, || {
                    handled = presentation
                        .focus_manager()
                        .dispatch_key_event(&keyboard_event)
                        .is_handled();
                });
                self.finish_addressed_input_dispatch(presentation, dispatch);
                Applied {
                    handled,
                    sequence: None,
                }
            }
            PlatformInput::DragDrop(drag_drop_event) => {
                // Not stamped (see this method's own doc): this event is
                // never routed and never requests a frame, so there is no
                // future produce to attribute it to.
                tracing::debug!(
                    drag_drop_kind = drag_drop_kind(&drag_drop_event),
                    "drag-and-drop input received; ui_runtime routing not implemented yet, dropping"
                );
                Applied::default()
            }
            PlatformInput::Capabilities(capabilities) => {
                // Not stamped and no frame requested: a capability snapshot or
                // change is session state, not an interaction. The latest one
                // per window is kept for the lifecycle capability that exposes
                // it to widgets (ADR-0213).
                tracing::debug!(?capabilities, "input capabilities received");
                presentation.input_capabilities.set(Some(capabilities));
                Applied::default()
            }
        }
    }

    /// The already resolved presentation owns both the measured prefix and the
    /// accepted observing input, even if motion changes the active focus owner.
    fn dispatch_after_pending_motion(
        presentation: &PresentationState,
        dispatch: impl FnOnce(),
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        let mut first = catch_unwind(AssertUnwindSafe(|| {
            presentation.gestures().flush_pending_input();
        }))
        .err();
        let resolved = catch_unwind(AssertUnwindSafe(|| {
            presentation.gestures().drain_deferred_arena_resolutions();
        }))
        .err();
        preserve_first_input_panic(&mut first, resolved, "input motion resolution");
        let dispatched = catch_unwind(AssertUnwindSafe(dispatch)).err();
        preserve_first_input_panic(&mut first, dispatched, "input after measured motion");
        first.map_or(Ok(()), Err)
    }

    /// Dispatch `pointer_event` on `surface` of `presentation`'s window;
    /// the input sequence it belonged to, or the first panic.
    pub(super) fn dispatch_pointer_event_entered(
        presentation: &PresentationState,
        surface: InputSurface,
        pointer_event: &PointerEvent,
    ) -> Result<Option<InputSequenceId>, Box<dyn std::any::Any + Send>> {
        let clock = presentation.clock();
        clock.stamp_input_epoch(clock.now());
        // The pointer's kind reaches hit testing, so an expanded hit area
        // sizes its minimum target for this kind of contact (ADR-0212 §4).
        let kind = flui_interaction::PointerEventExt::pointer_kind(pointer_event)
            .unwrap_or(flui_interaction::events::PointerKind::Unknown);
        let mut sequence = None;
        let routing_panic = catch_unwind(AssertUnwindSafe(|| {
            let outcome =
                presentation
                    .gestures()
                    .dispatch_pointer_on(surface, pointer_event, |position| {
                        let mut result = flui_interaction::pointer::HitTestResult::new();
                        let offset =
                            flui_foundation::geometry::Offset::new(position.dx, position.dy);
                        presentation
                            .renderer()
                            .hit_test_in_view(&mut result, offset, kind, 0);
                        if !result.is_empty() {
                            tracing::debug!(hits = result.len(), "Hit test found targets");
                        }
                        result
                    });
            sequence = outcome.sequence();
            if let Some(sequence) = sequence {
                tracing::trace!(sequence = sequence.get(), "pointer input dispatched");
            }
        }))
        .err();
        let deferred_panic = catch_unwind(AssertUnwindSafe(|| {
            presentation.gestures().drain_deferred_arena_resolutions();
        }))
        .err();

        let mut first_panic = None;
        preserve_first_input_panic(&mut first_panic, routing_panic, "pointer routing");
        preserve_first_input_panic(
            &mut first_panic,
            deferred_panic,
            "deferred arena resolution",
        );
        match first_panic {
            Some(payload) => Err(payload),
            None => Ok(sequence),
        }
    }

    /// The pointer entered (`true`) or left (`false`) `presentation_id`'s
    /// native window. Leave sweeps that presentation's hover state —
    /// `MouseRegion::on_exit` fires for every hovered region and the cursor
    /// resets — then pumps a frame so the un-hovered visuals actually paint.
    /// Enter is a no-op: the next pointer move re-primes hover from a fresh
    /// hit test on its own. A stale or unknown `presentation_id` is a traced
    /// no-op, the same posture as every other addressed signal.
    pub fn handle_window_hover_addressed(&self, presentation_id: PresentationId, inside: bool) {
        if inside {
            return;
        }
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::trace!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a window-hover signal for a presentation this ui_runtime no longer hosts"
            );
            return;
        };
        presentation.held_pointer_input().borrow_mut().drop_hovers();
        presentation.gestures().handle_pointer_left_window();
        self.request_redraw_for(presentation);
    }

    /// Unwrap [`HostInput::Accessibility`](crate::HostInput::Accessibility):
    /// decode `request` and invoke it through `presentation_id`'s semantics
    /// owner. Returns whether a handler ran.
    ///
    /// An assistive action is an input sequence of its own: its identity comes
    /// from the presentation's gesture binding, never a second mint (ADR-0207
    /// §4). A request FLUI cannot route (an unknown node, an action with no
    /// counterpart, a stale snapshot, a closed presentation) is a traced
    /// drop, since screen readers may act on a snapshot that is gone.
    pub fn dispatch_accessibility(
        &self,
        presentation_id: PresentationId,
        request: &accesskit::ActionRequest,
    ) -> bool {
        if !self.begin_recorded_input() {
            return self.apply_accessibility(presentation_id, request).handled;
        }
        let applied = catch_unwind(AssertUnwindSafe(|| {
            self.apply_accessibility(presentation_id, request)
        }));
        self.record_dispatched(
            presentation_id,
            InputSurface::Window,
            crate::HostInput::Accessibility(request.clone()),
            crate::owner::InputOutcome::Handled,
            applied.as_ref().ok().and_then(|applied| applied.sequence),
        );
        applied.map_or_else(|payload| resume_unwind(payload), |applied| applied.handled)
    }

    fn apply_accessibility(
        &self,
        presentation_id: PresentationId,
        request: &accesskit::ActionRequest,
    ) -> Applied {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::trace!(
                stamped_presentation_id = presentation_id.as_u64(),
                "dropping an accessibility action for a presentation this ui_runtime no longer hosts"
            );
            return Applied::default();
        };
        let decoded = match flui_semantics::semantics_action_request_for(request) {
            Ok(decoded) => decoded,
            Err(error @ flui_semantics::ActionDecodeError::UnknownTarget(_)) => {
                tracing::warn!(%error, "dropping accessibility action out of contract");
                return Applied::default();
            }
            Err(error) => {
                tracing::trace!(%error, "dropping undecodable accessibility action");
                return Applied::default();
            }
        };
        let sequence = match presentation
            .gestures()
            .begin_contactless(flui_interaction::ContactlessSource::Assistive)
        {
            Ok(sequence) => {
                tracing::trace!(
                    sequence = sequence.id.get(),
                    "assistive input sequence started"
                );
                Some(sequence.id)
            }
            Err(error) => {
                tracing::debug!(%error, "assistive action runs without a sequence id");
                None
            }
        };
        let handled = match presentation.dispatch_semantics_action(decoded) {
            Ok(()) => true,
            Err(error) => {
                tracing::trace!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                    ?error,
                    "dropping semantics action against a stale snapshot"
                );
                false
            }
        };
        Applied { handled, sequence }
    }

    /// Note the vsync of the frame about to be delivered to `presentation_id`
    /// (`HostInput::Frame`): that frame samples resampled motion there, on
    /// the event timeline, instead of at the frame clock's own reading.
    ///
    /// This is where a [`HostInput::Frame`](crate::HostInput::Frame) is
    /// recorded; the verdicts the frame makes follow it when the pump ends.
    pub fn note_frame_vsync(&self, presentation_id: PresentationId, vsync: EventTime) {
        let recording = self.begin_recorded_input();
        if let Some(presentation) = self.presentations.get(presentation_id) {
            presentation.frame_vsync.set(Some(vsync));
        }
        if recording {
            self.record_applied(
                presentation_id,
                crate::HostInput::Frame { vsync },
                crate::owner::InputOutcome::Handled,
            );
        }
    }

    /// The latest input capabilities `presentation_id`'s window reported, if
    /// any (`PlatformInput::Capabilities`, ADR-0213).
    #[must_use]
    pub fn input_capabilities(
        &self,
        presentation_id: PresentationId,
    ) -> Option<flui_platform_api::input_capabilities::InputCapabilities> {
        self.presentations
            .get(presentation_id)
            .and_then(|presentation| presentation.input_capabilities.get())
    }

    /// Record that `presentation_id`'s native window just gained OS focus —
    /// [`FocusCoordinator::note_focus_gained`]'s sole write side. A stale or
    /// unknown `presentation_id` (already closed, or a forged/mixed address)
    /// is a traced no-op: focus arbitration never points at a presentation
    /// this UI runtime does not currently host.
    pub(crate) fn notify_presentation_focus_gained(&self, presentation_id: PresentationId) {
        if self.presentations.get(presentation_id).is_none() {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a focus-gained notification for a presentation this ui_runtime no longer hosts"
            );
            return;
        }
        self.focus_coordinator.note_focus_gained(presentation_id);
    }
}
