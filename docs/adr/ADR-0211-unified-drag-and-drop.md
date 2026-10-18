# ADR-0211: Drag-and-drop runs in-window on pointer capture and escalates to the OS

- **Status:** Accepted (2026-10-13). Implemented: the session core, identity, the escalation
  decision, sessions that reach the other windows of their runtime on the source's capture, the
  host's deferred start and delivery to receivers in the runtime, spring-loading
  and autoscroll frames in the session core, and `Scrollable`'s edge autoscroll, run headless;
  the Win32 OLE target and source, run on a Windows host within one process; the `Draggable`
  and `DropTarget` widgets, the cursor, Escape and the keyboard and assistive cut, copy and
  paste, run headless; the Web HTML target and source, run in headless Chrome with constructed
  events; the Android target and source through `FluiView`, run on a device within one app.
  Pending ([tasks](../plans/specs/flui-interaction/completeness/tasks.md)): every other native
  drag source and target.
- **Amended by:** [ADR-0244](ADR-0244-android-java-view.md) (2026-10-17): the Android target is
  `FluiView.onDragEvent`, answered from the cached feedback; the source is
  `FluiView.startDragAndDrop` (API 24) with the whole clip at the start; a drag from another app
  permits a copy; `OnReceiveContentListener` receives only pasted and keyboard text.
- **Date:** 2026-10-13
- **Amends:** [ADR-0038](ADR-0038-data-transfer-architecture.md) §2 (several concurrent offers
  and sessions where the platform delivers them), §5 (DnD gains a source side and in-window
  sessions) and §8 (the Web gets an HTML DnD target and source)
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md),
  [ADR-0209](ADR-0209-nested-scroll-connection.md)

## Context

ADR-0038 built the target half: `DataTransferOffer`, `DataTransferId`, cached `DropFeedback` and
`PlatformInput::DragDrop`. Nothing starts a drag, so reorderable lists, kanban boards and slidable
rows are app-local gestures with no path out of the window, and §8 left the Web without DnD.
Desktop OS drags are modal or callback-driven sessions (`DoDragDrop`, AppKit dragging sessions)
that would take touch, pen and a second finger away from the arena if every drag ran there.
Platforms also differ: Web data must be in the store at `dragstart`, X11 reports the action only
at the drop, and winit 0.31 has no outgoing DnD on X11
([best practices](../research/2026-10-12-interaction-input-best-practices.md) §5.2,
[dependency selection](../research/2026-10-13-interaction-dependency-selection.md) §4).

## Decision

1. **Source side.** The `Draggable` widget starts a session with `DragItem`s and `AllowedActions`;
   the `DropTarget` widget (today's `DragTarget`, renamed by the naming pass with
   `#[doc(alias = "DragTarget")]`) receives enter, move, leave and drop and answers with
   `DropFeedback` as in ADR-0038 §1. A `DragItem` holds `InProcess(Rc<dyn Any>)` and ordered
   `Serialized(TransferFormat, Lazy)` representations (`with_value::<T>`,
   `value::<T>() -> Option<&T>`); the lazy provider is a `!Send`
   `Box<dyn FnOnce() -> Result<.., TransferError>>`. `DragTrigger::{Immediate, LongPress}` starts
   the session through a `Sequenced` arena branch (ADR-0208).
