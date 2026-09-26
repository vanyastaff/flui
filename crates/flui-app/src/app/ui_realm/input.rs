//! Input dispatch and focus coordination.

use super::UiRealm;
use super::presentation_lifecycle::HostLifecycle;
use crate::app::presentation::PresentationState;
use flui_foundation::PresentationId;
use flui_interaction::PointerEvent;
use flui_platform_api::{DragDropEvent, PlatformInput};
use flui_rendering::binding::RendererBinding as _;
use flui_runtime::epoch::FrameCommitState;
use flui_scheduler::AppLifecycleState;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

/// Realm-level arbitration of which presentation currently owns OS keyboard
/// focus (issue #555's addressed-routing slice). One [`FocusManager`](flui_interaction::FocusManager) exists per presentation
/// (`PresentationState::focus_manager`), but only ONE presentation's tree
/// should ever receive a keyboard event at a time — the one whose native
/// window the platform most recently reported as focused.
/// [`UiRealm::handle_input_addressed`]'s `Keyboard` arm routes there,
/// deliberately never to whichever presentation an individual event happens
/// to be stamped for: a stray or racing keyboard event addressed to a
/// presentation that is not (or no longer) the OS-focused one must still
/// land on the active one, matching a real OS's single-keyboard-focus model
/// rather than trusting the per-event address.
///
/// Defaults to the realm's initial presentation. A realm may host N resident
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
    /// gains OS focus; [`UiRealm::close_presentation_entered`] calls this
    /// with the surviving primary when `id` is the presentation being
    /// closed and it was the active one (never leave this pointing at a
    /// presentation the forest is about to drop).
    ///
    /// A `WindowFocus(false)` (focus LOST) never calls this: losing focus
    /// names no new owner, so the coordinator keeps pointing at whichever
    /// presentation held it most recently — the same retention Flutter's
    /// own single-view focus model has (losing OS focus does not un-focus
    /// the last-focused element; a *different* window gaining focus does).
    pub(super) fn note_focus_gained(&self, id: PresentationId) {
        self.active.set(id);
    }
}

/// Moved here from the retired `AppBinding`, unchanged.
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
        tracing::error!(
            phase,
            "input phase panicked after an earlier phase; only the first panic is resumed"
        );
        // A panic payload may itself panic while being dropped. Leaking only
        // the secondary exceptional payload keeps the original failure stable.
        std::mem::forget(candidate);
    }
}

