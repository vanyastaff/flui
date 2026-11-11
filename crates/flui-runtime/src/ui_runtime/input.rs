//! Input dispatch and focus coordination.

use super::UiRuntime;
use super::presentation_lifecycle::HostLifecycle;
use crate::epoch::FrameCommitState;
use crate::presentation::PresentationState;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{InputSequenceId, PresentationId};
use flui_interaction::{InputSurface, PointerEvent};
use flui_platform::keyboard::KeyState;
use flui_platform::pointer::{ButtonChange, PointerButton};
use flui_platform::{DragDropEvent, EventTime, FrameRequest, PlatformInput};
use flui_rendering::binding::{RendererBinding as _, ViewHitTest};
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
    /// `PlatformToUi::WindowFocus(true)` handling (`flui-app`'s `runner/owner_dispatch.rs`) calls this
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
/// key a shortcut or focused node handled), the input sequence its
/// dispatch placed it in, and whether it was held for its presentation's
/// committed tree instead, its row then recorded where it is dispatched.
#[derive(Debug, Clone, Copy, Default)]
struct Applied {
    handled: bool,
    sequence: Option<InputSequenceId>,
    held: bool,
}

/// Where one platform input is dispatched, and so where its row says it
/// went: the presentation whose tree receives it and the surface of that
/// presentation's window it is applied to.
#[derive(Debug, Clone, Copy)]
struct InputTarget {
    presentation: PresentationId,
    surface: InputSurface,
}

/// Who sent an input: the platform, or the development agent injecting it (ADR-0288 §9). The
/// two take one path; an agent's journal marks the agent's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputOrigin {
    /// The platform: the user's input, or a host replaying it.
    Platform,
    /// The development agent's `input.inject` or `input.release`.
    Agent,
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
            PlatformInput::Pointer(_)
            | PlatformInput::PointerFrame(_)
            | PlatformInput::DragDrop(_) => true,
            // Keyboard/IME keep flowing; capabilities describe the session,
            // not an interaction, so a suspended window keeps them current.
            PlatformInput::Keyboard(_) | PlatformInput::Ime(_) | PlatformInput::Capabilities(_) => {
                false
            }
        },
        PresentationLifecycle::SurfaceAttached => false,
    }
}

/// Marks `presentation`'s press as committing its composition for as long as
/// it lives, restoring the enclosing mark on every exit, unwinding included.
struct PressCommit<'a> {
    cell: &'a Cell<Option<PresentationId>>,
    previous: Option<PresentationId>,
}

impl<'a> PressCommit<'a> {
    fn begin(cell: &'a Cell<Option<PresentationId>>, presentation: PresentationId) -> Self {
        Self {
            cell,
            previous: cell.replace(Some(presentation)),
        }
    }
}

impl Drop for PressCommit<'_> {
    fn drop(&mut self) {
        self.cell.set(self.previous);
    }
}

/// Whether `input` begins a pointer contact: a press, alone or in a frame.
/// A press resolves the presentation's composition before it is dispatched
/// (ADR-0142 item 4).
fn begins_contact(input: &PlatformInput) -> bool {
    let is_press = |event: &PointerEvent| matches!(event, PointerEvent::Down(_));
    match input {
        PlatformInput::Pointer(event) => is_press(event),
        PlatformInput::PointerFrame(frame) => frame.events().iter().any(is_press),
        PlatformInput::Keyboard(_)
        | PlatformInput::Ime(_)
        | PlatformInput::DragDrop(_)
        | PlatformInput::Capabilities(_) => false,
    }
}

