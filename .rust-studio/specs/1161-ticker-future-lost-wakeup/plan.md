# Plan — #1161 ticker futures

Base `main` @ `6ee4a51e`. **Revision 2**, after `harsh-critic` (RESHAPE NEEDED) and
`concurrency-specialist` (NEEDS WORK). r1 is recorded in `## Review history`; read it
before re-proposing anything it dropped.

## The defect list grew from three to five

1. **Lost wakeup, a permanent hang.** Both `poll` impls read the state, drop the guard, then
   `listen()`. A resolution in that window notifies zero listeners and is lost; the transition is
   once-only, so nothing re-notifies.
2. **`Event::notify` wakes under `event-listener`'s own list lock**, and a panicking waker unwinds
   out of its notify loop.
3. **No coverage at all** for a woken awaiter, a cancelled await, a re-entrant or panicking waker,
   or lock discipline. Confirmed by enumerating all 24 tests in the file.
4. **wasm32 `when_complete_or_cancel` reports a completion that has not happened** — on a *pending*
   future it drops the registration and invokes the callback immediately. **Not new**:
   `docs/audits/2026-07-25-upgrade-pack-audit.md` already recorded it; r1 wrongly called it novel.
5. **`mute()` then `start()` orphans a pending `TickerFuture` forever.** *(Fix: refuse on
   `inner.active_future.is_some()`, NOT on a state enumeration — Flutter's predicate is literally
   `isActive => _future != null`, and enumerating states is exactly how this hole opened; a third
   non-`Active` state added later would reopen it and no test would notice. Widen the existing
   `debug_assert!` and return the live `active_future`, so an existing awaiter stays valid.
   Verified: no test in the crate starts a muted ticker — all 14 `.mute()` sites are followed by
   `unmute`, `tick`, `execute_frame`, or a state assertion.*
   ***Two hard constraints on the fix, both found by review:*** *(i) **the refusal's `return` must be
   hoisted out of the `inner.lock()` scope.** It currently fires inside it, so the caller's
   `Box<dyn FnMut>` drops **under `Mutex<TickerInner>`** — a user `Drop` that touches the ticker
   self-deadlocks on a non-reentrant mutex. Pre-existing on the `Active` arm; widening the predicate
   routes the ordinary `mute(); … start(cb)` sequence down it. Every sibling (`stop`, `dispose`,
   `reset`, `set_pending_callback`) already binds the displaced callback out and drops it after.
   (ii) **pair `AnimationController::restart_ticker` in the same PR**: it guards on
   `TickerState::can_tick()`, which is `Active` only, so from `Muted` it skips `stop()`, and after
   this fix it gets the **old** future back and silently fails to restart the animation. Widen to
   `is_running()` or drop the guard — `stop()` on `Idle`/`Stopped` is already a no-op. Without it
   this relocates defect 5 into another crate rather than fixing it.)* `Ticker::mute` never touches
   `active_future`; `start_inner` refuses only when `state == Active`, so from `Muted` it proceeds
   and overwrites the field, dropping the previous future unresolved. All `pub`, no race, no
   concurrency. **Flutter's guard is an `assert`, stripped in release** — so it orphans there too. The accurate and
   more useful statement: FLUI's refusal is *already better* than the reference (it refuses in
   release, logs, and returns the live future instead of overwriting) — it is simply **keyed on the
   wrong predicate**. Flutter keys on `_future != null`; FLUI keys on a state-enum variant, and the
   enum collapse is what narrowed it. Latent, not live: `Ticker::mute()` has **zero production
   callers** (every caller is in `tests/integration_tests.rs`; `TickerMode` mutes a registry, not a
   `Ticker`). Owes a replacement test.

## D1 — primitive: minimum fix, one shared helper

Keep `event-listener`; keep the `Mutex`; reject the `Event<T>` tag; defer the `AtomicU8`. Extract one
private `poll_resolution` with the `async-broadcast` shape — **register, `continue`, re-read**.

Three implementation facts the helper must carry, all verified at the source:

- **The re-read is sufficient alone; the latch is the redundant second net — and r2 had this
  backwards.** `resolve` writes the durable state *before* `notify`, so any notify that has happened
  is visible to a re-read. The latch (a notify landing on a registered-but-never-polled `Created`
  entry marks it `Notified`, and the next `register` returns `Notified`) closes only the
  **`listen()` → poll** half. The window that *is* defect 1 — state read → `listen()` — has no entry
  to latch, so the latch cannot touch it. Proof by construction: today's code **is** the latch
  without the re-read, and it hangs. Written the other way round, the paragraph tells a reader the
  `continue` is optional.
