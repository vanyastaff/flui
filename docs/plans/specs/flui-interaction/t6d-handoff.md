# Handoff to send-flip T6d: defects in T6d's zone of flui-interaction

T6d owns every file in `crates/flui-interaction/**` except `recognizers/**`, `arena/**` and the pointer vocabulary (orchestration.md, scope decision of 2026-10-06). This list covers only that zone.

**What this work does for each item.** It writes a contract test, `#[ignore = "contract: <behaviour>"]`, through the public API in `crates/flui-interaction/tests/main.rs` modules. Where the item is a compile-time contract, it writes a trybuild fixture instead. It also hands over a verified fix patch. T6d lands the fix and removes the `ignore`.

**Conventions.**
- Paths are relative to `crates/flui-interaction/src/`.
- Evidence comes from reading the code at `81f39f137`. Sources are ledger Z2/Z4 re-checked here.
- Any hostile-`Drop` or abort case runs in a bounded child process, following the existing `pointer_router_competing_retirement_preserves_first_failure_and_recovery` harness (tests/interaction_lane.rs:219).
- Each test must fail for the stated reason with the fix reverted.

**Count: 19 items.**
- 15 are `#[ignore]` contract tests: H1, H3-H7, H9-H17.
- 2 are trybuild fixtures: H18, H19.
- 2 need an owner decision (marked **DECISION**): H2, H8. Each also has a test once decided.

A further list of non-test type and shape changes (N1-N9) follows.

---

## Panic containment and retention

### H1. A focus listener panic must not hide the transition from later listeners
- **Ignore text:** `contract: every focus listener sees a committed transition even after an earlier listener panicked`
- **Code:**
  - focus.rs:262-272 commits `primary_focus`.
  - focus.rs:288-289 runs `notify_focus_nodes` and then `notify_listeners`.
  - focus.rs:511-515 and focus_scope.rs:804-808 call `failure.finish()` per listener, which resumes immediately.
- **Scenario:**
  1. Create a `FocusManager` with node listener `L1` on node A, which panics.
  2. Add manager listener `M`, which records `(prev, new)`.
  3. Catch around `A.request_focus()`.
  4. Today `M` is never called, while `manager.primary_focus() == A`.
  5. Second row: two manager listeners, where the first panics and the second records.
- **Real impact:**
  - EditableText's IME and focus listeners (flui-widgets text/editable_text.rs:1400, 1511) miss the edge, so the IME stays attached to the old field.
  - The semantics `focused` flag goes stale.
- **Fix shape:**
  - Deliver the round from a snapshot.
  - Catch each listener, keep the first payload and retain the rest per ADR-0127.
  - Resume after **both** the node pass and the manager pass.
  - This is the same shape as `ResolvedHitRoute::invoke` (interaction_lane.rs:599-640).
- **Coordination:** focus-keyboard T3 also edits these files (tasks.md, "occupied files").

### H2. Queued accepted focus requests are discarded on a listener panic (DECISION)
- **Ignore text:** `contract: a focus request accepted during notification is applied or reported refused after a listener panic`
- **Code:**
  - focus.rs:120-137: `NotificationDepthGuard::drop` clears `pending_focus_transitions` while panicking.
  - focus.rs:96-106 documents this as deliberate.
- **Scenario:**
  1. Listener `L1` calls `B.request_focus()`. It returns `true`, because the request is queued.
  2. `L1` then panics.
  3. After the catch, `primary_focus() != B`.
- **Conflict:** AGENTS "Accepted work must remain deliverable".
- **Owner choice:** either drain the queue after containment, or make a request queued during notification return a pending result instead of `true`.

### H3. Recovery from a panic must not abort on reentry
- **Ignore text:** `contract: a reentrant drop during focus-panic recovery is retained, not a double panic`
- **Code:**
  - focus.rs:123-136: the `pending.borrow_mut()` guard is held across `tracing::warn!` (130) and across `pending.clear()` (136).
  - That clear drops queued `Rc<FocusNode>` values, which own the key handler, listeners, rect provider and context (focus_scope.rs:404-410).
