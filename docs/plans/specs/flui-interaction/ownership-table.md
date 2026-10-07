# flui-interaction: ownership, cycles, borrows across user code, lifetimes, generics, Send

This audit was done by reading the code at base `81f39f137`. Paths are relative to `crates/flui-interaction/src/` unless they are prefixed otherwise.

**Zone.** **mine** covers `arena/**`, `recognizers/**` and the pointer vocabulary. **T6d** covers the rest of the crate.

**Sources.** The per-site borrow tables are in  and ; both were re-read and spot-checked here.

**Temporary-scope rule.** It was checked against primary sources rather than memory: the Rust Reference `src/destructors.md` (`r[destructors.scope.temporary.enclosing]`) and the edition guide `src/rust-2024/temporary-if-let-scope.md` (fetched with `gh api`).
- **`if let` / let-chain.** Scrutinee temporaries live through the **then** block. In edition 2024 they are dropped before `else`.
- **`while let`.** The condition's temporaries live through the loop body.
- **`match`.** Scrutinee temporaries live to the end of the whole `match`.
- **Plain `if cond`.** The condition is its own temporary scope, so its temporaries drop before the body.
- **Block tail expressions.** In 2024, temporaries in a tail expression drop at the end of that tail expression.
- **`let x = a.borrow().clone();`** The `Ref` drops at the `;`.
- **Edition.** The workspace is `edition = "2024"` (Cargo.toml:176), and flui-interaction inherits it.

---

## A. Owners and teardown

