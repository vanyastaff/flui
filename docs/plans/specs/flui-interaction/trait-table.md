# flui-interaction — trait table (read-only audit, base 81f39f137)

Paths are relative to `crates/flui-interaction/src/` unless they are prefixed otherwise.
**Zone** values:
- **mine**: `recognizers/**`, `arena/**`, and the pointer vocabulary (ADR-0089, which covers `events.rs` and the vocabulary half of `traits.rs`).
- **T6d**: everything else in the crate.

**Evidence** values:
- "rustc-verified": checked with a standalone `rustc 1.99.0 --edition 2024` probe at .
- "read": taken from reading the code.

**Inventory command.** `rg -n '^\s*(pub(\(crate\))? )?trait '` returns 21 traits in `src/`, plus a test-only local `AssertSendSync` at `lib.rs:376`, which is not counted.

**Route, hover, focus and key handlers are not traits.** They are `Rc<dyn Fn..>` aliases: `PointerRouteHandler`/`GlobalPointerHandler` (pointer_router.rs:44,47), `MouseEnter/Exit/HoverCallback` (interaction_lane.rs:354-360), `CursorChangeCallback` (mouse_tracker.rs:187), `KeyEventHandler`/`RectProvider`/`FocusNodeChangeCallback` (focus_scope.rs:40-56), `FocusChangeCallback`/`KeyEventCallback` (focus.rs:21,24), `SignalCallback` (signal_resolver.rs:49), and 30 recognizer callback aliases. They are already dyn heterogeneous storage, so they are covered in §3 and not as rows.

## 1. Summary

| Verdict | Count | Traits |
|---|---|---|
| keep (dyn, add a dyn-compatibility test) | 4 | `GestureArenaMember` (reshaped), `FocusTraversalPolicy` (reshaped), `HitTestProbe`, `MultiDragHandle` (drop the `'static` supertrait) |
| keep: the extension point, unsealed, reshaped | 1 | `GestureRecognizer` |
| keep (crate-private) | 1 | `Retain` |
| collapse into composition (`RecognizerBase` helper fields) | 2 | `OneSequenceGestureRecognizer`, `PrimaryPointerGestureRecognizer` |
| collapse into one, then inherent (vocabulary P2) | 2 | `PointerEventExt` (events.rs), `PointerEventExtTrait` (traits.rs) |
| remove: fake seal | 4 | `sealed::{hit_testable, gesture_recognizer, arena_member, focus_node}::Sealed` |
| remove: rename shim / unwired abstraction | 7 | `CustomGestureRecognizer`, `CustomHitTestable`, `HitTestable`, `HitTestTarget`, `GestureCallback` (GAT), `GestureRecognizerExt`, `Disposable` |
| **total** | **21** | keep 6 (one of them crate-private), collapse 4, remove 11 |

Nothing ends up sealed. No trait in the crate has outside impls that are illegitimate:
- **Recognizer and member traits.** Outside impls are the extension point.
- **`FocusTraversalPolicy` and `MultiDragHandle`.** These are user policy and handle objects.
- **`HitTestProbe`.** Its one implementation lives in `flui-rendering` (a sibling crate), so a private seal is impossible. It stays `#[doc(hidden)]`-grade composition.
- **The `PointerEventExt` pair.** These are the only candidates for sealing. They become inherent methods on FLUI's own `PointerEvent` (ADR-0089 P2), so the trait disappears instead of being sealed.

**Associated types.**
- None are warranted today. The details types are consumed only through each concrete type's `with_on_*` builders, and there is no generic consumer.
- An `type Details` on `GestureRecognizer` would make `dyn GestureRecognizer` unusable in heterogeneous storage, because `dyn GestureRecognizer<Details = X>` cannot mix kinds.
- Errors that a recognizer can surface (disposed, owner closed) are framework-defined. They belong in a crate `thiserror` enum, not an associated type.
- No GATs are needed. The only GAT (`GestureCallback`) has zero impls, and it is what makes that trait dyn-incompatible.

## 2. Rows