- **Scenario (child process):**
  1. Listener `L1` queues `N.request_focus()`.
  2. `L1` detaches `N`, drops the caller's last strong `Rc<FocusNode>`, and panics.
  3. `N` carries a `register_context` value whose `Drop` calls `manager.unfocus()`.
  4. The guard's `clear()` drops `N` → `unfocus()` → `pending_focus_transitions.borrow_mut()` → `BorrowMutError` while panicking → abort.
  5. A second row uses a tracing subscriber that calls `request_focus` from `on_event`.
- **Fix shape:**
  - `let dropped = mem::take(&mut *self.pending.borrow_mut());`
  - Release the borrow, then warn.
  - Retain `dropped` through `Retain`, because the thread is unwinding.

### H4. `update_all_devices` must deliver every device's committed enter and exit
- **Ignore text:** `contract: a device's committed hover transitions are delivered even if another device's callback panics`
- **Code:**
  - mouse_tracker.rs:577-582 commits state per device.
  - mouse_tracker.rs:624-626 then runs a plain `for work in pending { work.invoke(); }`.
  - A `hit_test_fn` panic at :552 drops the work already queued for earlier devices.
  - The caller has no containment either: flui-runtime ui_realm/frame.rs:806.
- **Scenario:**
  1. Mouse (device 1) and pen (device 2) hover regions R1 and R2.
  2. The test's `hit_test_fn` moves both devices into new regions R1' and R2'.
  3. R1'.`on_enter` panics.
  4. After the catch, R2'.`on_enter` was never called.
  5. A second `update_all_devices` still does not call it, because the committed state already says the device is inside.
  6. Second row: `hit_test_fn` panics for device 2, and device 1's work is lost.
- **Fix shape:**
  - Invoke each `DeviceWork` under `RoutePanic::preserve_first`.
  - Retain the remaining work on unwind.
  - Contain `hit_test_fn` per device, matching `dispatch_window_left` (mouse_tracker.rs:340-359).

### H5. Annotation `Drop` must not run under the tracker borrow
- **Ignore text:** `contract: a mouse-region capture dropped by the tracker can reenter the tracker`
- **Code:** mouse_tracker.rs:232-235, 242-243, 394-397, 446-448, 559-561, 604-606. Each `annotations.insert` that replaces or `remove`s a value drops a `ResolvedMouseTrackerAnnotation` while `inner.borrow_mut()` is alive.
- **Scenario:**
  1. Register a mouse region through the lane. Its `on_exit` capture's `Drop` calls `tracker.unregister_annotation(other)` or `tracker.current_cursor()`.
  2. Hover into the region, then call `unregister_mouse_region` (interaction_lane.rs:1529, public through flui-view render.rs:290). That keeps the callbacks, so the tracker is now their last owner.
  3. Move the pointer out of the region.
  4. The remove at 447 drops the capture, and its `Drop` hits `BorrowMutError`.
- **Fix shape:** carry replaced and removed annotations out of the guard block inside `DeviceWork`, and drop them after `invoke()`.

### H6. A listener panic on a wheel or pinch must not skip the scroll or pan-zoom claim walk
- **Ignore text:** `contract: a wheel tick reaches the scroll claim walk even when a pointer target on the path panics`
- **Code:**
  - binding.rs:1276-1279: `dispatch_ephemeral(..)` then `panic.resume()`, which happens before `fresh_result.dispatch_scroll`.
  - binding.rs:1229-1231 and 1244-1246: the same pattern before pan-zoom arbitration.
- **Scenario:**
  1. Build a path with a `PointerTarget` that panics on `Scroll`, above a `ScrollTarget` that records.
  2. Call `handle_pointer_event(Scroll)` and catch.
  3. Today the `ScrollTarget` never sees the tick.
  4. Pan-zoom row: `PointerEvent::Gesture` reaching a `PanZoomTarget`.
- **Fix shape:** capture the pointer-target panic, run the claim walk, then resume the first payload. The claim walk's own panic follows the first-failure rule.

