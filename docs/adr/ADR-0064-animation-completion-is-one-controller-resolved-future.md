# ADR-0064: Animation completion is one controller-resolved future; the ticker resolves nothing

*A ticker that resolves its own future was one fact told at the wrong layer:
every `ticker.stop()` `AnimationController` makes already runs under the
controller's own lock. Moving resolution up to that lock's owner — not
inventing a new contract — closes the hazard #1161's lost-wakeup fix made
reachable and gives the navigator (#1161) something to await.*

---

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** @vanyastaff
- **Scope:** `flui-scheduler`'s `Ticker`/`TickerFuture` public surface
  (`ticker.rs`) and `flui-animation`'s `AnimationController` run-starting and
  run-ending methods (`controller.rs`). Amends
  [ADR-0020](ADR-0020-transition-modal-route-seam.md) §1.5 and §2.

---

## Context

Issue #1161 found that `TickerFuture`/`TickerFutureOrCancel` had never been
exercised against cancellation, an inline re-poll, or a panicking waker.
The register-before-read fix for #1161 closed the lost-wakeup defect that
finding led to (register a listener before the decisive state read, not
after) and, in doing so, made every waker on these futures fire for the
first time in this crate's history — previously the hang suppressed them.
That is exactly the class of change that turns a latent hazard into a live
one: a precondition nothing ever exercised is now exercised on every stop,
dispose, and reset.

The hazard is concrete. `Ticker::start`/`stop`/`dispose`/`reset` returned or
resolved a `TickerFuture` the ticker itself owned (`TickerInner.active_future`),
matching Flutter's `Ticker.start()` → `_ticker!.start()`/`stop` →
`_ticker!.stop` (`animation_controller.dart:861-871`, `:891-900`). But every
production caller of `ticker.stop()` is `AnimationController`, and every one
of *those* calls runs **under the controller's own `inner` mutex**, in
`controller.rs`: `reset`, `settle_at_target`, `tick_simulation`, both ends of
`tick_time_based` (repeat-exhausted and non-repeating), `dispose`,
`restart_ticker`, and `stop_running`. A future the ticker resolves runs its
wakers and — after this same issue's completer redesign —
its continuations from inside `resolve`/`deliver`. Held under a
non-reentrant `parking_lot::Mutex`, that is a caller code path (a status
listener, a waker, a `when_complete_or_cancel` callback) re-entering the
controller and self-deadlocking, now reachable for the first time because
#1161's lost-wakeup fix made the wake path actually fire.

Flutter cannot have this bug: `TransitionRoute`'s controller is vsynced to
the *navigator*, not to a route-owned ticker with its own lock, and
`_startSimulation`/`stop` forward to the ticker with nothing else held. FLUI's
`AnimationController` is exactly the layer Flutter's design avoids needing —
one lock, many run-starting entry points — and that is precisely the layer
this contract needed to live at.

## Decision

**1. The ticker resolves nothing.** `Ticker::start`/`start_default`/
`start_typed` return `()`; `stop`/`dispose`/`reset` are fire-and-forget.
`TickerInner.active_future` is deleted. The "started twice" refusal survives,
restated directly on the fact it always meant: `TickerState::is_running()`
(`Active | Muted` — muting pauses a run without ending it), not a future's
presence. A refused start logs at `error!` and drops the caller's callback,
both with `Mutex<TickerInner>` released.

**2. One future, resolvable by anyone who holds its write half.**
`TickerFutureOrCancel` and `TickerFuture::or_cancel` are deleted.
`TickerFuture: Future<Output = Result<(), TickerCanceled>>` is the only
ticker-adjacent future, and it resolves on cancellation too — Flutter's
`_cancel` completing only a secondary future was a consequence of the base
future being owned by code that could not safely resolve it on that path
either; with resolution moved to a caller-owned completer, there is no
reason left to keep two futures for one outcome.

**3. A two-phase completer.** `TickerFuture::pending() -> (TickerCompleter,
TickerFuture)`. `TickerCompleter::complete`/`cancel(self) -> TickerDelivery`
**publish**: under one lock, set the durable resolution and take every
continuation registered so far. `TickerDelivery::deliver(self)` **delivers**:
run each continuation (its own `catch_unwind`, the payload logged at
`error!` immediately), then notify pollers, then re-raise the first caught
payload — unless `std::thread::panicking()`, in which case it is logged
instead of replacing an unwind already in flight. `Drop for TickerDelivery`
delivers if `deliver()` was never called; `Drop for TickerCompleter`
publishes `Canceled` and delivers. Splitting publish from delivery is what
lets a caller finish a state change while still holding its own lock and
defer the fan-out until after that lock releases — the exact shape
`AnimationController` needed and did not have.

`TickerFuture::when_complete_or_cancel(&self, f: impl FnOnce(Result<(),
TickerCanceled>) + Send + 'static)` replaces the blocking version: it never
parks a thread. On an already-resolved future — including in the window
between publish and delivery — `f` runs immediately, on the calling thread;
a divergence from Dart's microtask scheduling that callers must be
re-entrancy-safe against, and cross-registrant ordering is not a contract.
This also removes the wasm-specific no-op branch entirely: there is no
blocking path left to fail on a target with no thread to park.

`TickerCanceled` becomes `#[non_exhaustive]` — a cancel reason is a
plausible additive field, and turning it into a `struct { .. }` later must
not be a breaking change for a match arm written today.

**4. `AnimationController` owns the one completer per run.**
`AnimationControllerInner.active_run: Option<TickerCompleter>`. Every
run-starting method (`forward`, `forward_from`, `reverse`, `reverse_from`,
`animate_to`, `animate_back`, `animate_to_curved`, `animate_back_curved`,
`repeat`, `repeat_with`, `fling`, `fling_with`, `animate_with`,
`animate_back_with`) returns `Result<TickerFuture, AnimationError>`.
Natural ends (`tick_time_based`, `tick_simulation`) take `active_run` and
publish `complete()` **before** the controller lock is dropped — Flutter's
own order in `_tick` (`stop(canceled: false)` completes the `Completer`
before `notifyListeners()`, `animation_controller.dart:951`) — and deliver
after. A run-starting site displaces `active_run` and cancels the displaced
completer, firing the *new* run's status before delivering the *old* run's
cancellation. `settle_at_target` (the zero-distance fast path) cancels the
displaced run and returns an already-complete `TickerFuture`.
`stop_running`/`dispose` take and cancel `active_run` the same way. Every
site funnels through one chokepoint, `AnimationController::finish`, that
owns `drop(inner)` then delivery — `deliver()` is called from nowhere else
in `controller.rs`, checked by a source-guard test over the file. See the
crate's `docs/ARCHITECTURE.md` `## Mapping decisions` for the full per-site
table.

**Constraint for the navigator consumer, load-bearing on the wiring that
awaits this future:** a continuation on `PushCompletion::Animating`'s
`TickerFuture` must be registered in `NavigatorShared::apply` — after the
flush that installs the entry, with the history unlocked — never inside
`handle_push` itself. `RouteHistory::flush()` re-drains commands between
passes, so a continuation registered mid-flush, on a future that is already
resolved by the time `apply` runs, would settle within that same flush
instead of on the next one, changing which flush a caller observes the
transition complete in. The `RebuildHandle` slot the continuation schedules
the Navigator through must be read at fire time, not at registration time:
`push_with_id` flushes before the Navigator is mounted, so the slot is still
empty when the future is created and only filled once mounting completes.

**5. `Drop` caveats, recorded rather than assumed.** There is no
`impl Drop for AnimationController`: the last `Arc` drops
`Mutex<AnimationControllerInner>`, and `active_run`'s own `Drop` (a
`TickerCompleter`) publishes `Canceled` and delivers — with, by
construction, no controller lock held, since a live `MutexGuard` holds a
strong count. Two consequences worth naming rather than discovering later:
(a) drop-delivery can run wherever the last clone of the controller happens
to die — `TransitionRoute::dispose` drops its clone inside a `controller.lock()`
scrutinee guard (edition-2024 scrutinee temporaries live through the
then-block), and `Vsync::unregister` may drop one under the registry lock;
both are inert today only because `dispose()` already took `active_run`
first, and a future change that reorders either teardown must re-check this;
(b) drop-cancel-on-last-clone is reachable only for `without_ticker`
(`_bounds`) controllers — `new` **and** `with_detached_ticker` both install a
real `Ticker` and, once a run starts, `restart_ticker` gives it a callback
that captures `self.clone()` regardless of whether that ticker is
scheduler-driven or detached, so both hold the cycle `inner.ticker →
callback → controller clone → inner` until an explicit `dispose()` — a
detached ticker never firing on its own does not stop it from holding the
same closure.

