# ADR-0208: One ownership table decides who owns a contact; arena composition nests

- **Status:** Accepted (2026-10-13)
  ([tasks](../plans/specs/flui-interaction/completeness/tasks.md); design §4, §18)
- **Date:** 2026-10-13
- **Supersedes:** [ADR-0167](ADR-0167-arena-owned-recognizer-composition.md)
- **Related:** [ADR-0112](ADR-0112-gesture-terminal-reasons.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md),
  [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md),
  [ADR-0164](ADR-0164-down-route-logical-pointer-capture.md),
  [ADR-0166](ADR-0166-combined-viewer-gestures.md),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md),
  [ADR-0209](ADR-0209-nested-scroll-connection.md)

## Context

ADR-0167's arena composes binarily and refuses nesting. Who owns a contact when axial drags,
pans, scale viewers and nested scrollables meet was left to first-claimant timing, so a list or
photo in a pager needs per-widget workarounds, as in Flutter (flutter/flutter#50946). Owning the
contact is the arena's question; sharing the scroll delta is the channel's (ADR-0209).

## Decision

### 1. The normative ownership table

Each cell is a row of `assert_ownership_table` (`flui-testing`); other documents cite the table.

| Member \ topology | Orthogonal ancestor scroll | Same-axis ancestor scroll | No competitor |
|---|---|---|---|
| Axial (drag, `Scrollable`) | decided on accumulated displacement at slop; the loser rejects | innermost owns; the ancestor is only a channel participant | holds until slop on its axis, or release |
| Free (pan) | yields to the axial member at ≥ cos 45° on its axis, unless the subtree's `PanClaim` is `None` | same | owns after slop |
| Scale / `InteractiveViewer` | channel leaf: does not yield; ancestors take the remainder through the channel. A scale that needs two contacts is a leaf only once it holds two: on a lone contact the table passes it by, so a pan beside it takes a one-finger drag (`drag_and_transform_slots_split_one_contact_from_two`) | same | owns |
| `AnchoredDrag` leaf (`SwipeActions` row) | axial member; consumes first, remainder to the ancestor | same | owns |
| `AnchoredDrag` ancestor (photo dismiss) | channel participant only | same | — |
| `Sequenced` (menu → drag, DnD, reorder, selection handles) | the hold is the arena member; after hand-off the successor owns the contact, its motion axial or free per its own configuration | same | owns |

### 2. Rules

1. **Admission first.** A recognizer's `allowed_devices: PointerKindSet` refuses contacts of
   other kinds before any rule runs (pen draws, finger scrolls). `PanClaim::{Auto, PanX, PanY,
   None}` names the axes axial members may claim from free members in a subtree; the effective
   claim is the intersection of the values on the hit path, per axis, up to the nearest axial
   member on that axis (CSS `touch-action`). `None` (map, signature) keeps the free member.
2. **Axis on accumulated displacement** from Down, evaluated when a slop is crossed, never on the
   first sample. Free vs axial: `|d · axis| ≥ |d| · cos 45°`, so exactly 45° goes to the axial
   member. Two orthogonal axial members crossing slop in one sample: the larger component wins;
   an exact tie goes to the innermost member on the route.
3. **Same axis.** An ancestor channel participant does not compete in the arena with a
   descendant channel leaf on that axis; the innermost claimant owns and the ancestor receives
   deltas. A free pan is not a channel leaf and follows rule 2. This replaces ADR-0167's sentence
   "Horizontal drag acquires no invented orthogonal-rejection behavior": an axial member rejects
   when the table gives the contact to an orthogonal winner, and a pan meeting an axial drag is
   decided by rule 2, not by first claim.
4. **Hold on Down.** Touching a moving scrollable admits a hold member that beats tap and
   long-press on that contact, not drag or scroll: an arena rule; children stay hit-testable.
   A hold is an admission kind (`ArenaMembership::holding_contact`, `HoldGestureRecognizer`):
   on its admission the contact's members admitted without a motion role lose, and such members
   joining after it are refused; members with a motion role compete as before and take the
   contact from the hold.
5. **Cancel on hand-off.** When the arena gives a continuing contact to its winner, losers and the
   pointer callbacks of the `Listener`s on the route, other than the one feeding the winning
   member (`GestureDetector` and `Draggable` feed recognizers through a `Listener`), receive
   exactly one `Cancel { reason: CancelReason::Claimed }` and nothing more for that
   `InputSequenceId`, as `pointercancel` does. `Claimed` is a new variant of the
   `#[non_exhaustive]` `flui_platform_api::pointer::CancelReason`, emitted only by the framework;
   an observing `Listener` is cancelled too. Verdicts precede it (ADR-0159 order); an accepted end
   still carries `GestureEndReason` (ADR-0112).
   Only a *claimed* hand-off cancels: a member that asked for the contact (an acceptance, a
   motion claim, an explicit resolution) and won it while the contact continues. A lone
   member's default win claims nothing, so no `Listener` is cancelled: otherwise every raw
   `Listener` beside a lone recognizer would lose its contact at Down — `EditableText` runs its
   selection drag in a raw `Listener` beside a lone double-tap recognizer, and that drag would
   stop on the first frame. A `Sequenced` hold whose win carries successors claims nothing
   either; the contact is claimed for the successor's target at the hand-off. Row
   `unclaimed_wins_cancel_no_listener` (`gesture_lifecycle_matrix`) pins both cases, and
   `claimed_contact_cancels_every_other_listener_once` the claimed one.
6. **No re-decision.** `PointerEvent::Revise` refines samples for the current owner, never
   reopening an axis, slop or winner. Hover freezes while a scroll sequence is latched: a
   contact is latched from the verdict that gives it to a member with a motion role, by claim
   or as the lone member, to its terminal event; a claimed trackpad pan-zoom session is latched
   too.
7. **No `Simultaneous`.** One contact has one owner. Continued observation goes through
   `GestureObserver`, shared continuous motion through the channel.

### 3. Composition

```rust
#[non_exhaustive] pub enum GestureComposition { Exclusive, RequireFailure }
#[non_exhaustive] pub enum CompositionKind { Exclusive, RequireFailure, Sequenced }
impl GestureArena {
    pub fn compose(&self, c: GestureComposition) -> Result<GestureBranches, CompositionError>;
    pub fn sequenced(&self) -> Result<SequencedBranch, CompositionError>;
}
#[non_exhaustive]
pub enum CompositionError { DelayCeilingExceeded { sum: Duration, ceiling: Duration }, Closed }
```

- A branch composes again into a tree: a child holds its parent with `Rc`, a parent its children
  with `Weak` (`Rc::new_cyclic`). `AlreadyComposed` is removed.
- `Sequenced` is one arena member with explicit hand-off from hold to successor; only
  `sequenced()`'s `SequencedBranch` carries that authority. `CompositionKind` is what each node
  reports to `GestureObserver` and `GestureInspector`. The menu stays visible until release;
  motion past slop closes it and starts the drag.
- A composition whose summed deadlines on any root-to-leaf path exceed the preset's
  `max_resolution_delay` returns `DelayCeilingExceeded`, which the detector reports as a build
  diagnostic. Every deadline is on the `EventTime` scale.
- Carried over from ADR-0167: branches share the presentation's clock, deadlines, owner closure
  and pointer slots; every admission, Tap's per-contact proxy included, inherits its handle's
  branch; relation identity is an owner-local `Rc` allocation, never a global counter.
  `RequireFailure` retains the second branch's accepted request while any first-branch member is
  viable; missing first-branch admission is failure after close; open-admission votes are
  rechecked after close; repeats are idempotent. A sweep cannot grant a blocked fallback and
  retains that exact generation; deadline polling, rejection or dead-owner pruning releases debt
  without another event; stale entry handles cannot resolve a fresh generation; cancellation
  abandons the competition; explicit resolution observes the dependency. A member joins one
  branch per contact generation. DoubleTap keeps its first-Up hold.
- From ADR-0159 and ADR-0161: every member settles exactly once; pending verdicts hold weak
  identities upgraded just before invocation, outside borrows; state and exact-slot removal
  commit before notification; the first failure stays authoritative (ADR-0127).

### 4. Configuration, not arena special cases

Rotation is Scale configuration (ADR-0166). `SwipeGestureRecognizer` (`SwipeStart::{Anywhere(Axis),
FromEdge(PhysicalEdge)}`; `Anywhere` names its axis so a start without one is unrepresentable) is
axial. `AxisLock` rails touch and touchpad only and projects release velocity onto the locked
axis; `TransformLock` locks rotation or scale in a gesture that begins as a pan.

## Consequences and verification

- `gesture_lifecycle_matrix` (`flui-interaction`) keeps every ADR-0167 row and adds nested
  composition, `Sequenced` hand-off, the delay-ceiling refusal, cancel on hand-off, hold on Down,
  `Revise` leaving decisions and hover frozen under a latch. `assert_ownership_table` runs one
  row per cell plus the 44°, 45° and 46° rows with first-sample noise, for touch and mouse
  through `PointerScript`, and widget rows on mounted widgets (pager, `InteractiveViewer`, a
  press and a vertical drag over a settling pager); the `SwipeActions` row lands with that
  widget. `assert_backend_conforms`
  has a row that no backend emits `CancelReason::Claimed`.
- All rows run headless in CI; native touch, pen and touchpad paths run only on a native host
  (`cargo xtask device <os>-input`, recorded traces).

## Alternatives rejected

- **`Simultaneous`** (SwiftUI `simultaneously(with:)`; [research](../research/2026-10-12-interaction-input-best-practices.md)
  §1): it duplicates what the channel shares by delta; UIKit's pairwise predicate is non-local.
- **Flutter `GestureArenaTeam`, axis races, imperative intercept** (Android
  `requestDisallowInterceptTouchEvent`, eager winner): replaced by `PanClaim` and rule 3.