### 2.1 `GestureRecognizer`: recognizers/recognizer.rs:33 [mine]

- **Impls (production, 10).**
  - tap.rs:658, double_tap.rs:559, long_press.rs:504, drag.rs:806, scale.rs:628
  - multi_tap.rs:426, force_press.rs:487, multidrag.rs:579, tap_and_drag.rs:370, eager.rs:119
- **Impls (tests, examples, benches): 0.** `examples/custom_recognizer.rs:28` implements only `CustomGestureRecognizer`, and `docs/GESTURES.md:390` shows an impl that does not compile (it uses the wrong arity for `start_tracking`, per Z1 §7). The documented extension point is never exercised.
- **Callers: static only, always on a concrete `Arc<Concrete>`.**
  - flui-widgets `interaction/gesture_detector.rs`: 909-913 (`dispose`), 1064/1074/1100/1104 (`add_pointer`), 1069/1116-1143 (`handle_event`).
  - `navigator/back_gesture.rs:306` (`add_pointer`) and :609-611 (`handle_event`).
  - `interaction/draggable.rs:1406` (`add_pointer`), :1412-1414 (`handle_event`), :1476 (`dispose`).
  - **No `dyn GestureRecognizer` anywhere in the workspace.**
- **Dyn-compatible: NO (rustc-verified).** `add_pointer(self: &Arc<Self>, ..)` (recognizer.rs:44) has a receiver that cannot be dispatched on (E0038). `self: Rc<Self>` or `&self` would be fine.
- **Verdict: keep. This is THE extension point, unsealed, and it needs reshaping and wiring.**
- **Rationale.** The owner's rule makes this the extension point. Today it is:
  - **Unwired for custom gestures.** No widget accepts a user recognizer. Every gesture widget hand-wires the same Listener → `add_pointer` → `handle_event` forwarding, in four places (Z7 §7.1).
  - **Not dyn-compatible.** A `RawGestureDetector`-style widget cannot hold a heterogeneous set.
  - **Redundant in its signature.** `add_pointer(pointer, position, global_position)` re-passes what the Down `PointerDispatch` already carries, and it loses the device kind. That loss is why `DoubleTapGestureRecognizer::add_pointer_with_kind` exists (gesture_detector.rs:1084-1097). Tap then also needs a second `handle_event(dispatch)` for the same Down (gesture_detector.rs:1064-1069).
  - **Carrying a member with no caller.** `primary_pointer()` (recognizer.rs:75) has no production caller through the trait; only tests use it (tests/interaction_lane.rs:764 and others), and every recognizer's `RecognizerBase` already answers it.
- **Before**, gesture_detector.rs:1064-1069 and 1093-1097:
  ```rust
  self.tap.add_pointer(pointer, position, global_position);
  if self.mounted.get() { self.tap.handle_event(dispatch); }
  ...
  self.double_tap.add_pointer_with_kind(pointer, position, global_position, kind);
  ```
- **After.** The recognizer registers its own identity through a `Weak<Self>` that `RecognizerBase` gets at construction (`Rc::new_cyclic`), so every method takes `&self` and the trait is dyn-compatible:
  ```rust
  pub trait GestureRecognizer: GestureArenaMember {
      fn add_pointer(&self, down: PointerDispatch<'_>);   // kind, both spaces, buttons from the event
      fn handle_event(&self, dispatch: PointerDispatch<'_>);
      fn dispose(&self);
  }
  // gesture_detector.rs
  self.tap.add_pointer(dispatch);
  self.double_tap.add_pointer(dispatch);
  ```
- **Wiring sketch, before**, back_gesture.rs:604-611 and draggable.rs:1400-1414 (the same 4-closure block):
  ```rust
  Listener::new().on_pointer_down(move |_cx, d| down.add_pointer(..))
      .on_pointer_move(move |_cx, d| mv.handle_event(d)).on_pointer_up(..).on_pointer_cancel(..)
  ```
  **After** (one flui-widgets primitive, generic or `Rc<dyn GestureRecognizer>`):
  ```rust
  Listener::new().recognizer(Rc::clone(&recognizer))
  ```
  `examples/custom_recognizer.rs` then implements `GestureRecognizer` and attaches through it.