(c) **Nothing delivery-bearing is live while foreign code runs under the
controller guard.** Foreign code does run there, unavoidably and as it
always has: every scheduler-backed run start calls `restart_ticker` →
`Ticker::start` → `schedule_frame_callback` → `request_frame`, which runs the
embedder's `on_frame_scheduled` hook and a `tracing::debug!` in
`flui-scheduler` under `inner`. The rule is therefore about ordering, not
abstinence: `restart_ticker` runs BEFORE any completer for the current run
exists (see its own doc), so a panic in that hook unwinds with no
`TickerDelivery`/`TickerCompleter` on the stack. Within `controller.rs`
itself no `tracing` call runs under the guard at all — `restart_ticker`
returns whether it found a ticker rather than warning inline, and `set_value`
captures `value.is_nan()` before taking the lock and canonicalizes silently
under it; both warn only after `AnimationController::finish` has unlocked
(`warn_if_no_ticker`, `warn_if_nan`). A `tracing` subscriber is arbitrary
user code, and a panic inside one that unwound from under `inner` would drop
whatever `TickerDelivery` was live at that point (running its continuations)
before `inner` itself dropped — the same lock-order hazard this ADR removes,
reintroduced by a diagnostic.

**6. `notify` stays uncontained, at two new sites.** The waker precondition
this ADR's predecessor documented (`Event::notify` calls `task.wake()` under
`event-listener`'s own list lock; a waker that re-polls or drops the future
from `wake()` deadlocks inside that dependency) is unchanged, and now applies
at `Drop for TickerDelivery` and `Drop for TickerCompleter` as well as at an
explicit `.deliver()` call — the same tracked class as `Drop for Ticker` →
`dispose` → `notify` (issue #1165), not worsened in kind by this change.

**7. Porting rule for a Flutter call site that chains off a ticker future.**
`.then`/`whenComplete` on Flutter's base `TickerFuture` relies on "never
fires on cancel" (`ExpansionTile._controller.forward().then(..)`,
`DrivenScrollActivity.animateWith(..).whenComplete(_end)`) — since FLUI's
one future fires on both outcomes, that idiom ports to
`when_complete_or_cancel(|r| if r.is_ok() { .. })`. Flutter's
`whenCompleteOrCancel` ports to an unconditional
`when_complete_or_cancel(|_| { .. })`.

