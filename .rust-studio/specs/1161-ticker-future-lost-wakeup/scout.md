# Code map — #1161 ticker futures

`main` at `6ee4a51e`. Citations are `file:symbol`; line numbers move, symbols do not.

## The finding that reframes the issue: zero awaiting consumers

A workspace-wide census of `TickerFuture` / `TickerFutureOrCancel` found **no caller anywhere that
`.await`s either future** — not in production, not in examples, not in tests.

- `crates/flui-animation/src/controller.rs`, `AnimationController::restart_ticker` — calls
  `ticker.start(..)` as a **bare statement and discards the return value**. `AnimationController`
  has no `TickerFuture` field and never inspects one. This is the only non-test production consumer
  in the workspace.
- `crates/flui-widgets/src/navigator/{route,page_route,transition_route,binding}.rs` — seven sites,
  **prose only**: each documents that FLUI's `AnimationController` returns no `TickerFuture`, which
  is *why* the navigator hand-drives transitions. No code reference.
- `crates/flui-scheduler/examples/animation_ticker.rs` and the crate's tests — **inspect only**
  (`is_pending`/`is_complete`/`is_canceled`), plus five single manual `poll`s, every one of them on
  an already-resolved future, which returns before `listen()` is ever reached.

**So the lost wakeup is latent, not live.** That does not lower the bar — it changes what the work
is. We are removing a trap *before* consumers exist, which is this project's standing "design for
the future, implement for the present" rule, and it raises the larger question the map exposes:
`TickerFuture` is a public export with a checked contract entry and **no awaiting consumer**, which
is the repository's own dominant defect class (memory: `shipped-seams-never-wired`).

## A discrepancy in the checked contract manifest

`docs/runtime-contract.toml`'s `[[surface]]` for `flui_scheduler::{Ticker, TickerProvider,
TickerGroup}` records `failure_semantics = "TickerCanceled futures on cancellation"` and
`consumers = ["flui-animation controllers", "flui-widgets implicit animations"]`.

Both named consumers exist, but **neither awaits a future**, so the recorded failure semantics
describe a contract nothing currently observes. Either the entry is aspirational and should say so,
or the consumers list is wrong. Worth settling in this change rather than leaving a manifest that reads
as evidence of coverage it does not have.

## Who resolves the futures, and from where

| Site | Visibility | Resolves |
|---|---|---|
| `ticker.rs:Ticker::stop` → `set_complete` | **`pub`** | `Complete` |
| `ticker.rs:Ticker::dispose` → `set_canceled` | **`pub`** (also via `impl Drop for Ticker`) | `Canceled` |
| `ticker.rs:Ticker::reset` → `set_canceled` | **`pub`** | `Canceled` |

`set_complete` / `set_canceled` are `pub(crate)` and have exactly these three drivers. Both drop the
`state` guard before `notify` — the *state* lock is clean; the hazard is `event-listener`'s own
internal `std::sync::Mutex`, held across `task.wake()`.

**Cross-thread reachable.** `Ticker` is auto-`Send + Sync`; `TickerFuture` is `Send + Sync` via its
`Arc`. The three drivers take `&mut Ticker`, so a cross-thread call needs an outer
`Arc<Mutex<Ticker>>` — a shape the crate's own tests already use
(`manual_and_auto_dispatch_from_two_threads_never_double_invoke`). Nothing pins the polling thread
to the resolving thread. The window is reachable from the public API alone:
`let f = ticker.start(cb); /* poll f */ … ticker.stop()` from another thread.

## Test coverage: the gap, by enumeration rather than assertion

24 `#[test]`s in `ticker.rs`. **Zero** exercise: a pending future woken by a completion, a dropped
await releasing its waker, a panicking or re-entrant waker, or lock discipline around
`TickerFutureInner::state` or the `Event`. The one `poll` in the file is on an already-`Canceled`
future and takes the fast return before `listen()`. The four adjacent tests in
`tests/integration_tests.rs` each poll **once** with a no-op vtable and assert `Pending`/`Ready`;
none completes a pending future or re-polls.

