# flui-interaction — panic matrix

**Basis.** Code reading at `81f39f137`; nothing was run.
- Every cell has file:line evidence. Paths are relative to `crates/flui-interaction/src/` unless noted.
- Each cell says whether an existing test asserts that exact cell ("verified by `<name>`"), or that no test does ("untested").
- Full per-cell evidence is in . I spot-checked these myself:
  - binding.rs:1272-1278 and 1229-1246
  - focus.rs:112-137, 284-289 and 498-518
  - mouse_tracker.rs:551-626
  - double_tap.rs:333-378

**Policy baseline.**
- **PANIC-POLICY.** Contain at a boundary and keep the first failure authoritative.
- **ADR-0127.** Withdraw before dropping. After a caught failure, or while unwinding, retain the remaining user owners. A non-last `Rc` is dropped normally, and an `Arc` is always retained.
- **AGENTS.** Accepted work must remain deliverable.

**Columns.**
- **C1.** A single panic in one callback.
- **C2.** Two competing failures: two pointers, two listeners, or two devices in one dispatch.
- **C3.** A panic during dispose or teardown, usually a capture's `Drop`.
- **C4.** The next operation after containment.

**Zones.**
- **mine.** `recognizers/**` and `arena/**`.
- **T6d.** Everything else.

## Matrix