- **Tests to add.**
  - `fn _dyn(_: &dyn GestureRecognizer) {}` in `tests/main.rs`.
  - Bench a static-vs-dyn `handle_event` case in `tap_detector_bench.rs`.

### 2.2 `GestureArenaMember`: arena/mod.rs:134 [mine]

- **Impls (production).**
  - The 10 built-in recognizers: tap.rs:852, double_tap.rs:625, long_press.rs:679, drag.rs:912, scale.rs:729, multi_tap.rs:487, force_press.rs:591, multidrag.rs:690, tap_and_drag.rs:695, eager.rs:224.
  - `CombiningMemberWrapper` at team.rs:304. Team is unwired.
  - A blanket impl for every `CustomGestureRecognizer` (mod.rs:192).
- **Impls (tests).**
  - In `src`: mod.rs:1806, 1828, 1846; team.rs:496; drag.rs:950, 987; long_press.rs:784.
  - Via the blanket: binding.rs:1673, 1686; tests/interaction_lane.rs:1094, 1245, 1306, 1320; flui-runtime `ui_realm/tests/closing_one_presentation_is_invisible_to_siblings.rs:364`; benches/gesture_arena_bench.rs:50.
- **Callers (dyn).**
  - mod.rs:1165-1166 (`dispatch_pending`), 1484-1485 (`close_owner`), 1603 (`poll_deadline`), 1626 (`has_pending_deadline`), 1648 (`next_deadline`).
  - team.rs:88, 284, 287; drag.rs:902 calls itself.
  - Storage: `Arc<dyn GestureArenaMember>` in `ArenaEntryData.members` (mod.rs:469), `eager_winner` (480), `DeadlinePoll` (304) and team (team.rs:67,122,126,268-269,346); `Weak<dyn ..>` in `GestureArenaEntry` (271) and `DeadlineWatcher` (298).
- **Dyn-compatible: YES (read, rustc-verified on the same shape).** All methods take `&self`, there are no generics, and the defaults are fine. The supertrait `Sealed` is an empty trait.
- **Verdict: keep as THE dyn storage trait. Unseal it honestly and reshape it.**
- **Rationale.** The arena stores a heterogeneous set, which is what dyn is for.
  - **The seal is fake.** `sealed::arena_member::Sealed` is `pub` (sealed.rs:207-214; lib.rs:153), so anyone can implement it. In-crate tests and every external crate already do, through the blanket.
  - **`CustomGestureRecognizer` is a rename shim.** It drops `poll_deadline`/`has_pending_deadline`/`next_deadline` (mod.rs:192-202). A custom recognizer therefore cannot own a deadline (Z1 §7). That is a capability lost through a wrapper trait.
  - **The deadline trio is a convention, not a type.** `has_pending_deadline() ⇔ next_deadline().is_some()` is documented (mod.rs:156-183) but representable as false. Collapse it to one query.
  - **`Arc` becomes `Rc`.** The graph is `!Send` (callbacks are `Rc<RefCell>`), so `Arc` only buys atomics. This is my zone, because the arena is excluded from T6d.
- **Before**, mod.rs:134-184 plus sealed.rs:82-94:
  ```rust
  pub trait GestureArenaMember: sealed::arena_member::Sealed { fn accept_gesture(&self, p: PointerId);
      fn reject_gesture(&self, p: PointerId); fn poll_deadline(&self) {}
      fn has_pending_deadline(&self) -> bool { false } fn next_deadline(&self) -> Option<Instant> { None } }
  pub trait CustomGestureRecognizer { fn on_arena_accept(..); fn on_arena_reject(..); }
  impl<T: CustomGestureRecognizer> GestureArenaMember for T { .. }   // deadlines lost
  ```
