# Plan v2 — #1161 closure: the animation-completion future the navigator awaits

Base `main` @ `f2f1c4d9`; `.flutter` @ `3.44.0`. v1 review folded (api-design-lead's must-fix = C; the two RESHAPE NEEDED lists =
reshapes 1–10). Anchors are `file:symbol` plus the line read on 2026-09-15. `crates/flui-cupertino/src/route.rs` has no `did_push`.

## 0. Decisions (coordinator-decided items marked ★; the rest are this plan's)

**★A. ALT-2: `Ticker::start` returns `()`; the controller owns the only future per run.** `Ticker::{start, start_default,
start_typed} -> ()`; `Ticker::{stop, dispose, reset}` resolve nothing; `active_future` leaves `TickerInner`. The one production
caller of the ticker's future was `restart_ticker` (controller.rs:1277), which discarded it; no standalone `Ticker` user exists
outside flui-scheduler/flui-animation. The `start` refusal while a run is installed stays (Flutter's "started twice"), keyed on
`TickerState::is_running()` (Active | Muted), logs at `error!` and drops the callback with the guard released. #1167's mute→start
orphan fix is **moot**: there is no ticker future left to orphan; its test becomes `mute_then_start_is_refused_and_keeps_the_muted_run`.
`TickerFuture`, `TickerCompleter` (and the new `TickerDelivery`) remain flui-scheduler primitives.
**The true reason, for ADR-0064:** every `ticker.stop()` in controller.rs runs under the controller's `inner` guard
(`reset` :560, `settle_at_target` :712, `tick_simulation` :1019, repeat-exhausted :1119, non-repeating end :1169, `dispose` :1223,
`restart_ticker` :1274, `stop_running` :1324), so a future the ticker resolves would run wakers and continuations under a
non-reentrant mutex. Flutter's topology is one fact (`_startSimulation` returns `_ticker!.start()` verbatim,
`animation_controller.dart:861-871`; `stop` forwards `canceled:` to `_ticker!.stop`, :891-900); FLUI keeps one fact by moving it
**up** to the controller, which is the layer that owns the lock and the cancel distinction. Not "different facts".

**B. One future.** `TickerFuture: Future<Output = Result<(), TickerCanceled>>` resolves on cancel too; `TickerFutureOrCancel`,
`or_cancel`, `TickerFuture::new`, `Default` deleted. Rationale and market comparison unchanged from v1 (Compose, SwiftUI 17, `oneshot`).