### H7. `InteractionLane` drop needs containment and retention
- **Ignore text:** `contract: dropping a lane with hostile captures preserves the first failure and retains the rest`
- **Code:** interaction_lane.rs:956-1004 takes the 8 maps and drops each `Vec` with a plain `drop`.
- **Scenario (child process):**
  1. `let lane = InteractionLane::try_new()?;`
  2. Inside `lane.enter`, use `lane.dispatch_handle()` to register two pointer targets whose captures panic in `Drop`.
  3. `drop(lane)` aborts on the second panic.
  4. Second row: drop the lane during an active unwind with one such capture.
  5. Third row: after the first failure, a healthy fresh lane on the same thread serves a route.
- **Fix shape:** retire each cell through `ClosePanic`/`Retain`, the way `DispatchCustody::retire` already does (interaction_lane.rs:1137-1200).

### H8. Cleanup after the first failure must follow one rule (DECISION)
- **Code:**
  - The test `cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic` (binding.rs:2276-2360) pins that later handler owners are *released* after the first failure.
  - binding.rs:1532-1557 implements that.
  - ADR-0127 §2 says to *retain*.
- **Owner choice:** amend ADR-0127 with an exception for caught, isolated per-owner cleanup, or change the code and the test. Code that silently disagrees with an accepted ADR is a defect (AGENTS ADR policy).

## Borrow or lock held across user code (subscriber reentry)

### H9. Pointer router traces under its borrow
- **Ignore text:** `contract: a tracing subscriber may re-enter the pointer router`
- **Code:** pointer_router.rs:145-148 (`add_route`), 156-170 (`remove_route`) and 232-238 (`remove_global_handler`). `tracing::trace!` runs while the `RefMut` lives.
- **Scenario:**
  1. Install a test `tracing` layer (flui-testing `log_capture` pattern) whose `on_event` calls `router.add_route(..)`.
  2. Call `router.add_route(..)`.
  3. Today this gives `BorrowMutError`.
- **Fix shape:** scope the mutation in a block, then trace.

### H10. Interleaved hover traces under the `targets` borrow
- **Ignore text:** `contract: a subscriber may register a pointer target during interleaved hover dispatch`
- **Code:** interaction_lane.rs:2208-2245. `let targets = lane.targets.borrow()` is alive at `tracing::debug!` (2229).
- **Scenario:**
  1. Interleave a hover over a stale mouse-region target.
  2. A subscriber registers a pointer target (`targets.borrow_mut()`, 1423).
  3. Today that panics.
- **Fix shape:** resolve under the borrow, collect the errors, release, then trace.

### H11. Resampler traces under a `parking_lot` lock, so reentry hangs
- **Ignore text:** `contract: a subscriber touching the resampler during a queue-full warning does not deadlock`
- **Code:** processing/resampler.rs:149-177 (warn at 173) and 208-224 (warn at 219). Both run under `inner: MutexGuard`.
- **Scenario (child process with a timeout):**
  1. A subscriber calls `resampler.has_pending_events()` on the warning.
  2. Overfill the queue.
  3. Today this deadlocks; parking_lot is not reentrant.
- **Fix shape:**
  - Warn after the guard is released.
  - Better: move to `Rc<RefCell>`, see N2. A reentry then fails deterministically instead of hanging.
- **Coordination:** the numeric resampler fixes are wave-1 I3 in this work; the lock change is T6d.

### H12. `RawInputHandler` calls its callback under a borrow
- **Ignore text:** `contract: a raw-input callback may replace itself`
- **Code:** processing/raw_input.rs:301-305. The let-chain `&& let Some(callback) = self.callback.borrow().clone()` keeps the `Ref` alive through the then-block while `callback(..)` runs.
- **Scenario:** the callback calls `set_callback(..)`, which does `borrow_mut()` at 272, and gets `BorrowMutError`.
- **Note:** the type is unwired (Z4 §1). If S2 deletes it, this item goes too. Otherwise the fix is to clone in its own `let` first. I3 already lists this fix.

## Behaviour (wave-1 I4/I5/I6 deliver the tests and patches; T6d owns the files)