- **After:**
  ```rust
  pub trait GestureArenaMember {                     // unsealed: implementing it is legitimate
      fn accept_gesture(&self, pointer: PointerId);
      fn reject_gesture(&self, pointer: PointerId);
      fn deadline(&self) -> Option<Instant> { None }  // has_pending == deadline().is_some(), by construction
      fn poll_deadline(&self, now: Instant) {}        // the arena passes the clock reading it already took
  }
  ```
  The real call site, flui-runtime `ui_realm/frame_clock.rs:461` (`presentation.gestures().next_deadline()`), is unchanged. `GestureBinding::next_deadline` (binding.rs:1015) keeps its name and folds `deadline()`. flui-runtime `closing_one_presentation_is_invisible_to_siblings.rs:364` changes `impl CustomGestureRecognizer { on_arena_accept/on_arena_reject }` to `impl GestureArenaMember { accept_gesture/reject_gesture }`.
- **Tests to add.**
  - `fn _dyn(_: &dyn GestureArenaMember) {}` in `tests/main.rs`.
  - A static-vs-dyn dispatch case in `gesture_arena_bench.rs`. Today every case already goes through `Arc<dyn>`, via the blanket at benches/gesture_arena_bench.rs:50, so there is no baseline.

### 2.3 `CustomGestureRecognizer`: sealed.rs:82 [mine]

- **Impls (production): 0.**
- **Impls (examples, benches, tests).** examples/custom_recognizer.rs:28; benches/gesture_arena_bench.rs:50; binding.rs:1673, 1686; tests/interaction_lane.rs:1094, 1245, 1306, 1320; flui-runtime test :364.
- **Callers.** Only through the blanket (mod.rs:192-202). The facade re-exports it (src/interaction.rs:27).
- **Dyn-compatible:** yes (read). Irrelevant, since it is never used as dyn.
- **Verdict: remove.**
- **Rationale.** It is a one-to-one rename of `accept_gesture`/`reject_gesture` that loses the deadline methods, and it is the reason a "sealed" trait is implementable from outside. Its doc examples still show `Arc::new` together with the 3-argument API (sealed.rs:18-38).
- **Before/after.** See 2.2. The real site is the flui-runtime test at :364. The example and bench switch to `impl GestureArenaMember` (bench) and `impl GestureRecognizer` (example).

### 2.4 `OneSequenceGestureRecognizer`: recognizers/one_sequence.rs:23 [mine]

- **Impls (production, 7).** drag.rs:888, eager.rs:187, force_press.rs:566, long_press.rs:617, scale.rs:701, tap.rs:808, tap_and_drag.rs:660.
- **Callers.**
  - No production caller.
  - Its own default `resolve` (one_sequence.rs:34-37).
  - One test calls `stop_tracking_pointer` on a concrete `DragGestureRecognizer` (tests/interaction_lane.rs:1270).
  - `rg 'dyn OneSequence|: OneSequence|impl OneSequence'` finds no generic or dyn use.
- **Dyn-compatible:** yes (read). The default `resolve` calls only `&self` methods.
- **Verdict: collapse into composition. Delete the trait and keep the useful bits as `RecognizerBase` helper fields.**
- **Rationale.**
  - This is the Flutter supertrait chain the owner rules out (OneSequence → PrimaryPointer → concrete). It is supertrait-only, with no consumer of the abstraction.
  - Its docs carry process markers ("migration wave", "commit f10d21b5", "Constitution Principle 4", one_sequence.rs:1-10, 21-22).
  - `tracked_pointers() -> Vec<PointerId>` allocates for a one-pointer recognizer.
  - The one-sequence-at-a-time invariant it names is not enforced anywhere. `start_tracking` overwrites the previous sequence silently (Z1 I19, defects 1-2). That should be a typed guard in `RecognizerBase`, not a trait.
- **Before**, tests/interaction_lane.rs:1270:
  ```rust
  recognizer.stop_tracking_pointer(PointerId::PRIMARY);
  ```
- **After:** the test drives the public path, a Cancel dispatch, or an inherent `DragGestureRecognizer::cancel_pointer`. A `RecognizerBase::begin_sequence(pointer) -> Result<SequenceGuard, SequenceBusy>` field-level helper then enforces one sequence, in place of `tracked_pointers`/`stop_tracking_pointer`.

