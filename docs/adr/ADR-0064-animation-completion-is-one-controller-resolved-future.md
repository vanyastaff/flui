# ADR-0064: Animation completion is one controller-resolved future; the ticker resolves nothing

- **Status:** Accepted
- **Date:** 2026-09-15
- **Supersedes:** ADR-0020's push-completion mechanism (a status-listener seam in place of a
  `TickerFuture`)

## Context

`Ticker::start`/`stop`/`dispose`/`reset` used to return or resolve a `TickerFuture` the ticker
owned, as Flutter's `Ticker` does. But every production caller of `ticker.stop()` is
`AnimationController`, and every one of those calls runs under the controller's own
non-reentrant `inner` mutex (`reset`, the natural ends of `tick_time_based`/`tick_simulation`,
`dispose`, `restart_ticker`, `stop_running`). A future resolved there runs its wakers and
continuations under that lock, so a status listener, waker or completion callback that touches
the controller deadlocks. Issue #1161's lost-wakeup fix (register before the decisive read)
made those wakers fire for the first time, turning the latent hazard into a live one.

Flutter cannot have this bug: its controller forwards to the ticker with nothing else held.
FLUI's controller is one lock with many run-starting entry points, so resolution has to live at
the layer that owns that lock. The navigator also needed a future to await for a route's
entrance transition (#1161).

## Decision

1. **The ticker resolves nothing.** `Ticker::start*` return `()`; `stop`/`dispose`/`reset` are
   fire-and-forget. The "started twice" refusal checks `TickerState::is_running()`
   (`Active | Muted`) and logs at `error!` with the ticker lock released.

2. **One future for both outcomes.** `TickerFuture: Future<Output = Result<(), TickerCanceled>>`
   resolves on completion and on cancellation; `TickerFutureOrCancel` is gone. Flutter keeps two
   futures because its base future's owner could not resolve it on the cancel path; with a
   caller-owned completer there is no reason for two. `TickerCanceled` is `#[non_exhaustive]`.

3. **A two-phase completer.** `TickerFuture::pending() -> (TickerCompleter, TickerFuture)`.
   `complete`/`cancel` **publish** under one lock (set the resolution, take the continuations)
   and return a `TickerDelivery`; `deliver()` runs each continuation under its own
   `catch_unwind`, notifies pollers, then re-raises the first payload (logged instead if already
   unwinding). Dropping an undelivered `TickerDelivery` delivers; dropping a `TickerCompleter`
   publishes `Canceled`. The split lets a caller finish a state change under its own lock and
   fan out after releasing it.

   `when_complete_or_cancel(f)` never blocks. On an already-resolved future (including between
   publish and delivery) `f` runs immediately on the calling thread — a divergence from Dart's
   microtask scheduling that callers must be re-entrancy-safe against.

4. **`AnimationController` owns one completer per run** (`active_run`). Every run-starting
   method returns `Result<TickerFuture, AnimationError>`. Natural ends publish `complete()`
   before the controller lock drops (Flutter's order: complete, then notify) and deliver after.
   Starting a new run cancels the displaced one, firing the new run's status before delivering
   the old run's cancellation. A zero-distance run returns an already-complete future. Every
   site funnels through one chokepoint, `AnimationController::finish`, which drops the lock and
   then delivers; a source-guard test keeps `deliver()` out of the rest of `controller.rs`.
   The per-site table is in `flui-animation`'s `## Mapping decisions`.

5. **Navigator consumer.** `PushCompletion::Animating(TickerFuture)` carries the future
   `did_push` returns. Its continuation is registered in `NavigatorShared::apply`, after the
   installing flush and with the history unlocked, never in `handle_push`: `flush` re-drains
   commands between passes, so an early registration on an already-resolved future would settle
   in the same flush and change which flush observes the transition complete. The rebuild slot
   the continuation uses is read at fire time, since the navigator may not be mounted yet when
   the future is created.

6. **Drop and foreign code.** There is no `Drop for AnimationController`; the last `Arc`
   drops `active_run`, which cancels and delivers with no controller lock held. Foreign code
   still runs under the controller guard (`restart_ticker` → `request_frame` → the embedder's
   `on_frame_scheduled`), so `restart_ticker` runs before any completer for the run exists, and
   `controller.rs` emits no `tracing` under the guard (warnings are raised after `finish`
   unlocks), because a subscriber is arbitrary code. Scheduler-driven and detached-ticker
   controllers hold a `ticker → callback → controller` cycle until an explicit `dispose()`.
   `Event::notify`'s waker precondition (a waker that re-polls from `wake()` deadlocks inside
   `event-listener`) is unchanged and applies at the new delivery sites (tracked in #1165).

7. **Porting rule.** Flutter's `.then`/`whenComplete` on a base `TickerFuture` means "only on
   completion": port as `when_complete_or_cancel(|r| if r.is_ok() { .. })`. Flutter's
   `whenCompleteOrCancel` ports unconditionally.

`TickerFuture` is not `#[must_use]` (a caller may only care that the run started, as with
`RouteResult`); `TickerFuture::pending()` is. Don't write `let _ = c.forward().unwrap()`, which
binds a future and trips `clippy::let_underscore_future`.

## Flutter divergence

Flutter resolves the future in the ticker because nothing holds a lock around its `stop()`.
FLUI moves the same fact — a run completes or is canceled, exactly once — to the controller,
the layer that owns the lock and the complete/cancel distinction.

Known gap: a zero-*duration* run over a real distance completes on the controller's first tick,
not synchronously in `forward()` as Flutter's `_animateToInternal` special-cases it (#1171).

## Consequences

- A waker or continuation re-entering a controller cannot deadlock: nothing in
  `flui-scheduler` resolves under a caller's lock, and only `AnimationController::finish`
  delivers in `flui-animation`.
- The navigator awaits route transitions directly.
- Breaking, on purpose, pre-1.0: `Ticker::start*`'s return type, the removed
  `TickerFutureOrCancel`/`or_cancel`, and the fourteen run-starting return types.
- `flui-scheduler` exports `TickerCompleter`/`TickerDelivery`; `flui-animation` does not
  re-export them.

## Alternatives rejected

- **`catch_unwind`-and-retry around `notify`.** `event-listener`'s notified counter underflows
  on a retried panicking waker, turning a contained panic into an abort in its `Drop`.
- **Keep the ticker as resolver; require callers to unlock before `stop()`.** The controller
  must update status, value and simulation atomically with the stop; splitting the tick lets a
  re-entrant `forward()` see a half-finished tick.
- **A single-phase `resolve()`.** That is the old shape, which cannot be called under a
  caller's lock.
