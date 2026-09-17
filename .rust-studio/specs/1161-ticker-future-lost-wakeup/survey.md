# Market survey — cancellation shape for `TickerFuture`, and the lost-wakeup trap

Research date: 2026-09-15. Scope: the three questions posed for issue #1161.

**Evidence rules used.** Every claim below is backed by an official API reference, the project's own
source at a pinned revision, or its issue tracker. Blog posts and forum answers were used only as
leads. Version-pinned docs.rs URLs were verified to return HTTP 200 on 2026-09-15; the version number
in each URL is the version the docs.rs "latest" page reported at fetch time.

---

## Question 1 — how other toolkits model "await until the animation ends, or until it is cancelled"

### 1.1 Jetpack Compose — a VALUE for intrinsic end reasons, an EXCEPTION for third-party interruption

Source: androidx GitHub mirror at commit `e6d33dd5d0a60001a5784d84123b05308d35f410` (authored 2025-05-16),
`compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/Animatable.kt`
and `.../AnimationEndReason.kt`.

The suspend functions return a value:

```kotlin
public suspend fun animateTo(
    targetValue: T,
    animationSpec: AnimationSpec<T> = defaultSpringSpec,
    initialVelocity: T = velocity,
    block: (Animatable<T, V>.() -> Unit)? = null,
): AnimationResult<T, V>
```
— `Animatable.kt`, `animateTo`. `animateDecay` has the same return type.

```kotlin
public class AnimationResult<T, V : AnimationVector>(
    public val endState: AnimationState<T, V>,
    public val endReason: AnimationEndReason,
)
```
— `Animatable.kt`, `class AnimationResult`.

The end-reason enum has **exactly two variants, and neither is "cancelled"**:

```kotlin
/** Possible reasons for [Animatable]s to end. */
public enum class AnimationEndReason {
    /**
     * Animation will be forced to end when its value reaches upper/lower bound (if they have been
     * defined, e.g. via [Animatable.updateBounds])
     * ...
     */
    BoundReached,
    /** Animation has finished successfully without any interruption. */
    Finished,
}
```
— `AnimationEndReason.kt`, verbatim.

Cancellation is a different channel entirely. From `animateTo`'s KDoc, verbatim:

> If the animation gets interrupted by 1) another call to start an animation (i.e.
> \[animateTo]/\[animateDecay]), 2) \[Animatable.stop], or 3)\[Animatable.snapTo], the canceled
> animation will throw a \[CancellationException] as the job gets canceled. As a result, all the
> subsequent work in the caller's coroutine will be canceled. This is often the desired
> behavior. If there's any cleanup that needs to be done when an animation gets canceled,
> consider starting the animation in a `try-catch` block.

— `Animatable.kt`, `animateTo` KDoc (the identical paragraph appears on `animateDecay`).

The mechanism is `mutatorMutex.mutate` plus an explicit catch-and-rethrow:

```kotlin
val endReason = if (clampingNeeded) BoundReached else Finished
endAnimation()
AnimationResult(endState, endReason)
} catch (e: CancellationException) {
    // Clean up internal states first, then throw.
    endAnimation()
    throw e
}
```
— `Animatable.kt`, `runAnimation`.