**8. Lint shape.** Under this workspace's `unused_must_use = "deny"` plus
`clippy::all` at `-D warnings`: `controller.forward()?;`,
`let _ = controller.forward();`, and `controller.forward().unwrap();` are all
clean — `TickerFuture` itself is deliberately **not** `#[must_use]`: a future
returned from a run-starting method reports how a run that already started
ends, not whether it started at all, so a caller who only needs the latter is
not forced to name it — the same rule a navigator's own `RouteResult` future
follows. `TickerFuture::pending()` **is**
`#[must_use]`, and — because rustc recurses into tuples — a bare
`TickerFuture::pending();` statement is already caught by "unused
`TickerCompleter` in tuple element 0" without needing the function's own
attribute; the attribute is kept anyway because it costs nothing and
documents the contract at the call site. `let _ = c.forward().unwrap()`,
`.expect(..)`, and `let _ = controller.forward()?;` all bind the `TickerFuture`
directly and fire `clippy::let_underscore_future` — do not write any of the
three.

## Why the obvious alternatives were rejected

- **Contain the ticker's own resolution in a `catch_unwind`-and-retry
  around `notify`.** Rejected by #1161's own lost-wakeup fix on measured
  evidence before this ADR: `event-listener`'s own notified-counter bookkeeping underflows on
  a retried panicking waker, turning a contained panic into a SIGABRT inside
  the dependency's `Drop`. Nothing here reopens that question — `notify`
  stays uncontained at every call site, old and new.
- **Keep the ticker as the resolver and require call sites to release their
  lock before calling `stop()`.** This is not a contract `AnimationController`
  can hold: `tick_time_based`/`tick_simulation` need to read and write
  `status`/`value`/`simulation` atomically with the ticker's own stop, and
  splitting that into "stop, unlock, then finish the rest of the tick" would
  let a re-entrant `forward()` from a value listener observe a
  half-finished tick. The controller's lock scope is not incidental; the fix
  has to work with it, not around it.
- **A single-phase `resolve()` that both publishes and delivers.** This is
  what the ticker's old `TickerFuture` did, and it is exactly the shape that
  cannot be called from under a caller's lock — the entire reason for this
  ADR. Splitting publish from delivery is the minimum change that lets
  `AnimationController::finish` unlock before running the fan-out this future
  family owns (continuations, listeners, wakers).

## Divergence from Flutter, and why it is an improvement