2. **In-window first.** A drag inside its window runs on pointer capture with ordinary pointer
   events, so touch, pen and other fingers keep working. It escalates to the OS session when the
   pointer leaves the window **and** either `outside_app` is non-empty or the source runtime has
   another open presentation; an escalation with empty `outside_app` offers only the private
   identity representation, so only that runtime's targets accept it. Such an identity-only OS
   session advertises the `within_app` actions to the OS as its allowed operations (an empty
   OS mask would make native targets refuse it before the sibling presentation can answer); the
   identity representation, not the advertised mask, limits who can redeem it, and item 5's
   drop rules apply as for any OS session. The sibling-presentation condition therefore needs
   non-empty `within_app` actions; a session whose `within_app` and `outside_app` are both
   empty is refused at start with `DragError::NoAllowedActions`, so no OS session ever
   advertises an empty mask (row `a_drag_with_no_allowed_actions_is_refused_at_start`). Otherwise leaving the
   window is a move with no target. Exceptions: item 11 (Web, iOS). This narrows ADR-0038 §5:
   `DragDropEvent` is for OS sessions only.

   **Other windows of the runtime.** Where the backend places its windows on one desktop
   (`PlatformWindow::window_at` answers which window's content is topmost under a point given in
   the source window's coordinates; Win32 and the headless double do), a session leaving its window
   first asks what is under the pointer. Over another presentation of its own runtime it stays
   in-window: it keeps the source's pointer capture, so that window's gestures never see the
   contact, and hit-tests that presentation's tree; its receivers there see `DragScope::Runtime`
   and redeem the value, and crossing between windows leaves every receiver of the last window
   before any receiver of the next one enters. Over anything else, the desktop, another runtime's
   window or another application, it escalates only when `outside_app` is non-empty: the
   sibling-presentation condition applies only where the backend answers `WindowAt::Unknown`
   (Wayland, the web, mobile), since there only the OS reaches a sibling. Another runtime of the
   process is outside the source's runtime and is reached only through the OS session and its
   serialized representations (item 4). A `DragReach::Window` session never looks. Closing the
   source window cancels its sessions wherever they hover; closing a window a session hovers leaves
   its receivers at once, while its tree still stands, and the drag goes on with no target.
3. **Identity.** A session is named by `(ProcessNonce, UiRuntimeId, DragSessionId)`, carried in a
   FLUI-private `TransferFormat::Custom` representation of the escalated offer. `DragSessionId` is a
   `NonZeroU64` newtype minted by `IdMint::next() -> Result<_, IdExhausted>`; exhaustion refuses
   permanently. The nonce is minted once per platform instance, not a `static`.
4. **Value scope.** The Rust value is redeemed only from the session table of the source's own
   `UiRuntime`, through any of its presentations. Another runtime, even on the same thread, sees
   only the serialized representations; the private identity tells it the drag is in-process,
   not how to reach the value. Apps that share values across windows use `WindowPolicy::Shared`,
   whose windows share one runtime. A cross-runtime value would need a `Send + Sync` payload bound
   that no single-runtime drag needs; redeeming through the source runtime would stall the Win32
   drop loop ([threading](../research/2026-10-13-interaction-threading-model.md) Q4).
5. **Actions and sessions.** `AllowedActions { within_app, outside_app }`, each a `TransferActions`
   set. Several sessions can be live at once (one per contact, as `UIDragSession` allows); each is
   keyed by its own id. This amends ADR-0038 §2: where a platform delivers several incoming OS
   offers to one window at once (iPadOS), each is a separate offer keyed by its `DataTransferId`,
   and a new offer no longer retires a live one; a platform that delivers one offer at a time keeps
   §2's behaviour. The same offer announced again over the same window is the same hover, which
   takes the newly permitted actions. A receiver is one registration: the lane ticket its node
   carries, holding that receiver. A ticket that was unregistered, or that now holds another
   receiver, no longer names it: it gets its one leave at the next move or tick, never
   spring-loads or takes a drop afterwards, and an OS offer's cached feedback falls back to none
   until the next move. Drop goes only to the target that accepted the **last** move. Every entered
   target other than the drop recipient, including one that declined the last move, receives
   exactly one leave; a target that accepted earlier and then declined gets no drop.
6. **State.** `pub(crate) DragSessions { Slab<Entry> }` lives per `UiRuntime` behind
   `Rc<RefCell<..>>` with short borrows, never held across user code. Each entry's `DragPhase` is
   an enum with data advanced by `mem::replace`: `Armed`, `InWindow { target }`,
   `EscalationPending`, `System { offer }`, `Concluded { action }`. No typestate: callbacks move
   sessions between phases, which a type parameter cannot follow. Errors are
   `DragError::{IdExhausted, NoAllowedActions, NoOutsideActions, NoSerializedRepresentation, SessionClosed, Unavailable}` plus `TransferError`.
7. **OS modal loop.** An escalation requested during dispatch is recorded, and the runner enters
   the OS session (`DoDragDrop` on Win32) at its outermost quiescent point, with no dispatch, borrow
   or frame in progress: the `OwnerHost` starts it when its outermost callback ends, with the
   source runtime resident, so the native callbacks a modal loop delivers are fresh outermost
   callbacks. While the loop runs, the runner keeps producing frames for every UI
   runtime on the thread, so a second window repaints and its targets answer from owner-local
   cached feedback. Reentrant native events queue in the backend as for any dispatch (ADR-0207).
8. **Spring-loading and autoscroll.** `SpringLoading` is a target policy: hovering past a deadline
   on `EventTime` activates the target (opens a folder or tab) and fires once per hover. A hover
   that carries no time (every OS drag) asks for a turn at once, which stamps it. Autoscroll is a
   receiver hook, `DropReceiver::autoscroll`, run once per `DragOwner::tick` while a drag hovers:
   the receivers under it are asked innermost first and the first that answers
   `Autoscroll::Scrolling` ends that frame, so nested scrollables never both scroll on their own.
   While one scrolls, frames are due one 60 Hz frame apart; the first frame after a move rides
   the frame that move requests; when none scrolls, frames stop until the next move. A frame
   that scrolled is followed, for an in-window session, by a hit test at the pointer's resting
   position, so the receiver scrolled under a pointer that does not move is entered and the one
   scrolled away is left.
   `Scrollable` is such a receiver and takes no drop: within a band along its edges it dispatches
   `ScrollCause::SideEffect` deltas of the drag's own contactless sequence through the nested
   scroll channel (ADR-0209), faster nearer the edge, and ends that sequence `Released` when the
   drag leaves the band or the scrollable, or the scrollable reaches its extent. The dragged
   item's element is kept alive while its session is live.
9. **Reorder is a session.** `ReorderableList` and `ReorderableGrid` start a DnD session with
   `outside_app` empty and an `InProcess` item, so within its window the fast path never mints an
   offer, never serializes and never escalates. How far a session may reach is typed in its
   configuration: `DragReach::{Window, Runtime, System}`, set by the source. `Draggable` derives
   `System` when `outside_app` is non-empty and `Runtime` otherwise; a session with non-empty
   `outside_app` whose items carry no `Serialized` representation (only `InProcess` values) is
   refused at start with `DragError::NoSerializedRepresentation`, since no target outside the
   runtime could redeem it (row `outside_actions_without_a_serialized_representation_are_refused`); `ReorderableList` and
   `ReorderableGrid` set `Window`. Items 2 and 11 escalate or start an OS session only for
   `Runtime` (identity-only offer, when a sibling presentation exists) or `System`; a `Window`
   session never becomes an OS session on any platform, Web and iOS included, so the decision
   reads the type, never widget provenance. `AnchoredDrag` stays a one-dimensional offset
   widget, not DnD.
10. **Native originating event.** Pointer assembly in `flui-platform` keeps the native event that
    began each contact, because the OS start calls need it (AppKit's dragging-session event,
    the Wayland grab serial). It is dropped when the contact ends.
11. **Platform limits are types.** On the Web and on iOS and iPadOS the OS-session decision is
    made at the start, by item 2's rule: at `dragstart` (Web) or lift (iOS) the session starts as
    an OS session when `outside_app` is non-empty **or** the source runtime has another open
    presentation, and with empty `outside_app` it offers only the private identity
    representation. On the Web it becomes an HTML drag that materializes its representations
    eagerly, since the browser allows neither a later start nor lazy data; the Web target copies
    requested representations out of `DataTransfer` during `drop` and serves them through
    `TransferRequest::ready`. On iOS it is a `UIDragSession` started through `UIDragInteraction`
    that never escalates mid-drag; UIKit keeps other touches flowing during it. On X11
    `InputCapabilities::outgoing_drag()` reports `Unavailable`
    ([ADR-0213](ADR-0213-input-capabilities.md)), the one authoritative availability path; an
    escalation that needs the OS fails with `DragError::Unavailable` (winit's `NotSupported` maps
    to it) and the session stays in-window; incoming drags work. The drop action is read at the
    drop, never assumed from hover, so X11's late action fits.
12. **Flag sets.** `TransferActions` becomes a `bitflags` 2 type in `flui-platform-api` with the
    same `COPY`, `MOVE` and `LINK`, replacing the hand-rolled `u8` with its own set algebra.
13. **Escape and the cursor.** Escape cancels the drags that run in its presentation's window,
    and a held transfer, before focus sees the key, as an OS drag loop does; with nothing to
    cancel the key reaches focus, and while an input method composes it is the method's. A mouse
    or pen drag owns the window's cursor while it runs there, since hover is frozen under it
    (ADR-0208 rule 6): the grabbing hand over nothing or over a receiver that would move it, the
    copy and alias badges, and no-drop over a receiver that declines. The OS draws the cursor of
    its own session; the session gives the cursor back when it ends or escalates.
14. **Keyboard and assistive transfer.** The alternative to dragging (ADR-0216) is the
    clipboard's cut, copy and paste: a cut or copy holds a session no pointer moves (at most one
    per runtime; a newer one cancels it), and a paste enters the receiver its target's ticket
    names and drops on it as a release would, as a move for a cut and a copy for a copy, at the
    target's reading-start top corner. A declined paste keeps the transfer held. The platform's
    cut, copy and paste keys reach it through the same intents an editable field answers.