**C. Completer with two phases.** `TickerFuture::pending() -> (TickerCompleter, TickerFuture)`. `TickerCompleter::{complete,
cancel}(self) -> TickerDelivery` **publish** (phase 1): in ONE `state.lock()` call set the durable resolution AND `mem::take` the
continuation `Vec` — a single critical section, an explicit code-review checkpoint (a registration landing between two locks would be
pushed and never drained). `TickerDelivery::deliver(self)` (phase 2): run continuations (per-continuation `catch_unwind`, each caught
payload logged at `error!` immediately) → `notify` wakers (uncontained, #1167) → re-raise the first payload **only if
`!std::thread::panicking()`**, else `tracing::error!` with the payload text (#1165). `Drop for TickerDelivery` delivers if
`deliver()` was not called; `Drop for TickerCompleter` publishes `Canceled` and delivers. **A completer or delivery dropped mid-unwind
does run its continuations, contained** — the route still settles, nothing is re-raised while panicking. Fan-out order is
continuations → wakers (v1 had it inverted: an uncontained panicking waker would unwind out with the drained `Vec` unrun and
`PushCompleted` never queued). `TickerCompleter` and `TickerDelivery`: `!Clone`, `Send + Sync`, `#[must_use = "a pending
TickerCompleter cancels its future if dropped unresolved"]` / `"…delivers on drop; call deliver() where continuations may run"`;
`TickerFuture::pending()` `#[must_use]` (the type attribute does not propagate through the tuple). ★`TickerCanceled` is
`#[non_exhaustive]` now (a cancel reason is a plausible additive field); its doctest, which compiles as an external crate, is
rewritten to obtain the value: `pending()` → `cancel().deliver()` → `Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop()))`
→ `Poll::Ready(Err(TickerCanceled { .. }))` (`Waker::noop` stable since 1.85, MSRV 1.97; precedent
`crates/flui-app/tests/data_transfer_transport.rs`). ★`TickerFuture` stays NOT `#[must_use]` (navigator `RouteResult` precedent,
2026-07-11 API review).

**D. `when_complete_or_cancel(&self, f: impl FnOnce(Result<(), TickerCanceled>) + Send + 'static)`** is a non-blocking registry
under the state mutex; on a resolved future (including between phase 1 and phase 2) it runs immediately on the caller's thread, a
divergence from Dart's microtask, so callers must be re-entrancy-safe and cross-registrant order is not a contract. The blocking
`wait()` path and both `target_family = "wasm"` branches are deleted.

**E. Controller.** `AnimationControllerInner.active_run: Option<TickerCompleter>`. Run-starting methods return
`Result<TickerFuture, AnimationError>` (`Err` = could not start; the future = how the run ended). Per site, all under the lock then
delivered after `drop(inner)`: natural end (`tick_time_based` ×2, `tick_simulation`): `take → complete()` **before** unlock, then
value listeners → status listeners → `deliver()`. This is Flutter's `_tick` order (`stop(canceled: false)` completes the
`Completer` at :951 before `notifyListeners()`/`_checkStatusChanged()`; only delivery is a microtask), and it is what makes a
panicking status listener leave the run `Ok`: the unwind drops the delivery, which delivers the already-published `Ok`. Run start
(`forward_from`, `reverse_from`, `drive_to`, `repeat_with`, `fling_with`, `drive_simulation`): `mem::replace(active_run, Some(new))`
→ `displaced.map(cancel)` under the lock; after unlock fire the **new status first, then deliver the cancel** (Flutter's observable
order: sync status, microtask cancel; v1 had it reversed). `settle_at_target`: cancel the displaced run, return
`TickerFuture::complete()`. `stop_running -> Option<TickerDelivery>` (`#[must_use]`) serves `stop`/`set_value`; `reset` same;
`dispose` (:1217-1227) gains a **new** unlock-then-deliver seam (it emits nothing today). Infinite `repeat` resolves only by cancel;
finite `repeat_with(count)` completes (Flutter 3.44 `_RepeatingSimulation.isDone`). Never assign `active_run = Some(..)` directly:
the displaced completer's `Drop` would deliver under the lock. The controller's own `Drop` (last clone) cancels a live run with no
lock held. Correction to v1: `TransitionRoute::install` uses `with_detached_ticker` (transition_route.rs:623), a ticker that never
fires; `without_ticker` is the shape of most other widget controllers, not the navigator's.

**F. Navigator awaits.** `PushCompletion::Animating(TickerFuture)` (`#[non_exhaustive]`, derives `Debug, Clone`; `Copy`/`PartialEq`
lost). `RouteEntry::handle_push` returns the future; the flush loop records `DeferredEffect::AwaitPush(RouteId, TickerFuture)`
(derives drop `Copy/PartialEq/Eq`); the history never retains it. **Load-bearing constraint:** the continuation is registered in
`NavigatorShared::apply` (post-flush, history unlocked), never inside `handle_push`: `flush()` re-drains commands between passes
(history.rs:1128/1138), so a mid-flush registration of an already-resolved future would settle in the same flush, and W5's
"Pushing after push, Idle after one pump" is the pin. The continuation may only push `RouteCommand::PushCompleted(id)` onto the Send
`RouteCommandQueue` and schedule the Navigator through the slot `NavigatorShared::settle_wake: Arc<Mutex<Option<RebuildHandle>>>`,
**read at fire time** (`push_with_id` flushes immediately, history.rs:837-847, so a pre-mount push registers while the slot is `None`;
a remount refills it). Filled in `init_state`, cleared in `dispose`; trigger #22 hygiene: the field is not named `rebuild_handle`
and no comment inside `fn build` carries that token. `NavigatorState::build` calls `pump_route_commands()` first. The continuation
never touches the history: `did_pop → reverse()` cancels the push future inside a flush; the flush tail drains the queue where the
entry is no longer `Pushing`. Complete and cancel do the same thing. Timing: realm (`ui_realm.rs::draw_frame_entered`) and harness
(`flui-testing::pump_frame`) tick `Vsync` before driving the frame and `build_scope` drains `external_inbox` at its head, so the
settle lands in the pump that completes the tick; hand-driven tests see it at the next `harness.tick()`. Delete
`RouteBinding::notify_push_completed` and the `Completed` arm's call in `TransitionInner::handle_status_changed` (keep
`set_entry_opaque`). `did_push`: `Ok(f) => Animating(f)`, `Err(_) => Immediate` + `error!`. ★flui-widgets re-exports
`TickerFuture` and `TickerCanceled` (navigator/mod.rs + the lib.rs `pub use navigator::{..}` list) so a third-party `Route` needs no
lower-layer dependency; flui-animation re-exports `TickerFuture, TickerCanceled, TickerCompleter, TickerDelivery` beside its existing
`Ticker, TickerCallback, TickerProvider` line.