| Object | Owner (strong) | Strong refs held | Weak refs held | Node unmount | Window (presentation) removal | Realm teardown | Zone |
|---|---|---|---|---|---|---|---|
| `GestureArena` (arena/mod.rs:820), a `Clone` handle of 7 `Arc`s | `GestureBinding.arena` (binding.rs:331). It is cloned into `GestureArenaScope` (flui-widgets gesture_arena_scope.rs:42) and into **every recognizer's `RecognizerBase.arena`** (recognizer.rs:89) | `entries`/`retained`: `Arc<DashMap<.., Arc<ArenaSlot>>>`. A slot holds `SmallVec<Arc<dyn GestureArenaMember>>` (mod.rs:469), plus `eager_winner` (480) | `GestureArenaEntry.{slot, member}` (mod.rs:270-271); `DeferredResolution.slot` (843); `DeadlineWatcher.member` (298); `GestureDeadlineRegistration.registry` (282) | Nothing arena-wide happens. Each recognizer's `dispose` → `RecognizerBase::reject` (recognizer.rs:372-391) removes that member from the slot | `close_gestures` (__runtime.rs:93) → `GestureBinding::close_with_mode` (binding.rs:815) → `arena.close_owner` (mod.rs:1463). That function detaches every slot, rejects members under `ClosePanic`, sets `owner_closed`, and clears `deferred` | The realm drops its presentations, and the arena drops when the last clone drops. That last clone is often a recognizer, not the binding | mine |
| Recognizers (`Arc<TapGestureRecognizer>` and the others) | The widget state: `GestureDetectorState.recognizers` (gesture_detector.rs:467-478), back_gesture.rs:557, draggable.rs:430 | `RecognizerBase.arena`. The callbacks are `Rc<RefCell<Callbacks>>` of `Rc<dyn Fn>`, and they capture **widget slots** (`Rc<RefCell<Option<GestureCallback>>>`) plus the writer (gesture_detector.rs:715-830) | `tracked_entry: GestureArenaEntry` (weak slot and member). `LongPress.deadline_registration` (long_press.rs:114) is a weak registry entry | `GestureDetectorState::dispose` (gesture_detector.rs:903-914) sets `mounted=false`, then calls `dispose()` on 5 recognizers in sequence. **No containment between them** (see panic matrix C3). The Listener's own closures (`Arc` clones) stay alive in cached routes until the terminal event | The arena is withdrawn by `close_owner`. The recognizers themselves die with the element tree (`withdraw_root_owner`, presentation.rs:1536/1598) | — | mine |
| `PointerRouter` (routing/pointer_router.rs:76) | `GestureBinding.pointer_router` (binding.rs:319), by value | `RefCell<HashMap<PointerId, Vec<Rc<dyn Fn>>>>` and the global handlers (80, 83) | none | Nothing in production registers routes (Z2 §1) | `GestureBinding::close_with_mode` → `pointer_router.close_with_mode` (binding.rs:844; pointer_router.rs:217) retires every handler under `ClosePanic` | Dropped with the binding. Final-owner `Drop` detaches through `get_mut` (ARCHITECTURE "Pointer route capture retirement") | T6d |
| `MouseTracker` (mouse_tracker.rs:183), `Clone` over `Rc<RefCell<Inner>>` | `GestureBinding.mouse_tracker` (binding.rs:322) | `annotations: HashMap<RegionId, ResolvedMouseTrackerAnnotation>` (199), each holding an `Rc<MouseRegionCell>` with 3 user `Rc<dyn Fn>`. `cursor_change_callback` (203) | The runtime's cursor callback captures `Weak<dyn PlatformWindow>` (flui-runtime presentation.rs:510) | `MouseRegion` calls `detach_mouse_region` (flui-widgets mouse_region.rs:268-274), which empties the cell, so the tracker's copy no longer owns callbacks. `unregister_mouse_region` (interaction_lane.rs:1529) does **not** empty the cell, so the tracker can become the **last owner** | `close_mouse_tracker` (presentation.rs:1556; mouse_tracker.rs:699-719) takes the callback and annotations out of the borrow, then retires them | Dropped with the binding | T6d |
| `FocusManager` (focus.rs:66), held as `Rc` | `PresentationState.focus: Rc<FocusManager>` (presentation.rs:257) and `WidgetsBinding::with_focus_manager` (presentation.rs:580) | `root_scope: Rc<FocusScopeNode>`; `primary_focus: Rc<FocusNode>` (68), which can keep a **detached** node alive (Z2 TOP-1); listeners `Rc<dyn Fn>` (69); global key handlers (73); `pending_focus_transitions: VecDeque<Option<Rc<FocusNode>>>` | `unfocused_key_claims: Vec<Weak<FocusNode>>` (77) | Widgets call `remove_listener(id)` in `dispose` (flui-widgets focus.rs:698, 866, 1210, 1220; editable_text.rs:1811-1815; flui-material text_field.rs:384/398). This is manual. **There is no RAII registration for listeners** | `close_focus` (presentation.rs:1561; focus.rs:793) commits terminal state, detaches the tree and retires per node (`ClosedFocusNode::retire`, focus_scope.rs:427) | Dropped by the last `Rc` | T6d (+ focus-keyboard T3) |
| `FocusNode` / `FocusScopeNode` (focus_scope.rs:395, 1505) | The parent's `children: Vec<Rc<FocusNode>>` (399). Widget state (`Focus`, `EditableText`) | Children; `on_key_event`, `listeners`, `rect_provider`, `context` (user `Rc<dyn ..>`, 404-410); scope `traversal_policy: Rc<dyn>` (1513) | `parent` (398), `scope_owner` (403), `manager_binding: Bound(Weak<FocusManager>)` (411), `focus_history: VecDeque<Weak<FocusNode>>` (1507). `FocusNodeRegistration { node: Weak }` is RAII (focus_scope.rs:307-313, Drop at 377) | Widget `dispose` → `remove_child`/detach. `FocusNodeRegistration` drops clear key handler, rect and context | See `FocusManager` | as above | T6d |
| `InteractionLane` (interaction_lane.rs:830) | `UiRealm.interaction_lane` (flui-runtime ui_realm/mod.rs:146), by value | `Rc<LocalLaneInner>`: 9 `RefCell<HashMap<TargetId, Rc<Cell>>>` maps (795-808), `routes: HashMap<RouteId, Rc<ResolvedHitRoute>>`, `target_owners: HashMap<TargetId, Arc<DispatchOwner>>` (796) | TLS registry `LOCAL_LANES: HashMap<LaneId, Weak<LocalLaneInner>>` (816-818). `InteractionDispatchHandle` holds only a ticket (lane id + thread) and `Arc<DispatchOwner>` (1029-1032) | `Listener` detach → `unregister_pointer`/`_scroll`/`_pan_zoom` (flui-widgets listener.rs:397, 446, 501-513) removes the cell from the map. **A cached route keeps `Rc<HandlerCell>` until Up/Cancel releases it** (documented; interaction_lane.rs:2154-2158) | `withdraw_dispatch(_in)` → `withdraw_owner_inner` (1315-1335) `take_targets` for that owner's ids. They are retired by `retire_dispatch` (presentation.rs:1576) | `Drop for InteractionLane` (956-1004) takes all maps and drops them collection by collection, **with no containment or retention** (see panic matrix) | T6d |
| `GestureBinding` (binding.rs:298) | `PresentationState.gestures` (presentation.rs:251), by value | `hit_tests: DashMap<PointerId, CachedPointerRoute>` (303): the route token, the hit path and a `PointerEventResampler` (`Arc<Mutex>`). `pending_moves: DashMap` (307). Arena, router, tracker | — | Nothing happens at the binding. Cached routes for an unmounted node keep its cells (see lane) | `close_gestures` → binding.rs:815-866: closed flag, drain `hit_tests` and `pending_moves` by sorted key, `arena.close_owner`, `pointer_router.close_with_mode`, release route tokens, retire `Owned(route)`/`Owned(event)` | Dropped with the presentation | T6d (arena part: mine) |
| `GestureArenaTeam` / `TeamEntry` (team.rs:342, 65) | Unwired (no production owner) | `combiners: DashMap<PointerId, Arc<Mutex<CombiningMember>>>`, `captain: Mutex<Option<Arc<dyn Member>>>`; `CombiningMember.members: SmallVec<Arc<dyn Member>>` | — | — | — | — | mine |
| `PointerSignalResolver` (signal_resolver.rs:100) | Unwired | `Rc<RefCell<ResolverInner>>` of `SignalCallback = Rc<dyn Fn>` | — | — | — | — | mine |