`a_panicking_tick_callback_leaves_the_slot_restored` covers a panicking **tick callback**, not a
waker. `stale_callback_is_dropped_outside_the_lock` is a lock-discipline test — of
`Mutex<TickerInner>`, not of the future's state or event.

## No oracle can see this family today

`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held` destructures `SchedulerInner`
and probes `FrameState`/`CallbackState`/`BindingState`/`TaskQueue`/`AsyncDriver`.
`TickerFutureInner::state` lives in an `Arc` owned by the `TickerFuture` and by
`TickerInner::active_future`, reachable from neither; `TickerInner`'s own mutex sits behind
`Ticker::inner`. The ticker's narrower `Ticker::try_state` (`#[cfg(test)]`) probes only
`Mutex<TickerInner>`.

**There is no existing oracle that can observe a lock held across a ticker-future notify**, and no
`static_assertions` pin on either future's auto-traits. Both have to be built.

## Sibling precedent to reuse rather than re-derive

`scheduler.rs` after #1163 — and note it uses a **hand-rolled `Weak` + `Waker` registry, not
`event-listener`**, so adopting its shape here means *replacing* the primitive, not wrapping it:

- `notify_frame_completion` — drain under the lock into a local **first**, `upgrade()` at loop-body
  scope (failed upgrade = cancelled, untraced), state written and waker `take`n under a scoped
  guard, guard released, then `catch_unwind` **per waker**, first payload `resume_unwind` after the
  whole loop.
- `FrameCompletionFuture::poll` — two acquisitions, never one: a clone-free fast path
  (`completed.take()`, then `will_wake`), the clone **outside** the guard, then a re-acquire that
  **re-checks `completed`** because the guard was released across the clone, and the displaced waker
  bound out and dropped after the guard. The drop-order dependency is documented as load-bearing.
- The registry lock-order rule: *"`completion_waiters` strictly before `FrameCompletionState`, never
  nested, and neither held across ANY `Waker` operation (`clone`, `wake`, or drop)."*
- `lock_discipline_tests.rs:completion_waker_runs_with_no_scheduler_lock_held` — the waker-shaped
  oracle to copy.
- `tests/end_of_frame_lifecycle.rs:dropping_a_polled_waiter_releases_the_executor_waker_immediately`
  — the `Wake`-impl strong-count cancellation reproducer to copy.

## `event-listener` in this workspace

Exactly one crate declares it (`flui-scheduler`, `event-listener = "5.3"`, resolving to 5.4.2, and
**not** via `[workspace.dependencies]` — a hygiene deviation from the repo's own convention), and
exactly one file uses it: `ticker.rs`. `.listen()` at three sites — `when_complete_or_cancel` (the
**correct** register-then-recheck order) and both `poll` impls (the racy order). `.notify(usize::MAX)`
at `set_complete` and `set_canceled`.

## Exported surface

`lib.rs` re-exports `Ticker, TickerCallback, TickerCanceled, TickerFuture, TickerFutureOrCancel,
TickerGroup, TickerId, TickerProvider, TickerState`; the `prelude` carries only
`Ticker, TickerProvider, TickerState`. The `pub use` line is **verbatim in
`docs/runtime-contract.toml`'s export manifest**, so changing the list fails
`just runtime-conformance-check` until updated deliberately.

## Gaps in this map

`.flutter` was read separately (`reference.md`). **`.gpui/` does not exist in this checkout** — not
"exists but empty", which is what I first wrote from a shell result I misread; `ls -d .gpui` returns
`No such file or directory`. Either way the GPUI reference is locally unavailable and must not be
reasoned about from memory, but the two claims are different and only one is true. Settled from the
public Zed repository instead (`survey-mechanisms.md`): GPUI has **no animation-completion API at
all** — `crates/gpui/src/elements/animation.rs`'s `done` is a local that never escapes and only
decides whether to request another frame, and `SpringPlayback::{Completed, Cancelled}` is a
caller-supplied input, not a status report. So GPUI offers no precedent here in either direction. `TickerGroup`'s surface
was not enumerated beyond confirming it produces no futures.