**G. DX and lint shape (binding for the builder and the rustdoc).** Clean under the workspace table (`clippy::all` warn +
`-D warnings`, `unused_must_use = "deny"`, `must_use_candidate` allow, Cargo.toml:390): `controller.forward()?;`,
`let _ = controller.forward();`, `controller.forward().unwrap();`. **Fires `let_underscore_future`:** `let _ = c.forward().unwrap()`,
`.expect(..)`, and `let _ = controller.forward()?;` (the bound value is the future) — 0 such sites today; do not write the last one
in docs. Do not advertise `forward()?.await?` (the second `?` reports routine cancellation as an error, plan.md D2). Show
`.when_complete_or_cancel(|end| ..)` and `match f.await { Ok(()) => .., Err(TickerCanceled { .. }) => .. }`. **Porting rule
(ADR-0064):** Flutter `.then`/`whenComplete` on the base future relied on "never fires on cancel" (`ExpansionTile`
`_controller.forward().then(..)`, `DrivenScrollActivity` `animateWith(..).whenComplete(_end)`) ⇒
`when_complete_or_cancel(|r| if r.is_ok() { .. })`; Flutter `whenCompleteOrCancel` ⇒ unconditional. `#![deny(missing_docs)]` holds in
all three crates; `forward()` and `animate_to()` rustdoc carry the `when_complete_or_cancel` idiom and a compiling awaited example
(`without_ticker`, `tick_at`, `Waker::noop()` poll) — the first awaiting consumers in the workspace.

## 1. Acceptance criteria
- Navigator under `VsyncScope`, `PageRoute` 300 ms pushed; clock at 0 and 150 ms with a pump each → `Pushing`; 300 ms and ONE pump
  → `Idle`, with `notify_push_completed` absent from the crate (W1).
- `without_ticker` controller: `forward()` → `Ok(f)`; `tick_at(d)` → `f` is `Ok(())`, status listeners saw `Completed` first, and a
  panicking status listener leaves it `Ok`. Same under `AnimationController::new(Duration::ZERO, &scheduler)` + `execute_frame()`.
- Live run: `stop`/`set_value`/`reset`/`dispose`/new run → `Err(TickerCanceled { .. })`, delivered with no controller lock held.
- Pop mid-push cancels the push future inside the flush; no deadlock; entry `Popping`. A completer dropped during an unwind with a
  panicking continuation: process survives, error logged. `just ci`, `just runtime-conformance-check`, `just wasm-check` green.

