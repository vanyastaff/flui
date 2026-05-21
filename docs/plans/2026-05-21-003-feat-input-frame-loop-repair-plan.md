---
date: 2026-05-21
type: feat
status: ready
origin: docs/brainstorms/input-frame-loop-repair-requirements.md
audit_origin: docs/research/2026-05-21-flui-interaction-scheduler-audit.md
depth: deep
target_crates: flui-interaction, flui-scheduler, flui-animation (source-only — disabled crate)
audit_items: "Findings I-1..I-22 + S-1..S-22 (all 44)"
predecessor_pr: "84 (Framework Spine Repair — eb95c2f2)"
flutter_reference: ".flutter/flutter-master/packages/flutter/lib/src/{gestures,scheduler}/"
---

# feat: Input + Frame-Loop Repair — Ticker dispose + Recognizer FSM consolidation + Focus + Principle 6 sweep

## Summary

Single comprehensive PR closing 44 audit findings across `flui-interaction` (12,360 LOC, 38 files) + `flui-scheduler` (9,064 LOC, 12 files), plus source-only migration of disabled `flui-animation`. **32 atomic commits** across 8 sequenced waves matching [PR #84](https://github.com/vanyastaff/flui/pull/84) precedent. Net delta: **~3,995 LOC zombie deletion + ~1,500 LOC restructuring + ~600 LOC commons/tests = ~3,000-3,500 LOC reduction**. Wave 1 deletes zero-consumer scaffolding (TypestateTicker / typestate.rs / OneSequence+PrimaryPointer skeletons / prelude_advanced / Handle<M> / ZST priorities / TypedTask / ext traits / arc_instance / VsyncDrivenScheduler / Ordered+DirectionalFocusPolicy / PointerEventData compat) before Wave 4's larger restructures. Wave 2 establishes type baseline (canonical 3-state GestureRecognizerState enum + TryFrom<u8>-Result-returning constructors + Microseconds u64 + ui_events::pointer::PointerId widening). Wave 3 adopts PR #84's `ChangeNotifier::dispose` pattern uniformly across Ticker + Recognizer + FocusManager + GestureBinding (dispose + Drop + `disposed: AtomicBool` + use-after-dispose `debug_assert!`). Wave 4 re-introduces the canonical OneSequence/PrimaryPointer Flutter trait hierarchy AS PROPER TRAITS THIS TIME, then migrates all 7 concrete recognizers to it (atomic commit per recognizer, ~250 LOC each), reshapes TickerProvider::create_ticker(callback) -> Ticker factory matching Flutter, absorbs ScheduledTicker into Ticker, unifies the two FocusManager implementations, migrates disabled flui-animation source. Wave 5 fixes per-event hot-path allocation + 2+N+M lock count in PointerRouter + atomic single-field reads. Wave 6 implements MouseTracker::update_all_devices, drains pointer HashMap on cancel, feature-gates testing/. Wave 7 closes Flutter parity (AppLifecycleState auto-toggle, persistent/post-frame strict immutability, Tap dispatch order, Priority numeric realignment to Flutter values 0/50000/100000/200000).

---

## Problem Frame

See [origin: docs/brainstorms/input-frame-loop-repair-requirements.md](../brainstorms/input-frame-loop-repair-requirements.md) Problem Frame.

---

## Requirements

R1–R26 carried from origin. R-V1–R-V5 verification gates per-commit. AE1–AE20 acceptance examples per-requirement.

---

## Output Structure

No new directory hierarchy. Three NEW files land:

- `crates/flui-interaction/src/recognizers/one_sequence.rs` (U13) — re-introduced as proper trait
- `crates/flui-interaction/src/recognizers/primary_pointer.rs` (U13) — re-introduced as proper trait
- `crates/flui-interaction/src/dispose.rs` (U10/U11/U12) — shared disposed-state pattern helpers; OR inline per-type если extraction not warranted.

Most work modifies existing files. Three files DELETED entirely:

- `crates/flui-interaction/src/typestate.rs` (U2)
- `crates/flui-interaction/src/recognizers/one_sequence.rs` (U3, before U13 re-introduces)
- `crates/flui-interaction/src/recognizers/primary_pointer.rs` (U3, before U13 re-introduces)
- Several module-internal types (TypestateTicker / Handle<M> / ZST priorities / etc.) deleted from existing files.

---

## High-Level Technical Design

### Unit dependency graph

```mermaid
graph TD
    subgraph "Wave 1: Zero-dep deletions (no consumer impact)"
        U1[U1: Scheduler zero-dep deletion<br/>TypestateTicker+prelude_advanced+Handle+<br/>ZSTPriorities+TypedTask+ExtTraits+<br/>arc_instance+VsyncDrivenScheduler]
        U2[U2: Interaction typestate.rs delete]
        U3[U3: OneSequence+PrimaryPointer<br/>scaffold delete]
        U4[U4: Interaction misc zombie delete<br/>OrderedTraversal+DirectionalFocus+<br/>PointerEventData+PointerEventKind]
    end
    subgraph "Wave 2: Type baseline (blocks Wave 3+4)"
        U5[U5: Consolidate GestureRecognizerState×3<br/>→ canonical 3-state enum]
        U6[U6: TryFrom u8 for SchedulerPhase+<br/>FrameSkipPolicy+AppLifecycleState]
        U7[U7: Result-returning constructors<br/>VsyncScheduler+FrameDuration+set_time_dilation]
        U8[U8: Microseconds i64 → u64]
        U9[U9: PointerId widening to<br/>ui_events::pointer::PointerId]
    end
    subgraph "Wave 3: Dispose pattern (PR #84 adoption)"
        U10[U10: Ticker::dispose + Drop +<br/>disposed: AtomicBool + start<br/>started-twice debug-assert]
        U11[U11: Recognizer::dispose with<br/>arena+router+timer cleanup]
        U12[U12: FocusManager+GestureBinding<br/>disposed-state pattern]
    end
    subgraph "Wave 4: Consolidation"
        U13[U13: Re-introduce OneSequence+<br/>PrimaryPointer canonical traits]
        U14[U14: TickerProvider::create_ticker<br/>reshape Flutter factory]
        U15[U15: Absorb ScheduledTicker<br/>into Ticker]
        U16[U16: Migrate Tap → PrimaryPointer]
        U17[U17: Migrate LongPress → PrimaryPointer]
        U18[U18: Migrate Drag → OneSequence]
        U19[U19: Migrate Scale → OneSequence]
        U20[U20: Migrate ForcePress → OneSequence]
        U21[U21: Migrate DoubleTap → GestureRecognizer]
        U22[U22: Migrate MultiTap → GestureRecognizer]
        U23[U23: Unify FocusManager+<br/>FocusManagerInner]
        U24[U24: flui-animation source migration]
    end
    subgraph "Wave 5: Hot path + sync"
        U25[U25: extract_pointer_id alloc removal +<br/>PointerRouter Map O(1) + dispatch order]
        U26[U26: Atomic state fields<br/>Ticker::state AtomicU8 +<br/>TaskQueue::len AtomicUsize]
    end
    subgraph "Wave 6: Unbounded + hygiene"
        U27[U27: MouseTracker::update_all_devices impl]
        U28[U28: GestureBinding lifecycle GC]
        U29[U29: testing/ feature-gate default=skip]
    end
    subgraph "Wave 7: Flutter parity"
        U30[U30: AppLifecycleState auto-toggle<br/>frames_enabled]
        U31[U31: Persistent+Post-frame strict<br/>immutability remove _remove APIs]
        U32[U32: Tap dispatch order +<br/>Priority numeric realign]
    end

    U1 --> U6
    U1 --> U10
    U2 --> U5
    U3 --> U13
    U4 --> U7
    U5 --> U11
    U5 --> U16
    U5 --> U17
    U5 --> U18
    U5 --> U19
    U5 --> U20
    U5 --> U21
    U5 --> U22
    U9 --> U11
    U9 --> U25
    U10 --> U12
    U10 --> U14
    U10 --> U15
    U10 --> U26
    U11 --> U12
    U13 --> U16
    U13 --> U17
    U13 --> U18
    U13 --> U19
    U13 --> U20
    U14 --> U15
    U15 --> U24
    U23 --> U27
    U12 --> U28
    U6 --> U30
    U10 --> U31
    U16 --> U32
```

### ElementOwner pattern adoption (carried from PR #84)

Wave 3 dispose plumbing follows PR #84's `ChangeNotifier::dispose` adoption template at [`crates/flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs). Same shape across `Ticker`, `Recognizer`, `FocusManager`, `GestureBinding`:

```rust
// crates/flui-scheduler/src/ticker.rs (Wave 3 U10 sketch — directional only)
pub struct Ticker {
    // ... existing fields ...
    disposed: AtomicBool,
}

impl Ticker {
    pub fn dispose(&mut self) {
        if self.disposed.swap(true, Ordering::Release) {
            return; // idempotent
        }
        if let Some(future) = self.future.take() {
            future.cancel(TickerCanceled);
        }
        self.state.store(TickerState::Disposed as u8, Ordering::Release);
        // Clear callback storage, unschedule from Scheduler...
    }

    fn assert_not_disposed(&self, op: &'static str) {
        debug_assert!(
            !self.disposed.load(Ordering::Acquire),
            "Ticker::{} called after dispose",
            op
        );
        if self.disposed.load(Ordering::Acquire) {
            tracing::warn!(op, "Ticker used after dispose");
        }
    }

    pub fn start(&mut self) {
        self.assert_not_disposed("start");
        debug_assert!(
            self.state.load(Ordering::Acquire) != TickerState::Active as u8,
            "A ticker was started twice"
        );
        // ...
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        if !self.disposed.load(Ordering::Acquire) {
            self.dispose();
        }
    }
}
```

### OneSequence + PrimaryPointer trait shape (Wave 4 U13 sketch)

```rust
// crates/flui-interaction/src/recognizers/one_sequence.rs (re-introduced as REAL trait, not scaffold)
use crate::recognizers::{GestureRecognizer, GestureRecognizerState};
use ui_events::pointer::PointerId;

pub trait OneSequenceGestureRecognizer: GestureRecognizer {
    /// Tracked pointers entered into this recognizer's arena.
    fn tracked_pointers(&self) -> &[PointerId];

    /// Add pointer to arena + register PointerRouter handler.
    /// Flutter parity: gestures/recognizer.dart:413+ (OneSequenceGestureRecognizer.addPointer)
    fn add_pointer(&mut self, event: &PointerEvent);

    /// Resolve arena for given pointer.
    /// Flutter parity: gestures/recognizer.dart:441+ (resolve)
    fn resolve(&mut self, pointer: PointerId, disposition: GestureDisposition);

    /// Stop tracking pointer (unregister route, drop arena entry).
    /// Flutter parity: gestures/recognizer.dart:471+ (stopTrackingPointer)
    fn stop_tracking_pointer(&mut self, pointer: PointerId);
}

// crates/flui-interaction/src/recognizers/primary_pointer.rs
pub trait PrimaryPointerGestureRecognizer: OneSequenceGestureRecognizer {
    fn primary_pointer(&self) -> Option<PointerId>;
    fn primary_pointer_position(&self) -> Option<Offset>;

    /// Flutter parity: gestures/recognizer.dart:625+ (didExceedDeadline / didStopTrackingLastPointer)
    fn did_exceed_deadline(&mut self);
}
```

Concrete recognizers (Tap, LongPress, etc.) impl `PrimaryPointerGestureRecognizer`; Drag/Scale/ForcePress impl `OneSequenceGestureRecognizer` directly. DoubleTap/MultiTap impl `GestureRecognizer` base (matches Flutter's `multitap.dart` inheritance).

### TickerProvider factory reshape (Wave 4 U14 sketch)

```rust
// crates/flui-scheduler/src/ticker.rs (U14 sketch)
pub type TickerCallback = Box<dyn FnMut(Duration) + Send + 'static>;

pub trait TickerProvider: Send + Sync {
    /// Flutter parity: scheduler/ticker.dart:248 (TickerProvider.createTicker)
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker;
}

// SchedulerTickerProvider impl
impl TickerProvider for Scheduler {
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker {
        Ticker::new(on_tick, self.clone_arc())
    }
}
```

---

## Implementation Units

Each unit has:
- **Goal**: 1-sentence outcome
- **Requirements**: R-IDs covered
- **Dependencies**: prerequisite unit IDs
- **Files**: modify / create / delete
- **Approach**: implementation strategy
- **Patterns**: idiomatic refs (book / Flutter / PR #84)
- **Test scenarios**: covers AE-IDs + edge cases
- **Verification**: per-commit gates

<!-- Execution parallelism plan (orchestrator annotation, added pre-Wave 1):
  Wave 1: Lane A = U1 (scheduler-only). Lane B = U2 → U3 → U4 serial (all touch flui-interaction/lib.rs).
          Physical cargo target dir contention → serial execution within coordinator.
  Wave 2: U6 depends on U1. U5 depends on U2+U3. U7/U8/U9 independent. Mostly parallel-eligible by file ownership; serial by build contention.
  Wave 3: U10 → U11 → U12 strict serial.
  Wave 4: U13 → {U14, U15, U16-U22} fan-out, U16-U22 each touches recognizers/mod.rs export list → coordinate exports.
          U23 + U24 after recognizer migration; U24 requires R-V5 manual flui-animation smoke.
  Wave 5: U25 + U26 independent file ownership, run after Wave 4.
  Wave 6: U27 + U28 + U29 independent.
  Wave 7: U30 → U31 → U32 strict serial; U32 includes audit Part IV closure annotation.
-->

### U1. Scheduler zero-dep deletion

**Goal:** Delete ~1,000 LOC of zero-consumer scheduler scaffolding.

**Requirements:** R1.

**Dependencies:** None.

**Files:**
- Delete: [`crates/flui-scheduler/src/typestate.rs`](../../crates/flui-scheduler/src/typestate.rs) (392 LOC — TypestateTicker<Active/Idle/Muted/Stopped>)
- Modify: [`crates/flui-scheduler/src/id.rs`](../../crates/flui-scheduler/src/id.rs) — delete `Handle<M>`, `FrameHandle`, `TaskHandle` (~110 LOC)
- Modify: [`crates/flui-scheduler/src/task.rs`](../../crates/flui-scheduler/src/task.rs) — delete `TypedTask<P>` (~75 LOC)
- Modify: [`crates/flui-scheduler/src/traits.rs`](../../crates/flui-scheduler/src/traits.rs) — delete `PriorityLevel`, `UserInputPriority`/`AnimationPriority`/`BuildPriority`/`IdlePriority` ZSTs, `PriorityExt`, `FrameBudgetExt`, `FrameTimingExt`, `ToMilliseconds`, `ToSeconds` (~280 LOC). File should shrink dramatically — verify if any remaining content; if empty, delete file entirely.
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) — delete `arc_instance()` static + `OnceLock<Arc<Scheduler>>` (~50 LOC)
- Delete or shrink: [`crates/flui-scheduler/src/vsync.rs`](../../crates/flui-scheduler/src/vsync.rs) — delete `VsyncDrivenScheduler` (134 LOC); keep `VsyncScheduler`/`VsyncMode`/`VsyncStats`/`VsyncCallback`
- Modify: [`crates/flui-scheduler/src/lib.rs`](../../crates/flui-scheduler/src/lib.rs) — delete `prelude_advanced` module + all re-exports of deleted types; clean up `prelude` to retain only the in-use exports

**Approach:** Pre-execute sweep `rg 'TypestateTicker|prelude_advanced|Handle<|FrameHandle|TaskHandle|UserInputPriority|AnimationPriority|BuildPriority|IdlePriority|PriorityLevel|TypedTask|arc_instance|VsyncDrivenScheduler|ToMilliseconds|ToSeconds|PriorityExt|FrameBudgetExt|FrameTimingExt' crates/` to confirm zero non-doc-comment consumers. Then atomic delete. Single commit, conventional message `refactor(scheduler)!: delete zero-consumer scaffolding (TypestateTicker, Handle, ZST priorities, ext traits, arc_instance, VsyncDrivenScheduler)`.

**Patterns:** Same pattern as PR #84 U4 (foundation cleanup — `MergedListenable`/`HashedObserverList` deletion). Pre-deletion grep + atomic delete + Cargo.toml dep adjustments.

**Test scenarios:** Covers AE15. No new tests; removed tests track with removed types.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'TypestateTicker|prelude_advanced|...' crates/` returns zero. `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test -p flui-scheduler --lib` clean.

---

### U2. Delete `flui-interaction::typestate` module

**Goal:** Delete 232 LOC of 13 ZST markers + 4 marker traits, zero impls workspace-wide.

**Requirements:** R2.

**Dependencies:** None.

**Files:**
- Delete: [`crates/flui-interaction/src/typestate.rs`](../../crates/flui-interaction/src/typestate.rs)
- Modify: [`crates/flui-interaction/src/lib.rs`](../../crates/flui-interaction/src/lib.rs) — remove `pub mod typestate;` and any re-exports.

**Approach:** Pre-execute `rg 'crate::typestate::|use crate::typestate' crates/flui-interaction/` confirm zero references. Audit already confirmed.

**Patterns:** Same as U1.

**Test scenarios:** Covers AE15.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'flui_interaction::typestate' crates/` returns zero.

---

### U3. Delete `OneSequence` + `PrimaryPointer` scaffold (re-introduced in U13)

**Goal:** Delete 823 LOC of zero-impl trait scaffolding before re-introducing as proper traits in U13.

**Requirements:** R2.

**Dependencies:** None.

**Files:**
- Delete: [`crates/flui-interaction/src/recognizers/one_sequence.rs`](../../crates/flui-interaction/src/recognizers/one_sequence.rs) (341 LOC)
- Delete: [`crates/flui-interaction/src/recognizers/primary_pointer.rs`](../../crates/flui-interaction/src/recognizers/primary_pointer.rs) (481 LOC)
- Modify: [`crates/flui-interaction/src/recognizers/mod.rs`](../../crates/flui-interaction/src/recognizers/mod.rs) — remove `mod one_sequence; mod primary_pointer;` declarations + re-exports of `PrimaryPointerState`/`PrimaryPointerStateHelper`/`GestureRecognizerState` triple

**Approach:** Sub-commit to U1's pattern. Two file deletes + mod.rs edit. The scaffolded traits had 0 `impl ... for` blocks (audit Finding I-3 confirmed) — pure deletion. Re-introduced in U13 as proper canonical traits.

**Test scenarios:** Covers AE15.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'one_sequence::|primary_pointer::|PrimaryPointerState' crates/flui-interaction/` returns zero outside `recognizers/mod.rs` re-export removal.

---

### U4. Interaction misc zombie deletion

**Goal:** Delete ~270 LOC of additional zero-consumer interaction types.

**Requirements:** R2.

**Dependencies:** None.

**Files:**
- Modify: [`crates/flui-interaction/src/routing/focus_scope.rs`](../../crates/flui-interaction/src/routing/focus_scope.rs) — delete `OrderedTraversalPolicy` + `DirectionalFocusPolicy` (lines 840-961, ~120 LOC). Keep `ReadingOrderPolicy` if it has consumers, verify.
- Modify: [`crates/flui-interaction/src/events.rs`](../../crates/flui-interaction/src/events.rs) — delete `PointerEventData` + `PointerEventKind` + `make_pointer_event` helpers (lines 135-249, 695-756 = ~150 LOC). Move any genuine non-test logic if needed (audit said all uses are testing-only).
- Modify: testing module if it currently consumes the deleted compat layer — replace with direct `PointerEvent` construction. [`crates/flui-interaction/src/testing/input.rs`](../../crates/flui-interaction/src/testing/input.rs) likely touched.
- Modify: [`crates/flui-interaction/src/lib.rs`](../../crates/flui-interaction/src/lib.rs) — remove re-exports of deleted types.

**Approach:** Pre-execute `rg 'OrderedTraversalPolicy|DirectionalFocusPolicy|PointerEventData|PointerEventKind|make_pointer_event' crates/`. Audit said zero non-testing usages of the events compat layer. Replace testing-module calls with direct ui_events::pointer::PointerEvent construction.

**Test scenarios:** Covers AE15. Update testing-module tests to use direct PointerEvent constructors.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U5. Consolidate `GestureRecognizerState` × 3 → canonical 3-state enum

**Goal:** Single canonical `GestureRecognizerState { Ready, Possible, Defunct }` enum matching Flutter.

**Requirements:** R3.

**Dependencies:** U2 (typestate deleted), U3 (PrimaryPointer scaffold deleted — removes one of the three colliding types).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/recognizer.rs`](../../crates/flui-interaction/src/recognizers/recognizer.rs) — replace existing `GestureRecognizerState` struct (line 59) with canonical 3-state enum
- Modify: [`crates/flui-interaction/src/recognizers/mod.rs`](../../crates/flui-interaction/src/recognizers/mod.rs) — remove old `GestureState` enum if it lives here; ensure single export of canonical enum
- Modify: each concrete recognizer in `recognizers/{tap,double_tap,long_press,drag,multi_tap,scale,force_press}.rs` — change ad-hoc state-tracking enums to use canonical enum (or leave private FSM enums in place if they encode richer state, but consolidate the public-facing GestureRecognizerState type only). Per Flutter `recognizer.dart:585-598` GestureRecognizerState enum — recognizers carry their own private state for internal transitions but expose the canonical 3-state to consumers.

**Approach:**
```rust
// crates/flui-interaction/src/recognizers/recognizer.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GestureRecognizerState {
    /// No pointer events received; ready to accept new sequence.
    /// Flutter parity: recognizer.dart:585 GestureRecognizerState.ready
    Ready,
    /// Has received pointer down; awaiting arena resolution.
    /// Flutter parity: recognizer.dart:591 GestureRecognizerState.possible
    Possible,
    /// Sequence is over or recognizer was rejected.
    /// Flutter parity: recognizer.dart:597 GestureRecognizerState.defunct
    Defunct,
}
```

**Patterns:** Flutter `recognizer.dart:585-598` GestureRecognizerState enum exact mapping. *Programming Rust* 2nd ed Ch. 10 "Enums and Patterns" — closed enum + `#[non_exhaustive]` for forward compat.

**Test scenarios:** Covers AE11. Test: `let state = GestureRecognizerState::Ready; assert_eq!(state.transition_on_down(), GestureRecognizerState::Possible);` etc. Per-recognizer FSM unchanged behaviorally.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'GestureRecognizerState|PrimaryPointerState|GestureState' crates/` returns single canonical type only.

---

### U6. `TryFrom<u8>` for SchedulerPhase + FrameSkipPolicy + AppLifecycleState

**Goal:** Eliminate 6 production `from_u8` panic sites.

**Requirements:** R4.

**Dependencies:** U1 (post-cleanup surface).

**Files:**
- Modify: [`crates/flui-scheduler/src/frame.rs`](../../crates/flui-scheduler/src/frame.rs) — `SchedulerPhase::from_u8` and `AppLifecycleState::from_u8`: change from `panic!` on invalid to `impl TryFrom<u8> for SchedulerPhase` returning `Result<Self, EnumOutOfRange>`. Add new sealed error `pub struct EnumOutOfRange { pub got: u8, pub enum_name: &'static str }` derives `Error + Debug + Display`.
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) lines 496, 501, 1035, 1065, 1111, 1140 — replace `SchedulerPhase::from_u8(...)` with `SchedulerPhase::try_from(byte).unwrap_or(SchedulerPhase::Idle)` (Flutter-faithful safe default: when atomic load produces stale/invalid, treat as Idle). Same for FrameSkipPolicy → Default, AppLifecycleState → Detached.

**Approach:** Add `try_from` impl, keep `from_u8` as deprecated alias (one cycle) OR delete outright since project pre-1.0. Delete outright per Scope Boundary "ABI / public-API breakage allowance".

**Patterns:** *Rust for Rustaceans* Ch. 5 "Errors" + Constitution Principle 6. Flutter `binding.dart` SchedulerPhase enum at [`binding.dart:160-199`](../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart) — closed enum, no invalid values possible in Flutter (statically checked).

**Test scenarios:** Covers AE17. `assert!(SchedulerPhase::try_from(255).is_err())`; production atomic-load round-trip returns Idle on invalid byte.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'panic!.*SchedulerPhase|panic!.*FrameSkipPolicy|panic!.*AppLifecycleState' crates/flui-scheduler/src/` returns zero.

---

### U7. `Result`-returning constructors for `VsyncScheduler::new` / `FrameDuration::from_fps` / `set_time_dilation`

**Goal:** Eliminate 3 production-path `assert!` panics.

**Requirements:** R5.

**Dependencies:** None (U4 cleanup helpful but not blocking).

**Files:**
- Modify: [`crates/flui-scheduler/src/vsync.rs:167`](../../crates/flui-scheduler/src/vsync.rs) — `VsyncScheduler::new(target_fps: u32) -> Result<Self, InvalidVsyncConfig>` returning `Err` on `target_fps == 0`. Add `thiserror::Error` for `InvalidVsyncConfig::ZeroFps`.
- Modify: [`crates/flui-scheduler/src/duration.rs:513`](../../crates/flui-scheduler/src/duration.rs) — `FrameDuration::from_fps(fps: u32) -> Result<Self, InvalidDurationConfig>` returning `Err` on `fps == 0`.
- Modify: [`crates/flui-scheduler/src/config.rs:97`](../../crates/flui-scheduler/src/config.rs) — `set_time_dilation(value: f64) -> Result<(), InvalidTimeDilation>` returning `Err` on non-positive / non-finite.
- Modify: callers of these three constructors (search via `rg 'VsyncScheduler::new\|FrameDuration::from_fps\|set_time_dilation' crates/`). Update to propagate `Result` via `?` or `unwrap_or_else(|_| Default::default())`.

**Approach:** Standard `Result`-conversion. Use `thiserror::Error` derive for new error types. Per CLAUDE.md Constitution Principle 6.

**Patterns:** *Programming Rust* 2nd ed Ch. 7 "Error Handling" + thiserror conventions.

**Test scenarios:** Covers AE17. Each constructor's `Err` branch exercised: `assert!(VsyncScheduler::new(0).is_err());`.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'assert!.*target_fps\|assert!.*fps\|assert!.*positive\|assert!.*time_dilation' crates/flui-scheduler/src/` returns zero.

---

### U8. `Microseconds(i64)` → `Microseconds(u64)`

**Goal:** Eliminate `to_std_duration` panic on negative; tighten type shape.

**Requirements:** R6.

**Dependencies:** None.

**Files:**
- Modify: [`crates/flui-scheduler/src/duration.rs`](../../crates/flui-scheduler/src/duration.rs) — `pub struct Microseconds(i64)` → `pub struct Microseconds(u64)`. Update arithmetic methods (Add/Sub via `saturating_sub` for previously-negative semantics). Update `From<Milliseconds>` / `From<Seconds>` arithmetic. Remove `to_std_duration` negative-value panic branch.
- Modify: any caller doing `as i64` cast → `as u64`. Likely scheduler.rs ticker.rs.

**Approach:** Per CLAUDE.md Constitution Principle 6 + Rust idiomatic — duration is monotonically non-negative. `saturating_sub` for differences that might underflow.

**Patterns:** *Programming Rust* 2nd ed Ch. 3 "Fundamental Types" + std::time::Duration parity (u64-backed nanoseconds).

**Test scenarios:** `let a = Microseconds(100); let b = Microseconds(200); assert_eq!(b.saturating_sub(a), Microseconds(100));` + `assert_eq!(a.saturating_sub(b), Microseconds(0))`.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U9. `PointerId` widening to `ui_events::pointer::PointerId`

**Goal:** Single canonical `PointerId` from `ui_events` crate, eliminate per-event `DefaultHasher` allocation.

**Requirements:** R7.

**Dependencies:** None (independent — ui_events::pointer::PointerId already imported in 14+ sites).

**Files:**
- Modify: [`crates/flui-interaction/src/ids.rs`](../../crates/flui-interaction/src/ids.rs) — delete local `pub struct PointerId(i32);` newtype; replace with `pub use ui_events::pointer::PointerId;` re-export.
- Modify: [`crates/flui-interaction/src/events.rs:671`](../../crates/flui-interaction/src/events.rs) — delete `extract_pointer_id` function (no longer needs DefaultHasher; ui_events PointerId is the source).
- Modify: every callsite that constructed `PointerId(i32_literal)` → use `ui_events::pointer::PointerId` constructor. ~15-20 sites in flui-interaction (arena.rs/recognizers/*.rs/routing/*.rs/binding.rs/testing/*.rs).
- Modify: [`crates/flui-interaction/src/lib.rs`](../../crates/flui-interaction/src/lib.rs) — update re-exports to ui_events PointerId.

**Approach:** Mechanical sweep via `rg 'PointerId\b' crates/flui-interaction/`. One atomic commit despite ~20-site ripple (per brainstorm Key Decision — splitting per-site adds 20 commits for no review benefit).

**Patterns:** Flutter `gestures/events.dart:PointerEvent.pointer` is `int`; ui_events crate provides W3C-compliant typed wrapper.

**Test scenarios:** Covers AE13. `let _ = ui_events::pointer::PointerId::new(42); rg 'pub struct PointerId|pub type PointerId' crates/flui-interaction/` returns zero (only re-export remains).

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'extract_pointer_id\|DefaultHasher' crates/flui-interaction/` returns zero.

---

### U10. `Ticker::dispose` + `Drop` + `disposed: AtomicBool` + `started-twice` debug-assert

**Goal:** Adopt PR #84's `ChangeNotifier::dispose` pattern on `Ticker` + `TickerGroup`.

**Requirements:** R8, R9.

**Dependencies:** U1 (post-zero-dep-cleanup), U6 (post-`SchedulerPhase` TryFrom).

**Files:**
- Modify: [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs) — add `disposed: AtomicBool` field on `Ticker`. Add `pub fn dispose(&mut self)` that idempotent-cancels TickerFuture + sets disposed. Add `impl Drop for Ticker` that calls `dispose` if not already. Add `fn assert_not_disposed(&self, op: &'static str)` helper. Wire to `start`/`stop`/`mute`/`unmute`/`schedule_tick`. Add `debug_assert!` in `start` for `state != Active` matching Flutter "started twice".
- Modify: `TickerGroup` (in same file) — same pattern.
- Modify: tests in same file to cover dispose + Drop + use-after-dispose + started-twice scenarios.

**Approach:** Direct mirror of [`crates/flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs) `ChangeNotifier::dispose`. AtomicBool with Acquire/Release ordering per Gjengset *Rust Atomics and Locks*.

**Patterns:** Flutter `ticker.dart:362-379` `dispose()` `@mustCallSuper`; `ticker.dart:188` `throw FlutterError('A ticker was started twice')`. PR #84 commit `eb95c2f2` template.

**Test scenarios:** Covers AE1, AE2. Multiple tests:
```rust
#[test]
fn ticker_use_after_dispose_panics_debug() {
    let mut ticker = ...;
    ticker.dispose();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ticker.start();
    }));
    assert!(result.is_err());
}

#[test]
fn ticker_drop_cancels_future() {
    let mut ticker = ...;
    let fut = ticker.start();
    drop(ticker);
    // fut should resolve to TickerCanceled
}

#[test]
fn ticker_started_twice_panics_debug() {
    let mut ticker = ...;
    ticker.start();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ticker.start();
    }));
    assert!(result.is_err());
}
```

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U11. `Recognizer::dispose` with arena + router + timer cleanup

**Goal:** All 7 concrete recognizers + their base traits clean up arena entries + router routes + timer service on dispose.

**Requirements:** R10.

**Dependencies:** U5 (canonical GestureRecognizerState), U9 (PointerId widening — recognizer holds Vec<PointerId>).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/recognizer.rs`](../../crates/flui-interaction/src/recognizers/recognizer.rs) — base `GestureRecognizer` trait: add `fn dispose(&mut self)` default impl that walks `self.tracked_pointers()` calling `arena.resolve(pid, Rejected)` + `pointer_router.remove_route(pid, ...)` + `timer_service.cancel_all(...)`. Add `disposed: AtomicBool` to base via composition (`pub struct RecognizerBase { disposed: AtomicBool, ... }`).
- Modify: each of the 7 concrete recognizers ([`tap.rs`](../../crates/flui-interaction/src/recognizers/tap.rs) / [`double_tap.rs`](../../crates/flui-interaction/src/recognizers/double_tap.rs) / [`long_press.rs`](../../crates/flui-interaction/src/recognizers/long_press.rs) / [`drag.rs`](../../crates/flui-interaction/src/recognizers/drag.rs) / [`multi_tap.rs`](../../crates/flui-interaction/src/recognizers/multi_tap.rs) / [`scale.rs`](../../crates/flui-interaction/src/recognizers/scale.rs) / [`force_press.rs`](../../crates/flui-interaction/src/recognizers/force_press.rs)) — add `Drop` impl calling base `dispose`. Add `debug_assert!(!self.is_disposed())` to public methods.
- Modify: arena.rs — ensure `GestureArena::resolve(pid, Rejected)` is idempotent on already-removed entry (no panic on double-reject).
- Modify: pointer_router.rs — ensure `remove_route` is safe for unregistered handlers (no panic).

**Approach:** Shared `RecognizerBase` composition pattern. Per Flutter `recognizer.dart:485-493` `dispose()` calls `_team?._dispose()` + clears `_entries` Map. FLUI analog cleans arena + router + timer.

**Patterns:** PR #84 ChangeNotifier::dispose. Flutter recognizer.dart:485-493.

**Test scenarios:** Covers AE5, AE6. Test scenarios across recognizers — verify arena state + router state post-dispose.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U12. `FocusManager` + `GestureBinding` disposed-state pattern

**Goal:** Uniform PR #84 dispose adoption across remaining lifecycle types in flui-interaction.

**Requirements:** R11.

**Dependencies:** U10 (dispose pattern established), U11 (recognizer dispose).

**Files:**
- Modify: [`crates/flui-interaction/src/routing/focus.rs`](../../crates/flui-interaction/src/routing/focus.rs) — add `disposed: AtomicBool` to `FocusManager`. `dispose(&mut self)` + `Drop` + `debug_assert!` in `request_focus`/`focus_next`/`focus_previous`/`unfocus`. Idempotent.
- Modify: [`crates/flui-interaction/src/binding.rs`](../../crates/flui-interaction/src/binding.rs) — `GestureBinding::dispose` semantics. Plus `hit_tests` DashMap drain on `Cancel` per-pointer (Flutter `binding.dart:_handlePointerEventImmediately` pointer-cancel branch).

**Approach:** Same shape as U10/U11.

**Test scenarios:** Covers AE4. FocusManager-disposed-then-request-focus panics-in-debug.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U13. Re-introduce `OneSequenceGestureRecognizer` + `PrimaryPointerGestureRecognizer` as canonical traits

**Goal:** Canonical Flutter trait hierarchy `GestureRecognizer ← OneSequence ← PrimaryPointer` ready for recognizer migration in U16-U22.

**Requirements:** R14 (trait infrastructure half — migration half is U16-U22).

**Dependencies:** U3 (old scaffolds deleted), U5 (canonical FSM enum), U9 (PointerId widened), U11 (Recognizer::dispose hooks ready).

**Files:**
- Create: [`crates/flui-interaction/src/recognizers/one_sequence.rs`](../../crates/flui-interaction/src/recognizers/one_sequence.rs) — proper trait per High-Level Technical Design sketch above. ~150-200 LOC.
- Create: [`crates/flui-interaction/src/recognizers/primary_pointer.rs`](../../crates/flui-interaction/src/recognizers/primary_pointer.rs) — proper trait. ~100-150 LOC.
- Modify: [`crates/flui-interaction/src/recognizers/mod.rs`](../../crates/flui-interaction/src/recognizers/mod.rs) — re-add `pub mod one_sequence;` + `pub mod primary_pointer;` + re-exports.
- Modify: [`crates/flui-interaction/src/lib.rs`](../../crates/flui-interaction/src/lib.rs) — add re-exports.

**Approach:** New traits with default-impl methods where possible (Flutter base classes are abstract — Rust traits can't be exactly abstract, but `fn add_pointer` default-impl can call back to `self.do_add_pointer_internal` via required method). Compile this unit with NO concrete impls yet — that's U16-U22.

**Patterns:** Flutter `recognizer.dart:404-485` (OneSequenceGestureRecognizer) + `:611-700` (PrimaryPointerGestureRecognizer). Rust adaptation: trait with associated types if needed. *Rust for Rustaceans* Ch. 2 "Foundations" + "Sealed Traits" — sealed via `mod sealed; pub trait OneSequenceGestureRecognizer: sealed::Sealed {}` so external crates can't impl (but FLUI internal can).

**Test scenarios:** Covers AE12 (compile-time assert framework prep — actual `static_assertions::assert_impl_all!` lands in U16-U22).

**Verification:** R-V1+R-V2+R-V3+R-V4. Traits compile, no broken downstream because no impls yet.

---

### U14. `TickerProvider::create_ticker` Flutter-factory reshape

**Goal:** Replace `schedule_tick(callback)` one-shot with Flutter's `create_ticker(callback) -> Ticker` factory.

**Requirements:** R12.

**Dependencies:** U10 (Ticker::dispose ready).

**Files:**
- Modify: [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs) — trait `TickerProvider` signature change. Old `schedule_tick(callback)` deleted; new `create_ticker(on_tick: TickerCallback) -> Ticker`.
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) — `impl TickerProvider for Scheduler` constructs `Ticker::new(on_tick, scheduler_arc)`.

**Approach:** Per High-Level Technical Design sketch. Trait stays object-safe via `&self` receiver + owned return. `TickerCallback = Box<dyn FnMut(Duration) + Send + 'static>` (`FnMut`, not `FnOnce` — ticker fires repeatedly).

**Patterns:** Flutter `ticker.dart:248` `Ticker createTicker(TickerCallback)` factory. Rust idiomatic builder/factory pattern (*Programming Rust* 2nd ed Ch. 9 "Structs"). PR #82 ClipContext factory pattern as reference.

**Test scenarios:** Covers AE20. Doctest:
```rust
/// ```
/// use flui_scheduler::{Scheduler, TickerProvider};
/// let scheduler = Scheduler::new();
/// let mut ticker = scheduler.create_ticker(Box::new(|elapsed| {
///     // do animation work
/// }));
/// ticker.start();
/// // ...
/// ticker.dispose();
/// ```
```

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U15. Absorb `ScheduledTicker` into `Ticker`

**Goal:** Single canonical `Ticker` type; auto-rescheduling built in via Scheduler persistent-frame-callback registration.

**Requirements:** R13.

**Dependencies:** U10 (Ticker::dispose ready), U14 (create_ticker factory).

**Files:**
- Modify: [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs) — `Ticker::start` registers `self` as a persistent-frame-callback consumer via `scheduler.add_persistent_ticker(arc_self)`. `Ticker::stop` unregisters. Per-tick `Box<dyn FnOnce>` allocation eliminated; the callback closure stored on Ticker itself and called from Scheduler's persistent-callback drain.
- Delete: `pub struct ScheduledTicker` + `pub use ticker::{ScheduledTicker, ...}` (~400 LOC).
- Modify: [`crates/flui-scheduler/src/lib.rs`](../../crates/flui-scheduler/src/lib.rs) — remove `ScheduledTicker` re-export.
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) — add `persistent_tickers: parking_lot::Mutex<Vec<Arc<Ticker>>>` (or similar) + drain in `handle_draw_frame`. Skip if `frames_enabled == false`.

**Approach:** Hot-path allocation elimination per *Rust Performance Book* "Heap Allocations". Ticker's `tick` becomes a method call on Scheduler-held `Arc<Ticker>` instead of a fresh `Box<dyn FnOnce>` per tick.

**Patterns:** Flutter `ticker.dart` single-Ticker model. Persistent registration pattern from Flutter binding.

**Test scenarios:** Covers AE20. Test that `Ticker::start` + `Ticker::stop` register/unregister with Scheduler. Test that ticking happens at frame rate post-start.

**Verification:** R-V1+R-V2+R-V3+R-V4. `rg 'ScheduledTicker' crates/` returns zero (except flui-animation source which U24 migrates).

---

### U16. Migrate `TapGestureRecognizer` to `PrimaryPointerGestureRecognizer`

**Goal:** First concrete recognizer migration; sets pattern for U17-U22.

**Requirements:** R14 (Tap mapping per Flutter `tap.dart:202` `BaseTapGestureRecognizer extends PrimaryPointerGestureRecognizer`).

**Dependencies:** U13 (PrimaryPointer trait exists), U5 (canonical FSM), U11 (dispose hooks).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/tap.rs`](../../crates/flui-interaction/src/recognizers/tap.rs) — add `impl PrimaryPointerGestureRecognizer for TapGestureRecognizer`. Remove ad-hoc state struct; use canonical GestureRecognizerState. ~150-200 LOC delta.

**Approach:** Per Flutter `tap.dart:202` — `BaseTapGestureRecognizer extends PrimaryPointerGestureRecognizer` overrides `handlePrimaryPointer(event)`, `didExceedDeadline()`. FLUI port adapts to the new trait surface.

**Patterns:** Flutter `tap.dart` line-by-line behavior parity (audit Part III tap.dart cross-ref confirms current FLUI behavior matches modulo on_tap_down dispatch order — fixed in U32).

**Test scenarios:** Existing tap tests pass post-migration. Add `static_assertions::assert_impl_all!(TapGestureRecognizer: PrimaryPointerGestureRecognizer)`.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U17. Migrate `LongPressGestureRecognizer` to `PrimaryPointerGestureRecognizer`

**Goal:** Second recognizer migration matching Flutter `long_press.dart:262`.

**Requirements:** R14.

**Dependencies:** U16 (pattern established).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/long_press.rs`](../../crates/flui-interaction/src/recognizers/long_press.rs) — same as U16 pattern.

**Approach:** Per Flutter `long_press.dart:262` `LongPressGestureRecognizer extends PrimaryPointerGestureRecognizer`.

**Test scenarios:** Existing long-press tests + assert_impl_all.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U18. Migrate `DragGestureRecognizer` to `OneSequenceGestureRecognizer`

**Goal:** Drag migration. Flutter `monodrag.dart:81` `sealed class DragGestureRecognizer extends OneSequenceGestureRecognizer`.

**Requirements:** R14.

**Dependencies:** U13.

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/drag.rs`](../../crates/flui-interaction/src/recognizers/drag.rs) — `impl OneSequenceGestureRecognizer for DragGestureRecognizer`. ~200-250 LOC delta.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U19. Migrate `ScaleGestureRecognizer` to `OneSequenceGestureRecognizer`

**Goal:** Scale migration. Flutter `scale.dart:345` `ScaleGestureRecognizer extends OneSequenceGestureRecognizer`.

**Requirements:** R14.

**Dependencies:** U13.

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/scale.rs`](../../crates/flui-interaction/src/recognizers/scale.rs).

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U20. Migrate `ForcePressGestureRecognizer` to `OneSequenceGestureRecognizer`

**Goal:** ForcePress migration. Flutter `force_press.dart:117` `ForcePressGestureRecognizer extends OneSequenceGestureRecognizer`.

**Requirements:** R14.

**Dependencies:** U13.

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/force_press.rs`](../../crates/flui-interaction/src/recognizers/force_press.rs).

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U21. Migrate `DoubleTapGestureRecognizer` to `GestureRecognizer` base

**Goal:** DoubleTap stays at base `GestureRecognizer` per Flutter `multitap.dart` inheritance (does NOT extend OneSequence in Flutter — handles multi-pointer differently).

**Requirements:** R14.

**Dependencies:** U13 (GestureRecognizer base ready), U5 (FSM enum).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/double_tap.rs`](../../crates/flui-interaction/src/recognizers/double_tap.rs) — `impl GestureRecognizer for DoubleTapGestureRecognizer` directly. Ensure dispose cleanup wires via base trait.

**Approach:** Verify Flutter inheritance — `multitap.dart` `DoubleTapGestureRecognizer extends GestureRecognizer`. Match.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U22. Migrate `MultiTapGestureRecognizer` to `GestureRecognizer` base

**Goal:** MultiTap stays at base per Flutter `multitap.dart`.

**Requirements:** R14.

**Dependencies:** U13, U21 (parallel multitap pattern established).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/multi_tap.rs`](../../crates/flui-interaction/src/recognizers/multi_tap.rs).

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U23. Unify `FocusManager` + `FocusManagerInner`

**Goal:** Single `FocusManager` fronting the working `FocusScopeNode` tree machinery; Tab navigation works.

**Requirements:** R15.

**Dependencies:** U12 (FocusManager disposed-state hooks).

**Files:**
- Modify: [`crates/flui-interaction/src/routing/focus.rs`](../../crates/flui-interaction/src/routing/focus.rs) — `FocusManager::focus_next` delegates to active FocusScopeNode::focus_next_in_scope. `focus_previous`/`focus_first`/`focus_last` follow same pattern. Remove `tracing::warn!("not yet implemented")` stubs.
- Modify: [`crates/flui-interaction/src/routing/focus_scope.rs`](../../crates/flui-interaction/src/routing/focus_scope.rs) — make `FocusManagerInner` impl details available via `pub(crate)` or merge into `FocusManager`. Decide based on cyclic-dependency check; if `FocusScopeNode` already references `FocusManager` then merging is cleaner.

**Approach:** Audit Part I Finding I-4 confirms `FocusScopeNode::focus_next_in_scope` at [`focus_scope.rs:663`](../../crates/flui-interaction/src/routing/focus_scope.rs) is fully implemented. Wire FocusManager's public API to call it on the active scope.

**Patterns:** Flutter `widgets/focus_manager.dart` (in `flutter/lib/src/widgets/`, not gestures — but logically same area). Active scope is the `FocusManager::current_scope`.

**Test scenarios:** Covers AE3.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U24. flui-animation source migration (ScheduledTicker → Ticker)

**Goal:** Migrate disabled `flui-animation/src/` to import `Ticker` instead of `ScheduledTicker` post-U15 absorption.

**Requirements:** R26.

**Dependencies:** U14, U15 (Ticker factory + ScheduledTicker absorbed).

**Files:**
- Modify: [`crates/flui-animation/src/controller.rs:8`](../../crates/flui-animation/src/controller.rs) — `use flui_scheduler::{ScheduledTicker, Scheduler}` → `use flui_scheduler::{Ticker, Scheduler}`. Update callsites where `ScheduledTicker::new(...)` → `Ticker::new(...)`.
- Modify: ~12 other flui-animation files importing scheduler types (verified in research phase via grep).
- DO NOT enable flui-animation in `Cargo.toml`. Manually `cargo build -p flui-animation` smoke-test to verify clean.

**Approach:** Mechanical sweep across `crates/flui-animation/`. Single atomic commit.

**Patterns:** ABI-shape regression pattern from PR #82 (`cargo build -p flui-hot-reload --features app-plugin --all-targets` smoke test).

**Test scenarios:** Covers AE16. Manual smoke test: temporarily uncomment `"crates/flui-animation"` in workspace members, run `cargo build -p flui-animation`, verify clean, re-comment.

**Verification:** R-V1+R-V2+R-V3+R-V4 plus R-V5 (`cargo build -p flui-animation` smoke clean).

---

### U25. `extract_pointer_id` alloc removal + `PointerRouter` Map O(1) + dispatch order

**Goal:** Hot-path fix — eliminate per-event hasher allocation; O(1) route lookup; Flutter dispatch order.

**Requirements:** R16, R17.

**Dependencies:** U9 (PointerId widened — ui_events::pointer::PointerId is the natural map key).

**Files:**
- Already done in U9: `extract_pointer_id` removed.
- Modify: [`crates/flui-interaction/src/routing/pointer_router.rs`](../../crates/flui-interaction/src/routing/pointer_router.rs) — `routes: HashMap<PointerId, Vec<RouteHandler>>` (O(1) per-pointer lookup). `route(event)`: take single `read()` lock, dispatch per-pointer handlers first, then global handlers. Match Flutter `pointer_router.dart:124` ordering.

**Approach:** Per Gjengset *Rust Atomics and Locks* — fewer lock acquisitions = lower contention. Single lock per dispatch.

**Test scenarios:** Covers AE7.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U26. Atomic state fields (`Ticker::state AtomicU8`, `TaskQueue::len/is_empty AtomicUsize`)

**Goal:** Lock-free trivial reads on hot single-field accessors.

**Requirements:** R18.

**Dependencies:** U10 (Ticker::dispose + state field already touched).

**Files:**
- Modify: [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs) — `state: AtomicU8` (single-byte FSM encoding: Inactive=0, Active=1, Muted=2, Disposed=3). `is_muted()`/`is_active()`/`state()` use `load(Acquire)`. Mutations use `store(Release)`.
- Modify: [`crates/flui-scheduler/src/task.rs`](../../crates/flui-scheduler/src/task.rs) — `len: AtomicUsize` mirrors inner BinaryHeap size, write-through on push/drain. `len()`/`is_empty()` use `load(Acquire)`. `count_by_priority` keeps lock (multi-field traversal).

**Patterns:** Gjengset *Rust Atomics and Locks* Ch. 2-3. Acquire/Release ordering for state visibility without full SeqCst overhead.

**Test scenarios:** Concurrent test: spawn 8 threads doing `ticker.is_active()` reads + 1 thread mutating; verify no lock contention via Loom (deferred — Outstanding Refactor per memo).

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U27. `MouseTracker::update_all_devices` implementation

**Goal:** Re-hit-test devices on dirty mark; emit PointerEnter/Exit for region changes.

**Requirements:** R19.

**Dependencies:** U23 (FocusManager unified — defensive sweep target stable).

**Files:**
- Modify: [`crates/flui-interaction/src/mouse_tracker.rs`](../../crates/flui-interaction/src/mouse_tracker.rs) — replace `tracing::trace!("update_all_devices called")` no-op with actual re-hit-test. Walk known devices' last positions, perform `EventRouter::hit_test(position)`, compare to last hit-set, emit PointerEnter for new regions + PointerExit for left regions.

**Approach:** Per Flutter `mouse_tracker.dart:_updateAllDevices`. Walk `_lastMouseEvent` map, per-device call `_handleDeviceUpdate`.

**Patterns:** Flutter mouse_tracker.dart exact behavior parity.

**Test scenarios:** Test scenario: mouse over region A, region moves, `update_all_devices` triggered, PointerExit(A) + PointerEnter(B) fired.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U28. `GestureBinding` lifecycle GC + AppLifecycle defensive sweep

**Goal:** Drain `hit_tests` + `pending_moves` + raw-input tracking on pointer-cancel/up + on AppLifecycleState::Paused.

**Requirements:** R20.

**Dependencies:** U12 (GestureBinding disposed pattern), U11 (Recognizer dispose chain).

**Files:**
- Modify: [`crates/flui-interaction/src/binding.rs`](../../crates/flui-interaction/src/binding.rs) — `handle_pointer_event` Cancel/Up branch drains per-pointer DashMap entries. Add `handle_app_lifecycle_changed(state)` defensive sweep on Paused/Detached.

**Test scenarios:** Covers AE8.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U29. `testing/` feature-gate (`default = []`)

**Goal:** 1,099 LOC testing module out of release binaries.

**Requirements:** R21.

**Dependencies:** U4 (PointerEventData compat layer removed from testing imports).

**Files:**
- Modify: [`crates/flui-interaction/Cargo.toml`](../../crates/flui-interaction/Cargo.toml) — add `[features] testing = []; default = []`. (Don't make testing default.)
- Modify: [`crates/flui-interaction/src/lib.rs`](../../crates/flui-interaction/src/lib.rs) — `#[cfg(feature = "testing")] pub mod testing;` instead of unconditional `pub mod testing;`.
- Modify: any in-crate tests that use testing helpers → `#[cfg(feature = "testing")]` or move to integration tests with `dev-dependencies = { flui-interaction = { features = ["testing"] } }` in `[dev-dependencies]`.

**Approach:** Per Cargo book "Features". Zero workspace consumers per research grep.

**Patterns:** Same Cargo feature-gate pattern as flui-foundation's optional features.

**Test scenarios:** Covers AE14.

**Verification:** R-V1+R-V2+R-V3+R-V4. `cargo build -p flui-interaction --no-default-features` clean. `cargo build -p flui-interaction --features testing` clean.

---

### U30. `AppLifecycleState` auto-toggle `frames_enabled`

**Goal:** Scheduler auto-flips `frames_enabled` from lifecycle state changes.

**Requirements:** R22.

**Dependencies:** U6 (AppLifecycleState TryFrom).

**Files:**
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) — `handle_app_lifecycle_changed(state)` method. `match state { Resumed | Inactive => frames_enabled.store(true), Paused | Hidden | Detached => frames_enabled.store(false) }`. Plus mute tickers via persistent-callback skip.

**Approach:** Per Flutter [`binding.dart:414-441`](../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart).

**Test scenarios:** Covers AE10.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U31. Persistent + Post-frame strict immutability

**Goal:** Revert FLUI's removable callbacks to Flutter strict — `add_persistent_frame_callback` + `add_post_frame_callback` return `()`, can't be unregistered.

**Requirements:** R23.

**Dependencies:** U10 (Ticker dispose ready, since old API surface used CallbackId for Ticker future).

**Files:**
- Modify: [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs) — `add_persistent_frame_callback(FrameCallback) -> ()`. Delete `remove_persistent_frame_callback`. Same for `add_post_frame_callback` + delete `cancel_post_frame_callback`. CallbackId type may be deleted entirely if only used here.
- Modify: callers (search via `rg 'remove_persistent_frame_callback|cancel_post_frame_callback' crates/`). Update to not retain CallbackIds.

**Approach:** Per Flutter strict contract [`binding.dart:773,802`](../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart).

**Test scenarios:** Covers AE9.

**Verification:** R-V1+R-V2+R-V3+R-V4.

---

### U32. Tap dispatch order + Priority numeric realignment

**Goal:** Tap fires `on_tap_down` only post-arena resolution. Priority enum values 0/50000/100000/200000.

**Requirements:** R24, R25.

**Dependencies:** U16 (Tap migrated), U21+U22 (DoubleTap+MultiTap migrated for parity).

**Files:**
- Modify: [`crates/flui-interaction/src/recognizers/tap.rs`](../../crates/flui-interaction/src/recognizers/tap.rs) — defer `on_tap_down` callback fire until arena `accept` callback invocation (currently fires pre-arena). Verify other recognizers' dispatch ordering per audit Part III tap.dart drift B.
- Modify: [`crates/flui-scheduler/src/task.rs`](../../crates/flui-scheduler/src/task.rs) — `Priority` enum: `Idle = 0, Build = 50_000, Animation = 100_000, UserInput = 200_000` (explicit discriminants).

**Patterns:** Flutter `tap.dart` exact dispatch order + `priority.dart:11-54` numeric values.

**Test scenarios:** Covers AE18, AE19.

**Verification:** R-V1+R-V2+R-V3+R-V4. Plus audit doc Part IV status annotation with merge commit hash (U32 closes; commit message references audit closure).

---

## Verification Matrix

| Unit | R-V1 build | R-V2 clippy | R-V3 test | R-V4 port-check | R-V5 anim smoke |
|------|-----------|------------|-----------|----------------|-----------------|
| U1   | ✓         | ✓          | ✓         | ✓              | —               |
| U2   | ✓         | ✓          | ✓         | ✓              | —               |
| U3   | ✓         | ✓          | ✓         | ✓              | —               |
| U4   | ✓         | ✓          | ✓         | ✓              | —               |
| U5   | ✓         | ✓          | ✓         | ✓              | —               |
| U6   | ✓         | ✓          | ✓         | ✓              | —               |
| U7   | ✓         | ✓          | ✓         | ✓              | —               |
| U8   | ✓         | ✓          | ✓         | ✓              | —               |
| U9   | ✓         | ✓          | ✓         | ✓              | —               |
| U10  | ✓         | ✓          | ✓         | ✓              | —               |
| U11  | ✓         | ✓          | ✓         | ✓              | —               |
| U12  | ✓         | ✓          | ✓         | ✓              | —               |
| U13  | ✓         | ✓          | ✓         | ✓              | —               |
| U14  | ✓         | ✓          | ✓         | ✓              | —               |
| U15  | ✓         | ✓          | ✓         | ✓              | ✓ (smoke)       |
| U16-U22 | ✓ each | ✓ each   | ✓ each    | ✓ each         | —               |
| U23  | ✓         | ✓          | ✓         | ✓              | —               |
| U24  | ✓         | ✓          | ✓         | ✓              | ✓ (smoke)       |
| U25  | ✓         | ✓          | ✓         | ✓              | —               |
| U26  | ✓         | ✓          | ✓         | ✓              | —               |
| U27-U32 | ✓ each | ✓ each   | ✓ each    | ✓ each         | —               |

All commits: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace --lib` + `bash scripts/port-check.sh -v` (7/7 institutional refusal triggers). R-V5 (flui-animation manual smoke) only on U15 + U24 commits.

---

## Risks & Mitigations

- **R1: Recognizer migration (U16-U22) introduces FSM regression.** Mitigation: per-recognizer test suite stays exercised; add `static_assertions::assert_impl_all!` for trait conformance. If regression found, revert single recognizer commit (atomic per-commit shape limits blast radius).
- **R2: flui-animation smoke break (U15/U24).** Mitigation: R-V5 smoke test runs at U15 + U24 commits via manual workspace-member uncomment. If smoke fails, U15 absorption strategy revised before U16+ recognizers land.
- **R3: PointerId widening (U9) breaks downstream API.** Mitigation: per Scope Boundary "ABI breakage allowed pre-1.0". Conventional commit type `feat!`. All downstream consumers (flui-platform / flui-engine) verified via `cargo build --workspace`.
- **R4: Wave 3 dispose pattern adoption forgets a callsite.** Mitigation: comprehensive `rg 'pub fn (start|stop|mute|unmute|schedule_tick|add_pointer|handle_event|request_focus|focus_next|focus_previous|unfocus)' crates/flui-interaction crates/flui-scheduler` checklist per unit. Each touched method gets `assert_not_disposed`.
- **R5: Persistent/post-frame removability removal (U31) breaks an internal consumer.** Mitigation: pre-execute grep `rg 'remove_persistent_frame_callback|cancel_post_frame_callback' crates/` to enumerate sites; migrate each. If callsite legitimately needs removability (e.g., test cleanup), use a different mechanism (test-only `&mut Vec<FrameCallback>` reset).
- **R6: Hot-path optimization (U25, U26) introduces data race.** Mitigation: Acquire/Release ordering per Gjengset; defer Loom property tests to Outstanding Refactor (per no-quick-wins memo, this is honest deferral with concrete blocker).
- **R7: Audit doc Part IV status annotation forgotten.** Mitigation: U32 explicitly includes audit doc Part IV "Status" block edit with merge commit hash.

---

## Key Technical Decisions

- **`OneSequence` + `PrimaryPointer` traits are SEALED.** Sealed via `mod sealed; pub trait OneSequenceGestureRecognizer: sealed::Sealed` so external crates can't impl. Per Constitution Principle 4 + *Rust for Rustaceans* "Sealed Traits". FLUI's gesture-recognizer set is finite + curated.
- **Dispose pattern: composition over trait method.** `RecognizerBase` struct carries `disposed: AtomicBool`; concrete recognizers embed `base: RecognizerBase` via composition. Alternative: blanket trait `Disposable` with default impl + interior mutability — rejected per Constitution Principle 4 (no `dyn` by default).
- **Atomic ordering: Acquire/Release, not SeqCst.** Per Gjengset *Rust Atomics and Locks* Ch. 3 — Acquire/Release sufficient for single-producer / multi-reader patterns (Ticker.state / TaskQueue.len). SeqCst overhead unnecessary.
- **Ticker callback closure stored on Ticker, not boxed per-tick.** Eliminates per-frame allocation. Closure stored as `Box<dyn FnMut(Duration) + Send>` once at construction; reused every tick. Per *Rust Performance Book* "Heap Allocations".
- **`PointerRouter` route storage as `HashMap<PointerId, Vec<RouteHandler>>` not `DashMap`.** Single-threaded gesture dispatch in Flutter; FLUI matches via `RwLock<HashMap>` single-`read()`-per-dispatch. DashMap allocates per-shard; HashMap allocates once.
- **`Priority` enum stays closed.** Flutter's open-class with operator+/- has zero usages in Flutter framework code itself; FLUI's 4-variant closed enum is Rust-simpler. Numeric realignment (`Idle=0, Build=50000, Animation=100000, UserInput=200000`) preserves forward-compat path if offset arithmetic ever needed.
- **flui-animation migration source-only.** Crate stays disabled in workspace.members; migrated source ensures clean re-enable later. Per no-quick-wins memo: would-need-edits-anyway, edit now.

---

## Deferred to Implementation

These items are explicitly punted into per-unit execution time, not pre-planned in this doc:

- **§I1: Concrete `from_u8` saturating-default choice per enum.** U6 picks `SchedulerPhase::Idle` / `FrameSkipPolicy::Default` / `AppLifecycleState::Detached`. Confirm during execution against Flutter exact behavior; revise if Flutter has different fallback semantics.
- **§I2: `OneSequence` + `PrimaryPointer` trait method signatures exact shape.** U13 lands the traits; method bodies refined per concrete recognizer needs at U16-U22.
- **§I3: PointerId widening callsite list.** U9 sweeps `rg 'PointerId\b' crates/flui-interaction/` at execution time; per-site decision (keep `PointerId(literal)` constructor vs `ui_events::pointer::PointerId::new`) made then.
- **§I4: `FocusManager` + `FocusManagerInner` merge strategy.** U23 picks between (a) keep both, FocusManager delegates; (b) merge into FocusManager. Decision based on cyclic-dependency check at execution.
- **§I5: Per-recognizer ad-hoc state retention.** U5 establishes the canonical 3-state public enum; concrete recognizers retain private FSM enums if they encode richer state (e.g., DoubleTap's "waiting-for-second-tap" sub-state). Per-recognizer call.
- **§I6: testing/ feature-gate scope.** U29 decides whether all submodules go behind `testing` or just the `recording.rs` heavy-LOC parts.
- **§I7: Audit doc Part IV "Status" annotation format.** U32 picks exact format — table row with merge commit hash, or inline status block.
- **§I8: flui-animation re-enable timing.** Out of scope. Per `flui-animation` crate's own roadmap; this PR makes source compile-clean only.
