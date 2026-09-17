# Cluster B implementation plan v2 — `flui-scheduler` (single PR)

Supersedes v1 (api-design-lead: ACCEPTABLE+should-do; harsh-critic: RESHAPE NEEDED); anchors re-verified against current `main`.

## 1. Acceptance criteria

**B1 — outcome-typed `end_of_frame` + teardown resolution**
- `end_frame_impl` close → `Ready(Ok(FrameOutcome::Completed(timing)))`.
- `abort_frame` close (panic before the pipeline's post-frame slot) → `Ready(Ok(Aborted { timing }))`.
- A post-frame-callback panic still resolves `Completed` (pipeline committed; only the callback
  failed) — pinned by a dedicated test, not assumed.
- Every strong `UpdateScheduler` handle drops with no frame closing the waiter →
  `Ready(Err(SchedulerClosed))`, waker woken once, no panic escapes `Drop`.
- **Fused for free:** `Result<FrameOutcome, SchedulerClosed>` is `Copy` (`FrameTiming: Copy`,
  `frame.rs:546`) — `poll` peeks by copy, not `.take()`, so re-polling after `Ready` repeats the
  same value, matching in-crate precedent `TickerFutureOrCancel`/`poll_resolution`
  (`ticker.rs:1424`). Replaces the old "non-fused / silent hang" doc.

**B2.1 — enabled-edge wake loss, ALT-1 (not `force_frame_demand`)**
- Confined to `finish_async_pump`. `set_frames_enabled`/lifecycle enable leg **untouched** —
  their documented one-wake-per-false→true-edge contract (`scheduler.rs:682-687, :1837-1842,
  :1923-1927`, test `frame_scheduled_hook_fires_once_per_transition:3444`,
  `runtime-contract.toml:963`'s "one flag and one wake") stays true.
- Given a live waiter with `frame_scheduled` already `true` when frames disable then re-enable,
  `finish_async_pump()` re-issues the demand at the point that would otherwise drop it, rather
  than trusting an earlier edge's coalesced wake — `wake_action` (`frame_pacing.rs:72-96`) reads
  `Skip` once `dirty=false, frame_scheduled=false`, and a bare `on_frame_scheduled` hook has no
  `needs_redraw` fallback (`runtime.rs:1400-1402` is flui-app-specific, not a scheduler contract).

**B2.2 — panicking payload's own panicking `Drop`, at BOTH sites**
- `notify_frame_completion`: A panics, B panics (payload's `Drop` also panics), C counts → C
  woken once; A's panic escapes, never B's drop-panic.
- **Symmetric defect, same fix:** `end_frame_impl:1389-1392` drops `notify_result.err()`
  uncontained when `callback_result` is also `Err` — same class, same `discard_panic_payload` helper.

**B3 — unbounded microtask flush**
- Self-re-enqueuing microtask → bounded to `MAX_MICROTASK_REENTRY_PASSES` passes, warns once
  **per flush call** (not a process latch). A microtask enqueued *by* another in the same flush
  still runs in **this** flush (Dart's nested semantics preserved); only the unbounded case is cut.
- Cap bounds reentrancy *depth*, not width (mirrors Build cap's own `:95-100` caveat: a 2-child
  fan-out still runs up to 2^31 tasks by pass 32). One caller (`handle_begin_frame:1102`);
  leftovers when capped request nothing themselves (`schedule_microtask:2057-2059` never calls
  `request_frame`, same as `TaskQueue::add`, pre-existing). Dart-divergence recorded in the
  mapping entry; `schedule_microtask` verified zero production callers.

**Doc fix** — `AGENTS.md`'s "Crate ARCHITECTURE.md" row adds `flui-platform`, `flui-scheduler` (both confirmed to exist).

## 2. Files, anchors, approach

`crates/flui-scheduler/src/scheduler.rs`:
- `FrameCompletionState.completed`: `Option<FrameTiming>` → `Option<Result<FrameOutcome, SchedulerClosed>>`.
- `FrameCompletionFuture::Output = Result<FrameOutcome, SchedulerClosed>`; `poll` reads
  `completed` **by copy**, fused. Replaces the "not fused" doc (`:174-188`) with the fused
  contract plus the partial-teardown caveat (a strong clone held by a task defers `Drop`; the
  async driver's own hook captures `Weak`, `:917-921`).
- New: `#[non_exhaustive] enum FrameOutcome { Completed(FrameTiming), Aborted { timing: FrameTiming } }`.
  `Completed` stays tuple: simple, hot, unlikely to grow. `Aborted` is struct-shaped on purpose:
  an abort reason/phase is a plausible additive field, and `abort_frame:1455` never records
  anything else about how it ended (no `pending_timings` push, unlike `end_frame_impl`); this
  field is the ONLY surface an aborted frame's data reaches. Derives: `Debug, Clone, Copy` only
  (`FrameTiming` has no `PartialEq`/`Eq`, confirmed `frame.rs:546`).
- New: `struct SchedulerClosed;`, a plain unit struct, **not** `#[non_exhaustive]`, mirroring the
  crate's closest sibling `TickerCanceled` (`ticker.rs:1788-1797`: `derive(Debug, Clone, Copy,
  PartialEq, Eq)`, manual `Display`+`Error`, doctest `let error = TickerCanceled;
  assert_eq!(error.to_string(), "...")`). Same shape and doctest style; message: "the scheduler
  was dropped before this frame's completion could be delivered".
- `notify_frame_completion(&self, outcome: FrameOutcome)`; add `discard_panic_payload` helper
  (contains a superseded payload's own panicking `Drop` in its own `catch_unwind`, traces, never
  propagates); use it for the second-waker-panic case.
- `end_frame_impl` (`:1374-1376`) → `notify_frame_completion(Completed(timing))`; restructure the
  `callback_result`/`notify_result` merge (`:1378-1394`) as a `match` over both so the discarded
  side (both `Err`) routes through `discard_panic_payload` instead of an uncontained
  `Option::or` drop. `abort_frame` (`:1467-1469`) → `Aborted { timing }`.
- New `impl Drop for SchedulerInner`: `self.frame.completion_waiters.get_mut().drain()`
  (parking_lot `get_mut`, no lock, sound via `Arc`'s strong-count-zero guarantee). Write
  `Some(Err(SchedulerClosed))` **unconditionally**, no `is_none()` guard: dead by construction
  (`drain` does `mem::take`, so a still-registered entry always has `completed == None`); a
  one-line comment states this instead of a runtime check.
- Contain both the wake panic and its payload's own drop panic; **never `resume_unwind`**, only
  `tracing::error!` (outside `catch_unwind`, consistent with every other panic-report site here).
- Document: may run on a **foreign thread** inside `Waker::wake()`, since the async-driver hook's
  temporary `upgrade()` at `:919-921` can be the last strong ref; the body touches only
  `Send+Sync` state. Field drops after the body are pre-existing; `RealmCapabilities` borrows the
  scheduler by reference (`ui_realm.rs:827`), not an owned clone.
- `finish_async_pump` (`:1969-1974`) — **ALT-1**, the only production-facing B2 change: after
  clearing the latch, `if frames_enabled { if completion_waiters.lock().has_live_waiter() /* guard
  dropped before */ { request_frame(); } }`. Every ordering of an enable edge vs. this pump ends
  with `frame_scheduled == true` or a frame having already run.
- **Dropped from v1:** the lifecycle-leg `force_frame_demand` change. `set_frames_enabled` has
  zero production callers (`renderer_binding.rs:777` inside `#[cfg(test)] mod tests` from `:718`).
  The lifecycle enable edge is straight-line, same-thread with the `PumpAsync` decision leading to
  `finish_async_pump` (`desktop.rs:497-522`; `realm_dispatch.rs:376/422/425` same shape): no
  concurrent second caller is possible there, and `lifecycle_ladder.rs:170-174` already calls
  `redirty_root_for_frames_reenable()` + `wake_frame()` unconditionally, independent of the
  scheduler's hook contract. ALT-1 alone closes the API hazard for a bare cross-thread clone
  without weakening the pinned one-wake contract or conflicting with cluster C's
  `ensure_visual_update` hunk near `:2694`.
- `flush_microtasks` (`:2057-2070`): private `const MAX_MICROTASK_REENTRY_PASSES: usize = 32`
  and private `flush_microtasks_pass(&self) -> usize` mirroring `TaskQueue::execute_until`'s
  count-budget shape; outer capped loop, `tracing::warn!` once per call on cap. No public export
  (pinning test lives in-crate).

`src/lib.rs` (`:196-199`) / `runtime-contract.toml` `:190`: add, alphabetically:
`FrameCompletionFuture, FrameOutcome, IdleDeadline, MAX_BUILD_REENTRY_PASSES, SchedulerBuilder,
SchedulerClosed, UpdateScheduler, WeakUpdateScheduler` — both literal strings byte-identical
(checked by `runtime-conformance-check`).

`runtime-contract.toml:3004` `notes`: rewrite the "cannot distinguish an aborted frame" claim.

`ARCHITECTURE.md`: revise the "cannot tell an aborted frame apart" and `flush_microtasks`
"unaudited" paragraphs (depth-not-width + Dart-divergence notes); add a `## Mapping decisions`
entry for #1162 (`Result<Outcome, Closed>` vs. the issue's flat enum, citing
`TickerFutureOrCancel`; the Dart `Future<void>` divergence).

`AGENTS.md` (~`:192`): add `flui-platform`, `flui-scheduler` to the row.

**Public-surface diff:** `+FrameOutcome` (`#[non_exhaustive]`), `+SchedulerClosed`
(`Error+Display+Debug+Clone+Copy+PartialEq+Eq`), `FrameCompletionFuture::Output` type change
(breaking, zero callers, verified); `finish_async_pump`'s signature unchanged; no other public change.

## 3. Tests + revert matrix

| Test | File | Reddens if reverted |
|---|---|---|
| `an_idle_registration_demands_one_frame_and_resolves_with_its_timing` → match `Ok(Completed(t))` | `tests/end_of_frame_lifecycle.rs:178` | Output type change |
| `assert_recovered_from_panic` → `Ready(Ok(Aborted{..}))` (7 sites: L180,228,278,345,433,490,531) | `tests/frame_panic_recovery.rs:99-154` | `abort_frame` not passing `Aborted` |
| **New:** `a_post_frame_callback_panic_still_resolves_completed_not_aborted` | `tests/frame_panic_recovery.rs` | `Completed`/`Aborted` path confusion |
| `test_end_of_frame_completes_after_frame` → destructure `Ok(Completed(t))` | `scheduler.rs:3858` | Output type change |
| **New:** `polling_after_ready_returns_the_same_value_again` | `scheduler.rs` | `.take()` reintroduced |
| **New:** `scheduler_drop_resolves_a_pending_waiter_with_scheduler_closed` | `scheduler.rs` | `Drop` absent/no-op |
| **New:** `scheduler_drop_wakes_a_later_waiter_when_an_earlier_waker_panics` | `scheduler.rs` | `resume_unwind` used in `Drop` |
| **New:** `notify_frame_completion_survives_a_panicking_payloads_own_drop_panic` — A: plain `panic!("waker A")`; B: `panic_any(PoisonPill)` (`PoisonPill::drop` panics); C: counts. Two distinct types (not one conditional) so the test's own final drop of A's payload never re-panics. | `scheduler.rs` | Uncontained `drop(payload)` on superseded path |
| **New:** `end_frame_impl_survives_a_panicking_notify_payloads_own_drop_panic` (same shape one level up) | `scheduler.rs` | `Option::or` merge instead of `discard_panic_payload` |
| **New:** `finish_async_pump_reissues_a_stranded_live_waiters_demand` — waiter→disable→enable(hook=1)→`finish_async_pump()`→assert `(is_frame_scheduled, hook_count)==(true,2)`. Not asserted before the pump (pins no surplus). | `scheduler.rs` | ALT-1 removed from `finish_async_pump` |
| `lifecycle_reenable_edge_schedules_exactly_one_frame`, `re_enabling_frames_re_demands_on_the_edge`, `frame_scheduled_hook_fires_once_per_transition` | unchanged | Guards, must stay green |
| `test_end_of_frame_future` → strengthen to `Ready(Ok(Completed(_)))` | `tests/integration_tests.rs:2883-2909` | Output type change (miscounted in v1) |
| **New:** `a_self_reenqueuing_microtask_is_bounded_by_the_reentry_cap_not_hung_forever` — `runs == MAX_MICROTASK_REENTRY_PASSES` exactly, warn once | `scheduler.rs` | Unbounded `flush_microtasks` |
| `test_microtask_execution`, `frame_completion_future_type_pins` (`end_of_frame_lifecycle.rs:519-520` must compile), `notify_frame_completion_tolerates_an_inline_polling_waker`, `completion_waker_runs_with_no_scheduler_lock_held` | unchanged | Guards |

## 4. Risks

- Output-type break: zero production callers, confirmed (`rg '\.end_of_frame\('`).
- ALT-1's extra scan runs only inside `finish_async_pump`'s rare `PumpAsync` cycle.
- Out of scope: `docs/PANIC-POLICY.md`'s Drop-panic rule (#1165, after #1161) — this PR's `Drop`
  states its own rule locally.
- Gates: ASYNC-GATE (no new `.await`/blocking, no `Mutex` held across a wake/hook call, new types
  are Send+Sync+'static plain data, `tracing` on every swallowed panic and the cap); API-GATE
  (deliberate zero-caller break, documented, no deprecation cycle per `active-dev.md`); `just ci`;
  `just runtime-conformance-check`.

## 5. Maintainer-grade verdict: **ACCEPTABLE** (pre-code)

ALT-1 replaces a contract-breaking mechanism with a one-method root-cause fix that never touches
the two call sites whose one-wake-per-edge behavior is independently pinned by an existing test
and runtime-contract entry. The Drop dead-code guard is removed on a verified basis, not
defended. B2.2 is applied symmetrically at both sites via one helper. The fused-future change is
grounded in an in-crate precedent (`TickerFutureOrCancel`), stronger than v1's external
citations. `Aborted`'s struct shape is justified by fact (`abort_frame` records the timing
nowhere else), not speculative flexibility. Every anchor was re-read against current `main`.

MEMORY: When a "was this ever silently dropped" fix has two candidate homes — the edge that
CREATES the demand, or the place that CLEARS it without checking — clearing-site fixes compose
with existing "one wake per edge" contracts; edge-site fixes break them. Fix where the state is
mutated, not where it's convenient to reason about.
MEMORY: A `mem::take`-based drain is a structural proof, not a runtime property — an `is_some()`
guard against "an entry the drain already processed" is dead code if the drain removes what it
processes; delete the guard, comment the invariant.
MEMORY: `Result<T, E>` degrades to "peek by copy, don't `.take()`" for free once both arms are
`Copy` — a future's non-fused doc warning is a design choice (owned-state consumption), not a
law; check whether the underlying type is `Copy` before assuming it.

## Plan-review amendments to v2 (harsh-critic confirmation — ACCEPTABLE once applied)

A1. **ALT-1 is a store-buffering (Dekker/SB) litmus test; Release/Acquire does not close it.** Pump thread A:
`frame_scheduled.store(false, Release)` then `frames_enabled.load(Acquire)`. Edge thread B:
`frames_enabled.swap(true, AcqRel)` (`set_frames_enabled` :2665-2669 / lifecycle :2320-2324) then
`request_frame_impl`'s `frame_scheduled.swap(true, AcqRel)` (:827). The outcome "B's swap reads the OLD `true`
(no hook) AND A's load reads the OLD `false` (no re-request)" is permitted (no reads-from edge between the
threads → no synchronizes-with) and observable on x86 (A's plain store sits in the store buffer while A's load
executes). Decision: option (a) — `SeqCst` on all four accesses: the pump's `frame_scheduled.swap(false, SeqCst)`
+ `frames_enabled.load(SeqCst)`; both edge sites' `frames_enabled.swap(.., SeqCst)`; `request_frame_impl`'s
`frame_scheduled.swap(true, SeqCst)` (same `xchg` on x86; negligible on ARM). Comment the SB hazard at the pump
site. No single-threaded test can redden this; a loom model is the only oracle and is optional — record that.
The plan sentence "every ordering … ends with frame_scheduled == true or a frame having run" holds only under
SeqCst; reword accordingly.
A2. Re-entrancy doc: a hook that itself calls `finish_async_pump` now recurses (pump → clear → swap false→true →
hook → pump …); today's bare store cannot. The hook contract (:1986-1988) already forbids re-entering the
scheduler — state the concrete consequence in `finish_async_pump`'s doc; no guard.
A3. Test pitfall: in `finish_async_pump_reissues_a_stranded_live_waiters_demand` the waiter must be BOUND
(`let _waiter = scheduler.end_of_frame();`), not `let _ = …`, or it drops immediately, `has_live_waiter()` is
false, and the test is red WITH the fix. Revert oracle confirmed red by walk: reverted → `(false, 1)`; ALT-1 →
`(true, 2)`.
A4. `tests/integration_tests.rs:2883` is `a_frame_completing_while_poll_clones_the_waker_still_resolves_it`, not
`test_end_of_frame_future` — verify names before citing; strengthen whichever `end_of_frame` assertions there
only check `.is_ready()`.
A5. Fused peek: confirmed no double delivery (demand predicate is `strong_count > 0`, independent of
`completed`; drain removes; `notifier()` private, once per fresh future; `!Clone` pinned). A post-`Ready` poll
returns before the slow path and stores no waker.