### 2.5 `PrimaryPointerGestureRecognizer`: recognizers/primary_pointer.rs:24 [mine]

- **Impls (production, 3).** tap.rs:840, long_press.rs:642, eager.rs:211.
- **Callers.** None. `deadline()` and `did_exceed_deadline()` are never invoked (rg). Long-press deadlines go through `GestureArenaMember::poll_deadline` instead.
- **Dyn-compatible:** yes (read).
- **Verdict: collapse into composition.**
- **Rationale.**
  - The owner's rule says primary pointer, deadline and slop are a helper field, not a supertrait.
  - `RecognizerBase` already owns the primary pointer and the initial contact (recognizer.rs:90-121).
  - The deadline lives per recognizer, as duplicated `deadline_registration` and timer code in long_press and double_tap.
  - Slop is re-implemented per file, with kind-awareness missing in Tap, DoubleTap and LongPress (Z1 §6).
- **Before:** long_press.rs:642-678 implements `deadline()`/`did_exceed_deadline()`, which nobody calls, while the real deadline is `poll_deadline` (long_press.rs:679+).
- **After:**
  ```rust
  pub struct RecognizerBase { .., deadline: DeadlineSlot /* Option<Instant> + registration */, slop: SlopGate /* kind-aware */ }
  // LongPress: self.state.deadline.arm(now + settings.long_press_timeout); GestureArenaMember::deadline() => self.state.deadline.get()
  ```

### 2.6 `MultiDragHandle`: recognizers/multidrag.rs:95 [mine]

- **Impls.** Production: flui-widgets `interaction/draggable.rs:1118` (`DragSession`). Tests: multidrag.rs:723, 736.
- **Callers (dyn).** multidrag.rs:424, 485, 514, 545, 568, 658. Storage is `Rc<dyn MultiDragHandle>` (149). The factory returns `Box<dyn MultiDragHandle>` (122).
- **Dyn-compatible: YES** (read). All methods take `&self`, with no generics.
- **Verdict: keep (dyn). Remove the redundant `: 'static` supertrait and return `Rc` from the factory.**
- **Rationale.**
  - It is a legitimate user object: one per pointer, heterogeneous.
  - `'static` adds nothing, because the default object lifetime of `Box<dyn ..>`/`Rc<dyn ..>` is already `'static`.
  - `Rc::from(handle)` at multidrag.rs:479 reallocates the box on every drag start.
- **Before**, draggable.rs:1350:
  ```rust
  }) as Box<dyn MultiDragHandle>)
  ```
- **After:**
  ```rust
  }) as Rc<dyn MultiDragHandle>)
  ```
  The `MultiDragStartCallback` alias changes to `-> Option<Rc<dyn MultiDragHandle>>`.
- **Tests to add:** a dyn-compatibility line in `tests/main.rs`.

### 2.7 `sealed::arena_member::Sealed`: sealed.rs:211 [mine]

- **Impls.**
  - Production: the 10 built-ins (sealed.rs:217-226), team.rs:302, and a blanket over `CustomGestureRecognizer` (214).
  - Tests: mod.rs:1787, 1826, 1844; team.rs:480; drag.rs:948, 985; long_press.rs:783.
- **Callers.** Supertrait only.
- **Dyn-compatible:** yes.
- **Verdict: remove.** The seal is fake: `pub mod sealed` + `pub trait Sealed` (lib.rs:153). The docs claim otherwise (lib.rs:19-21; ARCHITECTURE.md "Sealed extension traits").
- **Before/after.** Delete the supertrait from `GestureArenaMember` (see 2.2).

### 2.8 `sealed::gesture_recognizer::Sealed`: sealed.rs:183 [mine]

- **Impls.** 10 built-ins (191-200) and a blanket over `CustomGestureRecognizer` (186).
- **Callers.** Supertrait of `OneSequence`/`PrimaryPointer` only (one_sequence.rs:23, primary_pointer.rs:25). It is not a supertrait of `GestureRecognizer`.
- **Verdict: remove** together with 2.4 and 2.5.