**Teardown order on window removal** (`PresentationState::close_impl`, flui-runtime presentation.rs:1487-1617):
1. Withdraw dispatch (1520-1527).
2. Withdraw keys (1533-1542).
3. Close the lifecycle source (1549-1554).
4. **Close the mouse tracker** (1556).
5. Close focus (1561).
6. Close text input (1563).
7. **Close gestures, which is the last owner close because it runs recognizer callbacks** (1568).
8. Retire the withdrawn keys (1571).
9. Retire dispatch (1576).

The order is deliberate (comment at 1564-1567) and correct. Every step runs under `PresentationCloseRecovery`, so the first failure stays authoritative.

---

## B. Cycles and how each is broken

| Cycle | Edges (strong) | Broken by | Evidence | Verdict | Zone |
|---|---|---|---|---|---|
| **arena ↔ recognizer** | slot `members: Arc<dyn Member>` (mod.rs:469) → recognizer → `RecognizerBase.arena` (recognizer.rs:89) → `Arc<DashMap>` `entries`/`retained` (mod.rs:824, 828) → `Arc<ArenaSlot>` | **Neither Weak nor RAII.** The cycle exists while a slot lists the member, and it is broken only by slot resolution: sweep or abandon from the binding, `RecognizerBase::reject` from `dispose`, or `close_owner`. `GestureArenaEntry` and the deadline registry are weak (mod.rs:270-271, 298), so they add no edge | mod.rs:1463-1493; recognizer.rs:372-391 | **Leaks when a dispose is skipped.** Example: `GestureDetectorState::dispose` disposes tap, long_press, double_tap, drag and horizontal_drag in order (gesture_detector.rs:909-913). A panic in `tap.dispose()` comes from the capture `Drop` under `borrow_mut` (tap.rs:776-790). The remaining four never reject, so their slots keep them, and they keep the arena, until a later sweep or the presentation closes. A held slot in `retained` that nobody releases lives until `close_owner`. **Fix shape:** the arena stores `Weak<dyn GestureArenaMember>` in slots, or the recognizer holds a `Weak` arena handle. Either way, `dispose` is not the only thing standing between unmount and a leak | mine |
| **recognizer ↔ widget callback ↔ state** | `GestureDetectorState` → `Arc<Recognizer>` → callbacks → `Rc<RefCell<Option<GestureCallback>>>` slot (shared with the state) → user closure | **An indirection slot plus `dispose`.** The recognizer captures the slot, not the state, so a rebuilt closure needs no recognizer rebuild. A user closure that captures its own detector's state would still close a cycle, broken only by `dispose` clearing the recognizer callbacks (tap.rs:776-790) | gesture_detector.rs:700-830, 903-914 | **OK by convention, not by type.** `dispose` clears callbacks under `borrow_mut` (defect B-1 below). A `Weak` slot handle in the recognizer callbacks would make the cycle impossible | mine (recognizers) / flui-widgets |
| **router ↔ handler** | `PointerRouter.routes` → `Rc<dyn Fn>` → (user capture) | **Nothing in production registers routes.** `remove_route` borrows the handler (`&Rc`), so removal never drops the last owner while the router is borrowed (pointer_router.rs:160, 234). Close and final drop detach before retirement | pointer_router.rs:138-250; ARCHITECTURE "Pointer route capture retirement" | OK (and unwired) | T6d |
| **lane route ↔ handler cell** | `routes: Rc<ResolvedHitRoute>` → `Rc<HandlerCell>` → user closure → (may capture `InteractionDispatchHandle`: a ticket, not a strong lane edge) | The route token is released on Up/Cancel (binding.rs:1091-1106, 1440-1445). The ticket is a non-owning id | interaction_lane.rs:1029-1032 | OK | T6d |
| **focus node → listener → node** | flui-widgets `Focus` registers `node.add_listener(Rc::new(move || { observed_node.borrow() … }))` (flui-widgets interaction/focus.rs:655-672). `observed_node: Rc<RefCell<Rc<FocusNode>>>` holds the **same node**, so node.listeners → closure → node | **Manual `remove_listener(id)` in `dispose`** (flui-widgets focus.rs:698, 866). There is **no Weak and no RAII guard** | focus_scope.rs:748-775 (`add_listener` returns a bare `ListenerId`) | **A real cycle, broken only by a manual call.** It leaks if `dispose` does not run (a panic earlier in the element's unmount) or if the id is lost. **Fix shape:** `add_listener` returns `#[must_use] FocusListenerRegistration { node: Weak<FocusNode>, id }` with `Drop` → `remove_listener`, the same pattern as `FocusNodeRegistration` (focus_scope.rs:307-377). The same applies to `FocusManager::add_listener` (focus.rs:452) | T6d |
| **focus manager → listener → node** | `FocusManager.listeners` → EditableText closure → `Rc<RefCell<Rc<FocusNode>>>` (editable_text.rs:1398-1415) → node → manager (**Weak**) | It is not a cycle, because node → manager is Weak (focus_scope.rs:411). It does extend the node's lifetime until `remove_listener` (editable_text.rs:1811-1815) | as cited | OK. The same RAII fix applies | T6d |
| **focus node ↔ scope** | `scope.inner: Rc<FocusNode>` (1506), and the node's `scope_owner: Weak<FocusScopeNode>` (403) | Weak | focus_scope.rs:403, 1506 | OK | T6d |
| **tracker → region cell → callbacks → tracker** | `annotations` → `Rc<MouseRegionCell>` → user `Rc<dyn Fn>` | Not a cycle unless the user captures a `MouseTracker` clone (`MouseTracker: Clone`, mouse_tracker.rs:182). `close_with_mode` takes the annotations out | mouse_tracker.rs:699-719 | OK (see the drop-under-borrow defect T-2) | T6d |
| **cursor callback → window** | The runtime callback captures `Weak<dyn PlatformWindow>` | Weak | flui-runtime presentation.rs:510 | OK | T6d |

---

## C. Borrows and locks alive while user code runs

User code here means calling a user callback, dropping a value that may own user captures, or emitting through `tracing` (the subscriber is user code). Every site was read; per-site tables are in the sweep files.

### Top violations (ranked by reachability from production)

| # | Site | Primitive / scope rule | User code under it | Consequence | Fix shape | Zone |
|---|---|---|---|---|---|---|
| 1 | recognizers/long_press.rs:468, 471; double_tap.rs:358; tap.rs:611 (all wired through `GestureDetector`/`EditableText`); also long_press.rs:285, 327, 369, 374; double_tap.rs:450 | `if let Some(cb) = self.callbacks.borrow().x.clone() { cb(..) }`. The `Ref` lives through the then-block | `on_long_press`, `on_double_tap`, `on_tap_move`, … | A callback that synchronously unmounts the detector reaches `dispose()` → `callbacks.borrow_mut()` (long_press.rs:596, double_tap.rs:615, tap.rs:776) and panics with `BorrowMutError`. `GestureDetector` documents synchronous unmount from a callback (gesture_detector.rs:1114) | `let cb = self.callbacks.borrow().x.clone();` as its own statement, as drag.rs already does | mine |
| 2 | recognizers/scale.rs:429, 490, 746; multi_tap.rs:329; force_press.rs:298, 326, 364, 377, 385, 401, 427, 445, 608 | same | Scale/force-press/multi-tap callbacks. scale.rs:746 and force_press.rs:608 run **inside arena dispatch** (`accept_gesture`/`reject_gesture`) | same (facade-exported, no widget user today) | same | mine |
| 3 | recognizers/tap.rs:776-790; long_press.rs:596-603; double_tap.rs:615-617; scale.rs:683-686; multi_tap.rs:478-479; force_press.rs:548-552; tap_and_drag.rs:454-460 | `let mut callbacks = self.callbacks.borrow_mut();` (or a statement-temporary `RefMut`), then `slot = None` | The last-owner `Drop` of every callback capture | A reentrant capture `Drop` (one holding a handle that disposes or rebuilds) panics. A panicking capture abandons the remaining slots, and the first failure is not preserved (drag.rs:860-872 is the hardened reference) | `mem::take` the callbacks out under the borrow, then retire one by one under `RoutePanic`/`Retain` | mine |
| 4 | routing/mouse_tracker.rs:232-235, 242-243, 394-397, 446-448, 559-561, 604-606 | Tracker `RefMut` (block or statement); `annotations.insert` replace / `remove` drops a `ResolvedMouseTrackerAnnotation` | The last-owner `Drop` of `Rc<MouseRegionCell>`, which owns 3 user callbacks. It becomes last-owner after the public `unregister_mouse_region` (interaction_lane.rs:1529, reachable via flui-view render.rs:290) | A capture `Drop` that touches the tracker (via a `MouseTracker` clone) panics with `BorrowMutError` mid-hover dispatch | Collect replaced and removed values out of the guard block into the `DeviceWork` batch, and drop them after `work.invoke()` | T6d |
| 5 | routing/focus.rs:123-136 (`NotificationDepthGuard::drop`, panicking branch) | `let mut pending = self.pending.borrow_mut();` across `tracing::warn!` (130) and `pending.clear()` (136) | The subscriber, and the last-owner `Drop` of queued `Rc<FocusNode>` (owning key handler, listeners, rect provider, context) | **This runs while the thread is already panicking.** Any reentry (`request_focus`/`unfocus` → `pending_focus_transitions.borrow_mut()`) is a panic during a panic, which **aborts the process** | `let dropped = mem::take(&mut *pending)`, release, warn, then retire `dropped` through `Retain` (retain while unwinding, per ADR-0127) | T6d |
| 6 | arena/mod.rs:1221 (`GestureArena::accept`) with `ArenaEntryData::accept` (540-557); `resolve` 1313 → 633; `collect_follow_up` 942 | Slot `parking_lot::Mutex` statement temporary | A caller-supplied `Arc<dyn GestureArenaMember>` that is **not a member**, or arrives after resolution, is dropped inside `accept`/`resolve` while the slot lock is held. If it is the last owner, user `Drop` runs under the lock | A `Drop` that re-enters the same pointer **deadlocks**, because parking_lot is not reentrant (RefCell would have panicked) | Return the rejected `Arc` out of `accept`/`resolve` (inside `ArenaFollowUp`) and drop it after unlock. Better, owner-local `RefCell` slots | mine |
| 7 | recognizers: every `with_on_*` builder (tap.rs:314-424, long_press.rs:210-270, double_tap.rs:180-207, scale.rs:261-285, multi_tap.rs:203-212, force_press.rs:214-247, tap_and_drag.rs:295-339) | `self.callbacks.borrow_mut().slot = Some(..)`. The statement `RefMut` is alive while the assignment drops the previous `Rc` | The previous callback's capture `Drop` | Reached only when a slot is set twice. A reentrant `Drop` panics | The `let _prev = cell.borrow_mut().slot.replace(new);` form, or the drag.rs `replace_callback` (325-340) | mine |
| 8 | processing/raw_input.rs:301-305 | Let-chain `&& let Some(callback) = self.callback.borrow().clone()`. The `Ref` lives through the then-block | `callback(raw.clone())` (304) | A callback calling `set_callback`/`clear_callback` (272/277) panics (`RawInputHandler` is unwired) | Clone in its own `let` before the `if` | T6d (file) / I3 claims the fix |
| 9 | routing/interaction_lane.rs:2208-2245 (`dispatch_hover_interleaved`) | `let targets = lane.targets.borrow();` across `tracing::debug!` at 2229 | The subscriber | A subscriber that registers or unregisters a target (`targets.borrow_mut()` at 1423/1460) panics mid-hover | Resolve under the borrow, collect errors, then trace after release | T6d |
| 10 | recognizers: `MonotonicClock::now()` (`MonotonicClock: Send + Sync`, user-implementable, flui-foundation clock.rs:33) under recognizer `parking_lot::Mutex`: long_press.rs:278, 437; double_tap.rs:234, 321, 438, 523; scale.rs:415; multi_tap.rs:223, 411; tap_and_drag.rs:401, 515, 558; drag.rs:538, 557, 622 | Let-bound `gesture_state`/`drag_state` guard | A user clock | A panicking clock leaves half-written state behind (long_press.rs:278: phase `Possible` and `down_time: None`). A reentrant clock deadlocks | Read `now` before locking (drag.rs:497 already does) | mine |

### Other sites (lower risk)

- **`tracing` under a borrow or lock.** The subscriber is user code; with parking_lot this is a deadlock, with RefCell a panic.
  - routing/pointer_router.rs:148, 170, 238 (`RefMut routes`/`handlers`): T6d.
  - processing/resampler.rs:173, 219 (parking_lot `MutexGuard`, so a reentrant subscriber deadlocks): T6d.
  - arena/team.rs:411-421: the combiner lock is held across `arena.add`, whose `#[instrument]` span and closed-arena `ClosePanic::finish` (a `resume_unwind`) both run under it: mine.
- **Drops under the team combiner lock.** team.rs:164 drops the old winner. team.rs:205-236 drops the winner/captain locals, and a replaced captain can be the last owner and is also never notified: mine.
- **signal_resolver.rs:175, 234, 240** drop callbacks under `borrow_mut` (unwired): mine.
- **`poll_deadlines` drops a member outside the catch.** arena/mod.rs:1597-1607: each `DeadlinePoll` (a strong `Arc`, possibly the last owner after user `poll_deadline`) drops at iteration end, outside `catch_unwind`. A panicking `Drop` discards the stored first panic and skips the remaining polls: mine.
- **Label lookup after `on_start`.** multidrag.rs:497-503: after `on_start`, the state is re-found by `PointerId` label (not by generation). A reentrant cancel plus re-add of the same id overwrites, and drops the nested client under the `pointers` Mutex: mine.
- **Already correct.**
  - `binding.rs`: no DashMap guard spans user code. Every remove loop iterates collected keys (818-833, 1510-1527). `get_mut` (637) is dropped explicitly at 642. `Entry` (1344) is consumed by `remove()`.
  - `text_input.rs` and `focus_scope.rs`: snapshot → release → invoke throughout.
  - Arena `dispatch_pending_capturing` (1152-1173) and `close_owner` (1463-1493) are clean.

### Nested lock and borrow order

The order is consistent, but **none of it is documented**.

- **Arena (inverted).**
  - `add` and `deadline_members_snapshot` take the DashMap shard, then the slot `Mutex` (mod.rs:1105-1117, 1545-1576).
  - `sweep_slot` and `sweep_detached` take the slot `Mutex`, then a shard (mod.rs:1375-1384, 1434-1437).
  - That is an inverted order. It is harmless only because the type is `!Send`/`!Sync` (asserted at mod.rs:1777-1778) and no user code runs inside. It becomes a live deadlock the moment a reentrant path appears.
  - Fix: owner-local `RefCell` maps (no order to keep), or compute under the slot lock and insert after release (mine).
- **Team:** combiner → {captain, combiners shard, arena shard → slot} (team.rs:83, 164, 206-212, 234, 253, 411-421): mine.
- **Recognizers:** recognizer state → {settings, `initial_contact`, callbacks `RefCell`}. There is no reverse edge: mine.
- **T6d.**
  - lane: `targets` → {`target_owners`, `LOCAL_LANES`, `ACTIVE_LANES`, `mouse_targets`, `MouseRegionCell.current`} (interaction_lane.rs:2032→2041, 2208→2216-2226).
  - tracker: `inner` → `MouseRegionCell.current`.
  - binding: `hit_tests` shard → resampler `Mutex` (binding.rs:469-472, 657-667, 735-754).
  - focus tree: parent `children` → child `children`; `focus_history` → `manager_binding` (focus_scope.rs:1008-1018, 1604-1609).
  - Debug impls: pointer_router.rs:88-89, text_input.rs:753-762.
- **DashMap:** no `Ref` is held while the same map is touched, and no iteration is combined with insert or remove (both sweeps).

---

## D. Lifetimes

- **Guards and `Ref`s returned to callers: none.** No `pub fn` returns a `Ref`, `RefMut`, lock guard or DashMap ref (both sweeps). `TransformGuard` (hit_test.rs:758) wraps `&mut HitTestResult` for depth restore, not a lock.
- **`'static` on callbacks that are not stored.**
  - The only case is `ClipboardHandle::read_text(on_done: impl FnOnce(..) + 'static)` (clipboard.rs:45). It is invoked immediately and never stored, deliberately, for a future async transport (module doc 16-19). Keep it or drop it, but say which in the doc: T6d.
  - All `with_on_*` / `register_*` / `set_callback` `'static` bounds are on stored values.
  - The `MultiDragHandle: 'static` supertrait (multidrag.rs:95) is redundant: mine.
- **RPIT `use<..>` candidates.** `rg 'impl (Iterator|Fn)'` in return position finds no public RPIT that captures `&self` unnecessarily. The crate returns concrete `Vec`/`SmallVec` snapshots by design (snapshot → release → call). **Nothing to change.**
- **Hit-test path: IDs, not references.**
  - `HitTestEntry` carries `RenderId`, `TargetId`s and transforms. It is `Send + Sync` data (lib.rs:385-387), not tree references.
  - `ResolvedHitRoute` holds `Rc<HandlerCell>` (handler cells, not render nodes).
  - `HitTestProbe::probe` fills an owned `HitTestResult`; `HitTestSnapshot` is owned on purpose (interaction_lane.rs:760-772).
  - **This is correct as it stands.**
- **Builder receivers.** `with_on_*(self: Arc<Self>) -> Arc<Self>` mutates a shared instance through `RefCell` (tap.rs:313-424 and others). These are setters with builder names, and they do not compose lifetimes. See trait-table §3 for the shape.

## E. Generics

- **Struct-level bounds:** none in either zone. The only generic structs are `WithdrawContact<'a>` (recognizer.rs:137), `TransformGuard<'a>`, `Owned<T>` (retain.rs:63) and `take_targets<T: ?Sized>`. None of them carries a bound.
- **Unneeded bound:** `RecognizerBase::start_tracking<T: GestureArenaMember + Clone + 'static>` (recognizer.rs:273). `Clone` is unused, because `recognizer.clone()` resolves to `Arc::clone`. Drop it; the coercion needs only `T: GestureArenaMember + 'static` (`Sized` is implied): mine.
- **Heavy generic bodies without a non-generic inner fn.**
  - `start_tracking<T>` is monomorphised per recognizer (10×). After the `Arc<dyn>` coercion at recognizer.rs:292, everything is type-independent. Split into `fn start_tracking<T>(.., r: &Arc<T>) { self.start_tracking_dyn(.., r.clone() as Arc<dyn GestureArenaMember>) }`: mine.
  - The lane `register_*(handler: impl Fn + 'static)` (interaction_lane.rs:1417-1867) is generic per closure type. Each body is small and boxes immediately, so this is fine.
  - `ClosePanic::retire<T: Retain>` is small. Fine.
- **PhantomData variance.** `ClipboardHandle { _owner: PhantomData<Rc<()>> }` (clipboard.rs:23) is deliberate `!Send`/`!Sync` with no lifetime, so variance is irrelevant. There are no other `PhantomData` uses in `src`.

## F. Send-ness: types that are owner-local but `Send` today

Two sources are combined below:
- **Compile-time evidence.** In-`src` `#[cfg(test)] assert_not_impl_any!` already pins `GestureArena`, `GestureArenaEntry` (mod.rs:1777-1778), `TextInputHandle`, `TextInputOwner` (text_input.rs:872-873), `MultiDragGestureRecognizer` (multidrag.rs:715), and the lane cells and routes (interaction_lane.rs:2408-2413). Those are unit asserts, not trybuild fixtures from `tests/`.
- **Field reading** for everything else. `InteractionDispatchHandle` is `Send + Sync` because every one of its fields is: `LaneTicket{LaneId(NonZeroU64), ThreadId}` and `Arc<DispatchOwner{AtomicBool, CloseTombstone(Arc<{AtomicBool, AtomicUsize}>)}>` (interaction_lane.rs:31, 40-43, 1029-1038; __runtime.rs:178-184).
- **Not run.** A probe binary () was written. It was stopped while queued behind the host build lock, so it was never compiled or run.

| Type | Today | Why | Should be | trybuild fixture candidate | Zone |
|---|---|---|---|---|---|
| `InteractionDispatchHandle` (interaction_lane.rs:1029) | **Send + Sync** | Fields: `LaneTicket` (Copy: lane id + `ThreadId`) and `Option<Arc<DispatchOwner>>` (`AtomicBool` + `CloseTombstone(Arc<atomics>)`) | `!Send`. Every operation re-checks the owner thread at run time (interaction_lane.rs:1339-1365, `WrongThread`), so the type can say it. No cross-thread consumer was found (Z2 §3). Then `Arc<DispatchOwner>` → `Rc`, and `AtomicBool` → `Cell` | `interaction_dispatch_handle_stays_on_its_thread` (it compiles today) | T6d |
| `PointerEventResampler` (processing/resampler.rs:96) | **Send + Sync**, asserted (lib.rs:388) | `Arc<Mutex<ResamplerInner>>`; `HandleEventCallback = Box<dyn FnMut + Send>` (76) | `!Send`. It lives in one `CachedPointerRoute` of an owner-local binding (binding.rs:103) and in the flui-testing replay. Use `Rc<RefCell>`, drop `+ Send` from the callback, and delete the `AssertSendSync` impl | `pointer_event_resampler_stays_on_its_thread` | T6d |
| `GestureArena` and `GestureArenaEntry` | `!Send`, `!Sync` (asserted) | Through `dyn GestureArenaMember` (no `Send`) | Already right. **But it pays for `Send`-grade primitives** (`Arc`, `DashMap`, `parking_lot`, atomics) that cannot be used across threads. Replace them with `Rc`/`RefCell`/`Cell` | Move the in-src assert to a `tests/` trybuild fixture `gesture_arena_stays_on_its_thread`, so it is pinned through the public API | mine |
| `RecognizerBase` and every concrete recognizer | `!Send` (through `GestureArena` and `Rc` callbacks) | — | Already right. Same note: `Arc<AtomicU64>`, `Arc<Mutex<..>>`, `Arc<AtomicBool>` fields (recognizer.rs:97-121) and per-recognizer `Arc<Mutex<GestureSettings>>` (tap.rs:272 and others) should become `Cell`/`RefCell`. The recognizer `Arc<Self>` should become `Rc<Self>` | Only `MultiDragGestureRecognizer` is pinned (in src). Add one fixture covering `TapGestureRecognizer` (the facade-visible extension type) | mine |
| `GestureBinding` | `!Send` (through `PointerRouter`'s `Rc`) | — | Already right. `hit_tests`/`pending_moves` `DashMap` → `RefCell<HashMap>` (Z2 TOP-6; orchestration assigns the `DashMap` replacement to T6d) | `gesture_binding_stays_on_its_thread` | T6d |
| `GestureArenaTeam` | `!Send` (through `dyn Member`) | `DashMap` + `parking_lot` | Same as the arena (unwired: wire or delete per S2) | — | mine |
| `HitTestTarget`, `CustomHitTestable` (traits) | `Send + Sync` supertraits | — | Delete (trait-table 2.11, 2.20) | — | T6d |
| `ClipboardHandle` | `!Send` via `PhantomData<Rc<()>>` (clipboard.rs:23) | — | Already right; it is the pattern to copy | already a candidate in send-flip R1 | T6d |
| Data plane: `HitTestResult`, `HitTestEntry`, IDs, `ScrollTarget` and the other target ids, `VelocityTracker`, `GestureSettings`, details structs | `Send + Sync` | Plain data | **Keep Send.** They are the data plane (ARCHITECTURE "Ownership and synchronization") | — | both |
