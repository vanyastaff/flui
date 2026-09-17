# Survey — #1055 `end_of_frame` demand and cancellation

Everything below was read, not recalled. Citations are file + symbol (line numbers move).

## 1. The issue's own comment thread

`gh issue view 1055 --comments` returns exactly one comment, mine, recording that the terrain
moved under the audited revision (#1156/#1158/#1160) and naming the two decisions to settle
before code. **No external signal in the thread** — no reproduction from a user, no suggested
crate, no linked upstream bug. Recorded because "I checked and it was empty" and "I did not
check" are different states, and only the first is evidence.

## 2. Flutter as reference (`.flutter` at tag 3.44.0, verified by `git describe --tags`)

`packages/flutter/lib/src/scheduler/binding.dart`:

- **`endOfFrame`** (getter). Three load-bearing facts:
  1. It requests a frame **only when `schedulerPhase == SchedulerPhase.idle`** — not
     unconditionally, and not in any other phase.
  2. The call it makes is **`scheduleFrame()`**, whose own body opens
     `if (_hasScheduledFrame || !framesEnabled) return;` — the enablement gate is *inside* the
     demand call, not at the call site. FLUI's equivalent pair is
     `schedule_frame_if_enabled()` (aliased `ensure_visual_update()`), not the ungated
     `request_frame()`.
  3. **All callers within one frame coalesce onto a single `_nextFrameCompleter`**, completed
     from one `addPostFrameCallback`. There is no per-waiter registry at all.
- Its doc names the hazard we inherit: *"If the device's screen is currently turned off, this may
  wait a very long time, since frames are not scheduled while the device's screen is turned
  off."* — i.e. Flutter deliberately does **not** force a frame past the enablement gate.

Divergence we keep: FLUI resolves with a `FrameTiming`, Dart's resolves with `void`. That
predates this issue and stays (it is strictly more informative). A richer outcome type that would
also distinguish an aborted frame was explored and deferred — see `spec.md` §Scope decision.

## 3. Flutter's issue tracker

Seven queries across open and closed issues for the same failure mode (`endOfFrame` hanging,
`endOfFrame` never completing, awaiting a frame while idle, a cancelled `endOfFrame` retaining
resources). **No relevant hits.**

That absence is itself the finding, and it is explainable rather than surprising: the
single-completer design has **no per-waiter state to leak** (a dropped `await` in Dart drops a
listener on one shared `Future`, and the completer is owned by the binding either way), and the
idle-demand call has been in `endOfFrame` since the getter was introduced. Flutter never shipped
the two defects #1055 describes, so its tracker has nothing to say about them. The shape that
makes the bugs impossible is the shape worth copying.

## 4. Ecosystem: how other Rust UI stacks expose "await the next frame"

| Project | Shape | What it tells us |
|---|---|---|
| `winit` | `Window::request_redraw()` — a demand call, no future at all | Demand and waiting are separate concerns; the redraw request is coalesced by the platform, not by a waiter list |
| `wgpu` | `SurfaceTexture::present()` + `Device::poll(Maintain::Wait)` | Completion is observed on the device, never as a per-caller registry |
| `bevy` | Frame boundaries are schedule stages; `async` work is a task pool that syncs at a stage | No "await the next frame" future is exposed — the boundary is the stage |
| `egui` | `Context::request_repaint()` (+ `request_repaint_after`) | Same split as winit: request demand, never await it |
| `Xilem`/`Masonry` | Driven by the windowing loop; async work lands via a proxy/message channel | The frame is not awaitable; a message crosses back in |

**Conclusion.** No Rust UI framework in the survey exposes an awaitable frame boundary, so none
offers a shape to copy for the *future*; every one of them separates **demand** (a request call)
from **observation**. That separation is the part worth importing, and it is what Flutter's
`endOfFrame` does internally anyway — the request is a side effect of registration, coalesced.

## 4b. Prior art OUTSIDE Rust — the section this survey was missing

§4 swept the Rust UI ecosystem and found nothing awaitable, and concluded there was no shape to
copy. That conclusion was drawn from too small a denominator. **Five toolkits in four other
languages expose exactly this API, and all five make the same design choice FLUI spent four plan
revisions failing to invent.**

| Toolkit | API | The demand decision |
|---|---|---|
| **Jetpack Compose** (Kotlin) | `MonotonicFrameClock.withFrameNanos`, `BroadcastFrameClock` | `onNewAwaiters` — "invoked whenever the number of awaiters has changed **from 0 to 1**" |
| **Android** (Java) | `Choreographer.postFrameCallback` | "runs once then is **automatically removed**"; posting schedules the vsync |
| **Android NDK** (C) | `AChoreographer_postFrameCallback64` | same; "to render continuously the callback should itself call" it again |
| **Web** (JS) | `requestAnimationFrame` / `cancelAnimationFrame` | `await new Promise(r => requestAnimationFrame(r))` — registering IS requesting |
| **Unity** (C#) | `Awaitable.NextFrameAsync(CancellationToken)` | cancellation is a first-class parameter |

**The shared answer: registration itself is the demand.** Not one of them reads a scheduler phase,
and not one keeps a per-frame "is a frame already coming" flag. Compose states the rule exactly:
demand fires on the **0 → 1 transition of the waiter set**. That is strictly simpler than a phase
predicate and it has no wedge, because there is no per-frame state to leave stale.

### `BroadcastFrameClock` answers three more of our open questions

- **Teardown**: `cancel(cancellationException)` "permanently cancels this clock and cancels all
  current **and future** awaiters." Note *future* — a dead clock fails a new awaiter immediately
  rather than letting it register and hang. Our deferred `SchedulerClosed` (#1162) should inherit
  that half.
- **A panicking demand hook**: "If `onNewAwaiters` fails by throwing an exception it will
  permanently fail this `BroadcastFrameClock`; all current and future awaiters will resume with the
  thrown exception." A defined state transition, where our spec had only a `# Panics` note for a
  now-panic-capable `end_of_frame`.
- **Awaiting one handle twice**: Unity documents it as undefined behaviour and pools the object.
  That is our `!Clone` / fused-poll question, answered by prohibition rather than by machinery.

### And AOSP hit our exact bug, in production

A 2025 androidx commit — **"Cancel BroadcastFrameClock awaiters without locks"** (bug b/407027032,
test `BroadcastFrameClockTest.locklessCancellation`, release note: *"Fixed a deadlock that may
affect Molecule users when a suspended call to `FrameClock.withFrameNanos` is cancelled while a
frame is being dispatched."*) — is #1055's Fix 1 + Fix 3 hazard, found by users and fixed by making
**cancellation acquire no locks at all**. Our `Weak`-handle design has that property by
construction (dropping the future takes no lock), which is now externally corroborated rather than
merely argued.

The history is the more useful half: an earlier (2020) `sendFrame` resumed its awaiters **inside**
`synchronized(lock)`. Compose walked the same path this issue is walking — resume-under-lock →
deadlock on concurrent cancellation → lock-free cancellation. A large team took years to arrive
where three review rounds pushed us in a day, which is a reason to trust the destination.

**Method note.** This section exists because the question "can't the answer be found in another
library, or another language?" was asked *after* four design revisions. §4's "no Rust UI framework
exposes an awaitable frame boundary" was true and useless: the denominator was one language. When a
problem has an obvious name — "await the next frame" — check the toolkits that have shipped it,
whatever the language, before designing.

## 5. Library survey (this is what reshaped the design)

The decisive question, asked explicitly: *is there a crate that already solves this, or a version
bump / feature flag that does?*

### `event-listener` — already a dependency, already used one file over

- `crates/flui-scheduler/Cargo.toml` declares `event-listener = "5.3"`; `Cargo.lock` resolves
  **5.4.2**, the current release line. **No version bump and no feature flag is needed** —
  `default = ["std"]` is what this crate wants; `loom` and `portable-atomic`/`critical-section`
  are for model-checking and `no_std`, neither of which applies here.
- It is also already in the workspace graph transitively via `async-lock`, `async-channel`,
  `async-broadcast`, `zbus`, and `moka`, so adopting it adds **zero** supply-chain surface.
- `crates/flui-scheduler/src/ticker.rs` already uses it for the **sibling problem**:
  `TickerFuture` / `TickerFutureOrCancel` are `Arc<{ state: Mutex<State>, event: Event }>` with
  a poll loop that reads the state, registers an `EventListener`, and re-loops. #1055's
  hand-rolled `Vec<FrameCompletionNotifier>` + per-waiter `Arc<Mutex<{completed, waker}>>` is a
  second, worse implementation of a primitive this crate already imports.
- The property the issue's second defect is asking for is the crate's headline behaviour: an
  `EventListener` **deregisters itself from the intrusive list when dropped**, in O(1) and with
  no scan, no tombstone, and no generation id. The whole "how do we remove exactly one waiter
  without a leak or an O(n) sweep" decision the plan was going to litigate is answered by not
  writing the registry.
- Documented semantics that matter here (`src/lib.rs`, `Event`'s own doc):
  - *"If there are no active listeners at the time a notification is sent, it simply gets lost."*
    → the predicate must be durable shared state, re-checked after registering. This is why the
    design below keeps a sequence number rather than relying on the notification.
  - *"If a notified listener is dropped without receiving a notification, dropping will notify
    another active listener."* → a cancelled waiter cannot swallow a completion owed to a sibling.
  - *"Listeners are registered and notified in the first-in first-out fashion, ensuring fairness."*

### The disqualifying objection

Adopting the library for the **notify side** was explored and is **rejected**, on a reason this
repository has already paid for once.

`Event::notify` → `Inner::notify` (`src/intrusive.rs`) calls `task.wake()` **inside the closure
passed to `with_inner`** — that is, while event-listener's own internal
`std::sync::Mutex<Inner<T>>` guard is held:

```rust
// event-listener 5.4.2, src/intrusive.rs, quoted verbatim
if let State::Task(task) = entry.state.replace(State::Notified { additional: is_additional, tag }) {
    task.wake();
}
```

That is a **lock held across user code**, and it is exactly the shape #1057 removed from this very
function. Two of this crate's currently-green tests pin the contract it breaks:

- `notify_frame_completion_tolerates_an_inline_polling_waker` asserts, through a `try_lock` oracle
  on the exact lock an inline re-poll would need, that **no lock is held when `wake()` is called**.
  Under `Event::notify` the lock that matters is event-listener's private list mutex: not
  probeable from outside, non-reentrant, and genuinely held. Any waker that re-enters the event —
  registering a fresh listener, or dropping an `EventListener` — self-relocks and hangs.
- `notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics` asserts a
  panicking waker does not starve the waiters after it. A panic unwinds straight out of
  `Inner::notify`'s `while n > 0` loop, so every later listener goes unwoken in that call.

The panic half alone would be recoverable — the notify loop advances its cursor and marks the
entry `Notified` *before* the wake, every internal lock is taken poison-tolerantly, and
`ListLock::Drop` reconciles the counter on unwind, so a `catch_unwind` around `notify` retried
until clean would resume at the next un-notified listener and terminate. The **lock-across-wake**
half is not recoverable: it is a property of the primitive, not of how we call it.

**Verdict: the hand-rolled fan-out stays.** `event-listener` is the right primitive for a
*single* shared condition with self-deregistering waiters; it is the wrong primitive for a
fan-out whose contract is "release every lock before touching a caller's waker."

### What the survey still bought

Rejecting a candidate on a test-backed structural reason is a result, not a wasted pass. It also
produced three things the design keeps:

1. **No version bump or feature flag helps** — asked explicitly, answered concretely. 5.4.2 is
   current; `loom` / `portable-atomic` / `critical-section` are for model-checking and `no_std`.
2. **A latent instance of the same #1057 defect in this crate** — `TickerFuture::set_complete`
   and `set_canceled` call `event.notify(usize::MAX)`, so they wake *their* callers' wakers under
   event-listener's internal lock too. It has simply never been exercised. §6.
3. **The durable-predicate discipline** — `Event`'s own "a notification with no listener is simply
   lost" is why the design below keeps a value the waiter can re-read, rather than trusting the
   wake. That lesson transfers even though the crate does not.

### Alternatives considered and rejected

| Candidate | Why not |
|---|---|
| `async-event` (asynchronics) | Better ergonomics (`wait_until` takes the predicate, so the re-check race is unrepresentable) and claims fewer lock operations — but it is a **new dependency** to replace one already in the crate and already used by its sibling module. Not worth a second primitive for the same job. Worth revisiting only if both `TickerFuture` and this migrate together. |
| `diatomic-waker` | Single-waiter only. `end_of_frame` is many-waiter by contract. |
| `tokio::sync::Notify` | Would drag tokio into a crate that has no runtime dependency. Non-starter. |
| `futures-channel::oneshot` | One channel per waiter re-creates the per-waiter allocation and registry the fix is removing; cancellation cleanup becomes ours again. |
| `event-listener-strategy` (`easy_wrapper!`) | Already in the lock file transitively, and it is the idiomatic way to build a future over `Event` with the register-then-recheck shape baked in. Rejected **for now** only because it is a second macro-shaped concept in a crate whose sibling future is hand-written; the hand-written poll loop here is ~15 lines and reviewable. Recorded as the reshape to take if a third such future appears. |
| `Event<T>` (tagged) instead of a shared slot | Would deliver `FrameTiming` as a notification tag and drop the slot entirely — but it loses the "already completed" fast path, cannot express the teardown `None`, and diverges from the untagged sibling. Rejected. |

## 6. Two defects found in the sibling implementation (pre-existing, out of scope, file both)

### 6a. Lost-wakeup window in the async polls

`TickerFuture::poll` and `TickerFutureOrCancel::poll` (`crates/flui-scheduler/src/ticker.rs`)
read the state, **drop the lock**, and only then call `event.listen()`. If `set_complete()` /
`set_canceled()` runs in that window, its `notify(usize::MAX)` reaches zero listeners and — per
`Event`'s documented "it simply gets lost" — is dropped; the state transition is guarded by
`if *state == Pending`, so **no later notification will ever be sent** and the future hangs
permanently. It is not a delay; it is a hang.

The same file already contains the correct shape, with a comment naming the hazard:
`TickerFuture::when_complete_or_cancel` does *"Register listener BEFORE re-checking state (avoid
race)"*. One of the two async polls and the one blocking path disagree, in the same file — which
is the strongest possible evidence that this is an oversight, not a deliberate divergence.

### 6b. `TickerFuture` wakes user wakers under event-listener's internal lock

`set_complete` / `set_canceled` drop the state guard correctly and then call
`self.inner.event.notify(usize::MAX)` — but `notify` itself calls `task.wake()` while holding
event-listener's private list mutex (§5). So the ticker futures carry the same
lock-held-across-user-code hazard that #1057 removed from `notify_frame_completion`: a waker that
re-enters the event (registering a listener, or dropping one) self-relocks a non-reentrant mutex
and hangs. Unlike the scheduler, the ticker has **no test in this repository exercising either
adversarial contract** — no inline-repoll oracle, no panicking waker, and no drop/waker-release
probe at all. Verified by grep, not assumed.

Both 6a and 6b belong in one follow-up issue: the async ticker futures have never been tested
against cancellation, inline re-poll, or a panicking waker, and each of the three has a known
failure mode.

**Bearing on #1055:** neither the ticker's storage shape nor its notification discipline is
safe to copy. The design in `spec.md` keeps the existing hand-rolled fan-out — which already
releases every lock before `wake()` and contains a panicking waker — and changes only what the
issue is about.

## 7. Repository-hygiene note

`event-listener = "5.3"` is declared crate-locally in `crates/flui-scheduler/Cargo.toml` rather
than in the root `[workspace.dependencies]`, which `AGENTS.md` §"Add a dependency" names as the
convention. Fixing it is a one-line move; it belongs in this PR only if the manifest is touched
anyway, otherwise it is noise. Flagged, not assumed.
