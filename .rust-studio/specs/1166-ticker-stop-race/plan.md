# Plan v2 — #1166: a concurrent stop() can leave a stopped ticker with a live frame registration

v1 (lead: ACCEPTABLE) proposed reusing `should_schedule_tick()` at both tails. Plan review:
concurrency lens ACCEPTABLE with a test-shape correction; harsh-critic RESHAPE NEEDED — the slot-`Ready`
term of `should_schedule_tick()` at a COMMIT site cancels a valid registration whenever another dispatch
holds the slot `CheckedOut`. Verified by reading `Ticker::tick(&self)` (`ticker.rs:~821-842`: checkout →
slot `CheckedOut`, callback runs with no lock) and the predicate (`ticker.rs:~309-313`). v2 keeps v1's
shape and replaces the predicate.

## Root cause (both tail sites)
`ARCHITECTURE.md:516-528` already records the shape and names both `schedule_tick_if_active`
(`ticker.rs:~943-993`, via `start()`/`unmute()`) and `tick_and_reschedule_static`'s tail
(`ticker.rs:~1012-1102`). Both: register with the scheduler (no ticker lock) → re-lock → test only
`scheduled_callback_id.is_none()` (#1059's "did another registration claim the slot"). A concurrent
`stop`/`dispose`/`reset`/`mute` in the register→record gap sets `state != Active`, finds the id `None`
(cancels nothing), and the tail then records a live id against a stopped/muted ticker.

## Fix — symmetric self-cancel at both tails, on a named COMMIT predicate
Add to `TickerInner`:
```rust
/// Commit-side predicate for the two registration tails: may the id we just
/// minted be recorded, or must it be cancelled? Deliberately NOT
/// `should_schedule_tick()`: that predicate DECIDES to schedule and includes
/// the slot term (a `CheckedOut` slot means a dispatch is in flight and will
/// re-arm itself). At the commit site the decision was already taken; a slot
/// checked out by a concurrent manual `tick()` is not a reason to retract the
/// registration — retracting it would leave the ticker active-but-unscheduled
/// forever, because `tick()` never schedules. So: still `Active`, and no other
/// registration claimed the slot while ours was in flight.
fn may_record_registration(&self) -> bool {
    self.state == TickerState::Active && self.scheduled_callback_id.is_none()
}
```
Both tails: `if guard.may_record_registration() { record } else { drop(guard); trace; cancel_frame_callback(cb_id) }`.
Split the trace so "already exists" (redundant) and "no longer active" (raced) are distinguishable — read
`state` before dropping the guard to pick the message.

Interleavings (both reviewers): before the re-read — gated already; between re-read and register, and
between register and tail lock — the tail sees `!Active` and cancels its own id; after record — `stop()`
cancels normally. Mutual exclusion: `stop()` sets `state` and takes the id in ONE critical section
(`ticker.rs:~704-712`); any tail deciding after it sees `!Active`. `stop()`→`start()` inside the window:
whichever tail records first wins; the other sees `Some(..)` and cancels its own — exactly one live id,
and both closures are `tick_and_reschedule_static` over the same `Arc`s (interchangeable). `mute()`→
`unmute()` in the window: tail sees `Muted` → cancels; `unmute()` re-arms via `schedule_tick_if_active`;
if both land before the tail lock, tail sees `Some(cb_B)` → cancels its own. `stop()`'s own take-then-
cancel two-step is safe: `CallbackId` is minted by a monotonic never-reused `fetch_add`
(`id.rs:~110-113`), so a newer id cannot alias the snapshot. Lock order: `cancel_frame_callback`
(`scheduler.rs:~1807-1832`) locks only `CallbackState::transient` and drops the removed box outside it;
no ticker call site nests the two locks. Lease ordering: `drop(lease)` (`~1056`) precedes the re-read
and the tail, so the tail never sees its OWN checkout.

## Files / anchors
1. `ticker.rs` `TickerInner` (~L157-313): add `may_record_registration()` with the doc above.
2. `ticker.rs:~982-992` (`schedule_tick_if_active` tail) and `~1090-1101` (`tick_and_reschedule_static`
   tail): swap the predicate; split the trace message.
3. Doc prose — `stop` (~L683-703), `dispose` (~L476-486), `mute`/`reset` by reference: restore the outcome
   guarantee: "never DELIVERS a tick to a stopped run — cancelled by this call, cancelled by the racing
   tail's own self-cancel, or inert on entry (the callback clears the id and returns on `state != Active`)".
   Do NOT claim the TOCTOU window is gone (`stop()` can return before the racing tail's cancel executes;
   and the transient loop's `cancelled` check is read outside the `transient` lock, so a popped callback
   can be invoked once — inert).
4. `ARCHITECTURE.md:516-528`: reclassify the concrete consequence (a live registration surviving past the
   race) as closed by symmetric self-cancel at both tails; the window-level sentence stays; DROP the
   "starvation/orphan hazard" label for cross-thread `mute()`→`unmute()` (traced: neither); add two
   residual notes: (a) the popped-then-cancelled invocation gap above; (b) the top-of-tick unconditional
   `scheduled_callback_id = None` (~L1028) assumes the firing closure is the recorded one — a stale closure
   fired concurrently with a fresh record would clobber it (needs two frames on two threads; structural,
   not #1166).

## Tests (deterministic — the synchronous `on_frame_scheduled` hook fires inside `schedule_frame_callback`
   on the `frame_scheduled` false→true edge; `handle_begin_frame` clears the latch before the transient
   drain; `request_frame_impl` clones the hook out of its mutex before calling it, so no scheduler lock
   is held in the hook. `mod tests` is a descendant of `ticker` and may touch `TickerInner` fields.)
| # | Test | Shape | Oracle | Revert |
|---|---|---|---|---|
| T1 | `a_stop_racing_the_tick_tail_leaves_no_live_registration` | `Arc<Mutex<Ticker>>` fixture; `start()`; THEN install a hook that calls the REAL `ticker.lock().stop()` (installing before `start()` would fire on `start()`'s own registration and test the wrong site — arm a one-shot flag too); `execute_frame()` | `scheduler.transient_callback_count() == 0` (queue length; the tail's cancel REMOVES the entry). Do not list `state == Stopped` as evidence — `stop()` sets it regardless | tail predicate back to `is_none()` alone → count 1 |
| T2 | `a_stop_racing_the_start_tail_leaves_no_live_registration` | hook installed before the first `start()`; the test thread is inside `start(&mut self)` holding the fixture guard, so the hook must FIELD-SIMULATE `stop()`'s critical section — mirror ALL FOUR writes of `stop()`'s block (~L709-716: state, `slot.clear_if_ready()`, `scheduled_callback_id.take()`, future resolution), not `state` alone; labelled as a simulation with this reason | count == 0 | `schedule_tick_if_active`'s predicate back to `is_none()` alone |
| T3 | `a_concurrent_checkout_at_the_tail_does_not_retract_the_registration` | `start()`; hook installed after; hook moves the `Ready` callback out of the slot into `CheckedOut` (simulating a manual `tick()` in flight on another thread), leaving state `Active`; after `execute_frame()` the test restores the slot | count == 1 and `scheduled_callback_id.is_some()` | predicate changed to `should_schedule_tick()` → count 0 (pins the critic's finding) |
| — | regression controls (pin nothing about this fix): `restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick`, `mute_then_unmute_inside_tick_delivers_next_frame_once`, `manual_and_auto_dispatch_from_two_threads_never_double_invoke` | re-run | — | — |
Revert matrix holds: the hook fires INSIDE `schedule_frame_callback`, i.e. after the pre-registration
re-read returned true, so the re-read cannot mask a reverted tail.

## Rejected alternatives
- One critical section around decide+register: reaches into the scheduler under the ticker lock — new
  lock-order edge, and `request_frame_impl` fires the embedder hook synchronously (user code under a lock).
- `RESERVED` sentinel / `pending_registration` flag written before registering: `stop()` can see a claim
  but cannot cancel an id that does not exist yet, so the tail must self-cancel anyway; pollutes the
  `cancelled` DashMap via `stop()`'s cancel path.
- Run-generation counter: the exact predicate, gets `CheckedOut` right by construction, but a new field
  incremented at five sites for no behaviour the two-field predicate misses. Reconsider if a third tail
  ever appears.

## Risks / gate
No public API change; one state read under a lock already taken. ASYNC-GATE; project gate `just ci`
(narrow first: `cargo nextest run -p flui-scheduler`, `cargo clippy -p flui-scheduler --all-targets -- -D warnings`,
`cargo fmt -p flui-scheduler -- --check`). Rejection reasons: one tail only; slot term in the commit
predicate; doc claims the window is closed; a simulated `stop()` where a real one is callable; the
"already exists" trace reused for the raced branch.