### H13. One device's exit must not erase another device's region
- **Ignore text:** `contract: each device receives its own on_exit for a shared region`
- **Code:**
  - The `annotations` cache (mouse_tracker.rs:199) is shared across devices.
  - Per-device exit removes the entry for everyone (446-448, 604-606).
  - A later exit lookup finds nothing (437-445, 595-603).
- **Scenario:**
  1. Mouse and pen both hover region R.
  2. The pen leaves, so `on_exit` fires once.
  3. The mouse leaves, and today no `on_exit` fires. R stays hovered.
- **Fix shape:**
  - Make the annotation cache per device, or reference-count it by active device.
  - Key on a typed `DeviceId`, not the 31-bit mask (events.rs:487-488, vocabulary P2).

### H14. Removing the focused node must leave it unfocused, even if a listener re-requests it
- **Ignore text:** `contract: a removed node never remains primary focus`
- **Code:** focus_scope.rs:1295-1300. `manager.unfocus()` runs while the child is still attached and still in the scope history. `forget_subtree` only runs at 1308, after detach at 1302-1306.
- **Scenario:**
  1. A manager listener reacts to the blur by calling `child.request_focus()`. The request is queued, drains inside `unfocus`, and passes eligibility.
  2. The child is then removed.
  3. Today `primary_focus()` names a detached node, keys go to its handler, and Tab does nothing (focus_scope.rs:1778-1780).
- **Fix shape:** detach and forget first, then `unfocus`. Better: restore focus to the enclosing scope's `focused_child`.
- **Coordination:** focus-keyboard T3.

### H15. `with_paint_offset` must refuse non-finite offsets
- **Ignore text:** `contract: a non-finite paint offset publishes no descendant entries`
- **Code:** hit_test.rs:421-432 has no admission check. Contrast `with_paint_transform`, which refuses via `try_inverse` (ADR-0113, hit_test.rs:450-462).
- **Scenario:**
  1. `with_paint_offset(Offset::new(f64::NAN, 0.0), |r| r.add(entry))`.
  2. Today the entry is published with a non-invertible transform. Its mouse annotation still enters and exits, while pointer delivery skips it (interaction_lane.rs:535-539).
- **Fix shape:** return `Option<R>` like `with_paint_transform`, and add a row to `hit_test_transform_admission`.

### H16. Localised events must localise every positional field
- **Ignore text:** `contract: coalesced, predicted and scroll-delta samples are localised under a transform`
- **Code:**
  - hit_test.rs:847-848 clones coalesced and predicted samples untransformed.
  - hit_test.rs:953-960 leaves the scroll `delta` unlocalised, while the pan-zoom `pan_delta` is localised (925-932).
- **Scenario:** a rotated (90°) target receives a Move whose `coalesced` sample sits at a known global point, plus a wheel `PixelDelta(10,0)`. Today the coalesced sample is global and the delta is not rotated.
- **Fix shape:** transform every sample, and the delta as a vector without translation. This becomes live once backends send coalesced samples (M1/P3).

### H17. Admission at the binding boundary
- **Ignore text (two rows):**
  - `contract: a non-finite pointer position is refused at the binding`
  - `contract: moves of a pointer refused at the cap are not delivered as hover`
- **Code:**
  - binding.rs:1069-1070 and 1117-1120 accept unchecked positions.
  - binding.rs:1057-1067 and 1149-1157: once the 33rd Down is dropped, that pointer's Moves fall through to the hover path, so targets get a Move with no Down.
- **Scenario:**
  - Row 1: a Down at `(NaN, 0)` reaches `hit_test_fn` and the targets today.
  - Row 2: 33 Downs; Move pointer 33; today a `PointerTarget` sees a Move on pointer 33.
- **Fix shape:** reject a non-finite position at entry. Track refused pointer ids until their Up or Cancel.
- **Coordination:** I5.

---

## Compile-time contracts (trybuild, `tests/ui/` or `crates/flui-interaction/tests/compile_fail/`)

These are items H18-H19 of the 19 above. They are verified by a compile-fail fixture rather than an `#[ignore]` test.