/// A safe, content-free discriminator for a [`PlatformInput`] — never the
/// payload itself. Logging text-input/IME payloads and drag-and-drop payloads
/// is forbidden; this is the only thing about an input event that may reach a
/// trace/log line. Moved here from the retired `AppBinding`, unchanged.
pub(super) fn input_kind(input: &PlatformInput) -> &'static str {
    match input {
        PlatformInput::Pointer(_) => "pointer",
        PlatformInput::PointerFrame(_) => "pointer_frame",
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

/// Whether `event` is the press of Escape that cancels a drag: its first
/// press, while no input method composes.
fn is_escape_press(event: &flui_platform::keyboard::KeyEvent) -> bool {
    use flui_platform::keyboard::{ImeComposition, Key, KeyRepeat, NamedKey};
    event.state() == KeyState::Down
        && event.repeat() == KeyRepeat::First
        && event.composition == ImeComposition::Inactive
        && event.key == Key::Named(NamedKey::Escape)
}

/// Whether `event` pressed a pointer button: a contact beginning, or another
/// button going down during one.
fn pointer_pressed(event: &PointerEvent) -> bool {
    matches!(
        event,
        PointerEvent::Down(_) | PointerEvent::ButtonChange(ButtonChange::Pressed(_))
    )
}

/// Whether `input` is a discrete act of the user: a key going down (not its
/// repeat) or a pointer button pressed. Only these try a dropped frame before
/// its backoff ends.
fn is_discrete_press(input: &PlatformInput) -> bool {
    use flui_platform::keyboard::KeyRepeat;
    match input {
        PlatformInput::Keyboard(event) => {
            event.state() == KeyState::Down && event.repeat() == KeyRepeat::First
        }
        PlatformInput::Pointer(event) => pointer_pressed(event),
        PlatformInput::PointerFrame(frame) => frame.events().iter().any(pointer_pressed),
        PlatformInput::Ime(_) | PlatformInput::DragDrop(_) | PlatformInput::Capabilities(_) => {
            false
        }
    }
}

/// The back button (X1) going down or up in `event`, and when.
fn back_button_change(event: &PointerEvent) -> Option<(KeyState, EventTime)> {
    let (state, button, time) = match event {
        PointerEvent::Down(press) | PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
            (KeyState::Down, press.button(), press.sample.time)
        }
        PointerEvent::Up(release) | PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
            (KeyState::Up, release.button(), release.sample.time)
        }
        _ => return None,
    };
    (button == PointerButton::BACK).then_some((state, time))
}

impl UiRuntime {
    /// Finish an addressed input dispatch without letting a secondary platform
    /// wake panic replace the dispatch failure that caused the redraw request.
    ///
    /// The redraw flags are set before the wake runs, so even a panicking wake
    /// leaves durable demand behind. When both phases panic, the dispatch
    /// payload remains the first failure and the wake payload is discarded by
    /// [`FirstPanic`].
    pub(super) fn finish_addressed_input_dispatch(
        &self,
        presentation: &PresentationState,
        mut dispatch: FirstPanic,
    ) {
        // The redraw wake runs contained so the dispatch failure is kept.
        dispatch.contain(|| {
            self.request_redraw_for(presentation);
        });
        if let Some(payload) = dispatch.into_payload() {
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
    /// (`flui-app`'s `runner/owner_dispatch.rs`) stamped this event with at enqueue time — the window
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
    /// recorded too and then resumed. Its row names the window and surface it
    /// was dispatched to, not the ones it was stamped for: a key on the active
    /// presentation's window. Pointer input held for the tree's commit is
    /// recorded where it is dispatched or discarded instead.
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
    /// surface reported them, and are recorded on it.
    pub fn handle_input_on(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: PlatformInput,
    ) -> bool {
        self.handle_input_from(presentation_id, surface, input, InputOrigin::Platform)
    }

    /// [`handle_input_on`](Self::handle_input_on) for input from `origin`.
    pub(super) fn handle_input_from(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: PlatformInput,
        origin: InputOrigin,
    ) -> bool {
        let recording = self.begin_recorded_input();
        // Resolved once, before dispatch: the row names where the input went
        // even when a handler it reaches moves keyboard focus.
        let target = self.input_target(presentation_id, surface, &input);
        if !recording {
            return self
                .apply_platform_input(presentation_id, target, input, false, origin)
                .handled;
        }
        let keyboard = matches!(input, PlatformInput::Keyboard(_));
        let recorded = input.clone();
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: platform input applies contained so the recording is finished"
        )]
        let applied = catch_unwind(AssertUnwindSafe(|| {
            self.apply_platform_input(presentation_id, target, input, true, origin)
        }));
        let (outcome, sequence) = match &applied {
            // Its row waits for the dispatch; the rows of input the hold
            // discarded go out now.
            Ok(applied) if applied.held => {
                self.deliver_input_records();
                return applied.handled;
            }
            Ok(applied) if keyboard && !applied.handled => {
                (crate::owner::InputOutcome::Unhandled, applied.sequence)
            }
            Ok(applied) => (crate::owner::InputOutcome::Handled, applied.sequence),
            Err(_) => (crate::owner::InputOutcome::Handled, None),
        };
        self.record_dispatched(
            target.presentation,
            target.surface,
            crate::HostInput::Platform(recorded),
            outcome,
            sequence,
        );
        applied.map_or_else(|payload| resume_unwind(payload), |applied| applied.handled)
    }