**The distinction the brief asked about, stated precisely.** Compose does not split "cancelled" from
"interrupted by another animation" — both produce `CancellationException`, because both are the same
mechanism (the losing coroutine's job is cancelled by `MutatorMutex`). `Animatable.stop()`, the closest
analogue of `Ticker::stop(canceled: true)`, is documented as "Stops any on-going animation with a
\[CancellationException]" (`Animatable.kt`, `stop` KDoc). That is an exception, not a value.

The split that *is* real in Compose is **intrinsic vs extrinsic**:

| How the run ended | Shape |
|---|---|
| Ran to its target (`Finished`) | value (`AnimationResult`) |
| Stopped early by its *own* configured bounds (`BoundReached`) | value (`AnimationResult`) |
| Stopped by a third party (`stop`, `snapTo`, a newer `animateTo`, or the caller's scope being cancelled) | `CancellationException` |

`BoundReached` is worth noting for this decision: it is a case where the animation *stopped early*
and Compose still reports it as a value, because the reason is part of the animation's own contract.
It is not a cancellation.

### 1.2 Kotlin coroutines — `CancellationException` is a *third* channel, explicitly not an error

This is what makes the Compose evidence load-bearing rather than a simple vote for "error".

Source: `Kotlin/kotlinx.coroutines` at tag `1.11.0` (released 2026-05-08).

```
 * Normal cancellation of a job is distinguished from its failure by the exception
 * that caused its cancellation.
 * A coroutine that throws a [CancellationException] is considered to be _cancelled_ normally.
 * If a different exception causes the cancellation, then the job has _failed_.
 ...
 * Note, that the [cancel] function on a job only accepts a [CancellationException] as a cancellation cause, thus
 * calling [cancel] always results in a normal cancellation of a job, which does not lead to cancellation
 * of its parent.
```
— `kotlinx-coroutines-core/common/src/Job.kt`, `interface Job` KDoc, verbatim.

```
We already know that a cancelled coroutine throws [CancellationException] in suspension points and that it
is ignored by the coroutines' machinery.
```
and

```
Cancellation is closely related to exceptions. Coroutines internally use `CancellationException` for cancellation, these
exceptions are ignored by all handlers, so they should be used only as the source of additional debug information, which can
be obtained by `catch` block.
```
— `docs/topics/exception-handling.md` at tag `1.11.0`, verbatim.

So Kotlin's model is three-valued: **success (value) / cancelled (transparent, ignored by handlers,
does not propagate as failure) / failed (real exception)**. Compose puts animation cancellation in the
middle channel, not the failure channel.

**Rust has no middle channel.** `Result::Err` is the failure channel. Mapping Compose's cancellation
onto `Err` conflates "cancelled normally" with "failed" — which is the same conflation Dart forced on
Flutter (§1.3), and Dart's version of it produced a real, reported hazard (§2). The nearest Rust
analogue of the transparent-cancellation channel is *dropping the future*, which is unavailable here
because in FLUI the cancelling party is not the awaiter.

### 1.3 Flutter — the error shape, and the framework's own avoidance of it

Source: `.flutter` at the pinned tag `3.44.0` (verified: `git -C .flutter describe --tags` prints
`3.44.0`), `packages/flutter/lib/src/scheduler/ticker.dart`, `class TickerFuture`.

Class doc, verbatim:

> If the \[Ticker] is disposed without being stopped, or if it is stopped with
> `canceled` set to true, then this Future will never complete.

`orCancel`'s doc carries the warning the brief asked about, verbatim:

> A future that resolves when this future resolves or throws when the ticker
> is canceled.
>
> If this property is never accessed, then canceling the ticker does not
> throw any exceptions. Once this property is accessed, though, if the
> corresponding ticker is canceled, then the \[Future] returned by this
> getter will complete with an error, and if that error is not caught, there
> will be an uncaught exception in the current zone.

`whenCompleteOrCancel` exists specifically to defuse that, verbatim:

> Calling this method registers an exception handler for the \[orCancel]
> future, so even if the \[orCancel] property is accessed, canceling the
> ticker will not cause an uncaught exception in the current zone.

**Is the error shape considered a wart upstream?** Not in writing — no upstream issue or design doc
says so (§2). But the framework's own usage is decisive evidence about how safe it is felt to be. A
census of `packages/flutter/lib/` at tag 3.44.0:

| Symbol | Non-doc-comment uses inside the framework |
|---|---|
| `.orCancel` | **0** — every one of the 21 occurrences is inside a `///` doc comment or a doc snippet |
| `whenCompleteOrCancel` | 3 — `material/tab_controller.dart`, `widgets/navigator.dart`, `widgets/draggable_scrollable_sheet.dart` |

(Commands: `grep -rn "\.orCancel" packages/flutter/lib --include="*.dart"` and the same for
`whenCompleteOrCancel`, at tag 3.44.0.)

In the test corpus, `TickerCanceled` appears in exactly one file:
`packages/flutter/test/animation/futures_test.dart` (two `on TickerCanceled` catches).

So: **Flutter ships the error shape, documents it, warns about it, and then never uses it internally —
it uses the callback form that pre-registers a handler.** The canonical consumer-facing example is a
doc snippet in `animation_controller.dart`:

```dart
Future<void> fadeOutAndUpdateState() async {
  try {
    await fadeAnimationController.forward().orCancel;
    await sizeAnimationController.forward().orCancel;
    setState(() { dismissed = true; });
  } on TickerCanceled {
    // the animation got canceled, probably because we were disposed
  }
}
```

The design intent is recorded in the PR that introduced `orCancel`,
[flutter/flutter#9389](https://github.com/flutter/flutter/pull/9389) (Hixie, merged 2017-04-15),
verbatim from the PR body: the `try`/`on TickerCanceled` form gives "a nice way of doing animations in
sequence without leaking the controller", the bare-`await` form means "the method will just silently
hang (and get GC'ed) if the widget is disposed, without leaking anything", and the whole arrangement is
"without spurious errors for uncaught exceptions on controllers." The hazard was understood at design
time; the two-future split *is* the mitigation.

### 1.4 Unity — `Awaitable` throws `OperationCanceledException`

Source: Unity 6000.0 Scripting API, `Awaitable.Cancel`
(<https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Awaitable.Cancel.html>), verbatim:

> Cancels the awaitable. If the awaitable is being awaited, the awaiter receives a
> System.OperationCanceledException.

and on the two cancellation models being equivalent:

> Some methods returning an `Awaitable` also accept a `CancellationToken` […] both cancellation models
> are equivalent: whether you call `Awaitable.Cancel` directly or trigger the supplied
> `CancellationToken`, the awaiter receives an `OperationCanceledException`.

Unity therefore uses the exception shape — but note that .NET, like Kotlin, treats
`OperationCanceledException` as a distinguished cancellation channel rather than a failure (it is the
exception `CancellationToken.ThrowIfCancellationRequested` raises, and `Task` records it as
`TaskStatus.Canceled`, not `Faulted`). This is the same "third channel" situation as §1.2, and it is
the same thing Rust lacks. **Open:** I did not fetch a primary .NET citation for the
`Canceled`-vs-`Faulted` `TaskStatus` distinction; treat that last sentence as uncited background, not
evidence.

### 1.5 Rust — no single convention; the split tracks *who is surprised*

Rust crates do all three things. What separates them is not taste, it is whether cancellation is an
expected outcome of the protocol or an accident of the counterparty disappearing.

**Error-shaped** (cancellation as `Err`):

| API | Output | Citation |
|---|---|---|
| `tokio::sync::oneshot::Receiver` | `Result<T, RecvError>`; `RecvError` is "returned by the receiver when the sender is dropped without sending", implements `std::error::Error` | <https://docs.rs/tokio/1.53.1/tokio/sync/oneshot/error/struct.RecvError.html> |
| `futures::future::Abortable` | `Result<Fut::Output, Aborted>`; "A future/stream which can be remotely short-circuited using an `AbortHandle`." `Aborted` is "Indicator that the `Abortable` task was aborted." and implements `Display` + `Error` (with `std`) | <https://docs.rs/futures/0.3.34/futures/future/struct.Abortable.html>, <https://docs.rs/futures/0.3.34/futures/future/struct.Aborted.html> |
| `tokio::task::JoinHandle` | `Result<T, JoinError>`; `JoinError` = "Task failed to execute to completion", with `is_cancelled()` = "Returns true if the error was caused by the task being cancelled" | <https://docs.rs/tokio/1.53.1/tokio/task/struct.JoinError.html> |

`Abortable` is the closest structural match to the question (a *third party* cancels a future the
awaiter is holding) and it is `Err`-shaped. So the error shape is genuinely a Rust convention, not a
Dart import.

**Value-shaped** (cancellation as a datum):

| API | Output | Citation |
|---|---|---|
| `CancellationToken::cancelled()` | resolves to `()` — "Returns a Future that gets fulfilled when cancellation is requested"; completes immediately if already cancelled | <https://docs.rs/tokio-util/0.7.17/tokio_util/sync/struct.CancellationToken.html> |
| `CancellationToken::run_until_cancelled()` | `Option<F::Output>` — "returns its result wrapped inside of an Option unless the `CancellationToken` is cancelled. In that case the function returns `None` and the future gets dropped." | same page |
| `async_task::Task::cancel()` | `Option<T>` — "Returns the task's output if it was completed just before it got canceled, or `None` if it didn't complete." | <https://docs.rs/async-task/4.7.1/async_task/struct.Task.html> |

**The pattern.** The `Err`-shaped APIs are all *generic plumbing* where the library cannot know whether
the awaiter expected cancellation: a dropped `oneshot` sender means the answer will never exist; an
aborted task means work was destroyed. The value-shaped APIs are all cases where cancellation is a
*designed, routine outcome of the protocol* — which is exactly what a ticker cancellation is (dispose a
widget, start a competing animation, close a route).

**Never-resolving** is not a Rust convention anywhere I found. Flutter's primary `TickerFuture` does
this; no Rust crate surveyed leaves a future permanently pending as its cancellation contract.
Prime-directive note: the primary future never completing is a Dart-era affordance, not a contract
worth porting — Dart can GC a suspended async method; a Rust future left pending is a live allocation
plus a registered waker that nothing will ever fire.

### 1.6 The decision: value or error?

**There is no consensus strong enough to call Flutter's error shape "wrong" for Rust.** `Abortable` is a
first-party `futures` API doing exactly the error shape for exactly this situation. Anyone who says the
Rust ecosystem has converged on a value is overstating it.

The evidence does, however, tilt toward a value (`TickerEnd::{Completed, Canceled}`), for four reasons,
in descending strength:

1. **The single hazard that the error shape is built around does not exist in Rust.** Flutter's entire
   two-future design (`orCancel` lazily created, `whenCompleteOrCancel` existing solely to pre-register
   a handler, the doc warning, the framework's own zero internal uses) is machinery for managing
   *uncaught zone exceptions*. A Rust `Result` is inert until inspected; an unhandled one produces a
   `#[must_use]` warning at compile time, never a runtime crash. Porting the error shape therefore ports
   the ergonomics of the mitigation without the problem it mitigates. This is a Prime-Directive rule-1
   case: the reference's contract is shaped by a constraint FLUI does not have.
2. **Cancellation here is an expected protocol outcome, not a counterparty failure**, which is the line
   the Rust ecosystem actually splits on (§1.5). Widget disposal cancelling a ticker is the normal case,
   not the exceptional one.
3. **Compose, the most recent greenfield design of this exact API, uses a value for every end reason it
   can express as one**, including an early stop (`BoundReached`), and reaches for the exception channel
   only because Kotlin *has* a non-failure exception channel. Rust does not, so the Compose design
   translated to Rust is "value".
4. **Conversion is asymmetric.** A value-shaped `TickerEnd` can hand back the `?`-friendly shape with a
   two-line adapter (`fn ok_or_canceled(self) -> Result<(), TickerCanceled>`); an error-shaped API cannot
   recover richer end reasons later without a breaking change. If a third end reason is ever wanted (a
   `BoundReached`-like "stopped by a competing animation" vs "disposed"), the value absorbs it and the
   error does not.

Counter-evidence to weigh honestly: the `?` operator makes `await?` genuinely more ergonomic for the
"abort my whole sequence if the animation was cancelled" case, which is the dominant use in Flutter's
own doc snippet and matches Compose's structured-concurrency default of cancelling the caller's
remaining work. A value forces a `match` at every await site unless the adapter above is provided.

Suggested shape if the value is chosen: `#[must_use]` on `TickerEnd` recovers "you must handle this"
at compile time, which is the property Flutter's error shape was buying at runtime — and it buys it
without the uncaught-exception failure mode.

---

## Question 2 — the Flutter issue tracker

**Searched, found nothing of the class in question.** This is a positive finding, not a failure to search.

Method (GitHub search API via `gh api -X GET search/issues`, open and closed, issues and PRs,
2026-09-15):

| Query | Total hits | Relevant |
|---|---|---|
| `repo:flutter/flutter TickerFuture in:title,body` | 27 | 0 about `TickerFuture` semantics |
| `repo:flutter/flutter "TickerFuture" in:title` | 0 | — |
| `repo:flutter/flutter orCancel in:title,body` | 9 | 3 |
| `repo:flutter/flutter TickerCanceled in:title,body` | 4 | 3 (same set) |
| `repo:flutter/flutter "ticker was canceled" in:title,body` | 1 | 1 |
| `repo:flutter/flutter whenCompleteOrCancel in:title,body` | 20 | 0 |
| `repo:flutter/flutter "never completes" animation ticker in:title,body` | 1 | 0 |
| `repo:flutter/flutter Ticker future does not complete in:title` | 0 | — |
| `repo:flutter/flutter Ticker uncaught exception animation canceled in:title` | 0 | — |

The 27 `TickerFuture` body hits and all 20 `whenCompleteOrCancel` hits are **stack-trace noise** —
crash reports from `Navigator`, `TabController`, `go_router`, Firebase, etc. whose frames happen to
include `ticker.dart`. None is a report about ticker-future behaviour.

What the substantive hits actually are:

- [#9389](https://github.com/flutter/flutter/pull/9389) (PR, merged 2017-04-15) — the PR that *introduced*
  `orCancel`. Design intent, quoted in §1.3. Not a defect.
- [#22777](https://github.com/flutter/flutter/issues/22777) (2018-10-07, closed 2019-02-18) — **the
  uncaught-`TickerCanceled` hazard materialising in the wild.** A user's `AnimationController.animateTo`
  produced `Dart Error: Unhandled exception: This ticker was canceled: Ticker(created by
  _MyHomePageState#…)`. Closed by a triager with, verbatim: "Sounds like expected behavior to me" —
  quoting the `orCancel` doc-comment back at the reporter — and "removing `.orCancel` from
  `progressBarAnimationController.forward().orCancel;` fixes the issue." **Working as intended; the API
  is the trap.** This is the one issue that directly bears on the shape decision.
- [#20792](https://github.com/flutter/flutter/issues/20792) (2018-08-19, closed 2020-08-17) — user
  confusion using `await controller.forward(from: 0.0).orCancel` inside `build`. Not a framework defect.

**Nothing found in any of the categories asked about:** no issue reporting a `TickerFuture` that never
completed when it should have, no missed completion, no cancellation that leaked, and no *open* issue
about the uncaught `TickerCanceled` (the only report of it was closed as intended behaviour).

Corroborating primary evidence that the class is genuinely absent rather than merely unreported: the
full commit history of `packages/flutter/lib/src/scheduler/ticker.dart` since #9389 landed (60 commits,
2017-04-15 → 2026-09-11, via `gh api repos/flutter/flutter/commits -f path=…`) contains **no bugfix to
`TickerFuture`'s completion logic**. Every commit is lints, formatting, doc/docImport changes,
null-safety migration, leak-tracker instrumentation, binding cleanups, or unrelated `Ticker` features
(`Support disabled animations` #20354, `Ticker should dispatch creation and disposal events` #137844,
`[Animation] Add granular frame forcing` #173862). The one entry that *looks* like a ticker-future
bugfix, "Bugfix in the `absorbTicker` method"
([#181706](https://github.com/flutter/flutter/pull/181706), merged 2026-02-25), is a one-line **debug
assertion** correction, `(originalTicker._future == null) == (originalTicker._startTime == null)` →
`(originalTicker._future != null) || (originalTicker._startTime == null)`, not a change to when or
whether the future resolves.

**Why the absence is expected, and what it does and does not license.** `TickerFuture` is built on
Dart `Completer`s (`_primaryCompleter`, `_secondaryCompleter` in `ticker.dart`). A `Completer` is a
durable one-shot: an awaiter attached before or after `complete()` is resolved either way, so the
register-vs-observe window this bug lives in **does not exist upstream**. Flutter's silence is therefore
evidence that the *structure* is safe, not evidence that FLUI's `event_listener`-based structure is —
the absence supports "do not look to Flutter for a fix", and nothing more.

---

## Question 3 — the general trap, named and sourced

Yes, it is canonical, and it has names at three levels.

### 3.1 The primitive that exists to solve it: **eventcount** (Dmitry Vyukov)

Both relevant Rust crates name the same source. `event-listener` v5.4.2, crate root doc, verbatim:

```
//! This is a synchronization primitive similar to [eventcounts] invented by Dmitry Vyukov.
//! [eventcounts]: https://www.1024cores.net/home/lock-free-algorithms/eventcounts
```
— `smol-rs/event-listener@v5.4.2:src/lib.rs`

`async-event` v0.2.1 subtitles itself "An efficient async condition variable for lock-free algorithms,
a.k.a. 'eventcount'" and links the same page
(`asynchronics/async-event@v0.2.1:README.md`).

**Open:** the primary write-up at <https://www.1024cores.net/home/lock-free-algorithms/eventcounts>
could not be fetched on 2026-09-15 — it returns `301 Moved Permanently` to `https://127.0.0.1/`, i.e.
the site is misconfigured/defunct. `web.archive.org` is not fetchable from this environment either. The
protocol name (`prepare_wait` → re-check the predicate → `commit_wait`/`cancel_wait`) is therefore
**uncited** here; only the fact that both crates attribute the primitive to Vyukov at that URL is
sourced.

### 3.2 The failure mode's name: **lost wakeup / lost signal**, and the fix idiom **register-then-recheck**

`event-listener`'s own docs state the edge property outright — verbatim,
`smol-rs/event-listener@v5.4.2:src/lib.rs`, `Event` struct doc:

> If there are no active listeners at the time a notification is sent, it simply gets lost.

and the crate-root example is literally the register-then-recheck loop, comments included:

```rust
loop {
    // Check the flag.
    if flag.load(Ordering::SeqCst) { break; }

    // Start listening for events.
    let mut listener = event.listen();

    // Check the flag again after creating the listener.
    if flag.load(Ordering::SeqCst) { break; }

    // Wait for a notification and continue the loop.
    listener.wait();
}
```
— `smol-rs/event-listener@v5.4.2:src/lib.rs`, crate-root doc example.

**The critique the brief expected exists, and it is explicit.** `asynchronics/async-event@v0.2.1:README.md`,
section "Differences with `event_listener`", verbatim:

> The API is more opinionated and designed to preventing potential misuse such as:
>
> - *Forgetting to check again the predicate after requesting a notification, i.e.
>   after a call to `Event::listen()` in the `event_listener` crate*.
>   `async-event` provides instead the `Event::wait_until` method which takes care
>   of checking the predicate whenever necessary to prevent races.

That is the trap, named, attributed to the exact API, with the fix idiom (fold the re-check into the
primitive so the caller cannot omit it).

`tokio::sync::Notify` documents the identical hazard for its own API (the `enable()`-before-check
requirement), verbatim from
<https://docs.rs/tokio/1.53.1/tokio/sync/struct.Notify.html>:

> Both calls to `try_recv` return `None`. Both new elements are added to the vector. The `notify_one`
> method is called twice, adding only a single permit to the `Notify`. Both calls to `recv` reach the
> `Notified` future. One of them consumes the permit, and the other sleeps forever.

and the fix:

> By adding the `Notified` futures to the list by calling `enable` before `try_recv`, the `notify_one`
> calls in step three would remove the futures from the list and mark them notified instead of adding a
> permit to the `Notify`. This ensures that both futures are woken.

### 3.3 The classic literature: condition variables

The oldest canonical statement is the POSIX rationale for `pthread_cond_wait`
(The Open Group Base Specifications Issue 8, IEEE Std 1003.1-2024,
<https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_cond_wait.html>), verbatim:

> The application needs to recheck the predicate on any return because it cannot be sure there is
> another thread waiting on the thread to handle the signal, and if there is not then the signal is
> lost.

> It is thus recommended that a condition wait be enclosed in the equivalent of a "while loop" that
> checks the predicate.

The same rule in the Rust standard library, `std::sync::Condvar::wait`
(<https://doc.rust-lang.org/std/sync/struct.Condvar.html>), verbatim:

> Note that this function is susceptible to spurious wakeups. Condition variables normally have a
> boolean predicate associated with them, and the predicate must always be checked each time this
> function returns to protect against spurious wakeups.

and on why the atomic unlock matters — the structural reason a condvar cannot lose a wakeup:

> This function will atomically unlock the mutex specified (represented by `guard`) and block the
> current thread. This means that any calls to `notify_one` or `notify_all` which happen logically
> after the mutex is unlocked are candidates to wake this thread up.

`Condvar::wait_while` is the std-library equivalent of `async-event`'s `wait_until` — the primitive
takes the predicate so the caller cannot forget the loop:

> `condition` is checked immediately; if not met (returns `true`), this will wait for the next
> notification then check again. This repeats until `condition` returns `false`, in which case this
> function returns.

### 3.4 The edge-vs-level vocabulary: `epoll(7)`

The terminology the brief uses ("edge-triggered notification used to resolve a durable one-shot
state") has its canonical definition and its canonical starvation warning in `epoll(7)`
(<https://man7.org/linux/man-pages/man7/epoll.7.html>), verbatim:

> If the *rfd* file descriptor has been added to the **epoll** interface using the **EPOLLET**
> (edge-triggered) flag, the call to epoll_wait(2) done in step **5** will probably hang despite the
> available data still present in the file input buffer.

Same shape as the FLUI defect: a consumer that observed the state, acted, and then waited for another
edge that will never come, because the state is already at its terminal value and the edge has already
been spent.

### 3.5 Summary of the citable idiom

| Level | Name | Primary citation |
|---|---|---|
| The primitive | eventcount (Vyukov) | `event-listener@v5.4.2:src/lib.rs`; `async-event@v0.2.1:README.md` (write-up URL itself **open** — dead) |
| The bug | lost wakeup / lost signal | POSIX `pthread_cond_wait` RATIONALE, Issue 8 |
| The property | a notification with no registered listener is lost | `event-listener@v5.4.2:src/lib.rs` `Event` doc |
| The idiom | register (listen/`enable`) → re-check the durable state → only then wait | `event-listener@v5.4.2:src/lib.rs` crate example; `tokio@1.53.1` `Notify` docs |
| The better idiom | fold the predicate into the primitive so it cannot be omitted | `async-event@v0.2.1` `Event::wait_until`; `std::sync::Condvar::wait_while` |
| The vocabulary | edge- vs level-triggered | `epoll(7)` |

---

## Open items

- **`1024cores.net` eventcounts write-up is unreachable** (301 → `127.0.0.1`), and `web.archive.org` is
  not fetchable here. The `prepare_wait`/`commit_wait`/`cancel_wait` protocol names are therefore
  recorded as unsourced; only the attribution is cited. §3.1.
- **.NET `TaskStatus.Canceled` vs `Faulted`** — asserted as background in §1.4 without a primary
  citation. If the Unity data point is to carry weight in the decision, fetch the .NET reference first.
- **No Rust UI-framework precedent found.** GitHub code search for `"AnimationEndReason" language:rust`
  returned 0 results, and no awaitable animation-completion API was located in the Rust GUI ecosystem
  (Xilem/Masonry, Iced, Dioxus, egui, GPUI). Searched, not found — but the search was shallow (one code
  search plus one org-scoped search), so treat this as "no precedent located", not "none exists".
- **Compose's `MutatorMutex` interruption vs external scope cancellation** were confirmed to produce the
  same `CancellationException` from the KDoc and `runAnimation`'s catch, but I did not trace
  `MutatorMutex` itself to confirm there is no distinguishing marker on the thrown exception.