## 2. Files and breaking changes
**PR-A (flui-scheduler, flui-animation, docs).** `ticker.rs`: `Ticker::start*` → `()`, refusal on `is_running()`, `active_future`
removed from `TickerInner`, `stop/dispose/reset` lose their resolve arms and "unschedule before resolving" comments; `TickerFutureInner`
(resolution + continuations under one mutex), `TickerCompleter`, `TickerDelivery`, `Future` impl, `when_complete_or_cancel`,
`#[non_exhaustive] TickerCanceled` + doctest; delete `TickerFutureOrCancel`/`or_cancel`/`new`/`Default`/blocking path/`Listener`
import; `lib.rs` export line; `ticker/future_tests.rs` rewrite; `tests/integration_tests.rs` (22 sites incl. every `ticker.start`
that bound a future, and the retained-ticker `or_cancel` teardown pin at ticker.rs:2152, which is deleted with its subject);
`examples/animation_ticker.rs` (5). `controller.rs`: `active_run`; 14 run-starting signatures via the six shared drivers; `settle_at_target`;
`stop_running -> Option<TickerDelivery>`; both tick branches; `reset`; `dispose`; `emit_status_after_unlock` gains the delivery; re-exports.
Docs: `crates/flui-scheduler/ARCHITECTURE.md` (replace the #1167 "base future parks with no waker" and mute→start entries with:
ticker resolves nothing; one future; publish/deliver; panicking gate); `crates/flui-animation/docs/ARCHITECTURE.md` (new
`## Mapping decisions`: E's per-site table, publish-before-listeners, status-before-cancel, zero-duration runs complete on the first
tick where Flutter returns `TickerFuture.complete()` synchronously — recorded gap, own issue); `docs/runtime-contract.toml` :192
export literal (`- TickerFutureOrCancel + TickerCompleter + TickerDelivery`) and :3028-3034 prose (`failure_semantics`: the ticker
resolves no future; `consumers`: "AnimationController owns and resolves the run future; awaited by the navigator in PR-B (#1161)";
drop the "NO CONSUMER" and undecided-output sentences); `docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md`
(A's lock-order reason, C/D/E, F's constraint, G's porting rule and lint shape); ★`docs/adr/ADR-0020-transition-modal-route-seam.md`:
status line gains "§1.5 and the §2 push-deferral row superseded by ADR-0064 (2026-09-15)", §1.5 gets the superseded note, the §2
table row becomes `PushCompletion::Animating(TickerFuture)` awaited by `NavigatorShared::apply`, and the :289 paragraph ("does not
return a TickerFuture… the right shape rather than a workaround") is marked superseded with the reason. Breaking: `Ticker::start*`
return type, `TickerFutureOrCancel`, `or_cancel`, `TickerFuture::new/default`, `TickerCanceled` construction outside the crate, the
controller's 14 return types; 0 production files break (151 controller call statements measured: 46 `let _ =`, 74
`.unwrap()/.expect()` statements, 3 matches, 0 `let _ = ….unwrap()`).
**PR-B (flui-widgets).** `route.rs`, `history.rs`, `navigator.rs` (`apply` registration; `settle_wake`; `build` pumps first; the
:251 destructure gains an arm; rewrite the "No `rebuild_handle()`" doc without the token in `build`), `binding.rs` (delete
`notify_push_completed`; module prose), `transition_route.rs` (`did_push`, `handle_status_changed`, three comments),
`page_route.rs:340` comment, `navigator/mod.rs` + `lib.rs` re-exports. Sites: `did_push` 12 (trait, default, `RouteRecord`,
3 production, 6 test); `Animating` constructions 8; `notify_push_completed` callers 7 test sites (tests.rs 1022/1214/1275/1463/
1489/1506, navigator_tests.rs 764) → hold a `TickerCompleter`, and **each gains an explicit flush/pump** (unmounted handles lose
today's immediate settle). `DeferredEffect` has no external `assert_eq!` user.

## 3. Tests and revert matrix (single production line each; "—" pins nothing; no `thread::sleep` anywhere)
The scheduler-backed `Ticker` reads wall-clock `start_time.elapsed()` (`tick_and_reschedule_static`), not the vsync stamp, so fixed
timestamps cannot advance it; the lever is `Duration::ZERO` (`tick_time_based`: `is_zero → t = 1.0`, done on the first `execute_frame()`).
S1 pending → `Err` on `cancel().deliver()` (manual poll) ← `Canceled => Ready(Err)` · S2 continuation on pending runs once with the
outcome ← the take in publish · S3 on a resolved future runs immediately ← fast path · S4 dropping a completer cancels ← `Drop for
TickerCompleter` · S5 continuations see the state lock free ← guard released before the loop · S6 **continuations run before wakers**
(a continuation observes wake count 0; 1 after `deliver`) ← the order · S7 panicking continuation does not starve siblings, first
payload re-raised ← per-continuation `catch_unwind` · S8 a continuation registered from inside a continuation runs (published state,
not a lost `Vec` slot) ← splitting publish and take into two locks · S9 drop a completer inside a `catch_unwind` panic with a
panicking continuation: process survives, `error!` captured via `flui_testing::log_capture::capture`, future `is_canceled` ← the
`thread::panicking()` branch (revert aborts the test process) · S10 `mute_then_start_is_refused_and_keeps_the_muted_run` ←
`is_running()` refusal · S11 auto-traits: `TickerCompleter`/`TickerDelivery` `Send + Sync + !Clone` — compile fence. Deleted: both
`or_cancel` tests, `a_canceled_base_future_holds_no_waker…`, `a_second_resolution_is_ignored…`, the blocking test, the retained-ticker
teardown pin.
A1 `forward` future `Ok` under `tick_at` ← `complete()` at the non-repeating site · A2 same under `new(Duration::ZERO, &scheduler)` +
one `execute_frame()` — route coverage, shares A1's line · A3 simulation run `Ok` ← `tick_simulation` site · A4 finite repeat `Ok`,
infinite only on `stop` ← repeat site / `stop`'s cancel · A5 `stop`/`set_value`/`reset`/`dispose` cancel ← each site · A6 new run:
status listener sees the new status before the old run's cancel continuation ← order swap · A7 zero-distance start returns a complete
future and cancels the live run ← `settle_at_target` · A8 every delivery runs with `inner.try_lock()` succeeding ← any `deliver()`
moved before `drop(inner)` · A9 a `Completed` listener that calls `forward()` leaves the finished run `Ok` ← `take` moved after unlock
· A10 a panicking status listener leaves the finished run `Ok` (caught in the test) ← publishing after listeners · A11 continuation
chain: `f.when_complete_or_cancel(|_| chained.animate_to(1.0, Some(10 s)))` under `new(Duration::ZERO, &scheduler)`, runs inside
the `TickerLease` checkout (#1059 path): `transient_callback_count() == 1` after frames 1 and 2, one value notification per frame,
`stop()` → 0 — mirrors `status_listener_chaining_forward_ticks_once_per_frame…` (controller.rs:2328) one step later ← restore-only-
from-`CheckedOut` in the lease · A12 disposed → `Err(Disposed)` — existing.
W1 outer acceptance: settles, and in one pump ← the `apply` registration / the `settle_wake` schedule · W2 source guard:
`notify_push_completed` absent from `src/navigator/` — pins the deletion · W3 pop mid-push cancels inside the flush, no deadlock,
`Popping` ← the `state == Pushing` guard in `apply_commands` · W4 canceled future still settles a `Pushing` entry ← an `is_ok()`
filter · W5 `Animating(TickerFuture::complete())` is `Pushing` after the push and `Idle` after one pump ← moving the registration
into `handle_push` (v1's line was wrong: a collapse gives `Idle` with no pump) · W6 `DeferredEntranceRoute` stays `Pushing` while its
completer lives — fixture · W7 a push through an unmounted `NavigatorHandle`, then mount, then resolve → one pump → `Idle` ←
capturing the handle value at registration instead of the slot · rewrite `transition_route_tests.rs:114` (settle at the next pump).

## 4. Risks and gates
Continuation fired inside a flush (W3) · a direct `active_run = Some(..)` assignment or a delivery dropped in the guard scope delivers
under the lock (A8, S5) · re-raise while panicking → abort (S9) · `PushCompletion`/`DeferredEffect` lose `Copy`/`PartialEq` (7 + 0
sites) · settle moves to the next build (recorded) · export literal (`just runtime-conformance-check`) · `RebuildHandle` acquired in
`init_state` (`just port-check`) · zero-duration runs complete on the first tick (recorded gap; A2/A11 depend on it) · `notify` stays
uncontained. Gates: `just ci`; narrowing: `cargo nextest run -p flui-scheduler -p flui-animation -p flui-widgets --locked`, the two
crates' `--doc`, `just clippy`, `just wasm-check`, `just runtime-conformance-check`, `just port-check`. API-GATE: export line,
`Result<TickerFuture,_>`, `#[non_exhaustive]` ×2, must_use trio, re-exports. ASYNC-GATE: single-critical-section publish,
continuations → wakers → gated re-raise, Drop paths, `Send` bounds, in-flush cancel.

## 5. Verdict
Pre-code **ACCEPTABLE** as v2. Two PRs, stacked; PR-B opened before PR-A merges; #1161 closes on PR-B (PR-A's `consumers` names PR-B).

## Plan-review amendments to v2 (harsh-critic confirmation — ACCEPTABLE once applied)

A1. **Delivery chokepoint (structural), not `#[must_use]` alone.** Measured under the workspace lint table:
`c.complete();` bare → error (caught); `let _ = c.complete();` → SILENT → `TickerDelivery` drops at statement
end and delivers under whatever guard is live; `displaced.map(TickerCompleter::cancel);` in statement position →
SILENT (`Option<T>` does not inherit `T`'s must_use); `stop_running();` silent unless the fn is `#[must_use]`.
Also `pending();` IS caught ("unused `TickerCompleter` in tuple element 0") — rustc recurses into tuples, so the
sentence "the type attribute does not propagate through the tuple" is FALSE and must not reach ADR/rustdoc (the
fn-level attribute is harmless; keep it, drop the claim). Close structurally: every end-of-run / run-start site
hands `(guard, Option<TickerDelivery>, status)` to ONE function (`emit_status_after_unlock` or a `finish`) that
owns `drop(inner)` then `deliver()`, so `deliver()` and an unbound `TickerDelivery` appear in exactly one
function and A8 pins that function. Plus a W2-style source-guard test over `controller.rs`: no `let _ =` on a
line containing `.complete()`, `.cancel()`, `stop_running(`, or `.map(TickerCompleter::`; no statement-position
`.map(TickerCompleter::cancel);`. The grep is the backstop, the chokepoint is the defense.
A2. Between-phase window: no deadlock (phase 1 runs under the controller guard with no user code before
`drop(inner)`; a cross-thread registrant blocks until unlock and the producer never waits on it; a same-thread
registrant can only be a listener firing after unlock). Observable effect: such a registrant runs BEFORE the
drained continuations — cross-registrant order, already non-contractual (D). S8 pins the fast path.
A3. Controller `Drop`: no `impl Drop for AnimationController`; the last `Arc` destroys the `Mutex<Inner>` and
`active_run`'s Drop publishes Canceled and delivers — "no controller lock held" is true by construction (a live
guard holds a strong count). Record in the ADR: (a) drop-delivery runs wherever the last clone dies — e.g.
`TransitionRoute::dispose` drops its clone inside the `controller.lock()` scrutinee guard (transition_route.rs:
751-755; edition-2024 scrutinee temporaries live through the then-block) and `Vsync::unregister` may drop one
under the registry lock — inert today because `dispose()` already took `active_run`; (b) every ticker-backed
controller (`new`, `with_detached_ticker`) holds the cycle `inner.ticker → callback → controller clone → inner`,
so its `Arc` never reaches zero without `dispose()` — drop-cancel is reachable only for `without_ticker`
controllers; say so in E instead of implying it covers all constructors.
A4. (ASYNC-GATE plan: ACCEPTABLE) ADR-0064 / ARCHITECTURE.md must extend the "`notify` stays uncontained" note
to name the two NEW Drop sites (`Drop for TickerDelivery`, `Drop for TickerCompleter`) — a waker panicking inside
`notify()` during a Drop that is itself mid-unwind is an abort, the same tracked class as `Drop for Ticker →
dispose → notify` (#1165), not worsened in kind. A8's oracle is correctly scoped to the controller mutex only
(ALT-2 removes the completer from `TickerInner`); A11 is the sufficient pin for the lease re-entry path.
A5. (API-GATE plan: ACCEPTABLE once applied) `TickerCompleter`/`TickerDelivery` are `pub` in flui-scheduler but
NOT re-exported by flui-animation (nothing in flui-animation's own public contract takes or returns them; the only
cross-crate consumer is flui-widgets' test fixtures, which already depend on flui-scheduler directly and mostly
never bind `TickerDelivery`'s name — `let _ = completer.complete();` in a TEST fixture is fine, Drop delivers).
flui-animation re-exports `TickerFuture, TickerCanceled` only. Keep the `Ticker*` names (`Run*`/`Animation*`
would bake an upper layer's vocabulary into a foundation crate — a layering violation; the module's convention is
caller-agnostic like `oneshot::Sender`). ADR precision: re-adding a `Ticker::start` future later is
source-compatible for bare-statement callers ONLY if the reinstated type stays non-`#[must_use]` — say that,
not the bare word "additive".

**Plan status: APPROVED (v2 + A1–A5).** Two stacked PRs: PR-A (flui-scheduler + flui-animation + ADR-0064 amending
ADR-0020), PR-B (flui-widgets navigator). #1161 closes on PR-B.