/// Whether [`UiRealm::handle_input_entered`] must drop `input` outright,
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
    lifecycle: crate::app::presentation::PresentationLifecycle,
    input: &PlatformInput,
) -> bool {
    use crate::app::presentation::PresentationLifecycle;
    match lifecycle {
        PresentationLifecycle::Created
        | PresentationLifecycle::Closing
        | PresentationLifecycle::Closed => true,
        PresentationLifecycle::Suspended => match input {
            PlatformInput::Pointer(_) | PlatformInput::DragDrop(_) => true,
            PlatformInput::Keyboard(_) | PlatformInput::Ime(_) => false,
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

impl UiRealm {
    // ========================================================================
    // Input dispatch (moved from the retired `AppBinding`)
    // ========================================================================

    /// [`Self::handle_input_addressed`], addressed to this realm's primary —
    /// the pre-addressed-routing shape, kept for every existing single-presentation
    /// test (production dispatch, `runner.rs`'s `PlatformToUi::run`, calls
    /// `handle_input_addressed` directly with the real stamped id).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production input dispatch calls handle_input_addressed directly with the \
                      real stamped presentation id (PlatformToUi::run); this primary-only \
                      convenience is exercised only by tests that never address a specific \
                      presentation"
        )
    )]
    pub(crate) fn handle_input_entered(&self, input: PlatformInput) {
        self.handle_input_addressed(self.presentations.primary().id(), input);
    }

    /// Handle a platform input event while this realm is already entered,
    /// addressed to exactly one presentation this realm hosts (issue #555
    /// the addressed-routing slice).
    ///
    /// `presentation_id` is the exact presentation `dispatch_platform_realm`
    /// (`runner.rs`) stamped this event with at enqueue time — the window
    /// that produced it, resolved through `WindowRegistry` at hop-1 — never
    /// assumed to be this realm's primary. A `presentation_id` this realm no
    /// longer hosts (closed between enqueue and drain) is a traced drop,
    /// never a panic or a silent fall-through to [`PresentationForest::
    /// primary`](crate::app::presentation_forest::PresentationForest::primary).
    ///
    /// Pointer/IME/drag-drop events go to the ADDRESSED presentation's own
    /// gesture/text-input state — never a sibling's, and never falling
    /// through to the primary when the addressed id is missing
    /// (`input_stamped_for_b_never_reaches_as_arena`,
    /// `ime_event_addressed_to_b_does_not_reach_as_session`,
    /// `input_addressed_to_a_closed_or_unknown_presentation_drops_traced_
    /// never_falls_through`). Keyboard is the one exception: it always
    /// goes to [`Self::focus_coordinator`]'s currently ACTIVE presentation
    /// instead of the stamped one — see [`FocusCoordinator`]'s own doc for
    /// why (`keyboard_routes_to_active_presentation_only`).
    ///
    /// [`input_dropped_by_lifecycle`] gates every kind next, against the
    /// RESOLVED target's own lifecycle (not necessarily `presentation_id`'s,
    /// for `Keyboard`): `Closing`/`Closed` refuses all input; `Suspended`
    /// drops pointer/drag-drop only (keyboard/IME keep flowing, so a flaky
    /// or absent occlusion signal never becomes a keystroke blackout).
    ///
    /// Pointer events are coalesced by the target presentation's own
    /// `GestureBinding` — high-frequency move events are stored and flushed
    /// once per frame via [`Self::render_frame_entered`].
    pub(crate) fn handle_input_addressed(
        &self,
        presentation_id: PresentationId,
        input: PlatformInput,
    ) {
        let target_id = match &input {
            PlatformInput::Keyboard(_) => self.focus_coordinator.active(),
            PlatformInput::Pointer(_) | PlatformInput::Ime(_) | PlatformInput::DragDrop(_) => {
                presentation_id
            }
        };
        let Some(presentation) = self.presentations.get(target_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = target_id.as_u64(),
                stamped_presentation_id = presentation_id.as_u64(),
                input_kind = input_kind(&input),
                "dropping input addressed to a presentation this realm no longer hosts"
            );
            return;
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
            return;
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
                presentation.text_input().dispatch(&ime_event);
                self.request_redraw_for(presentation);
            }
            PlatformInput::Pointer(pointer_event) => {
                let should_hold_pointer = presentation.frame_commit_state()
                    != FrameCommitState::Committed
                    || !presentation.held_pointer_input().borrow().is_empty();
                if should_hold_pointer {
                    let pointer_id = flui_interaction::events::extract_pointer_id(&pointer_event);
                    let has_active_contact_sequence =
                        presentation.gestures().has_hit_test(pointer_id);
                    presentation
                        .held_pointer_input()
                        .borrow_mut()
                        .append_with_active_contact(pointer_event, has_active_contact_sequence);
                    return;
                }
                let dispatch = Self::dispatch_pointer_event_entered(presentation, &pointer_event);
                self.request_redraw_for(presentation);
                if let Err(payload) = dispatch {
                    resume_unwind(payload);
                }
            }
            PlatformInput::Keyboard(keyboard_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                presentation
                    .focus_manager()
                    .dispatch_key_event(&keyboard_event);
                self.request_redraw_for(presentation);
            }
            PlatformInput::DragDrop(drag_drop_event) => {
                // Not stamped (see this method's own doc): this event is
                // never routed and never requests a frame, so there is no
                // future produce to attribute it to.
                tracing::debug!(
                    drag_drop_kind = drag_drop_kind(&drag_drop_event),
                    "drag-and-drop input received; realm routing not implemented yet, dropping"
                );
            }
        }
    }

    pub(super) fn dispatch_pointer_event_entered(
        presentation: &PresentationState,
        pointer_event: &PointerEvent,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        let clock = presentation.clock();
        clock.stamp_input_epoch(clock.now());
        let routing_panic = catch_unwind(AssertUnwindSafe(|| {
            presentation
                .gestures()
                .handle_pointer_event(pointer_event, |position| {
                    let mut result = flui_interaction::routing::HitTestResult::new();
                    let offset = flui_types::Offset::new(position.dx, position.dy);
                    presentation
                        .renderer()
                        .hit_test_in_view(&mut result, offset, 0);
                    if !result.is_empty() {
                        tracing::debug!(hits = result.len(), "Hit test found targets");
                    }
                    result
                });
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
            None => Ok(()),
        }
    }

    /// The pointer entered (`true`) or left (`false`) `presentation_id`'s
    /// native window. Leave sweeps that presentation's hover state —
    /// `MouseRegion::on_exit` fires for every hovered region and the cursor
    /// resets — then pumps a frame so the un-hovered visuals actually paint.
    /// Enter is a no-op: the next pointer move re-primes hover from a fresh
    /// hit test on its own. A stale or unknown `presentation_id` is a traced
    /// no-op, the same posture as every other addressed signal.
    pub(crate) fn handle_window_hover_addressed(
        &self,
        presentation_id: PresentationId,
        inside: bool,
    ) {
        if inside {
            return;
        }
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::trace!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a window-hover signal for a presentation this realm no longer hosts"
            );
            return;
        };
        presentation.held_pointer_input().borrow_mut().drop_hovers();
        presentation.gestures().handle_pointer_left_window();
        self.request_redraw_for(presentation);
    }

    /// Record that `presentation_id`'s native window just gained OS focus —
    /// [`FocusCoordinator::note_focus_gained`]'s sole write side. A stale or
    /// unknown `presentation_id` (already closed, or a forged/mixed address)
    /// is a traced no-op: focus arbitration never points at a presentation
    /// this realm does not currently host.
    pub(crate) fn notify_presentation_focus_gained(&self, presentation_id: PresentationId) {
        if self.presentations.get(presentation_id).is_none() {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a focus-gained notification for a presentation this realm no longer hosts"
            );
            return;
        }
        self.focus_coordinator.note_focus_gained(presentation_id);
    }
}
