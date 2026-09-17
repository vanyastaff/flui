# Plan v2 — split by mechanism: PR-P (pacing) + PR-L (lock-drop sweep)

Supersedes v1 (single-PR). Two v1 claims corrected below with evidence, not carried forward silently.

**v1 corrections:**
1. v1 said the gate is low-value because production doesn't reach it and `request_redraw` coalesces — wrong.
   `wake_action` (frame_pacing.rs:72-95) ORs two INDEPENDENT inputs: `dirty` and `frame_scheduled` (`if dirty {
   Render } … if frame_scheduled && !fallback.pending { Render }`). `frame_scheduled` is real and separately
   consulted (a live ticker with no other dirty state renders through it alone). winit 0.30.13 docs
   ("Queues a `RedrawRequested` event") promise no dedup; X11 queues a second one. What prevents today's
   surplus frame is `UiRealm::mark_rendered` (ui_realm.rs:1851) + `frame_is_dirty` (frame_pacing.rs:99-135) →
   `Skip` — not coalescing. My argument answered the wrong question (`needs_redraw`'s path, not `frame_scheduled`'s).
2. v1 flagged `async_driver.rs:221` (`RemoveZombieSlotOnUnwind::drop`) as a real hazard. Re-traced: `poll_ready`
   (L396-410) does `task.future.take()` BEFORE the guard exists, so the guard's `remove()` drops a
   `Task{future: None, ready/cancelled: Arc<AtomicBool>}` — no significant drop, no user code. Not a bug.

---

## PR-P — pacing (#1157 items 1-2), `flui-scheduler` only