### 2.9 `sealed::hit_testable::Sealed`: sealed.rs:169 [T6d]

- **Impls.** A blanket over `CustomHitTestable` (172). There are 0 concrete impls.
- **Verdict: remove** with 2.10 and 2.11.

### 2.10 `HitTestable`: routing/hit_test.rs:790 [T6d]

- **Impls.** Production: a blanket over `CustomHitTestable` only (800), and `CustomHitTestable` has 0 impls. README.md:38 shows a direct impl that cannot compile.
- **Callers.** `routing/event_router.rs:75, 90, 182, 202` (`&mut dyn HitTestable`). `EventRouter` is unwired (Z2 §1). flui-rendering mentions `HitTestable` only in a doc comment (rendering `binding/mod.rs:101`).
- **Dyn-compatible:** yes (read). It takes `&self` and has a default method. The `&mut dyn` in `EventRouter` is unnecessary, because the methods take `&self`.
- **Verdict: remove** with `EventRouter`. The production hit test is `HitTestProbe` + flui-rendering `HitTestableRender*`.
- **Before/after.** There is no real site outside `EventRouter`. Delete `routing/event_router.rs` and these three traits.

### 2.11 `CustomHitTestable`: sealed.rs:136 [T6d]

- **Impls: 0.** It carries a `: Send + Sync` supertrait, which runs against ADR-0027's owner-local direction (Z4 §6).
- **Verdict: remove.**

### 2.12 `sealed::focus_node::Sealed`: sealed.rs:234 [T6d]

- **Impls: 0. Users: 0.** Nothing names it as a supertrait.
- **Verdict: remove.** It is dead code.

### 2.13 `HitTestProbe`: routing/interaction_lane.rs:738 [T6d]

- **Impls.** Production: flui-rendering `pipeline/hit_test_probe.rs:57` (`PipelineHitTestProbe`), installed by flui-runtime `presentation.rs:603`.
- **Callers (dyn).** interaction_lane.rs:936 (`HitTestHandle::hit_test_at`). Storage: `Rc<dyn HitTestProbe>` (900, 906).
- **Dyn-compatible: YES** (read). It takes `&self`, with no generics, and returns `Result`.
- **Verdict: keep (dyn).** It is a cross-crate inversion seam (interaction must not depend upward on rendering). It cannot be privately sealed, because the impl lives in another crate.
- **Tests to add.** A dyn-compatibility line. T6d's `!Send` flip: `HitTestHandle` holds `Rc<dyn HitTestProbe>`, so it is already `!Send`; see the ownership table.
- **No change at the call site**, presentation.rs:603.

### 2.14 `FocusTraversalPolicy`: routing/focus_scope.rs:1954 [T6d; coordinate with focus-keyboard T3]

- **Impls.**
  - Production: `ReadingOrderPolicy` (1963).
  - Tests: focus.rs:1207, 1616; tests/focus_retention.rs:194; flui-widgets tests/shortcuts.rs:103, 215.
- **Callers (dyn).** focus_scope.rs:1751 (`policy.sort_descendants(&nodes)`, inside `failure.run`). Storage: `RefCell<Rc<dyn FocusTraversalPolicy>>` (1513) and `ClosedFocusNode.policy` (424).
- **Dyn-compatible: YES** (read).
- **Verdict: keep (dyn, user-implementable). Reshape the method so that a policy can only permute.**
- **Rationale.**
  - Today the policy returns a fresh `Vec<Rc<FocusNode>>`. It can add, drop or duplicate nodes, and traversal patches that up afterwards, for example with the cursor-membership check at focus_scope.rs:1742-1743.
  - An in-place permutation makes the illegal outputs unrepresentable and removes one allocation and n `Rc` clones per Tab.
  - It is also the place to fix `ReadingOrderPolicy`'s per-comparison `rect()` calls (focus_scope.rs:1976-1977; Z2 §6).
  - `set_traversal_policy` has no production caller (Z2 §1), so wiring is part of focus-keyboard.