- **Keep `*slot = None` on the listener's `Poll::Ready`.** In the notify-first interleaving
  `register` has already **removed** the entry; re-polling that same `EventListener` hits
  `NEVER_INSERTED_PANIC`. Today's code is saved by exactly that assignment. One deleted line from a
  panic.
- **A precondition, not a footnote.** `notify` holds the list lock across `wake()`, and both
  `register` and `remove` re-take it — so a waker that re-polls **or drops** this future from inside
  `wake()` self-deadlocks *inside the dependency*. Measured, not inferred. **Reachable on `main`
  today, made deterministic by the fix** — not created by it: `poll` already registers the waker
  before any resolver runs, so `start` → `poll` → `stop` reaches `wake()` under the list lock today.
  The lost wakeup needs a *concurrent* resolver; the fix narrows that race, it does not open the wake
  path. It goes in the `Future`
  impl's doc as a stated precondition on the waker, so the first consumer's executor choice is a
  checkable decision rather than an accident. Standard parker-based executors are all safe.

## D2 — keep `Result<(), TickerCanceled>`, on the reasons that survived

Two of r1's four pillars were falsified and are struck:

- **"An error grows additively" is false.** `pub struct TickerCanceled;` is a unit struct in the
  *value* namespace; turning it into an `enum` destroys `Err(TickerCanceled)` as both expression and
  pattern, breaking this crate's own doctest and its own test before any downstream. Adding
  `#[non_exhaustive]` now is equally breaking. Growing the error is a *smaller* break than changing
  `Output` — not an additive one.
- **"Zero awaiters, so choosing now is designing against a hypothesis" inverts this repo's own
  rule.** `AGENTS.md` Prime Directive #2: *"Breaking changes are cheap today and ossify once
  consumers exist; do not defer a better shape to 'later'."* Zero consumers is the **cheapest**
  moment, not a reason to defer.

What survives is **stronger than r2 first wrote it, not weaker.** With D3 dropped this PR contains
**no public semantic change at all** — the `pub use` line is untouched and nothing moves except
defect 5's refusal and the wasm32 no-op — so `TickerEnd` would be the *only* public break in an
otherwise pure correctness PR. "Do not bundle a semantic break with a hang fix" is therefore a
sharper argument here than it was in r1, not a thinner one. Plus `futures_util::Abortable`'s
`Result<T, Aborted>` as the first-party Rust precedent for a third party cancelling a future the
awaiter holds.

And the milestone rule is the pillar that **replaces** the two struck ones: the output type is
decided in the same milestone, against a live call site — which *satisfies* "shape contracts for N
consumers now" rather than deferring past it. That is the answer to the Prime-Directive #2 objection,
not an evasion of it.