Touches `ensure_visual_update` (scheduler.rs:2694) + its test module only. Not `schedule_frame_if_enabled`
(L2679, shared with `end_of_frame`, cluster-B territory) or `set_frames_enabled` (cluster-B v2 doesn't touch it).

### P1. `ensure_visual_update` phase gate — decision (a), kept and gated
Add Flutter's phase switch (`.flutter/…/scheduler/binding.dart:893-916`, verified @3.44.0) inside
`ensure_visual_update` only: idle/PostFrameCallbacks schedule, the 3 mid-frame phases no-op. It's a
Flutter-named public entry point (`RendererBinding::request_visual_update` → it, renderer_binding.rs:624-634)
an embedder will reach for; its contract must be right regardless of today's caller graph.
**ARCHITECTURE.md mapping entry:** pipeline visual updates are realm-owned pacing (ADR-0027 leapfrog zone) —
`PipelineOwner::request_visual_update` (accessors.rs:60) → `fire_need_visual_update()` → presentation.rs:524-529
(`visual_wake()`+`request_redraw()`), bypassing `UpdateScheduler` entirely. `frame_scheduled` carries
scheduler-side demand only; both carriers OR in `wake_action`.
**#1157's "widget-tier frame-count" criterion is unsatisfiable today** — no widget-reachable path calls
`ensure_visual_update` (only invoker: dead trait-default `handle_metrics_changed`, binding/mod.rs:274-293,
never called from flui-app). Record as dropped-by-decision, not met.
**Follow-up issue (file separately, don't implement — runtime-topology change, live-wire risk):**
> **Title:** Unify frame demand: route pipeline visual updates through `ensure_visual_update`
> **Body:** `PipelineOwner::request_visual_update` and `UpdateScheduler::ensure_visual_update` are two
> independent, OR'ed carriers in `wake_action` (frame_pacing.rs:72-95) — one realm-owned, one scheduler-owned.
> Unifying under one gated entry lets #1157's phase check govern all visual-update demand, not just
> ticker/`end_of_frame`. Live-wire risk: presentation.rs:524-529's callback runs inside the pipeline's own
> reentrancy-safe zone (doc: runs "while the CALLER holds the pipeline cell checked out") — routing through
> the scheduler needs that guarantee re-proven, not assumed.

### P2. Lock-discipline oracle completeness
- **DashMap probe re-targeted** (v1 targeted the wrong branch): `cancel_frame_callback` (scheduler.rs:1807-1831)
  takes `position`+`remove` for a still-queued id and never touches `cancelled`; `.insert(id,())` (L1831) fires
  only on the not-found branch. Test: cancel an already-fired/unknown id from inside a callback, `try_get` that
  id on `cancelled`, assert not `Locked`. Completeness-pin label kept; probe is per-key/shard (no all-shards API
  in DashMap 6.2.1) — state both.
- **`schedule_local_from_inside_local_post_frame_callback_defers` → real regression pin.** `take_queue`
  (post_frame.rs:78, `.take()`) swaps in an empty `Vec`, releasing the `RefCell` borrow before callbacks run.
  Reverting to in-place `borrow_mut()` iteration would `BorrowMutError` here — test reddens under that reversion.
- `LocalPostFrameLane::is_unlocked`: `#[cfg(test)] pub(crate) fn is_unlocked(&self) -> bool { self.inner.queue.try_borrow_mut().is_ok() }`, mirrors `TaskQueue::is_unlocked` (task.rs:515).

**Tests:** `ensure_visual_update_noop_during_persistent_callbacks` (red pin — registration methods,
scheduler.rs:2023-2047, don't request a frame, confirmed), `..._schedules_from_post_frame_callbacks`
(characterization pin, labelled), `cancelled_dashmap_not_locked_on_reentrant_cancel_of_a_settled_id`,
`schedule_local_from_inside_local_post_frame_callback_defers` (real pin now).

---

## PR-L — lock-drop sweep (#1150 remainder), `flui-scheduler` + `flui-foundation`

### L1. `TaskQueue::clear` — DELETE, not fix
Zero production callers (only `task.rs:483`'s own def + `tests/integration_tests.rs:1906`). Precedent:
`ARCHITECTURE.md:260-285`'s own entry for the retired `schedule_frame`/`current_frame()` family — identical
shape, identical choice ("delete… had no distinct semantics to preserve"). **Correction:** cite that
ARCHITECTURE.md entry, not Trigger 11 (scans `pub mod` decls only) or Cross.H7 (guards resurrected names) —
neither literally applies to one `pub fn`. Action: delete `TaskQueue::clear`; update the test call site.

### L2. `RemoveZombieSlotOnUnwind` — untouched, no test
Confirmed via re-trace (v1-correction #2): nothing significant drops. Do not add
`remove_zombie_slot_on_unwind_drops_future_outside_guard`; cluster D replaces the struct anyway.

### L3. Notifier removal sites — hazard class, one helper
`notifier_generic.rs:122,138,148,163,185`. `notify_unchecked` (L213-230) already snapshots-then-fires
correctly; these 5 are the only hazard. Real chain confirmed: `ListenerSubscription::drop`
(listener_registry.rs:249-254) → `RemoveFrom::remove` (L83-106) → `Notifier::remove`/`ChangeNotifier::remove_listener`.
No existing closure re-enters the same notifier on drop — report as a **hazard class with a self-authored red
test** (a synthetic listener whose drop calls back in), not a reproduction. Fix: one private
extract-then-drop helper backing `remove`/`remove_even_if_disposed`/`remove_all_unchecked`/`dispose` (2 tests
cover all 4). `add_unchecked`: `let evicted = …insert(id, listener); debug_assert!(evicted.is_none(), "listener
ids are monotonic, notifier_generic.rs:76-78");` — states the invariant, no red test possible. Add
`#[cfg(test)] pub(crate) fn is_unlocked(&self) -> bool { self.listeners.try_lock().is_some() }`.

### L4. Assignment-through-guard (`*x.lock() = new` drops the OLD value under the guard)
- **`async_driver.rs:536` is test-only** (inside `mod tests`, starts L515-516 — `Controlled::poll`'s fixture),
  not production as the brief claimed. Production task-waking builds a FRESH `Waker` every poll (L341, L416),
  never persists/overwrites an `Option<Waker>` slot — no production hazard exists in this file. Don't fix a
  site that isn't there.
- **`listener_registry.rs:157,163`** (`set_on_first_listener`/`set_on_last_listener`) — confirmed real:
  overwrites a previously-installed `Box<dyn FnMut>` under the guard. Fix: extract-then-drop-after. Red test:
  a Drop-canary hook overwritten by a second `set_on_first_listener` call, probing `is_unlocked()` from its drop.
- `flui-scheduler`'s 10 non-test hits (scheduler.rs:592,1018,1030,1383,1399,1464,1623,2702,2814,2835) are all
  plain-`Copy` data (`Option<Instant>`, `Duration`, `PerformanceMode` — `#[derive(Copy)]` config.rs:105 — or
  `FrameTiming`, all-plain-data fields, no `impl Drop`, frame.rs:547-565). **Decision: a same-line-above
  marker** (`// LOCK-OK: plain data, no significant drop`, its own line above — MEMORY: markers must survive
  rustfmt, which moves trailing comments but not a preceding line), not a per-path allowlist.

### L5. Port-check trigger, both regexes re-run
Regex 1, widened (unwrap/expect/trailing is_some support, write/borrow_mut receivers): **same 6 hits as v1's
narrower regex** — the 5 notifier sites + `async_driver.rs:221` (L2's confirmed false positive, excluded with
reasoning, not fixed). No new violations surfaced by widening. Regex 2: results above (2 real, 10 marked).
**Test-hits policy:** accept test hits as violations too (tests are where the shape gets copied from) unless
unreasonable — measured: all non-listed hits are inside confirmed `#[cfg(test)] mod tests` boundaries
(scheduler.rs:3074, async_driver.rs:515, task.rs:554), trivial types (`bool`/`i32`/test-local markers) — small
enough to leave, noted as a real but low-value residual, not silently ignored. **Trigger name:**
`LockDiscipline/StatementDrop` (bare 1-23 closed per AGENTS.md; #22/#23 already allocated). `docs/PORT.md`
entry: both regexes, block-scope-then-drop escape (worked example: `cancel_frame_callback`, scheduler.rs:1807-1831),
`LOCK-OK` marker convention.

### L6. Lint — decided: do NOT enable `significant_drop_tightening` anywhere
Nursery; `#[expect]` breaks as `unfulfilled_lint_expectations` on toolchain drift; `-D warnings` would make a
crate `#![warn]` deny. **v1 conflated two lints into one count — corrected:** `significant_drop_tightening`
alone flags `claim_slot.rs:330,403` + 9 documented-invariant sites + `async_driver.rs:440`; `claim_slot.rs:119`
needs the SEPARATE `significant_drop_in_scrutinee` lint (why a tightening-only run shows 330/403 but not 119,
while v1's combined run showed all three as one number — sloppy). Fix the two real sites on their own merits:
- **`claim_slot.rs:118-122`, `wake_task`** — confirmed by direct empirical repro (not just lint output): built
  a throwaway `if let Some(x) = mutex.lock().unwrap().take() { try_lock probe }` under rustc 1.98.1, editions
  2021 AND 2024 — guard is **still held** during the body (`try_lock` fails inside it). Matches
  `ARCHITECTURE.md:266-269`'s own prior documented bug in this crate (same shape, "the if-let scrutinee's
  temporary lives through the block even under edition 2024's if-let rescoping") — a known recurring shape.
  Fix: `let woken = self.waker.lock().take(); if let Some(w) = woken { w.wake(); }`. Red test: a waker whose
  `wake()` probes a new `ClaimSlot::is_unlocked()` (add if absent), asserts not locked.
- **`task.rs:491`, `count_by_priority`** — scope the lock to the `for` loop only; no hazard, no red test needed.

### L7. `TaskToken::cancel` panic policy — decided: PROPAGATE, `Drop` guards on `thread::panicking()`
`cancel()` always propagates; `Drop for TaskToken` propagates unless already unwinding, else contains with
`tracing::error!`. **v1's "contain" recommendation is replaced:** `cancel()`'s dominant path is `ViewState::dispose`
→ `FutureBuilder::unsubscribe` (future_builder.rs:241) → `dispose` (L347-349), and `on_unmount`
(behavior.rs:803) ALREADY wraps dispose in `catch_unwind(..)`, reporting via `owner.record_hook_panic(...)`.
Containing again inside `cancel()` would be redundant AND hide the panic from that recovery accounting. The
real risk PROPAGATE must guard is a **double panic during Drop while already unwinding** — concretely
`secondary_window.rs`'s TLS `PENDING_SECONDARY_WINDOW_OPENS: RefCell<Vec<TaskToken>>` (~L225-231) dropping
during thread teardown mid-unwind — hence the `thread::panicking()` guard, matching std's own Drop convention.
Record on both docs. Tests: `cancel()` propagates an exact panic payload; a Drop while `thread::panicking()`
does not abort and logs via `tracing::error!`.

### L8. Revert-matrix, precise redden descriptions
- `poll_ready_self_cancel_during_pending_removes_slot_once` — the future's `Drop` probes
  `AsyncDriver::is_unlocked()` (confirmed exists, async_driver.rs:489), fails fast on a guard-order regression.
  Redden: reverting the order makes `poll_ready` unconditionally `task.future = Some(future)`, **resurrecting a
  self-cancelled future** — a correctness failure, not a hang.
- `nested_token_disposal_cancels_child_and_parent` — relabel as a **widget-level acceptance test**: oracle
  mirrors `future_builder_dispose_cancels_and_never_rebuilds`. Cannot be bounded (`ElementTree` is `!Send`, no
  watchdog thread) — the async_driver-level bounded test above is the actual deadlock backstop; this only
  proves widget-tier wiring reaches `cancel()`. Add an in-destructor `is_unlocked` probe so a regression fails
  fast rather than hangs the suite.

### Cross-PR
PR-P: `ensure_visual_update` + test module only — no conflict with cluster B (v2 doesn't touch
`set_frames_enabled`) or D. PR-L touches `async_driver.rs` only for `cancel`/`Drop` policy — cluster D
rewrites `poll_ready`/`Inner`; **land PR-L before D; D rebases.**

MEMORY: `wake_action` (frame_pacing.rs) ORs two independent frame-demand carriers —
`UpdateScheduler::frame_scheduled` (ticker/end_of_frame) and the realm's `needs_redraw` (pipeline visual-update
pacing, ADR-0027 zone). Tracing only one and calling the other dead is the mistake v1 made.

MEMORY: a Drop guard that runs AFTER its caller already `.take()`n the dangerous field drops nothing
significant — trace what the guard's own scope actually holds at the moment it runs, not "it drops under a
lock" in the abstract (`RemoveZombieSlotOnUnwind`).

MEMORY: `if let Some(x) = mutex.lock().unwrap().take() { BODY }` holds the guard through BODY in rustc 1.98.1,
edition 2021 AND 2024 (empirically reproduced via `try_lock` inside BODY) — edition-2024 if-let rescoping does
not fix this shape; this exact codebase already documents an identical prior bug in a retired API for the
same reason (`flui-scheduler/ARCHITECTURE.md:266-269`).

**Verdict: pre-code ACCEPTABLE for PR-P and PR-L as specified** — conditional on PR-P recording the
"unsatisfiable by decision" criterion honestly (filing, not implementing, the follow-up issue) and PR-L
deleting `TaskQueue::clear` rather than patching it, and NOT adding the two v1 tests since disproven
(`remove_zombie_slot_on_unwind_drops_future_outside_guard`; an `async_driver.rs:536` fix).

## Plan-review amendments to v2 (harsh-critic confirmation — ACCEPTABLE once applied)

A1. **L5 test-hits policy — decided.** Regex 2 measured: 12 PROD hits (2 real + 10 `LOCK-OK`) and 20 inline
`mod tests` hits (scheduler.rs ×11, async_driver.rs ×4, task.rs ×1, notifier_generic.rs ×1, notifier.rs ×1, …);
`check`'s only file-level exclusion (`--glob '!**/tests/**' --glob '!**/test*.rs'`, port-check.sh:597-600) does
not see inline modules, so the trigger would print 20 VIOLATION lines on day one. Policy: (d) FIRST refine regex 2
to exclude a literal RHS (`true|false|None|[0-9]+(\.[0-9]+)?|"…"`) — a slot assigned a bool/int/None literal holds
plain data, so the OLD value's drop cannot be significant; this is a precision refinement, not an exclusion. Then
(b) FIX every remaining test hit (e.g. `*c.lock() = Some(x)` → `let _prev = mem::replace(&mut *c.lock(), ..)`
with the guard scoped, or an `AtomicBool::store`), never leave one. Re-measure and list the final hit set in the
PR body. Option (c) (awk scope tracker keyed on the first `#[cfg(test)]`) is rejected as imprecise.
A2. **Marker mechanics.** Use `PORT-CHECK-OK-LOCK: <reason>` (the existing family: SP3/SP4/SP6/SP8/UNIT/STUB/
DOWNCAST; a bare `LOCK-OK:` breaks the one-census `rg PORT-CHECK-OK`). Matching: copy trigger 10's SP3 windowed
scan (same line OR the line above OR the next two lines — port-check.sh:588-596, with its rustfmt rationale),
implemented as its own rg+awk pass like trigger 10, NOT via the `check` helper (whose marker filters are same-line
`grep -Ev` only). State this in the PORT.md entry.
A3. **Move `poll_ready_self_cancel_during_pending_removes_slot_once` to cluster D** — it pins the drop order at
async_driver.rs:440-453, the exact block D's `PumpGuard` rewrites; written in L it is rewritten in D. PR-L keeps
`nested_token_disposal_cancels_child_and_parent` (widget wiring, D-independent) and the `cancel()`/`Drop`
policy (TaskToken only, independent of `poll_ready`/`Inner`). Order stands: PR-L before D; D rebases.