15. **Failures.** A release whose receivers panic still ends the session, cancelled (a source
    that moves keeps its item), with its cursor and capture released, and the first panic then
    propagates. A drag reports its end to the `on_drag_end` of the build that started it.

## Consequences and verification

- One target API serves in-app and OS drags; in-app reorder pays no serialization.
- `bitflags` becomes a public dependency of a Stable contract crate.
- The `drag_sessions` table in `crates/flui-interaction/tests/` holds
  `in_window_drag_keeps_second_finger_and_pen` (where escalation is late, not on Web or iOS),
  `escalates_only_outside_window_with_outside_actions_or_sibling_presentation`,
  `drag_between_presentations_of_one_runtime_redeems_the_value` (the escalated identity-only
  session advertises the `within_app` actions and the sibling's drop completes with the
  accepted action),
  `drop_reaches_only_last_accepting_target`, `declined_target_under_pointer_receives_leave`,
  `concurrent_sessions_are_independent`, `session_id_exhaustion_refuses_permanently`,
  `value_is_redeemed_only_in_source_runtime`, `two_sessions_competing_for_one_target`,
  `a_receiver_removed_mid_hover_leaves_once_and_takes_no_drop`,
  `a_replaced_registration_is_a_new_receiver`,
  `a_receiver_panicking_in_spring_loading_is_contained`, `an_undated_hover_still_comes_due`,
  `a_repeated_offer_keeps_one_hover_and_concurrent_offers_stay_apart` and
  `autoscroll_frames_ask_innermost_first_until_one_scrolls`.
  The `nested_scroll_channel` table in `crates/flui-widgets/tests/contracts.rs` holds
  `drag_autoscroll_dispatches_side_effect_deltas`. Its `drag_and_drop` table holds the widget
  rows `a_drag_drops_on_the_target_that_accepted_its_last_move`,
  `a_drag_leaving_the_window_escalates_to_the_os_when_it_may_reach_it`,
  `escape_cancels_a_drag_in_its_window`, `cut_and_paste_by_keyboard_drop_like_a_drag`,
  `a_target_removed_mid_drag_takes_no_drop`, `unmounting_the_source_mid_drag_cancels_it`,
  `a_panicking_drop_callback_ends_the_drag_cancelled_and_the_next_drops`,
  `a_paste_drops_at_the_targets_reading_start_corner`, `the_cursor_shows_what_a_drop_would_do`,
  `spring_loading_fires_once_per_hover`, `a_long_press_trigger_lifts_after_the_hold` and
  `a_drag_reports_to_the_callbacks_it_started_with` and
  `autoscroll_finds_the_target_scrolled_under_a_resting_pointer`, and its `gesture_alternatives` table the
  input-path matrix row `draggable_and_drop_target_by_every_path`;
  `reorder_never_mints_an_offer` comes with `ReorderableList`;
  `crates/flui-runtime/tests/` holds `drag_and_drop_reaches_the_runtime`, whose
  `a_drag_between_runtimes_delivers_serialized_representations_only` runs two runtimes of one
  host sharing its nonce, and whose `a_drag_crosses_into_a_sibling_window_on_its_capture`,
  `closing_the_source_window_cancels_the_drag_over_its_sibling`,
  `a_hovered_window_that_closes_is_left_at_once` and
  `drags_from_both_windows_cross_independently` run two placed windows of one runtime.
  Linux CI runs these headless.