| Row (zone) | C1 single | C2 competing | C3 dispose / teardown | C4 next operation |
|---|---|---|---|---|
| **R1 Recognizer callbacks** (mine) | **DEFECT** double_tap (see D1). **DEFECT** multi_tap (see D2). Tap, long_press, scale, force_press and tap_and_drag commit their phase before the callback and heal at the next `start_tracking` (tap.rs:545-562, long_press.rs:357-378, scale.rs:482-500). One exception: tap_and_drag re-fires `on_tap_down` on every move after a panic at :502, which runs before `phase=Dragging` at :509. Only the tap cancel cell is tested: `panicking_cancel_callback_cannot_strand_tap_tracking` (tap.rs:932). Drag is tested by `drag_callback_body_failure_retains_its_capture` (tests/interaction_lane.rs:1015). | OK at route and arena level. Each recognizer is its own target, and the first payload is kept (interaction_lane.rs:599-640; arena mod.rs:1158-1174). Inside one recognizer, a panic in `on_tap_up` skips `on_tap` by design. **Untested with real recognizers.** | **DEFECT** (D3). tap.rs:776-793, long_press.rs:596-603, double_tap.rs:615-617, scale.rs:683-686, multi_tap.rs:478-479, force_press.rs:548-552 and tap_and_drag.rs:454-460 drop their captures under `borrow_mut`. They have no retention and do not preserve the first failure. A reentrant capture gets `BorrowMutError`. Two panicking captures, or one panicking capture during an unwind, abort the process. A panicking `reject()` cancel also skips clearing the callbacks, so double_tap's `first_entry.release()` never runs. Drag and multidrag are hardened, verified by `drag_dispose_preserves_first_capture_failure`, `drag_dispose_during_active_unwind`, `drag_independent_capture_failures` (tests/interaction_lane.rs:859-944) and `dispose_finishes_every_pointer_before_resuming_a_cancel_panic` (multidrag.rs:843). The widget side is also affected: `GestureDetectorState::dispose` (flui-widgets gesture_detector.rs:909-913) disposes 5 recognizers with no containment, so a panic in the first one strands the other four in the arena (see the cycle in ownership-table §B). | **DEFECT** for double_tap and multi_tap: they stay wedged. The rest heal but are untested. **No "next gesture after recovery" row exists for any recognizer.** |
| **R2 Arena members** (mine) | Arena OK. Each member is caught, the slot is removed before callbacks, and the first payload resumes (mod.rs:1152-1174, 1523-1537). Verified by `explicit_resolution_removes_slot_rejects_losers_then_finishes_panicking_winner` (mod.rs:1947), though there the panicking member comes last. **DEFECT** team (D4): `PendingTeamNotifications::dispatch` (team.rs:282-289) and `TeamEntry::resolve` (team.rs:75-92) have no containment. Team is unwired. | Arena delivers to every member. Later payloads are `mem::forget`-ed (mod.rs:1533) rather than `retain_opaque_payload`. **nonconf:** after a failure, `drop(member)` (an `Arc`) still runs (mod.rs:1170), but it is caught, so it cannot abort. **DEFECT** (D5): in `poll_deadlines` (mod.rs:1597-1607) the `DeadlinePoll` member `Arc` drops outside `catch_unwind`. A panicking last-owner `Drop` replaces the stored first payload and skips the remaining polls. **Untested with two panicking members.** | OK. `close_owner` runs callbacks via `ClosePanic::run` and retires members and slots as `Owned` (mod.rs:1463-1493). **Untested with a panicking capture.** **DEFECT** (low reach, unwired): `PointerSignalResolver` `unregister`/`clear`/`clear_all` drop callbacks under `borrow_mut` (signal_resolver.rs:175, 234, 240). | OK for state: the binding ends with no slot. Verified by `arena_accept_panic_cleans_up_handle_pointer_event` (binding.rs:2511). The next Down is not exercised. Team losers stay stale (D4). |
| **R3 Focus listeners, key handlers, policy, RectProvider** (T6d) | **DEFECT** (T1). Primary focus is committed (focus.rs:262-272). Then `notify_focus_nodes` and `notify_listeners` (focus.rs:288-289) call `failure.finish()` per listener (focus.rs:511-515; focus_scope.rs:804-808), which resumes immediately. Every later node listener **and every manager listener** misses a transition that already happened. **DEFECT / design conflict** (T2): `NotificationDepthGuard::drop` clears queued requests that had returned `true` (focus.rs:120-137). The doc says this is deliberate (focus.rs:96-106), which contradicts "accepted work must remain deliverable". Key handlers abort the claim chain, which is acceptable (focus.rs:664-680; focus_scope.rs:950-962). The traversal policy is contained (focus_scope.rs:1748-1756). | **DEFECT** (T1). With two panicking listeners, the second never runs, nor does anything after it. **Untested.** | OK. `FocusClosePanic` commits terminal state first and retains after the first failure or during an unwind. Verified by `close_retires_each_capture_and_preserves_first_failure` (focus.rs:1214), `close_during_active_unwind_preserves_outer_failure` (:1560), `closed_rejections_preserve_outer_failure_and_healthy_retirement` (:1687), `close_from_key_callback_preserves_first_failure_against_hostile_capture` (:1520). **DEFECT** (T3): `NotificationDepthGuard::drop` holds `pending.borrow_mut()` across `tracing::warn!` and `clear()` while already panicking (focus.rs:123-136). Any reentry from the subscriber, or from a last-owner `FocusNode` capture `Drop`, is a panic during a panic, which aborts. | OK for depth: verified by `listener_panic_does_not_leave_notification_depth_stuck` (focus.rs:1894). Missed manager notifications are never repaired (T1). |
| **R4 Hover / enter / exit / cursor** (T6d) | OK. `DeviceWork::invoke` catches per callback and per snapshot cleanup (mouse_tracker.rs:804-866). Verified by `mouse_callback_panic_continues_later_callbacks_then_resumes_first_panic` (mouse_tracker.rs:896). | Same device: OK. **DEFECT** (T4): `update_all_devices` commits every device's state (mouse_tracker.rs:577-582), then runs `for work in pending { work.invoke(); }` (624-626). If device A panics, B's already-committed enter/exit is dropped uninvoked during the unwind, not retained. A `hit_test_fn` panic at :552 has the same effect. The caller adds no containment (flui-runtime ui_realm/frame.rs:806). `dispatch_window_left` is correct per device (mouse_tracker.rs:340-359). **Untested multi-device.** | OK. `close_with_mode` takes callbacks and annotations out before `ClosePanic` retirement (mouse_tracker.rs:699-719). **Untested with a panicking capture.** **DEFECT** (T5): annotations dropped under the tracker `borrow_mut` (mouse_tracker.rs:232-235, 242-243, 394-397, 446-448, 559-561, 604-606) run a last-owner capture `Drop` inside the borrow, after `unregister_mouse_region`. | **DEFECT** (T4). The lost enter/exit is never replayed, because the next update diffs against the committed state. |
| **R5 Routes, claim walks, router, binding arms, lane drop** (T6d) | Routes, router and binding Down, Move, Up and Cancel are OK: per-target capture, and cleanup before resume (interaction_lane.rs:599-640; binding.rs:1038-1115, 1385-1449; pointer_router.rs:285-345). Verified by `per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence` (binding.rs:2175). **DEFECT** (T6): the Scroll arm resumes a pointer-target panic **before** `dispatch_scroll` (binding.rs:1276-1279). The Gesture arm does the same before pan-zoom arbitration (binding.rs:1229-1231, 1244-1246). The wheel tick or pinch is lost. **Untested.** | OK for behaviour. The first payload wins and every target is delivered. Verified by `invoke_isolates_per_target_panics_and_returns_the_first_payload` (interaction_lane.rs:2445) and `target_panic_wins_over_a_later_route_cleanup_panic` (binding.rs:2225). **Policy conflict** (T7): `cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic` (binding.rs:2276) pins "release later handler owners after the first failure", but ADR-0127 §2 says retain. One of them must change. | Router, routes and custody OK. Verified by `pointer_router_competing_retirement_preserves_first_failure_and_recovery` (tests/interaction_lane.rs:219, with owner drop at :439 and saved routes at :524). **DEFECT** (T8): `Drop for InteractionLane` (interaction_lane.rs:956-1004) drops 8 maps with plain `drop`. It has no per-capture containment and no retention. Two panicking captures, or one during an unwind, abort. Only the healthy path is tested (`dropping_the_lane_releases_its_payloads`, interaction_lane.rs:2608). | OK. There is no cached route, pending move or slot left over, and the next route is served. Verified by `per_target_panic_still_delivers…` (binding.rs:2175), `assert_saved_route_entry_retirement` (tests/interaction_lane.rs:524) and `assert_router_retirement_recovery` (:307). |