- **H18 `interaction_dispatch_handle_stays_on_its_thread`.**
  - `InteractionDispatchHandle` (interaction_lane.rs:1029-1032) is `Send + Sync` today. Its fields are a `Copy` ticket and `Arc<DispatchOwner{AtomicBool, CloseTombstone(Arc<atomics>)}>`.
  - Every operation re-checks the owner thread at run time (`WrongThread`, interaction_lane.rs:1339-1365), and no cross-thread consumer was found.
  - Change: make it `!Send` (`Rc<DispatchOwner>` with a `Cell<bool>`, or `PhantomData<Rc<()>>` as `ClipboardHandle` does).
  - The fixture compiles today, so it should fail to compile after the change.
- **H19 `pointer_event_resampler_stays_on_its_thread`.**
  - `PointerEventResampler` is asserted `Send + Sync` (lib.rs:388).
  - It sits behind `Arc<Mutex<ResamplerInner>>`, with `HandleEventCallback = Box<dyn FnMut + Send>` (processing/resampler.rs:76, 96-97).
  - It is owner-local, one per binding route (binding.rs:103).
  - Change: delete the assertion, drop `+ Send` from the callback, and move to `Rc<RefCell>`.

## Non-test handoff (type and shape changes in T6d's zone; no behaviour to pin)

- **N1.** binding.rs:303 and 307: `DashMap` → `RefCell<HashMap>`. Today's use is correct (sweep-t6d.md §OK). The change makes a future reentrant mistake a deterministic `BorrowMutError` instead of a silent shard deadlock, and removes per-event shard locking. Measure with `pointer_route_bench` before and after.
- **N2.** processing/resampler.rs: `Arc<parking_lot::Mutex>` → `Rc<RefCell>` (pairs with H11 and H19).
- **N3.** `InteractionLane.target_owners: HashMap<_, Arc<DispatchOwner>>` (interaction_lane.rs:796): `Arc` → `Rc` (pairs with H18).
- **N4.** Focus listener registration. `FocusNode::add_listener` and `FocusManager::add_listener` (focus_scope.rs:748, focus.rs:452) should return a `#[must_use]` RAII registration that holds a `Weak` and calls `remove_listener` in `Drop`, like `FocusNodeRegistration` (focus_scope.rs:307-377).
  - Why: flui-widgets `Focus` registers a listener that captures `Rc<RefCell<Rc<FocusNode>>>` of the same node (flui-widgets interaction/focus.rs:655-672). That forms a node → listener → node cycle that only a manual `remove_listener` in `dispose` breaks (focus.rs:698, 866).
  - Test, once the API lands: dropping the registration frees the node (a `Weak::upgrade` returns `None`).
- **N5.** `FocusTraversalPolicy::sort_descendants(&[Rc]) -> Vec<Rc>` → `order(&self, &mut [Rc<FocusNode>])`, so a policy can only permute (trait-table 2.14). `ReadingOrderPolicy` should also compute rects once instead of per comparison (focus_scope.rs:1976-1977).
- **N6.** Delete the unwired surface:
  - `EventRouter`, together with `HitTestable`, `CustomHitTestable` and `sealed::hit_testable`;
  - `HitTestTarget`, `Disposable`, `GestureCallback`/`BoxedCallback`, `GestureRecognizerExt` (traits.rs);
  - `sealed::focus_node`;
  - `traps_focus`/`autofocus` dead flags (focus_scope.rs:1566-1584);
  - the 11 unwired `MouseTracker` methods (Z2 §1).
  - Each removal follows S2's scope decision.
- **N7.** focus_scope.rs:1897-1907: `Debug for FocusScopeNode` calls `focused_child()`, which takes `borrow_mut` and mutates. Make it a read-only peek.
- **N8.** clipboard.rs:45 `read_text(.. + 'static)`: the bound is deliberate for a future async transport (module doc 16-19). Keep it with that note, or drop it until the transport exists.
- **N9.** Lock and borrow order. It is consistent everywhere, but undocumented. Write one line per owner:
  - lane: `targets` → `target_owners`/`mouse_targets`/TLS;
  - tracker: `inner` → `MouseRegionCell.current`;
  - binding: `hit_tests` → resampler;
  - focus: parent → child, and `focus_history` → `manager_binding`.