- The OLE loop's deferred start, reentrancy and second-window repaint rows, and
  `a_window_finds_the_window_of_this_process_under_a_point` (the Win32 `window_at` on two real
  windows), run in `ole_drag_and_drop_on_real_windows` on a Windows host, where Win32 also defers its own start to
  the owner window's next turn, since the host's quiescent point is still inside the window's
  callback, and paces frames from a thread, since `DoDragDrop`'s loop dispatches no timer or paint
  while the pointer rests. AppKit, UIKit, Android, Wayland and Web sessions run only on native
  hosts or the browser through `cargo xtask device`; CI type-checks them. `assert_backend_conforms` has typed rows for the X11 refusal
  (`outgoing_drag()` is `Unavailable`, a start fails with `DragError::Unavailable`), for the iOS
  session that starts at lift and never escalates, and, on Web and iOS,
  `start_decision_includes_sibling_presentation_with_identity_only_offer`.

## Alternatives rejected

- **Every drag as an OS session:** a modal loop for each reorder, and touch and pen leave the arena.
- **Moving the Rust value between runtimes:** needs a `Send + Sync` payload bound that no
  single-runtime drag needs; redeeming through the source runtime would stall the Win32 drop loop
  (threading Q4).
- **Typestate sessions:** callbacks drive the transitions; a runtime enum is the honest shape.
- **`enumset` or the hand-rolled `u8`** (dependency selection §10): `enumset` needs fieldless
  enums; the `u8` is a second copy of set algebra.