- **Before**, focus_scope.rs:1956:
  ```rust
  fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>>;
  ```
  At flui-widgets tests/shortcuts.rs:104-109:
  ```rust
  let mut order = ReadingOrderPolicy.sort_descendants(nodes);
  ```
- **After:**
  ```rust
  fn order(&self, nodes: &mut [Rc<FocusNode>]);
  // ReadingOrderPolicy: cache rects once, then sort_by_cached_key
  ```

### 2.15 `PointerEventExt`: events.rs:458 [mine, vocabulary P2]

- **Impls.** `PointerEvent` (469), the `ui_events` type.
- **Callers.** Wired:
  - flui-widgets gesture_detector.rs:11 import, with `.pointer_id()`/`.position()` at 1057-1062.
  - back_gesture.rs:307-309; draggable.rs:1404-1410.
  - flui-rendering hit_testing re-export.
  - About 7 production files (Z4 §1).
- **Dyn-compatible:** yes, and that is irrelevant.
- **Verdict: collapse with 2.16. Then, under P2 (own pointer vocabulary), turn it into inherent methods and delete the trait.**
- **Rationale.**
  - It is an extension trait over a foreign upstream type. ADR-0089 says the Stable surface carries no upstream types.
  - Two public traits are exported under the same name (lib.rs:306 vs events.rs:458; facade `src/interaction.rs:15` vs prelude), and both define `position()`. With both in scope the call is E0034-ambiguous (Z4 defect 3).
  - `device_id()` (events.rs:487-488) masks the `PointerId` to 31 bits. That is an identity collision; identity is not a label.
- **Before**, gesture_detector.rs:11-14:
  ```rust
  use flui_interaction::{.., PointerEventExt, ..};
  let pointer = event.pointer_id();
  ```
- **After:** FLUI's own `PointerEvent::pointer_id()` and `position()` as inherent methods, with no import. `device_id()` becomes a typed `DeviceId` newtype carried by the event, not derived from it.

### 2.16 `PointerEventExtTrait`: traits.rs:84, re-exported as `PointerEventExt` at lib.rs:306 [mine, vocabulary P2]

- **Impls.** `PointerEvent` (traits.rs:107). It delegates to 2.15 and `extract_pointer_id`.
- **Callers.** No production caller resolves to this trait rather than to `events::PointerEventExt` (Z4 §3). It only sits in the prelude.
- **Verdict: remove.** Fold `is_down`/`is_up`/`is_move`/`starts_gesture`/`ends_gesture` into the inherent API from P2, or delete them, since they are equivalent to a single `matches!`.

### 2.17 `GestureCallback` (GAT): traits.rs:60 [T6d by file; recognizer-facing]

- **Impls: 0** (rg). The only "impl" is in a doc comment (traits.rs:52).
- **Callers: 0.** Re-exported at lib.rs:305 and in the facade.
- **Dyn-compatible: NO (rustc-verified, E0038 "contains generic associated type").** The doc claims the GAT allows `BoxedCallback` dynamic dispatch, but the alias `BoxedCallback<D> = Box<dyn Fn(D)>` (traits.rs:74) does not use the trait.
- **Verdict: remove** (`BoxedCallback` too). Every recognizer stores `Rc<dyn Fn(Details)>` aliases directly.

### 2.18 `GestureRecognizerExt`: traits.rs:146 [T6d by file; recognizer-facing]

- **Impls: 0.** Callers: 0 (facade re-export at src/interaction.rs:29).
- **Dyn-compatible: NO** (read). It has only associated functions without a receiver and without `where Self: Sized`.
- **Verdict: remove.** `exceeds_slop` duplicates `settings::exceeds_touch_slop` (settings.rs:595), and `primary_delta` duplicates drag maths. The kind-aware slop belongs in the `SlopGate` composition helper (2.5).

### 2.19 `Disposable`: traits.rs:188 [T6d by file]

- **Impls: 0. Callers: 0.**
- **Dyn-compatible:** yes (`&mut self`).
- **Verdict: remove.** `GestureRecognizer::dispose(&self)` is the real lifecycle. A `&mut self` dispose cannot be called on the shared `Arc`/`Rc` recognizers anyway.