## Defects in my zone (recognizers/arena), for wave-1 I1/I2

- **D1 double_tap wedged after an `on_double_tap` panic.**
  - On SecondDown-Up the code sets `phase=Completed` (double_tap.rs:340), resolves `first_entry` Accepted and takes it (350-353), and then calls the callback (358) under a `Ref`.
  - If the callback panics, four steps are skipped: `entry.release()` (367-369), the reset to `Ready` (370-376), `stop_tracking` (378), and the release of the held slot in `retained` (arena↔recognizer cycle; ownership §B).
  - The next Down or Up falls into `_ => {}` (272, 379). Only a Cancel or an arena loss resets it, and that fires a spurious `on_double_tap_cancel`.
  - **Scenario.** `GestureDetector{on_double_tap: panics once}`. After the panic is caught at the binding boundary, every later double tap is ignored.
- **D2 multi_tap permanently dead after an `on_multi_tap` panic.**
  - The panic leaves `phase=Completed` with `pointers` still set (multi_tap.rs:315-343).
  - `add_pointer` then skips `start_tracking` (440-443), `handle_pointer_down` ignores the event (267), `handle_event` ignores unknown pointers (455), and `check_timeout` only acts in `Collecting` (409).
  - Unwired today, but it is on the facade.
- **D3 seven recognizers' `dispose` drops captures under `borrow_mut`.**
  - Locations: tap.rs:776-793, long_press.rs:596-603, double_tap.rs:615-617, scale.rs:683-686, multi_tap.rs:478-479, force_press.rs:548-552, tap_and_drag.rs:454-460.
  - There is no retention and the first failure is not preserved.
  - **Fix shape.** Use the drag.rs:860-872 pattern: `mem::take` under the borrow, then retire each callback under `RoutePanic`/`Retain`.
  - **Contract rows to add per recognizer:**
    - a reentrant capture `Drop`;
    - two panicking captures;
    - dispose during an active unwind (in a bounded child process, as the drag rows do);
    - the next gesture after dispose.
- **D4 team dispatch uncontained.**
  - Locations: team.rs:282-289 and 75-92.
  - If the winner's `accept_gesture` panics, the queued `reject_gesture`s for team losers are skipped, and the combiner is already resolved and removed (197-238).
  - Wire it or delete it under S2. If kept, dispatch every notification and preserve the first panic, as `dispatch_pending_capturing` does.
- **D5 `poll_deadlines` drops a member outside the catch.**
  - Location: mod.rs:1597-1607.
  - A panicking last-owner `Drop` overwrites the first payload and skips the remaining polls.
  - Move the drop inside the per-poll capture, as `dispatch_pending_capturing` (1169) already does.
- **D6 (C1 minor) tap_and_drag repeats `on_tap_down`.**
  - A panic at tap_and_drag.rs:502, which runs before `phase=Dragging` (509), makes every later move re-fire `on_tap_down`.
  - Commit the phase first.
- **Coverage gap.** There is no recognizer-level C2 row (two real recognizers both panicking on one pointer). There is no C4 "next gesture" row for any recognizer except tap's cancel path.

## Defects in T6d's zone

These are T1-T8, detailed with scenarios and fix shapes in `t6d-handoff.md`:
- T1 focus listeners abort later listeners (C1/C2).
- T2 queued accepted requests are discarded (needs an owner decision).
- T3 the `NotificationDepthGuard` borrow during panic aborts the process.
- T4 `update_all_devices` has no per-device containment.
- T5 the tracker drops annotations under its borrow.
- T6 the Scroll/Gesture arms skip the claim walk on a listener panic.
- T7 test binding.rs:2276 conflicts with ADR-0127 §2.
- T8 `InteractionLane::drop` has no containment.