    /// Where `input`, stamped for `presentation_id` and reported by `surface`
    /// of its window, is dispatched. Keyboard goes to the active
    /// presentation whatever presentation it was stamped for
    /// ([`FocusCoordinator`]); every other kind to the stamped one. Pointer
    /// input stays on the surface that reported it; keyboard, IME,
    /// drag-and-drop and capability input belong to the window.
    fn input_target(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: &PlatformInput,
    ) -> InputTarget {
        match input {
            PlatformInput::Pointer(_) | PlatformInput::PointerFrame(_) => InputTarget {
                presentation: presentation_id,
                surface,
            },
            PlatformInput::Keyboard(_) => InputTarget {
                presentation: self.focus_coordinator.active(),
                surface: InputSurface::Window,
            },
            PlatformInput::Ime(_) | PlatformInput::DragDrop(_) | PlatformInput::Capabilities(_) => {
                InputTarget {
                    presentation: presentation_id,
                    surface: InputSurface::Window,
                }
            }
        }
    }

    /// Apply `input`, stamped for `presentation_id`, at `target`; while
    /// `recording`, pointer input held for the tree's commit takes its input
    /// row along. Input from `origin` is journaled for an agent as it is
    /// routed.
    fn apply_platform_input(
        &self,
        presentation_id: PresentationId,
        target: InputTarget,
        input: PlatformInput,
        recording: bool,
        origin: InputOrigin,
    ) -> Applied {
        let target_id = target.presentation;
        let Some(presentation) = self.presentations.get(target_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = target_id.as_u64(),
                stamped_presentation_id = presentation_id.as_u64(),
                input_kind = input_kind(&input),
                "dropping input addressed to a presentation this ui_runtime no longer hosts"
            );
            return Applied::default();
        };
        if self.input_dropped_for(presentation, &input) {
            return Applied::default();
        }
        let pointer = matches!(
            input,
            PlatformInput::Pointer(_) | PlatformInput::PointerFrame(_)
        );
        if pointer && self.press_commit.get() == Some(target_id) {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = target_id.as_u64(),
                input_kind = input_kind(&input),
                "dropping pointer input sent from the composition commit of a press"
            );
            return Applied::default();
        }
        if is_discrete_press(&input) && presentation.retry_failure_early() {
            self.request_redraw_for(presentation);
        }
        if !begins_contact(&input) {
            return self.journal_and_route(target, presentation, input, recording, origin);
        }
        // A press anywhere in the presentation, the composing field included,
        // commits the composition before the press is held or hit-tested, so
        // its handlers (a "Save" button) see the text the user sees (ADR-0142
        // item 4). The commit runs owner code, which may close the
        // presentation: the press then goes nowhere. Pointer input that code
        // sends this presentation is not a second contact ahead of this one,
        // and is dropped. The commit's failure is the first, raised once the
        // press was dispatched.
        let mut committed = FirstPanic::new();
        {
            let _commit = PressCommit::begin(&self.press_commit, target_id);
            // Completing a composition runs contained before the press dispatches.
            committed.contain(|| {
                presentation.text_input().complete_composition();
            });
        }
        // Input routing runs contained so the press commit is released.
        let mut routing_failure = FirstPanic::new();
        let routed = routing_failure.contain(|| {
            if self.input_dropped_for(presentation, &input) {
                Applied::default()
            } else {
                self.journal_and_route(target, presentation, input, recording, origin)
            }
        });
        match (committed.is_held(), routed) {
            (false, Some(applied)) => applied,
            (false, None) => resume_unwind(
                routing_failure
                    .into_payload()
                    .expect("BUG: a failed routing holds its failure"),
            ),
            (true, _) => {
                committed.absorb(routing_failure);
                resume_unwind(
                    committed
                        .into_payload()
                        .expect("BUG: the commit's failure stays first"),
                )
            }
        }
    }

    /// Whether `input` for `presentation` is dropped: the presentation is
    /// closing, the host detached, or its lifecycle refuses this kind.
    fn input_dropped_for(&self, presentation: &PresentationState, input: &PlatformInput) -> bool {
        let dropped = presentation.closing_requested.get()
            || self.host_lifecycle.get() == HostLifecycle::Observed(AppLifecycleState::Detached)
            || input_dropped_by_lifecycle(presentation.lifecycle(), input);
        if dropped {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation.id().as_u64(),
                lifecycle = ?presentation.lifecycle(),
                input_kind = input_kind(input),
                "dropping input due to presentation lifecycle"
            );
        }
        dropped
    }

    /// Journal `input` from `origin` for an agent, then route it at `target`.
    fn journal_and_route(
        &self,
        target: InputTarget,
        presentation: &PresentationState,
        input: PlatformInput,
        recording: bool,
        origin: InputOrigin,
    ) -> Applied {
        self.journal_input(presentation, &input, origin == InputOrigin::Agent);
        self.route_platform_input(target, presentation, input, recording)
    }

    /// Route `input`, which `presentation` accepts, at `target`.
    fn route_platform_input(
        &self,
        target: InputTarget,
        presentation: &PresentationState,
        input: PlatformInput,
        recording: bool,
    ) -> Applied {
        let surface = target.surface;
        // Telemetry: stamp this event's arrival on the RESOLVED target's own
        // clock (never a wall-clock read) before dispatch, so a consumer can
        // later attribute a produced frame's latency back to this specific
        // input — see `FrameClock::stamp_input_epoch`'s own doc. Stamped
        // INSIDE each arm below, only for a kind this method actually
        // routes (every arm that stamps also calls `request_redraw_for`,
        // its own contribution to demand) — never for a capability snapshot,
        // which is session state and requests no frame. Stamping an event
        // this method does not route would leave a phantom epoch sitting in
        // the ring until eviction, attributed to whichever unrelated frame
        // happens to produce next.
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
                    let row = recording
                        .then(|| self.held_input_row(presentation, surface, &pointer_event));
                    presentation
                        .held_pointer_input()
                        .borrow_mut()
                        .append_recorded(surface, pointer_event, has_active_contact_sequence, row);
                    return Applied {
                        held: true,
                        ..Applied::default()
                    };
                }
                let mut sequence = None;
                let mut dispatch = FirstPanic::new();
                match Self::dispatch_pointer_event_entered(presentation, surface, &pointer_event) {
                    Ok(dispatched) => sequence = dispatched,
                    Err(payload) => dispatch.record(payload),
                }
                self.finish_addressed_input_dispatch(presentation, dispatch);
                Applied {
                    sequence,
                    ..Applied::default()
                }
            }
            PlatformInput::PointerFrame(frame) => {
                let should_hold_pointer = presentation.frame_commit_state()
                    != FrameCommitState::Committed
                    || !presentation.held_pointer_input().borrow().is_empty();
                if should_hold_pointer {
                    // Without a committed tree the events wait one by one,
                    // each with its own row: what is held is replayed in
                    // order once the tree commits.
                    let mut applied = Applied::default();
                    for event in frame.into_events() {
                        let held = self.route_platform_input(
                            target,
                            presentation,
                            PlatformInput::Pointer(event),
                            recording,
                        );
                        applied.held |= held.held;
                        applied.handled |= held.handled;
                    }
                    return applied;
                }
                let mut sequence = None;
                let mut first_panic = FirstPanic::new();
                // The pointer frame runs contained so deferred work still runs; the
                // events' own failures are recorded into `first_panic` as they come.
                let mut frame_failure = FirstPanic::new();
                frame_failure.contain(|| {
                    presentation.gestures().pointer_frame(|| {
                        for event in frame.events() {
                            match Self::route_pointer_event_entered(
                                presentation,
                                surface,
                                event,
                                false,
                            ) {
                                Ok(dispatched) => sequence = sequence.or(dispatched),
                                Err(payload) => first_panic.record(payload),
                            }
                        }
                    });
                });
                first_panic.absorb(frame_failure);
                // Deferred arena resolutions run contained after the pointer frame.
                first_panic.contain(|| {
                    presentation.gestures().drain_deferred_arena_resolutions();
                });
                self.finish_addressed_input_dispatch(presentation, first_panic);
                Applied {
                    sequence,
                    ..Applied::default()
                }
            }
            PlatformInput::Keyboard(keyboard_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                let mut handled = false;
                let dispatch = Self::dispatch_after_pending_motion(presentation, || {
                    // Escape cancels the drags of this window before focus
                    // sees it, as an OS drag loop does, unless an input
                    // method is composing: then it is the method's (ADR-0211).
                    handled = is_escape_press(&keyboard_event)
                        && self.drag.cancel_on_escape(presentation.id());
                    if !handled {
                        handled = presentation
                            .focus_manager()
                            .dispatch_key_event(&keyboard_event)
                            .is_handled();
                    }
                });
                self.finish_addressed_input_dispatch(presentation, dispatch);
                Applied {
                    handled,
                    ..Applied::default()
                }
            }
            PlatformInput::DragDrop(drag_drop_event) => {
                // An OS drag over this window: the receivers under it answer, and the
                // answers reach the OS as drop feedback (ADR-0211 §5). Like the
                // other routed kinds it requests a frame, since a receiver may restyle.
                tracing::trace!(
                    drag_drop_kind = drag_drop_kind(&drag_drop_event),
                    "drag-and-drop input delivered"
                );
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                let mut hit_panic = FirstPanic::new();
                let dispatch = Self::dispatch_after_pending_motion(presentation, || {
                    self.drag
                        .deliver(presentation.id(), drag_drop_event, &mut |position, kind| {
                            let mut result = flui_interaction::pointer::HitTestResult::new();
                            // A busy tree answers nothing rather than "over
                            // nothing", which would make every receiver leave.
                            match presentation.renderer().hit_test_in_view(
                                &mut result,
                                position,
                                kind,
                                0,
                            ) {
                                ViewHitTest::Answered { panic } => {
                                    if let Some(payload) = panic {
                                        hit_panic.record(payload);
                                    }
                                    Some(result.path().to_vec())
                                }
                                ViewHitTest::TreeBusy => None,
                            }
                        });
                });
                // The hit test ran before the receivers it found were told.
                hit_panic.absorb(dispatch);
                self.finish_addressed_input_dispatch(presentation, hit_panic);
                Applied::default()
            }
            PlatformInput::Capabilities(capabilities) => {
                // Not stamped and no frame requested: a capability snapshot or
                // change is session state, not an interaction (ADR-0213).
                tracing::debug!(?capabilities, "input capabilities received");
                Applied::default()
            }
        }
    }

    /// The already resolved presentation owns both the measured prefix and the
    /// accepted observing input, even if motion changes the active focus owner.
    fn dispatch_after_pending_motion(
        presentation: &PresentationState,
        dispatch: impl FnOnce(),
    ) -> FirstPanic {
        // Pending input flushes contained so dispatch still runs.
        let mut first = FirstPanic::new();
        first.contain(|| {
            presentation.gestures().flush_pending_input();
        });
        // Deferred arena resolutions run contained so dispatch still runs.
        first.contain(|| {
            presentation.gestures().drain_deferred_arena_resolutions();
        });
        // The dispatch runs contained so the first input panic is kept.
        first.contain(dispatch);
        first
    }

    /// Dispatch `pointer_event` on `surface` of `presentation`'s window;
    /// the input sequence it belonged to, or the first panic.
    pub(super) fn dispatch_pointer_event_entered(
        presentation: &PresentationState,
        surface: InputSurface,
        pointer_event: &PointerEvent,
    ) -> Result<Option<InputSequenceId>, Box<dyn std::any::Any + Send>> {
        Self::route_pointer_event_entered(presentation, surface, pointer_event, true)
    }

    /// [`dispatch_pointer_event_entered`](Self::dispatch_pointer_event_entered),
    /// leaving the lone-member wins the event deferred to the caller when
    /// `drain_deferred` is false: a pointer frame drains them once, after its
    /// last event.
    fn route_pointer_event_entered(
        presentation: &PresentationState,
        surface: InputSurface,
        pointer_event: &PointerEvent,
        drain_deferred: bool,
    ) -> Result<Option<InputSequenceId>, Box<dyn std::any::Any + Send>> {
        let clock = presentation.clock();
        clock.stamp_input_epoch(clock.now());
        // The pointer's kind reaches hit testing, so an expanded hit area
        // sizes its minimum target for this kind of contact (ADR-0212 §4).
        let kind = flui_interaction::PointerEventExt::pointer_kind(pointer_event)
            .unwrap_or(flui_interaction::events::PointerKind::Unknown);
        // A press decides the input modality before anything it reaches can
        // move focus: focus a click moves is not shown.
        let mut modality_panic = FirstPanic::new();
        if pointer_pressed(pointer_event) {
            // A focus manager's note runs contained so the event still routes.
            modality_panic.contain(|| {
                presentation.focus_manager().note_pointer_input();
            });
        }
        let mut sequence = None;
        // A render object that panicked in the hit test is a miss there; its
        // panic is reported once the event reached the rest of the path.
        let mut hit_panic = None;
        // Event routing runs contained so the hit panic is reported after.
        let mut routing_panic = FirstPanic::new();
        routing_panic.contain(|| {
            let outcome =
                presentation
                    .gestures()
                    .dispatch_pointer_on(surface, pointer_event, |position| {
                        let mut result = flui_interaction::pointer::HitTestResult::new();
                        let offset =
                            flui_foundation::geometry::Offset::new(position.dx, position.dy);
                        match presentation
                            .renderer()
                            .hit_test_in_view(&mut result, offset, kind, 0)
                        {
                            ViewHitTest::Answered { panic } => hit_panic = panic,
                            ViewHitTest::TreeBusy => {
                                tracing::debug!("pointer hit test found the render tree busy");
                            }
                        }
                        if !result.is_empty() {
                            tracing::debug!(hits = result.len(), "Hit test found targets");
                        }
                        result
                    });
            sequence = outcome.sequence();
            if let Some(sequence) = sequence {
                tracing::trace!(sequence = sequence.get(), "pointer input dispatched");
            }
        });
        let mut deferred_panic = FirstPanic::new();
        if drain_deferred {
            // Deferred arena resolutions run contained after the frame.
            deferred_panic.contain(|| {
                presentation.gestures().drain_deferred_arena_resolutions();
            });
        }

        // The back button (X1) is also the Back key, after its pointer event
        // reached the widgets under it.
        let mut back_panic = FirstPanic::new();
        if let Some((state, time)) = back_button_change(pointer_event) {
            // The Back key's handlers run contained after the pointer event.
            back_panic.contain(|| {
                let _ = presentation
                    .focus_manager()
                    .dispatch_pointer_back(state, time);
            });
        }

        let mut first_panic = modality_panic;
        if let Some(payload) = hit_panic {
            first_panic.record(payload);
        }
        first_panic.absorb(routing_panic);
        first_panic.absorb(deferred_panic);
        first_panic.absorb(back_panic);
        match first_panic.into_payload() {
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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: accessibility actions run contained so the recording is finished"
        )]
        let applied = catch_unwind(AssertUnwindSafe(|| {
            self.apply_accessibility(presentation_id, request)
        }));
        // A request no handler ran for is recorded as unhandled, as a key is,
        // so a replay tells a dropped action from one that ran.
        let (outcome, sequence) = match &applied {
            Ok(applied) if !applied.handled => {
                (crate::owner::InputOutcome::Unhandled, applied.sequence)
            }
            Ok(applied) => (crate::owner::InputOutcome::Handled, applied.sequence),
            Err(_) => (crate::owner::InputOutcome::Handled, None),
        };
        self.record_dispatched(
            presentation_id,
            InputSurface::Window,
            crate::HostInput::Accessibility(request.clone()),
            outcome,
            sequence,
        );
        applied.map_or_else(|payload| resume_unwind(payload), |applied| applied.handled)
    }

    /// Move the assistive technology's cursor in `presentation_id`, as its
    /// adapter reported, and tell the subtrees it left and entered. Recorded
    /// as [`HostInput::AssistiveFocus`](crate::HostInput::AssistiveFocus).
    /// `false` when the presentation is gone.
    pub fn dispatch_assistive_focus(
        &self,
        presentation_id: PresentationId,
        node: Option<accesskit::NodeId>,
    ) -> bool {
        let recording = self.begin_recorded_input();
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: accessibility focus runs contained so the recording is finished"
        )]
        let applied = catch_unwind(AssertUnwindSafe(|| {
            let Some(presentation) = self.presentations.get(presentation_id) else {
                tracing::trace!(
                    stamped_presentation_id = presentation_id.as_u64(),
                    "dropping an assistive focus move for a presentation this ui_runtime no longer hosts"
                );
                return false;
            };
            // A zero id names no node: the cursor is in no subtree.
            let node = node.and_then(|node| flui_semantics::AccessibilityNodeId::from_u64(node.0));
            // The handlers run once the pipeline is no longer borrowed.
            let change = presentation
                .pipeline()
                .with_mut(|pipeline| pipeline.move_assistive_focus(node));
            change.invoke();
            true
        }));
        if recording {
            let outcome = match &applied {
                Ok(false) => crate::owner::InputOutcome::Unhandled,
                _ => crate::owner::InputOutcome::Handled,
            };
            self.record_dispatched(
                presentation_id,
                InputSurface::Window,
                crate::HostInput::AssistiveFocus(node),
                outcome,
                None,
            );
        }
        applied.unwrap_or_else(|payload| resume_unwind(payload))
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
        let decoded = match presentation
            .pipeline()
            .with(|pipeline| pipeline.decode_semantics_action(request))
        {
            Ok(decoded) => decoded,
            Err(
                error @ (flui_semantics::ActionDecodeError::UnknownTarget(_)
                | flui_semantics::ActionDecodeError::UnknownTree(_)),
            ) => {
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
        // A scroll the action drives names its deltas by this sequence.
        let dispatched = match sequence {
            Some(id) => presentation
                .gestures()
                .dispatch_contactless(id, || presentation.dispatch_semantics_action(decoded)),
            None => presentation.dispatch_semantics_action(decoded),
        };
        let handled = match dispatched {
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
        Applied {
            handled,
            sequence,
            held: false,
        }
    }

    /// Note the request of the frame about to be delivered to `presentation_id`
    /// (`HostInput::Frame`): that frame samples resampled motion at its vsync, on
    /// the event timeline, instead of at the frame clock's own reading.
    ///
    /// This is where a [`HostInput::Frame`](crate::HostInput::Frame) is
    /// recorded; the verdicts the frame makes follow it when the pump ends.
    ///
    /// The command inbox is drained first. The frame applies what it holds
    /// (assistive requests among it) before it renders, so their rows precede
    /// the frame's row and a replay applies them before rendering it too. A
    /// drain failure is resumed after the frame's row is recorded.
    pub fn note_frame_request(&self, presentation_id: PresentationId, request: FrameRequest) {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: queued commands drain contained so the frame request is recorded"
        )]
        let drained = catch_unwind(AssertUnwindSafe(|| self.drain_commands())).err();
        let recording = self.begin_recorded_input();
        if let Some(presentation) = self.presentations.get(presentation_id) {
            presentation.frame_request.set(Some(request));
            // The platform's report of an earlier frame shown: the
            // presentation's frame clock keeps the newest, on its own clock.
            if let Some(presented) = request.presented
                && let Some(at) = self.instant_of(presented.at)
            {
                presentation
                    .clock()
                    .record_presentation(at, presented.refresh);
            }
        }
        if recording {
            self.record_applied(
                presentation_id,
                crate::HostInput::Frame {
                    vsync: request.vsync,
                    target_present: request.target_present,
                    presented: request.presented,
                },
                crate::owner::InputOutcome::Handled,
            );
        }
        if let Some(payload) = drained {
            resume_unwind(payload);
        }
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