### 2.20 `HitTestTarget`: traits.rs:26 [T6d]

- **Impls.** 0 concrete. There is a blanket for `Box<T>` (203). flui-rendering has its own trait with the same name; render_view.rs:504 notes that the impl was deleted.
- **Dyn-compatible:** yes. It carries `Send + Sync`, which runs against the owner-local stance.
- **Verdict: remove.**

### 2.21 `Retain` (`pub(crate)`): retain.rs:13 [T6d by file; used by arena/recognizers too]

- **Impls.**
  - Blanket impls: `Rc<T>`, `Arc<T>`, `Box<T>`, `Option<T>`, `Vec<T>`, `Owned<T>` (retain.rs:18-65).
  - Concrete impls: `MouseRegionCallbacks` (interaction_lane.rs:374), `RoutePanic` (717), `ResolvedMouseTrackerAnnotation` (mouse_tracker.rs:128), `TextInputClient` (text_input.rs:74).
- **Callers (static).** `ClosePanic::retire<T: Retain>` (__runtime.rs:301), `finish_with` (345), and the binding, arena and mouse tracker close paths.
- **Dyn-compatible: NO** (`fn retain(self)` takes `Self` by value without `Sized`), and that is fine: the use is static only.
- **Verdict: keep.**
- **Rationale.** It encodes ADR-0127 retention as a type (Rc: forget only if last; Arc: always forget).
- **Note.** After the T6d/arena `Arc`→`Rc` moves, more values hit the cheaper `Rc` arm. That is a behaviour improvement: fewer leaks on the failure path.

## 3. dyn `Fn` aliases (heterogeneous storage that is not a trait)

All of them are `Rc<dyn Fn..>`, which is correct for heterogeneous owner-local storage. Findings:

- **A `Send` callback inside an owner-local type.**
  - `HandleEventCallback = Box<dyn FnMut(PointerEvent) + Send>` (processing/resampler.rs:76) is the only `Send` callback. It forces `PointerEventResampler` to be `Send + Sync` (asserted at lib.rs:388) and to use `Arc<Mutex<ResamplerInner>>`, although each resampler lives in one `CachedPointerRoute` of an owner-local binding.
  - **Zone:** T6d by file. Drop `+ Send` and the `Send + Sync` assertion, and move it to `Rc<RefCell>`.
- **Redundant `+ 'static`** on `CursorChangeCallback` and `MouseEnter/Exit/HoverCallback` (mouse_tracker.rs:187, interaction_lane.rs:354-360). It is already the default object lifetime. Cosmetic.
- **`'static` bounds on callback parameters are justified.** All 60+ `impl Fn(..) + 'static` parameters are stored: the recognizer `with_on_*`, the lane `register_*`, `text_input.rs:94`, `clipboard.rs:45`, `raw_input.rs:271`. None of them is an invoked-but-not-stored callback (rg over `impl Fn[^)]*\)[^,]*'static`).
- **Builders mutate a shared instance.** The recognizer `with_on_*` builders take `self: Arc<Self> -> Arc<Self>` (e.g. tap.rs:313-422, drag.rs:448-483) and replace the callback inside a shared `Rc<RefCell>` under `borrow_mut`. They are setters wearing builder names: every clone of the `Arc` observes the replacement, and the old capture drops under the borrow (see ownership-table §B).
  - **Better shape:** a `TapGestureRecognizer::builder(arena).on_tap(..).build() -> Rc<Self>`, with callbacks frozen at build. Callers that need live callbacks already route through their own slots (gesture_detector.rs:715-830).
- **The dyn cost is unmeasured.**
  - `gesture_arena_bench` and `pointer_route_bench` exist, but every case already goes through dyn, so they cannot show the cost. Add a monomorphic baseline row to each:
    - arena: a static `GestureArenaMember` loop vs the `Arc<dyn>` slot dispatch;
    - route: a direct closure call vs the `Rc<HandlerCell>` snapshot plus `dyn Fn`.
  - Report the two side by side, measured in one environment.