Flutter's topology — the ticker resolves its own future, forwarded to
directly from `stop`/`dispose` — is one fact told once, at the layer that
happens to never need a lock around it (`TransitionRoute`'s controller is
vsynced to the navigator, and nothing calls its ticker's `stop()` under a
mutex). FLUI's `AnimationController` is a different layer with a different
constraint: one lock, many entry points, all of which call `ticker.stop()`
under it. Moving resolution to the layer that owns the lock and the
cancel/complete distinction keeps the same fact — "a run either completes or
is canceled, exactly once" — without asking a non-reentrant mutex to survive
the future's own fan-out (continuations, wakers) running under it. This is a moved fact, not a different
one; the accounting Prime Directive rule #1 asks for is here rather than in
a claim that FLUI's ticker and Flutter's `Ticker` were never comparable.

**Recorded gap, not claimed parity:** a zero-*duration* run (a real distance
covered in `Duration::ZERO`, as distinct from a zero-*distance* run at the
target already) completes on the controller's first tick rather than
synchronously at the call to `forward()`, where Flutter's
`_animateToInternal` special-cases it (`animation_controller.dart:674-684`:
`stop(); if (simulationDuration == Duration.zero) { … return
TickerFuture.complete(); }`). See the crate's `docs/ARCHITECTURE.md` `##
Mapping decisions` entry for detail; tracked as issue #1171, and closing it
is not part of this change.

## Consequences

**Positive**
- The lock-order hazard #1161's lost-wakeup fix made reachable (a waker or
  continuation re-entering a controller's own mutex) cannot occur: nothing
  in `flui-scheduler` resolves anything under a caller's lock any more, and
  `AnimationController::finish` is the one place in `flui-animation` that
  ever calls `deliver()`.
- The navigator (#1161's stated goal) has something to await:
  `PushCompletion::Animating(TickerFuture)`, in the navigator wiring for
  #1161, awaits exactly the future this ADR's design produces. Before this
  change, "no consumer awaits either future" was the recorded state of the
  `flui_scheduler::{Ticker, ..}` runtime-contract entry; that wiring closes
  it.
- `TickerCanceled` being `#[non_exhaustive]` and the `pending()`/
  `TickerCompleter`/`TickerDelivery` split are additive room for a future
  cancel reason or a differently-timed delivery policy without a second
  breaking change.

**Negative / trade-offs, stated**
- Breaking, in one PR, on purpose (active-dev.md — no deprecation cycle for
  an unpublished internal API): `Ticker::start*`'s return type,
  `TickerFutureOrCancel`/`or_cancel`/`TickerFuture::new`/`Default`,
  `TickerCanceled`'s external construction, and the fourteen
  `AnimationController` run-starting return types. Measured against 151
  production call statements across the workspace: 46 `let _ = ..`, 74
  `.unwrap()`/`.expect()` statements, 3 `match` sites, 0 sites of the
  `let _ = ..unwrap()` shape the new lint would catch — zero production
  source files required a call-site edit for the return-type change itself.
- A caller that reinstates a `Ticker::start` future later (undoing decision
  1) is source-compatible with today's bare-statement callers **only if**
  the reinstated type stays non-`#[must_use]`, precisely as `TickerFuture`
  is now — marking it `#[must_use]` would turn every existing
  `ticker.start(..);` statement into a new warning under this workspace's
  `-D warnings` gate.
- `flui-scheduler` re-exports `TickerCompleter`/`TickerDelivery` as `pub`,
  but `flui-animation` does not re-export either — nothing in
  `flui-animation`'s own public contract takes or returns them, and its one
  cross-crate consumer (`flui-widgets`' navigator, in the wiring for #1161)
  already depends on `flui-scheduler` directly for its test fixtures.

## Amendments to ADR-0020

- **Status line.** Add: "§1.5 and the §2 push-deferral row superseded by
  ADR-0064 (2026-09-15)."
- **§1.5** (`didPush` returns a `TickerFuture`, and the navigator waits on
  it): the note that FLUI's `AnimationController` had "no analogue" to
  `_controller.forward()`'s returned `TickerFuture` is superseded —
  `forward()` (and every other run-starting method) now returns one.
- **§2's "What already exists, and is enough" table**, the "The push-deferral
  seam" row: the navigator wiring for #1161, not yet landed, will have
  `PushCompletion::Animating` carry the `TickerFuture`
  `AnimationController::forward` returns and await it from
  `NavigatorShared::apply`, replacing today's sole backing —
  `RouteHistory::notify_push_completed`'s status-listener seam.
- **The paragraph at §2's close** ("`AnimationController` does not return a
  `TickerFuture` from `forward()`… `PushCompletion::Animating` is the right
  shape rather than a workaround") is superseded: the controller does return
  one now, for the lock-order reason this ADR records. Once the navigator
  wiring for #1161 lands, `PushCompletion::Animating(TickerFuture)` will
  carry that future, not stand in for its absence.