**The cost of keeping `Err`, stated rather than buried:** `a.or_cancel().await?` inside a function
returning `anyhow::Result` turns a routine, expected route-transition cancellation into a reported
application error. That is the real trade. It is decided by the first consumer (#1162's sibling), in
that PR, with a live call site to judge against.

## D3 — **dropped from this PR.** The retry is not containment.

r1 shipped a `catch_unwind`-and-retry around `notify`, on a harmlessness argument that is **false**
and was disproved by running code, not by reading it.

`Inner::notify` marks the entry `Notified` and calls `task.wake()` **before** `self.notified += 1`,
so **a panicking waker** skips the increment — the *unwind* skips it, not the catching, which is why
this is pre-existing rather than something a `catch_unwind` introduces. `Inner::remove` then does `self.notified -= 1`
**unconditionally** whenever `state.is_notified()` — and that covers `NotifiedTaken`, so both
`EventListener::drop` and `poll`→`register` decrement. The next removal underflows a `usize`:

```
attempt 1: waker panicked, retrying
attempt 2: notify returned 0, clean
thread 'main' panicked at event-listener-5.4.2/src/intrusive.rs:328:
attempt to subtract with overflow          ← inside PinnedDrop for InnerListener
```

**The consequence on `main` today is not counter drift — it is SIGABRT.** Measured, single-threaded,
no race, exit code 134: the awaiter future is a local in the frame that calls `stop()` (what every
test and most `block_on` sites look like), the waker panics, the unwind skips the increment and then
unwinds *that same frame*, dropping the future → `remove` → `0usize - 1` **inside
`<InnerListener as Drop>::drop` during cleanup** → "panic in a destructor during cleanup" →
non-unwinding panic → abort. The killing panic is an arithmetic overflow in a dependency's
destructor with no textual connection to the ticker.

So the policy **plants the abort it was chosen to avoid**, in a `Drop` frame inside the dependency,
outside any `catch_unwind` FLUI owns, with no textual connection to the ticker. In release the
counter wraps silently instead. Retrying is not what breaks it — *any* unwind through `task.wake()` does, caught or not, so
the corruption is **pre-existing on `main`** and this change does not cause or cure it. But it **does
make it newly reachable, by verbatim the argument D1's third bullet already makes for the §0.3
deadlock**: today the lost wakeup is what keeps wakers from firing at all, so after the fix they fire
reliably and both exposures change together. Recording it for one and denying it for the other was
inconsistent; they are the same exposure change one bullet apart. What r1 would have added on top is
deferral and silence.

**Taken instead:** leave `notify` uncontained. A panicking waker propagates out of
`stop`/`dispose`/`reset` exactly as today — loud, immediate, attributable. **Three** residuals, each
with an issue number, each stated precisely rather than as "a known hang":

1. **Later listeners are not woken by the notify that panicked.** Precisely: a task whose *only* wake
   source is this ticker hangs unconditionally; a task with any other wake source self-heals,
   because `poll_resolution` reads the durable state first. Unqualified "starvation" is what invites
   the next person to re-add the retry just removed.
2. **The counter corruption** — an upstream `event-listener` bug worth reporting regardless, since a
   panicking waker is a legal thing for an executor to have.
3. **The abort path D3 was chosen to avoid is still open, and dropping D3 does not close it.**
   `impl Drop for Ticker` → `dispose` → `notify` → a panicking waker is a panic in a `Drop`; if that
   drop is itself during an unwind, the process aborts. Live on `main` today, with or without this
   PR — both preconditions already hold — which is why it does not flip the decision. It is the
   concrete thing `thread::panicking()` would buy, and it belongs on the record rather than in the
   gap between two issues.

The uniform-vs-split question r1 put to review is therefore moot here — but its answer is worth
recording for #1162, which faces the identical decision when it adds `Drop for SchedulerInner`:
`if std::thread::panicking() { report } else { resume_unwind }` is the **complete** discriminator,
because a caller can invoke `stop()` from their own `Drop` during an unwind, which a per-call-site
split cannot see.

## D4 — scope

**In:** the `poll_resolution` helper; the `Canceled` arm that registers a listener which can never
fire; **defect 5 (mute→start)**; the §0.3 precondition doc; the `runtime-contract.toml` `consumers`
correction; three `## Mapping decisions` entries — register-before-the-decisive-read, the second
resolution silently ignored where Flutter asserts, and the base future returning `Pending` with **no
waker registered at all** (a literal `Future`-contract violation that is nonetheless right, being
Flutter's `_cancel` parity — the entry a reviewer will otherwise trip on).

**Out, each with a reason:** D3 (above); the `AtomicU8` (own issue); `TickerEnd` (own issue, decided
by the first consumer); `event-listener` → `[workspace.dependencies]` (unrelated root-manifest
hygiene, contradicting this PR's own "small and obviously right" standard); **removing**
`when_complete_or_cancel` on wasm32 — instead take the smaller fix that removes the lie without
removing the API: keep the already-resolved fast path on all targets, and make only the *pending*
wasm32 path `tracing::error!` + `debug_assert!(false)` and **not** invoke the callback. Where a
`cfg` is still needed, mirror upstream's own predicate `target_family = "wasm"`, not
`target_arch = "wasm32"` — `Listener::wait` is gated on the former and the existing import already
carries the skew.

**The milestone rule, from review and worth repeating:** wiring `TransitionRoute::handle_push` is a
separate PR but the **same milestone**, and #1161 does not close until it lands. Shipping the fix
alone produces one more correct, tested seam that nothing calls — this repository's dominant defect
class, and exactly what the scout report warned about.

## Tests — corrected

| # | Test | Revert to redden |
|---|---|---|
| T1 | `a_poll_registers_its_listener_before_the_decisive_state_read` — trace `[0, 1]` **plus** `total_listeners() == 1` **and** the waker's `Arc` strong count ≥ 2. **The last two are sampled from the TEST, after `poll()` returns `Pending`** — only the trace is internal | the `continue;` |
| T0 | `the_resolution_is_published_before_the_notification` — a waker that asserts `is_complete()` is *already* true from inside `wake()`. Safe: `is_complete` takes the FLUI state mutex, never the list lock, so it cannot hit the §0.3 deadlock | moving `*state = …` after `notify` in `resolve` |
| T1b | the same on `TickerFutureOrCancel` — D1's premise is "one state machine written twice"; a shared helper is only *proven* shared if both impls are exercised | the `continue;` |
| T2 | `a_pending_await_is_woken_and_resolves_when_the_ticker_stops` — worker polls manually and signals **after** observing `Poll::Pending`, then blocks on a channel the waker feeds; `recv_timeout(5s)`; **never `join()`** | the `notify` call |
| T3 | `a_canceled_base_future_holds_no_waker_and_no_listener` — **polls a second time after the cancel**, else the first poll's `Notified` entry is still linked and the oracle reads 1 regardless | `Resolved::Canceled => Poll::Pending` |
| T5' | `mute_then_start_does_not_orphan_the_pending_future` — defect 5 | `start_inner`'s `active_future.is_some()` refusal |
| T5'' | `a_refused_start_drops_the_callers_callback_outside_the_lock` — a `Drop` canary on the rejected callback probes `Ticker::try_state()`, the `stale_callback_is_dropped_outside_the_lock` shape | hoisting the refusal's `return` back inside the guard scope |
| T6 | `an_awaiters_waker_runs_with_no_ticker_lock_held` — probes **both** `TickerFutureInner::state` and `Ticker::try_state()`. **Discriminating**, contrary to r2: `drop(state)` exists on `main` in `set_complete`/`set_canceled`, but `resolve` is code this PR authors | `drop(state);` in the new `resolve` |
| T7 | `a_second_resolution_is_ignored_and_the_first_outcome_stands` — also pins the invariant that makes "two `resolve`s cannot interleave" true | the `if *state != Pending` guard |
| T8 | `ticker_future_auto_traits` — **labelled a compile-time fence, no production revert** | — |
| T9 | `or_cancel_accessed_after_resolution_resolves_immediately` — **contract documentation, pins nothing** | — |

**T0 exists because the single most load-bearing line in the file was unpinned.** Register-then
-recheck is correct *only* because `resolve` writes the durable state before it notifies — and
nothing in r2's table could go red if that order were ever reversed. T1 is single-threaded and would
pass; T7 pins the once-only guard, not the ordering; T2's race is nondeterministic.

**A third wrong implementation passes the strengthened T1, so the sampling site moved.** Hand
`poll_resolution` a **local** `Option<EventListener>` instead of `&mut self.listener`: the trace is
`[0, 1]`, the listener count is 1, the waker strong count is 2 — all three pass — and then the local
slot drops on return, unlinking the entry *after* the oracle sampled. Permanent hang. That is exactly
where a two-impl→one-helper extraction goes wrong, which is what D1 is. Sampling from the call site
after `Poll::Pending` closes it for free.

**Label which assertion is which in T1.** The `[0, 1]` trace is a **shape** pin — this plan itself
notes it fails a strictly-correct "register first, read once" helper — while the listener count and
waker strong count are the **invariant** pin. Keeping both is right; not labelling them means the
next person who legitimately reshapes the helper reads a red `[0,1]` as a correctness regression.

**T1 needs a hand-rolled waker, and that means `unsafe` in a test file.** `flui-scheduler` has no
`futures`/`futures-util` dependency (normal or dev), so there is no `futures::task::waker` to build a
strong-count-observable waker from. Either add `futures-util` as a dev-dependency or pre-write the
`RawWakerVTable` with `Arc::{increment,decrement}_strong_count` and a `// SAFETY:` block — the
crate's existing test wakers are no-op vtables and carry no `unsafe` beyond `Waker::from_raw`.

**T1's r1 oracle was gamed and is replaced.** `read_trace == [0, 1]` alone passes on at least two
wrong implementations — one that takes the listener and forgets to store it back, and one that
registers and returns `Pending` without ever polling the listener (entry stays `Created`, never
woken) — and *fails* a strictly-correct "register first, read once" shape. The listener-count and
strong-count assertions are what make it pin the invariant rather than the written shape.

**Five of r1's nine tests did not discriminate, not two.** T6's and T7's revert lines are on `main`
today; T8 is a compile fence; T9 pins nothing; T2 passes on the buggy code. Only T1/T1b, T3 and T5'
can go red on this change. Saying so is the point.

T4/T5 from r1 are **deleted** — they tested D3, and as written they could not have passed: both end
in the underflow above.

## Risks

1. **The §0.3 deadlock becomes newly reachable** — today's bug suppresses it by never firing wakers.
   Mitigated by the stated precondition, not by a test. The nearest real shape is
   `AnimationController::{dispose, restart_ticker}` calling `ticker.stop()` **under the controller's
   own lock**, which the first consumer will hit — it belongs in the follow-up PR's brief.
2. *(deleted — it described D3, which is gone. The verified fact travels to #1162's brief instead:
   `lock_api` declares no `UnwindSafe`/`RefUnwindSafe` impls, so `parking_lot::Mutex<T>` inherits
   `!RefUnwindSafe` from its `UnsafeCell`, unlike `std::sync::Mutex` which has an explicit impl
   because it poisons.)*
3. **The reorder's justification was void and is restated.** Nothing a caller can observe changes:
   all three drivers already commit ticker state under the lock before either step, and
   `cancel_frame_callback` runs no user code **at these call sites** — the cancelled entry is the
   ticker's own auto-schedule closure, capturing three `Arc`s the live `Ticker` keeps alive. The
   unqualified form would be false: `add_frame_callback` is `pub`, so cancelling an arbitrary
   registration drops an arbitrary closure. Take the reorder because it is Flutter's order
   (`_future = null` → `unscheduleTick()` → `_complete()`), not for unwind safety. Note that
   `retained_tickers_future_resolves_canceled_after_owner_disposes_past_scheduler_drop`'s doc
   comment becomes **false prose** while its assertion still passes — the exact surviving-but-wrong
   rationale this repo keeps producing.
4. `read_trace` needs interior mutability (`Mutex<Vec<usize>>`); the helper holds `&Arc<…Inner>`.

## Files
- `crates/flui-scheduler/src/ticker.rs` — the `poll_resolution` helper; both `Future` impls delegate;
  the `Canceled` arm; defect 5's `active_future.is_some()` refusal **hoisted out of the lock scope**;
  the reorder in `stop`/`dispose`/`reset`; the §0.3 precondition on the `Future` impl's doc;
  `#[must_use]` on **`TickerFutureOrCancel` only** — *not* on `TickerFuture`, because
  `AnimationController::restart_ticker` calls `ticker.start(..)` as a bare statement and
  `just clippy` runs `-D warnings`; the wasm32 pending-path `error!` + `debug_assert!(false)` with a
  doc naming `or_cancel` as the supported route and one line justifying the debug-panic against
  `assert_not_disposed`'s in-crate precedent.
- `crates/flui-scheduler/src/ticker/future_tests.rs` — **new** child `#[cfg(test)] mod`, for private
  -field access and because `ticker.rs` is **2378 lines today** and nine new tests plus a
  counting-waker vtable clear 2.5k inline.
- `crates/flui-animation/src/controller.rs` — `restart_ticker`'s `can_tick()` guard widened (defect
  5's interaction 2). **Same PR, not a follow-up** — without it the defect relocates rather than
  closes.
- `crates/flui-scheduler/ARCHITECTURE.md` — the three `## Mapping decisions` entries, plus the
  one-line pointer to #1162's `thread::panicking()` decision so the rule is reachable from the crate
  rather than only from an issue.
- `docs/runtime-contract.toml` — the `consumers` correction.

## Gates

`just gate` · `cargo nextest run -p flui-scheduler -p flui-animation --locked` ·
`cargo test -p flui-scheduler --doc --locked` · `just wasm-check`

The last three are **mandatory, not prudent**: `-p flui-animation` because defect 5's fix changes a
predicate `AnimationController` depends on; `--doc` because D2's pillar turns on the doctests
(`Err(TickerCanceled)` as a pattern, `let error = TickerCanceled;` as an expression) and doctests
compile as an external crate, which is exactly what makes that argument true; `just wasm-check`
because D4 changes wasm32 behaviour.

**Zero executing wasm32 coverage, restated because r2 deleted it while *escalating* the wasm32
change.** `flui-scheduler` declares no wasm32 `wasm-bindgen-test` dev-dependency, so `just wasm-test`
does not discover it. The `cfg` is proven by `just wasm-check` compiling and by nothing else. Stated,
not papered over — and it matters more now that the change alters behaviour there rather than
deleting a method.


## Review history

- **r1** — three defects, D3 retry containment, nine tests. Reviews found: the retry corrupts
  `event-listener`'s counter and lands an abort in the dependency's `Drop` (measured); a fifth
  defect (mute→start) absent from the enumeration; a new unresolved-future path created by the
  reorder; T1's oracle passing on two wrong implementations; five non-discriminating tests declared
  as two; §0.4's "additively" false; the zero-consumer argument inverting `AGENTS.md`; §0.6 already
  recorded in an existing audit.
