# Survey — how other toolkits implement "await until this animation ends"

> Scope: the **implementation mechanism**, not the API contract (covered by a sibling pass).
> Every claim below is settled against a primary source — the project's own source, its official
> API reference, or its spec. Where a claim could not be settled, it is marked **OPEN**.
> Retrieved 2026-09-15.

## Relationship to the sibling documents in this directory

This file is the **implementation-mechanism** pass. Three siblings cover adjacent ground and should
be read as a set, not merged:

- `survey.md` — the **contract-shape** pass: value-vs-error for the cancel outcome, Kotlin's
  `CancellationException` as a third channel, Flutter's own issue tracker, and the general
  lost-wakeup / register-then-recheck literature (eventcount, condition variables, `epoll(7)`
  edge-vs-level).
- `primitives.md` — which Rust primitive to build the fix on.
- `reference.md` — `.flutter` at tag 3.44.0, `ticker.dart`.

The only real overlap is Jetpack Compose, approached from opposite ends: `survey.md` §1.1 asks what
*shape* the cancel outcome takes, this file §3 asks what *machinery* delivers it and whether a late
observer can exist at all. The conclusions agree.

## The classification axis

The single question asked of every toolkit is: **when a late observer arrives — after the animation
has already ended or was already cancelled — what is the source of truth?**

- **(A) durable resolved state** — the outcome is stored; a late arrival reads it and resolves immediately.
- **(B) edge-triggered signal** — the outcome exists only as a notification; a late arrival gets nothing, ever.
- **(C) hybrid** — an edge signal *plus* a readable status the API tells you to check.
- **(N) no such API** — the toolkit offers nothing for awaiting an animation's end. A documented
  absence is a finding, not silence.

This is exactly FLUI's defect shape. `TickerFuture` already *stores* the outcome
(`ticker.rs:TickerFutureInner` holds `state: Mutex<TickerFutureState>`) but its `poll` reads that
state and *then* subscribes, so a resolution landing in between is lost:

```
// crates/flui-scheduler/src/ticker.rs, impl Future for TickerFuture::poll
let state = *self.inner.state.lock();   // read  ← guard dropped here
match state { … Pending => { if self.listener.is_none() {
    self.listener = Some(self.inner.event.listen()); }   // subscribe ← too late
```

`event-listener` 5.4.2 (the workspace pin, `crates/flui-scheduler/Cargo.toml`) documents the
primitive as edge-triggered and documents the required ordering:

> "If there are no active listeners at the time a notification is sent, it simply gets lost."
> — `Event` struct docs, <https://docs.rs/event-listener/5.4.2/event_listener/struct.Event.html>

> "users of this `new` method must be careful to ensure that the [`EventListener`] is `listen`ing
> before waiting on it"
> — same page, `EventListener::new` caveats

## The two class-(A) exemplars, normatively

These are the reference points the rest of the survey is measured against; both are settled in a
spec or in the language runtime's own source, not in prose.

**JS `Promise` — ECMA-262 §27.5.5.4.1 `PerformPromiseThen`.** The operation branches on the stored
`[[PromiseState]]`: pending appends a reaction record; **fulfilled or rejected enqueues the reaction
job immediately** against the stored `[[PromiseResult]]`:

> "Else if promise.[[PromiseState]] is fulfilled, then Let value be promise.[[PromiseResult]]. Let
> fulfillJob be NewPromiseReactionJob(fulfillReaction, value). Perform
> HostEnqueuePromiseJob(fulfillJob.[[Job]], fulfillJob.[[Realm]])."
>
> <https://tc39.es/ecma262/multipage/control-abstraction-objects.html#sec-performpromisethen>

A late `.then()` is not a special case in the spec; it is one of the three normative branches.

**Dart `Completer`/`Future` — the SDK's own implementation.** `_Future._resultOrListeners` is a
union field: before completion it is the listener chain, after completion it is the *result*.
`_Future._addListener` therefore has an explicit late-arrival branch, commented as such:

```dart
// sdk/lib/async/future_impl.dart, _Future._addListener
} else {
  …
  assert(_isComplete);
  // Handle late listeners asynchronously.
  _zone._scheduleMicrotaskZoned(_zone, () {
    _propagateToListeners(this, listener);
  });
}
```
<https://github.com/dart-lang/sdk/blob/main/sdk/lib/async/future_impl.dart> (`_Future._addListener`)

This is why Flutter's `TickerFuture` cannot have FLUI's bug: the resolved outcome *is* the storage,
and "late" is a first-class branch rather than a race.

---
## 1. Web Animations API — `Animation.finished`

**Sources.** CSSWG editor's draft <https://drafts.csswg.org/web-animations-1/> (§4.5.11–4.5.14,
§4.5.17) and the W3C Working Draft of 5 June 2023 <https://www.w3.org/TR/web-animations-1/>
(same text, numbered §4.4.11–4.4.14, §4.4.17). The two agree word for word on every passage quoted
here; section numbers differ only by the ED having inserted a section earlier in §4.

### The mechanism

The promise is a **field on the animation**, not a per-call object:

> "Each animation has a current finished promise. The current finished promise is initially a
> pending Promise object. The object is replaced with a new promise every time the animation leaves
> the finished play state." — §4.5.11 (TR §4.4.11)

The IDL attribute is a bare accessor with no side effect:

> "finished, of type Promise<Animation>, readonly — Returns the current finished promise for this
> object." — §6.5 (the `Animation` interface attribute table)

Resolution happens in *update an animation's finished state* (§4.5.12 / TR §4.4.12), whose finish
notification steps re-check the play state before resolving and then **resolve the stored promise
object**, and separately queue a `finish` `AnimationPlaybackEvent`:

> "If animation's play state is not equal to finished, abort these steps. Resolve animation's
> current finished promise object with animation. Create an AnimationPlaybackEvent, finishEvent." — §4.5.12

### Is the promise re-created on replay? Yes — and only on leaving the finished state

The last step of §4.5.12 is the replacement rule:

> "If current finished state is false and animation's current finished promise is already resolved,
> set animation's current finished promise to a new promise in the relevant Realm of animation." — §4.5.12

So the resolved promise is **retained for exactly as long as the animation stays finished**. The
only other replacement point is `cancel()`. `play()` does not replace it directly: the ED's `play`
procedure (§4.5.8) resolves the *ready* promise and then runs "update an animation's finished state"
— it is that procedure's last step, quoted above, that installs the replacement once the animation
has left the finished play state.

### The late-arrival answer — and it splits by outcome

This is the part that maps directly onto FLUI's bug, and the answer is **not uniform**:

**Attaching after a natural finish → resolves immediately. Class (A).** The getter returns the
stored promise; that promise was resolved by §4.5.12 and has not been replaced, because the
animation has not left the finished play state. A `.then()` attached afterwards takes ECMA-262's
"already fulfilled" branch above.

**Attaching after a `cancel()` → gets a *fresh pending* promise. The cancel outcome is NOT durably
readable through `.finished`.** The cancel procedure rejects the promise the caller may already
hold, and then installs a replacement:

> "Reject the current finished promise with a DOMException named "AbortError". Set the
> [[PromiseIsHandled]] internal slot of the current finished promise to true. Let current finished
> promise be a new promise in the relevant Realm of animation." — §4.5.14 (TR §4.4.14)

The cancel procedure ends by making the hold time and start time unresolved (§4.5.14), which by the
play-state table in §4.5.17 puts the animation in **`idle`**. So after a cancel the durable,
readable fact is `animation.playState === "idle"` — not the promise. A late arrival that only
awaits `.finished` on a cancelled animation waits forever (until some later play-and-finish
resolves the *replacement* promise).

**Classification: (C) hybrid, with the (A) half covering the finish outcome only.** The durable
half is the retained resolved promise plus the `playState` attribute (§4.5.17: `idle`, `running`,
`paused`, `finished`); the edge half is the `finish` / `cancel` `AnimationPlaybackEvent`s (§4.5.18.3),
which a listener registered after dispatch never receives.

### Two spec details worth importing verbatim into the fix

1. **`[[PromiseIsHandled]] = true` on the rejection.** The spec deliberately marks the rejected
   finished promise as handled so that cancelling an animation *nobody is awaiting* does not raise
   an unhandled-rejection report. The design constraint this encodes — **the cancel path must not
   require an awaiter to exist** — is the same one FLUI needs, where the Rust analogue is "cancel
   must not panic, log at error level, or leak when no one is polling".
2. **The spec manufactures an already-resolved promise when it wants a late reader to succeed.** In
   *reset an animation's pending tasks* (§4.5.14) the ready promise is replaced not by a pending
   promise but by "the result of creating a new **resolved** Promise object with value animation".
   The spec is explicit that a durable pre-resolved handle is the tool for "anyone who looks later
   must succeed immediately".

---
---

## 2. Android — `Animator.AnimatorListener`

**Sources.** AOSP `main`: `frameworks/base/core/java/android/animation/{Animator,ValueAnimator}.java`
and `core/java/android/view/ViewPropertyAnimator.java`
(<https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/animation/Animator.java>);
AndroidX `androidx-main`: `core/core-animation/.../androidx/core/animation/{Animator,ValueAnimator}.java`;
reference <https://developer.android.com/reference/android/animation/Animator.AnimatorListener>.

### `onAnimationEnd` does fire after a cancel — documented

`Animator.cancel()`'s javadoc, identical in the published reference:

> "Cancels the animation. Unlike `end()`, `cancel()` causes the animation to stop in its tracks,
> sending an `Animator.AnimatorListener.onAnimationCancel(Animator)` to its listeners, **followed by**
> an `Animator.AnimatorListener.onAnimationEnd(Animator)` message."

`ValueAnimator.cancel` dispatches `AnimatorCaller.ON_CANCEL` then falls into `endAnimation()`, which
dispatches `ON_END`. The consequence matters for our axis: **`onAnimationEnd` is parameterless.**
Unlike Apple's `finished: Bool`, it carries no outcome, so distinguishing completed from cancelled
requires having been subscribed early enough to catch the *preceding* cancel edge. A subscriber that
arrives even one edge late cannot interpret the end.

Edge case from source: cancelling an animator that was never started dispatches **neither** callback
— `ValueAnimator.cancel` guards on `(mStarted || mRunning || mStartListenersCalled) && mListeners != null`.

### The late-arrival answer: nothing, ever — and the docs never address it

`addListener`'s entire javadoc is *"Adds a listener to the set of listeners that are sent events
through the life of an animation, such as start, repeat, and end."* **OPEN (documentation): the
published contract states no late-registration rule.** The answer below is settled from AOSP source
only.

**Storage.** `Animator.mListeners` is a lazily-allocated `ArrayList<AnimatorListener>`.
`Animator.notifyListeners` → `Animator.callOnList` **snapshots the list into an array before
iterating** (`list.toArray(array)`), which is the concurrent-modification handling: a listener may
remove itself from inside its own callback, and a listener removed *during* dispatch still receives
the in-flight event because the array was taken first.

**The list is not cleared on end** — neither `endAnimation()` nor `cancel()` ever assigns or clears
`mListeners`. **But the end event fires exactly once and is never re-dispatched**, latched twice:

```java
// ValueAnimator.endAnimation(boolean) — re-entry latch
if (mAnimationEndRequested) { return; }
…
mAnimationEndRequested = true;
mRunning = false;
mStarted = false;
```
```java
// Animator.notifyEndListeners — the "start was seen" latch  (independently verified)
void notifyEndListeners(boolean isReversing) {
    boolean startListenersCalled = mStartListenersCalled;
    mStartListenersCalled = false;
    if (mListeners != null && startListenersCalled) {
        notifyListeners(AnimatorCaller.ON_END, isReversing);
    }
}
```

So the **list is durable and the event is not**. A listener added after `endAnimation()` sits in
`mListeners` and receives nothing; no stored outcome, no pending-delivery queue, no path that
re-walks the list for a past event. It fires only if the animator is `start()`ed again — a new run,
not a replay.

### No readable terminal status — this is what forecloses class (C)

`Animator` exposes `isRunning()`, `isStarted()`, `isPaused()` and nothing else. Both relevant ones
are documented as *in-flight* predicates, phrased "and not yet ended":

> "**isRunning()** — Returns whether this Animator is currently running (having been started and gone
> past any initial startDelay period **and not yet ended**)."

> "**isStarted()** — Returns whether this Animator has been started **and not yet ended**. … Non-reusable
> animators will always return true after they have been started, because they cannot return to a
> non-started state."

`ValueAnimator.endAnimation` sets `mRunning = false; mStarted = false;` — **byte-identical to the
initial field state** (`private boolean mRunning = false; private boolean mStarted = false;`). A late
arrival polling an ended `ValueAnimator`/`ObjectAnimator` sees exactly what a fresh, never-started one
shows. The one bit that does record termination, `mAnimationEndRequested`, is `private` with no
accessor — and even it records *that* it ended, never *how*: no flag anywhere distinguishes completed
from cancelled.

**Classification: (B).** The end event is dispatched once by walking the listener list at the instant
of transition; nothing about that transition is retained.

### AndroidX backport — no divergence

`androidx.core.animation` uses the same design: the same `ArrayList<AnimatorListener> mListeners`,
the same `mRunning`/`mStarted`/`mAnimationEndRequested` triple, the same `if (mAnimationEndRequested)
return;` latch, the same reset to `false`. It snapshots with an inline `mListeners.clone()` where the
framework factored this into `callOnList`. `getListeners()` is package-private in AndroidX and public
in the framework. **Classification: (B).**
(<https://android.googlesource.com/platform/frameworks/support/+/refs/heads/androidx-main/core/core-animation/src/main/java/androidx/core/animation/Animator.java>)

### `ViewPropertyAnimator.withEndAction` — a better contract, still (B)

> "Specifies an action to take place when the next animation ends. The action is only run if the
> animation ends normally; **if the ViewPropertyAnimator is canceled during that animation, the
> runnable will not run.**"

This is a *better* contract than `AnimatorListener`, because the outcome is encoded by the action
simply not running — no separate cancel edge to correlate. The implementation is a consumed, keyed
record: `withEndAction` stashes into `mPendingOnEndAction`; on start it moves into
`mAnimatorOnEndMap`; `onAnimationCancel` does `mAnimatorOnEndMap.remove(animation)` (this is exactly
how the documented "will not run on cancel" is enforced — evicting the entry before the subsequent
`onAnimationEnd` can find it); `onAnimationEnd` does get → run → remove. **The record is deleted on
both exit paths**, so nothing survives for a later caller. **Classification: (B).**
(<https://developer.android.com/reference/android/view/ViewPropertyAnimator#withEndAction(java.lang.Runnable)>)

**OPEN (Android).** A `@hide` static toggle (`sPostNotifyEndListenerEnabled`) can defer the end
notification by one frame via `AnimationHandler.postEndAnimationCallback`; a listener added in that
gap would be in the snapshot and would fire. That is a one-frame scheduling artifact behind a hidden
flag, not a durable record, and not part of the public contract.
**OPEN:** `android.graphics.animation.RenderNodeAnimator` (backing the non-reusable
`createCircularReveal` animator) was not read; marginal, and it does not affect the
`ValueAnimator`/`ObjectAnimator` conclusion.
## 3. Jetpack Compose — `Animatable.animateTo`

**Sources.** androidx source at `compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/Animatable.kt` and
`compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/MutatorMutex.kt`, branch `androidx-main`
(<https://github.com/androidx/androidx>, mirror of android.googlesource.com/platform/frameworks/support);
official reference <https://developer.android.com/reference/kotlin/androidx/compose/animation/core/Animatable>.

### The mechanism: there is no handle to arrive late at

`animateTo` is a `suspend fun` returning `AnimationResult<T, V>`. The awaiter **is** the caller;
there is no shared future object that a second party could subscribe to. That makes the
late-arrival question structurally inapplicable rather than answered — worth stating plainly,
because it is the single biggest difference from FLUI's design, where `TickerFuture` is a
multi-awaiter handle that outlives the call.

### How interruption is distinguished from coroutine cancellation: **it is not**

This was the specific question. `AnimationResult.endReason` carries only **two** values, and
interruption is not one of them. From `Animatable.animateTo`'s KDoc:

> "Returns an [AnimationResult] object. It contains: 1) the reason for ending the animation, and 2)
> an end state of the animation. The reason for ending the animation can be either of the following
> two: - [Finished], when the animation finishes successfully without any interruption, -
> [BoundReached] If the animation reaches the either [lowerBound] or [upperBound] in any dimension …
>
> If the animation gets interrupted by 1) another call to start an animation (i.e.
> [animateTo]/[animateDecay]), 2) [Animatable.stop], or 3) [Animatable.snapTo], the canceled
> animation will throw a [CancellationException] as the job gets canceled. As a result, all the
> subsequent work in the caller's coroutine will be canceled."
> — `Animatable.kt`, KDoc on `animateTo`

The implementation confirms the split cleanly — `endReason` is computed from a *clamping* flag only,
and every interruption arrives as an exception:

```kotlin
// Animatable.kt, private suspend fun runAnimation
return mutatorMutex.mutate {
    try {
        …
        val endReason = if (clampingNeeded) BoundReached else Finished
        endAnimation()
        AnimationResult(endState, endReason)
    } catch (e: CancellationException) {
        endAnimation()          // clean up internal state first, then rethrow
        throw e
    }
}
```

So Compose's answer to "how do you tell interruption from cancellation" is: **you don't — they are
the same mechanism.** Mutual exclusion is implemented by cancelling the previous holder's coroutine
job. `Animatable`'s class KDoc states the policy:

> "Unlike [AnimationState], [Animatable] ensures *mutual exclusiveness* on its animations. To
> achieve this, when a new animation is started via [animateTo] (or [animateDecay]), any ongoing
> animation will be canceled via a [CancellationException]." — `Animatable.kt`, class KDoc

`MutatorMutex` does the cancelling, with `job.cancel(MutationInterruptedException())`, where:

```kotlin
// MutatorMutex.kt
internal class MutationInterruptedException :
    PlatformOptimizedCancellationException("Mutation interrupted")
…
private class Mutator(val priority: MutatePriority, val job: Job) {
    fun cancel() = job.cancel(MutationInterruptedException())
}
```

There **is** a distinct exception type for interruption — but it is `internal`, so it is not part of
the public contract and a caller outside androidx cannot `catch` it by type. Treat "Compose
distinguishes them" as false at the API level; the distinction exists only as an implementation
detail and a message string.

### The readable-status half

`Animatable` exposes `public var isRunning: Boolean by mutableStateOf(false)` (`Animatable.kt`) —
an observable Compose snapshot-state flag, plus `value` and `targetValue`. A late observer can read
"not running", but `isRunning` records **liveness, not outcome**: it cannot distinguish finished from
interrupted. This is the same weakness as Qt's `Stopped` and Android's `isRunning`.

**Classification: (A) for the single caller** — the outcome is delivered as a return value or a
thrown exception, and structured concurrency guarantees exactly one recipient, so nothing can be
lost. But **not multi-awaiter**, and the readable status alone is (B)-grade for a third party.
Compose sidesteps the whole problem by refusing to hand out a shared handle.
---

## 4. Apple — Core Animation, UIKit, SwiftUI

**Sources.** developer.apple.com documentation only. Core Animation and UIKit are closed-source, so
what follows settles **what the documentation says**; where the implementation would be needed, it is
marked **OPEN**.

### `CAAnimationDelegate.animationDidStop(_:finished:)` — the outcome is on the edge

> **flag** — "A flag indicating whether the animation has completed by reaching the end of its duration."
>
> Discussion: "The animation may have ended because it has completed its active duration **or because
> it has been removed from the layer it is attached to**. `flag` is true if the animation reached the
> end of its duration **without being removed**."

So `removeAnimation(forKey:)`, `removeAllAnimations()`, or removal of the layer all produce
`animationDidStop` with `finished == false`. Unlike Android's parameterless `onAnimationEnd`, an
observer present at fire time learns the outcome from one call.
(<https://developer.apple.com/documentation/quartzcore/caanimationdelegate/animationdidstop(_:finished:)>)

### `CAAnimation.delegate` is a *single* slot — one observer maximum

> `var delegate: (any CAAnimationDelegate)? { get set }`
>
> Important: "The `delegate` object **is retained by the receiver**. This is a rare exception to the
> memory management rules…"

There is no `addDelegate`/`removeDelegate`; a second assignment replaces the first. This is a
strictly weaker observation surface than Android's list.
(<https://developer.apple.com/documentation/quartzcore/caanimation/delegate>)

**Can a delegate assigned after the animation stopped ever be called?**
**OPEN — the documentation does not state this, and Core Animation is closed-source.** Four
documented facts make replay structurally inexpressible, but this is an inference, not a guarantee:
(1) `animationDidStop` is push-only with no pull counterpart; (2) there is exactly one delegate slot,
so no queue could hold a pending delivery; (3) `CAAnimation` exposes no status property a
re-dispatch could consult (below); (4) with `isRemovedOnCompletion == true` (the default) the
animation is detached on completion and `layer.animation(forKey:)` then returns nil, so in the common
case a late arrival cannot even obtain the object.

### `CAAnimation` exposes no readable status at all — the strictest (B) here

Enumerating the DocC topic sections of the `CAAnimation` page gives: `isRemovedOnCompletion`,
`timingFunction`, `defaultValue(forKey:)`, `delegate`, `shouldArchiveValue(forKey:)`,
`usesSceneTimeBase`, `fadeInDuration`, `fadeOutDuration`, `animationEvents`, `init(SCNAnimation:)`,
`init(coder:)`, `preferredFrameRateRange`. Every one is configuration or archiving. There is no
`isRunning`, no `isFinished`, no `state`; the `CAMediaTiming` conformance is likewise input, not
observed status. The only observable proxy is absence — `layer.animation(forKey:)` returning nil —
which cannot separate finished from cancelled from never-started.

**Classification: (B), the strictest instance in this survey** (the "listener list" has capacity one
and there is nothing to poll).

### `UIViewPropertyAnimator.addCompletion(_:)` — the closest any doc comes, and it is narrower than it reads

```swift
func addCompletion(_ completion: @escaping (UIViewAnimatingPosition) -> Void)
func addCompletion() async -> UIViewAnimatingPosition
```

> "Completion blocks are executed after the animations finish normally. If you call the
> `stopAnimation(_:)` method, the completion blocks are not called if you specify true for the
> method's parameter. If you specify false … the animator executes the completion blocks normally
> after you call its `finishAnimation(at:)` method. **You may add completion blocks to an animator at
> any time, including while it is stopped.**"

That last sentence licenses adding a block **while in the `.stopped` state** — the window *after*
`stopAnimation(false)` and *before* `finishAnimation(at:)`, during which the completion blocks have
**not yet run**. A block added there is not a late arrival at all; it is an early one joining before
the edge fires.

**OPEN: the documentation is silent on adding a completion block after the animator has finished
normally and returned to `.inactive`.** No primary-source statement either way was found.

### `UIViewAnimating.state` — a finished animator is indistinguishable from a fresh one

Documented three separate times:

> "The animator remains in the active state until its animations finish, **at which point it moves
> back to the inactive state**." — `state` Discussion
>
> "The inactive state is the animator's initial state. … **Similarly, an animator that has finished
> its animations returns to the inactive state.**" — `UIViewAnimating` overview
>
> "`.inactive` — **The animations have not yet started executing.** This is the initial state of the
> animator object." — `UIViewAnimatingState.inactive`

So a late arrival cannot distinguish "never ran" from "already finished". The `.stopped` state is the
one genuinely readable terminal-ish status and it is deliberately transient, existing only between
`stopAnimation(false)` and `finishAnimation(at:)`.

### Who runs completion blocks

| Path | Completion blocks run? | Ending state |
|---|---|---|
| Animations finish naturally | Yes, with `.end` | `.inactive` |
| `stopAnimation(true)` | **No — never** | `.inactive` |
| `stopAnimation(false)` | Deferred | `.stopped` |
| …then `finishAnimation(at:)` | Yes, at the given position | `.inactive` |
| …`finishAnimation` never called | **Never run** | stays `.stopped` |

> "It is a programmer error to call [`finishAnimation(at:)`] at any time except after a call to the
> `stopAnimation(_:)` method where you pass false… Calling this method is not required, but is
> recommended in cases where you want to ensure that completion blocks or other final tasks are
> performed."

**`stopAnimation(true)` is a documented permanent-starvation path**: completion blocks never run and
the animator lands in `.inactive`, identical to never-started, with nothing to poll. Through the
`async` `addCompletion()` this is **a suspension that never resumes** — precisely FLUI's failure mode,
reached by a documented API call rather than by a race. Android has no equivalent: `cancel()` always
delivers `onAnimationEnd`.

**Classification: (B), with a narrow (C) window** — inside the `.stopped` gap the API is genuinely
hybrid (an edge is pending, a status is readable, and the docs explicitly permit registering).

(<https://developer.apple.com/documentation/uikit/uiviewpropertyanimator/addcompletion(_:)>,
<https://developer.apple.com/documentation/uikit/uiviewanimating/state>,
<https://developer.apple.com/documentation/uikit/uiviewanimating/stopanimation(_:)>,
<https://developer.apple.com/documentation/uikit/uiviewanimating/finishanimation(at:)>)

**`pausesOnCompletion` inverts the model.** > "When the value of this property is true, the animator
remains in the active state when the animation finishes, **and it does not execute its completion
handler.** … **Because the completion handler is not called when this property is true, you cannot use
the animator's completion handler to determine when the animations have finished running. Instead,
you determine when the animation has ended by observing the `isRunning` property.**" This is a
documented mode where UIKit switches the edge off and instructs you to poll — the inverse of (C).
(<https://developer.apple.com/documentation/uikit/uiviewpropertyanimator/pausesoncompletion>)

### `UIView.animate(withDuration:animations:completion:)` — degenerate (B)

> **completion** — "A block object to be executed when the animation sequence ends. This block has no
> return value and takes a single Boolean argument that indicates whether or not the animations
> actually finished before the completion handler was called."

A class method that binds the completion at the call site and returns `Void`. No handle is vended, so
"late arrival" cannot be expressed — observably the same as (B): anyone not present at the call site
gets nothing.
(<https://developer.apple.com/documentation/uikit/uiview/animate(withduration:animations:completion:)>)

### SwiftUI `withAnimation(_:completionCriteria:_:completion:)` — the delivery outlier

> "This function sets the given Animation as the animation property of the thread's current
> Transaction as well as calling `Transaction/addAnimationCompletion` with the specified completion.
> **The completion callback will always be fired exactly one time. If no animations are created by the
> changes in `body`, then the callback will be called immediately after `body`.**"

Two properties no other API here offers: **exactly-once delivery** (contrast `stopAnimation(true)`,
documented to skip completion blocks entirely), and **immediate resolution in the degenerate case** —
the only documented immediate-resolution behaviour found anywhere in this survey, and the same shape
as a `Completer` completed with an already-known value. `Animation.logicallyComplete(after:)` extends
it: *"If the animation is removed before the given duration is reached, logical completion will be
reported immediately."* — removal (SwiftUI's cancellation analogue) still fires the callback.

**Classification: the axis does not apply** — the completion is registered into the `Transaction`
synchronously during the call and no handle is vended, so a late observer cannot exist. But the
*guarantee* is class-(A)-like in spirit: a registered caller is never starved. This argues the axis
conflates two independent properties — **addressability** (can a late observer register?) and
**delivery guarantee** (can a registered observer be starved?). SwiftUI is weakest on the first and
strongest in this survey on the second; FLUI's bug is a failure of the second, which is the one worth
fixing first.
(<https://developer.apple.com/documentation/swiftui/withanimation(_:completioncriteria:_:completion:)>,
<https://developer.apple.com/documentation/swiftui/animationcompletioncriteria>,
<https://developer.apple.com/documentation/swiftui/animation/logicallycomplete(after:)>)

**OPEN (Apple).** No documented cancellation semantics for the `async` `addCompletion()` variant —
in particular whether it is cancellation-safe or what happens if the task is cancelled while
suspended. No `async` variant of `UIView.animate(withDuration:animations:completion:)` was found.
---

## 5. Qt — `QAbstractAnimation`

**Sources.** `qtbase` and `qtdeclarative` branch `6.8` (logic identical on `dev`);
<https://doc.qt.io/qt-6/qabstractanimation.html>, <https://doc.qt.io/qt-6/signalsandslots.html>.

### The signals

> "QAbstractAnimation emits [`finished()`] after the animation has stopped and has reached the end.
> This signal is emitted after stateChanged()." — qdoc in `qabstractanimation.cpp`

> "QAbstractAnimation emits [`stateChanged()`] whenever the state of the animation has changed from
> `oldState` to `newState`. This signal is emitted after the virtual updateState() function is called."

The complete signal set on the type is `finished`, `stateChanged`, `currentLoopChanged`,
`directionChanged` (`qabstractanimation.h`). The `started()`/`stopped()` signals in the same header
belong to **`QAnimationDriver`**, not to `QAbstractAnimation`.

### `Stopped` is documented as *both* the initial and the terminal state

> "**Stopped** — The animation is not running. **This is the initial state of QAbstractAnimation, and
> the state QAbstractAnimation reenters when finished.** The current time remain unchanged until
> either setCurrentTime() is called, or the animation is started by calling start()."

**Answer to the key question: a late connector reading `state()` cannot distinguish never-started /
ran-to-completion / stopped-early.** There is no fourth enum value and no `isFinished()` accessor.

### A documentation defect, verified independently at source

The doc says: *"If the animation stops by itself after reaching the end … the finished() signal is
emitted."* The `Stopped` arm of `QAbstractAnimationPrivate::setState` is wider than that — verified
by fetching `qabstractanimation.cpp` at branch `6.8` directly:

```cpp
case QAbstractAnimation::Stopped:
    // Leave running state.
    int dura = q->duration();
    if (deleteWhenStopped) q->deleteLater();
    if (dura == -1 || loopCount < 0
        || (oldDirection == QAbstractAnimation::Forward && (oldCurrentTime * (oldCurrentLoop + 1)) == (dura * loopCount))
        || (oldDirection == QAbstractAnimation::Backward && oldCurrentTime == 0)) {
            emit q->finished();
    }
```

The first two disjuncts are not covered by the documented condition: an **explicit `stop()` does emit
`finished()`** when `duration() == -1` (indefinite) or `loopCount < 0` (infinite) — animations that can
never "reach the end". The intent is visible in the QML engine's identical branch
(`qabstractanimationjob.cpp`, `QAbstractAnimationJob::finished`): `//this is an uncontrolled
animation, need to notify the group animation we are finished`.

Qt's own tests confirm both directions: `tst_QPropertyAnimation::statesAndSignals` (finite, explicit
`stop()`) asserts `finishedSpy.size() == 0`; `tst_qquickanimations::alwaysRunToEndInSequentialAnimationBug`
drives a `loops: Animation.Infinite` sequence, calls `stop()`, and asserts `onFinishedCalled == true`.
(<https://github.com/qt/qtbase/blob/6.8/src/corelib/animation/qabstractanimation.cpp>,
<https://github.com/qt/qtdeclarative/blob/6.8/tests/auto/quick/qquickanimations/tst_qquickanimations.cpp>)

### Inferring completion from `state()` + `currentTime()` is unsound, and undocumented

After a forward natural completion `state() == Stopped && currentLoopTime() == duration() &&
currentLoop() == loopCount()-1` — asserted in `statesAndSignals`. But Qt's own test supplies the
counterexample, *before the animation is ever started*:

```cpp
anim->setCurrentTime(1);
anim->setCurrentTime(100);        // duration() == 100
QCOMPARE(finishedSpy.size(), 0);
QCOMPARE(anim->state(), QAnimationGroup::Stopped);
```

`state() == Stopped && currentTime() == duration()` while the animation never ran. `setCurrentTime` is
public and writable, so any caller can forge this. Further defeaters, all documented: `start()`
rewinds ("If the animation is currently stopped or has already reached the end, calling start() will
rewind the animation and start again from the beginning"), erasing the evidence; backward animations
complete at `currentTime() == 0`, identical to never-started; zero-duration completes at `0 ==
duration()`; `duration() == -1` and infinite loops have no terminal time at all; `loopCount == 0`
makes `start()` a silent no-op.

**Is reading state documented as a completion check? No.** The class docs, the
`state`/`currentTime`/`currentLoop`/`duration`/`loopCount` property docs, and the *Animation
Framework* overview (<https://doc.qt.io/qt-6/animation-overview.html>) nowhere instruct the reader to
inspect state or time to determine completion. The readable state is **incidental**, not a sanctioned
status half — which keeps Qt at (B) rather than (C).

### No replay — settled from source, because the docs never state the negative

The signals-and-slots page documents only forward delivery ("When a signal is emitted, the slots
connected to it are usually executed immediately, just like a normal function call"); it contains no
statement about late connections. `QObject`'s `doActivate` walks the sender's connection list at call
time and keeps no record — it even snapshots a connection-id high-water mark specifically to exclude
connections made *during* an emission:

```cpp
// We need to check against the highest connection id to ensure that signals added
// during the signal emission are not emitted in this emission.
uint highestConnectionId = connections->currentConnectionId.loadRelaxed();
```
(<https://github.com/qt/qtbase/blob/6.8/src/corelib/kernel/qobject.cpp>, `doActivate`)

An emission with no connections is an early `return`. Nothing is stored.

### QML

`Animation`'s QML surface is `running`, `paused`, `alwaysRunToEnd`, `loops` — **no state enum and no
`currentTime`**, so strictly *less* readable status than C++. Signals:

- `stopped()` — "This signal is emitted when the animation ends. **The animation may have been stopped
  manually, or may have run to completion.**"
- `finished()` (5.12+) — "emitted when the animation has finished naturally. It is not emitted when
  `running` is set to `false`, **nor for animations whose `loops` property is set to
  `Animation.Infinite`**."

> 🚩 **The `Animation.Infinite` claim is contradicted by Qt's own shipping behaviour.**
> `QQuickAbstractAnimation::setLoops` normalises `Animation.Infinite` (`-2`) to `-1`;
> `QAbstractAnimationJob::setState`'s `Stopped` arm fires `finished()` when `m_loopCount < 0`; and the
> regression test cited above asserts `onFinishedCalled == true` after an explicit `stop()` on an
> `Animation.Infinite` sequence. Qt has no test asserting the documented exclusion.

`Behavior` has **no completion channel at all** — its signals are `enabledChanged`,
`targetValueChanged`, `targetPropertyChanged`, and the `Animation` docs explicitly carve out
"animations in a Behavior or Transition"; `tst_qquickanimations::finished` asserts
`finishedSpy.size() == 0` for an animation inside a `Behavior`.

### Any future-shaped API? Generic only, and it does not solve late arrival

Grepping all seven public headers of `src/corelib/animation` for `future|promise|await|coroutine|
co_await` gives **zero hits**. The generic adapter `QtFuture::connect` exists:

> "Creates and returns a QFuture which will become available when the sender emits the signal. … If
> the sender is destroyed before the signal is emitted, the resulted QFuture will be canceled.
> **Note:** The connected future will be fulfilled only once, when the signal is emitted for the
> **first time**."

Subscribe-time by construction: it resolves on the *next* emission after the `connect` call and
captures no past one. A late arrival's future simply never resolves. It converts an edge into a
one-shot future; it does not convert it into durable state. (<https://doc.qt.io/qt-6/qtfuture.html>)

### `QTimeLine` — same collision, cleaner contract

`State { NotRunning, Paused, Running }`, with `NotRunning` again documented as both initial and
terminal. `finished()`'s sole emission site is the internal tick path; `QTimeLine::stop()` only calls
`setState(NotRunning)` and emits nothing — so unlike `QAbstractAnimation`, `QTimeLine::stop()` never
emits `finished()`, with no uncontrolled-animation carve-out.

**Net: Qt = class (B).** Emission is a synchronous call over the connections present at emit time with
no record kept; `state()`/`currentTime()` are readable but `Stopped` is documented as both initial and
terminal, the inference is forgeable, and Qt never documents reading them as a completion test.

**OPEN (Qt).** The QML infinite-loop `finished()` path was not *executed*; the claim rests on two
independent source implementations plus Qt's own passing regression test — a code-and-test reading,
not an observed run.
---

## 6. WPF and WinUI — `Storyboard.Completed`

**Sources.** `dotnet/wpf` `main`
(`src/Microsoft.DotNet.Wpf/src/PresentationCore/System/Windows/Media/Animation/{Clock,ClockState,Timeline,FillBehavior}.cs`),
`microsoft/microsoft-ui-xaml` `main` (`dxaml/xcp/core/animation/{storyboard,Timeline}.cpp`), and
learn.microsoft.com.

### WPF `Timeline.Completed` — the documented contract

> "Occurs when this timeline has completely finished playing: it will no longer enter its active period."
>
> Remarks: "… **Stopping a timeline does not trigger its completed event, but skipping it to its fill
> period does.** The Object parameter of the EventHandler event handler is the timeline's Clock.
> **Although this event handler appears to be associated with a timeline, it actually registers with the
> Clock created for this timeline.**"
(<https://learn.microsoft.com/en-us/dotnet/api/system.windows.media.animation.timeline.completed>)

### Does `Stop()` raise `Completed`? No. Does `Remove()`? Yes.

`Clock.InternalStop` only sets `PendingInteractiveStop = true`; the tick that consumes it raises three
*other* events and conspicuously not `Completed`:

```csharp
if (PendingInteractiveStop) {
    PendingInteractiveStop = false;  IsInteractivelyStopped = true;
    … if (current._currentClockState != ClockState.Stopped) {
            current.ResetCachedStateToStopped();
            current.RaiseCurrentStateInvalidated();
            current.RaiseCurrentTimeInvalidated();
            current.RaiseCurrentGlobalSpeedInvalidated(); }
}
```

Documented twice: "Stopping a clock triggers the CurrentGlobalSpeedInvalidated and
CurrentStateInvalidated events, **but not the Completed event**" (`ClockController.Stop`); "Note that
stopping a storyboard does not fire the Completed event" (`Storyboard.Stop`).

`Remove()` **does**, from `Clock.ComputeEvents`:
```csharp
if (PendingInteractiveRemove) {
    RaiseRemoveRequestedForRoot();
    RaiseCompletedForRoot(true);
    PendingInteractiveRemove = false;
}
```
Natural completion does too, from `ComputeCurrentState`'s postfill branch. Those are the **only two**
`RaiseCompletedForRoot` call sites in `Clock.cs` — the enumeration is what makes "Stop never raises
Completed" a safe claim rather than an unobserved one.
**OPEN:** the `ClockController.Remove` reference page has no Remarks; that behaviour is source-only.

### A handler attached after completion never fires — and WPF keeps no durable bit

`Completed` is a handler-store event dispatched by a plain read-and-invoke (`Clock.FireEvent`).
Decisively, the completion flag is a **per-tick dedup flag cleared immediately after dispatch**:

```csharp
// Clock.RaiseAccumulatedEvents
if (CompletedEventRaised) { FireCompletedEvent(); }
…
finally  // Reset the flags to make the state consistent, even if the user has thrown
{ …  CompletedEventRaised = false;
```

`RaiseCompleted`'s own comment confirms the scope: *"We only need to raise this event once per tick."*

**A second hazard worth importing as a design lesson:** `Clock`'s constructor snapshots
`_eventHandlersStore = timeline.InternalEventHandlersStore` **by reference at construction**. If that
store was null when the Clock was built, a later `Timeline.Completed +=` creates a *new* store the
existing Clock never sees. "Attach before `Begin()`" is load-bearing, not stylistic — a
subscribe-ordering trap of the same family as FLUI's.

### `ClockState` is the readable half — durable, but lossy

> **Active** — "The clock is currently active meaning that the current time of the clock changes
> relative to its parent time."
> **Filling** — "The clock is currenty [sic] in a fill state meaning that its current time and progress
> do not change relative to the parent's time, but the clock is not stopped."
> **Stopped** — "The clock is currently stopped which means that **its current time and current progress
> property values are undefined**."

`FillBehavior` defaults to `HoldEnd` (`Timeline.cs`, `FillBehaviorProperty` metadata), so by default a
naturally-completed WPF timeline sits readably in `Filling` **indefinitely**. That is the only durable,
public, readable terminal indication found anywhere in this survey outside a promise. It is still
lossy: under `FillBehavior.Stop` natural completion lands in `Stopped`, which conflates never-begun,
explicitly stopped, and completed — and in `Stopped` the docs declare time and progress *undefined*,
so nothing can disambiguate. And no Microsoft doc instructs "check `CurrentState == Filling` to test
completion", so like Qt's `state()` this is descriptive, not sanctioned.
(<https://learn.microsoft.com/en-us/dotnet/api/system.windows.media.animation.clockstate>)

**No awaitable API.** Namespace enumeration of `System.Windows.Media.Animation` shows no `Task`,
`Task<T>`, or awaiter-shaped member on `Storyboard`, `Timeline`, `Clock`, or `ClockController`.

### WinUI 3 / UWP — the same enum, minus the prose that made it useful

`Storyboard.Completed` — "Occurs when the Storyboard object has completed playing." **No Remarks at
all.** `Stop()` — "Stops the storyboard." **No Remarks at all.** The method set is `Begin`,
`GetCurrentState`, `GetCurrentTime`, `Pause`, `Resume`, `Seek`, `SeekAlignedToLastTick`, `SkipToFill`,
`Stop` — **there is no `Remove()`**, so WinUI loses the one WPF verb that raises `Completed` on demand.

**The "WinUI's `ClockState` is smaller" premise is false.** Both WinUI and UWP have exactly three
members with identical values to WPF's: `Active` (0), `Filling` (1), `Stopped` (2). The divergence is
prose, not membership — and it runs the wrong way. WinUI's `GetCurrentState` Remarks classify
`Filling` as **running**:

> "An animation that is stopped (GetCurrentState returns ClockState.Stopped) can still be modified.
> **An animation that is running (GetCurrentState returns either ClockState.Active or
> ClockState.Filling) cannot be modified**, and you'll get an error if you try."

So WinUI strips exactly the reading that gave WPF its (C)-grade status half.
(<https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.media.animation.clockstate>)

**Does WinUI `Storyboard.Stop()` raise `Completed`? OPEN per documentation; no per source.**
`CStoryboard::StopPrivate` sets `m_clockState = Stopped` then:

```cpp
// WinRT DComp will still fire Completed events if the animations are explicitly stopped. Make sure we unattach from
// the events and clear out the map in CTimeManager so that Xaml doesn't think the WUC animations completed.
DetachDCompCompletedHandlerOnStop();
IFC_RETURN(core->GetTimeManager()->RemoveTimeline(this));
```

`CTimeline::FireCompletedEvent` is reached only from `CParallelTimeline::ComputeState`, which cannot
run again once the timeline is removed from the time manager. WinUI matches WPF behaviourally, but
only the source says so. A further late-arrival hazard: `GetCurrentState()` maps the internal
`NotStarted` onto `Active`, so between `Begin()` and `BeginTime` an observer reads `Active`; the engine
does keep a durable `m_IsCompletedEventFired` bit, **private and exposed nowhere**.

### `CompositionScopedBatch` — its status properties are about a different thing

> `Completed` — "Event triggered once all animations and effects in the CompositionScopedBatch have completed."
> `IsActive` — "Indicates whether the CompositionScopedBatch is **currently opened for objects to be aggregated**."
> `IsEnded` — "Indicates whether the CompositionScopedBatch **has been closed and can no longer accept changes**."

`IsActive`/`IsEnded` describe the batch's **aggregation membership lifecycle**, not whether the batched
animations finished — `IsEnded` flips true the instant you call `End()`, before any animation
completes. There is no readable completion property; `Completed` is the sole channel and is
edge-triggered. `CompositionAnimation` has no completion event of its own, and the documented
workaround is *"If you want to know when a single animation ends, you need to create a scoped batch
that will include just the animation you are targeting"* — inheriting the edge semantics wholesale.
`AnimationController` exposes `Progress`/`ProgressBehavior`/`PlaybackRate`/`Pause`/`Resume` — a readable
*position*, no terminal flag. No `Task`/`IAsyncAction`-returning animation API exists.
(<https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.composition.compositionscopedbatch>)

**Net: WPF = class (C), weak** — a durable public readable terminal status (`ClockState.Filling`)
survives the edge, which is strictly more than Qt offers, but it is lossy, defeated by
`FillBehavior.Stop`, and never documented as the sanctioned test. **Net: WinUI = class (B)** — same
enum, terminal reading stripped, and on the Composition side the readable properties are about batch
membership rather than completion.
---

## 7. Rust UI stacks

**Headline: not one of the seven offers an awaitable completion handle.** No futures, no oneshot
channels, no `Completer` equivalent anywhere in core. The field splits between *pollable per-frame
state* and *pre-registered edge callbacks*. **Class (A) does not appear at all.**

Every absence claim below names the version and the surface searched, so it is falsifiable. The
`cratesio` docs-search MCP tool proved unreliable for absence (it reported "no items matching
'animation'" for both iced 0.14.0 and slint 1.17.1, where a direct `all.html` fetch showed matching
items) — **every absence here is grounded in a direct `all.html` fetch or a repo tree listing.**

### bevy — `bevy_animation` 0.19.1 → **(C)**, leaning (A) on the happy path

Pollable flags on the player, no completion event, no future:
`ActiveAnimation::is_finished()` (`match self.repeat { Forever => false, Never => self.completions >= 1,
Count(n) => self.completions >= n }`), `completions() -> u32` (monotonic), `just_completed()` ("true if
the animation was completed at least once **this tick**"; `update()` opens with
`self.just_completed = false;`), `AnimationPlayer::all_finished()`, `is_playing_animation()`.

**No completion `Event` exists.** The only event machinery is `AnimationClip::add_event(time, event)` —
a *user-authored* event at a *user-chosen keyframe*, never emitted by the player on completion.

Two traps: `just_completed()` is a one-tick edge cleared at the top of the next `update()`; and
`AnimationPlayer::stop()` does `self.active_animations.remove(&animation)`, so cancellation **erases**
the record rather than resolving it — after a stop, `animation()` returns `None`. Worse,
`all_finished()` is `.values().all(is_finished)`, **vacuously `true` on an empty map**: never-played,
stopped, and all-finished are indistinguishable. That is the FLUI-relevant lesson: *a durable state
that is deleted on cancel is not durable for the cancel outcome.*
(<https://docs.rs/bevy_animation/0.19.1/bevy_animation/struct.ActiveAnimation.html>,
<https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_animation/src/lib.rs>)

### bevy_tweening 0.16.0 — **ECOSYSTEM, not core bevy** → (C), but (B) by default

`AnimCompletedEvent { anim_entity, target }` and `CycleCompletedEvent`, plus `TweenState::{Active,
Completed}` and `TweenAnim::tween_state()`. (The `TweenCompleted` / `with_completed_event` API named in
older material **no longer exists at 0.16.0**.) Both events are edge-triggered on two axes: as an
`EntityEvent` they go to observers synchronously at trigger time; as a `Message` they are
double-buffered and silently dropped —

> "[`Messages`] is implemented using a variation of a double buffer strategy. Each call to `update`
> swaps buffers and clears out the oldest one… `MessageReader`s that read after two updates are
> guaranteed to drop all messages that occurred before those updates."
> — <https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/message/messages/struct.Messages.html>

And the durable half defaults **off**: `destroy_on_completion` defaults to `true`, despawning the
`TweenAnim` on completion. The crate's own doc concedes it: *"Note that commonly the `TweenAnim` is
despawned on completion, so can't be queried anymore with this entity."*

### iced 0.14.0 — **(N)**

`iced::animation` contains exactly four items: `Animation`, `Easing`, `Float`, `Interpolable`.
`Animation<T>` wraps `lilt::Animated<T, Instant>`; every query takes `at: Instant`
(`is_animating(at)`, `remaining(at)`, `value()`, `interpolate_with(f, at)`) — it stores a transition
origin and *computes* progress, with no clock, task, or scheduler. The driver is a separate
`iced::window::frames()` subscription.

**Absence evidence:** iced 0.14.0's complete public item index (`all.html`) contains **zero** items
matching `complet|finish|done|settled`; the only `anim` entries are `Animation` and
`animation::Animation`. A late arrival calls `is_animating(Instant::now())` → `false`, but that is
recomputed arithmetic, not a stored outcome — and iced has no cancellation concept to record.

**Design intent confirmed:** the introducing PR ([iced-rs/iced#2757](https://github.com/iced-rs/iced/pull/2757))
describes the model as *"The `animation` module exposes an `Animation` type that wraps some state… When
combined together with a `window::frames` subscription, it can be effectively used to animate smoothly
at maximum framerate!"* It renamed `in_progress` → `is_animating` and proposed **no** completion
callback, Task, or Event. Poll-inside-`window::frames` is the intended model.
(<https://docs.rs/iced/0.14.0/iced/animation/struct.Animation.html>)

### egui 0.36.2 — **(N)**

The complete public animation surface is six `Context` methods, every one returning `f32`:
`animate_bool`, `animate_bool_responsive`, `animate_bool_with_easing`, `animate_bool_with_time`,
`animate_bool_with_time_and_easing`, `animate_value_with_time`. `AnimationManager` is **`pub(crate)`** —
not public API. The implementation makes the model explicit:

```rust
let animation_in_progress = 0.0 < animated_value && animated_value < 1.0;
if animation_in_progress { self.request_repaint(); }
```

`animation_in_progress` is a *local variable*: in progress → schedule another frame; finished → do
nothing. Nothing is stored, nothing is signalled. The public index has no `complet*`/`finish*` item.

The immediate-mode framing matters: the *value* is durable readable state keyed by `Id`, and in that
narrow sense it is A-like — but there is no observer and no await. "Late observer" is not a coherent
question in a toolkit where observing *is* the render loop.
(<https://docs.rs/egui/0.36.2/egui/struct.Context.html>,
<https://raw.githubusercontent.com/emilk/egui/master/crates/egui/src/animation_manager.rs>)

### xilem 0.4.0 / masonry 0.4.0 — **(N)** for both

masonry's entire animation surface is one widget callback and one request:

```rust
fn on_anim_frame(&mut self, ctx: &mut UpdateCtx<'_>, props: &mut PropertiesMut<'_>, interval: u64) {}
pub fn request_anim_frame(&mut self) { self.widget_state.request_anim = true; self.widget_state.needs_anim = true; }
```

Its doc states the contract exactly: *"This method should itself call `request_anim_frame` **unless the
animation has finished**."* Completion is the widget privately deciding to stop re-arming.

**Absence evidence:** masonry 0.4.0's public index has **zero** items matching
`anim|complet|finish|tween|transition` beyond the three `on_anim_frame` entries; xilem 0.4.0's returns
*"No items matching 'anim' found in xilem v0.4.0."* Across the repo tree only three paths contain
"anim", and of 214 files under `xilem/`, `xilem_core/`, `xilem_web/`, **zero** do.
`masonry/src/properties/animation.rs` is 716 bytes defining one thing: `struct AnimationDuration { pub
seconds: f64 }`, a styling `Property`.

No wontfix rationale found; open work is feature-shaped
([#368](https://github.com/linebender/xilem/issues/368),
[#497](https://github.com/linebender/xilem/pull/497)), and `anim.rs` carries a `// TODO - switch anim
frames to being about age / an absolute timestamp` — unfinished rather than deliberately closed.
(<https://raw.githubusercontent.com/linebender/xilem/main/masonry_core/src/core/widget.rs>)

### dioxus 0.7.10 — **(N)** core; dioxus-motion 0.3.5 — ECOSYSTEM, **(C)**

Core dioxus has no animation engine. The only animation-shaped items are DOM event-listener bindings
behind feature `html` — `onanimationstart/iteration/end`, `ontransitionend`, and their data types.
`onanimationend` is an RSX attribute forwarding the browser's CSS `animationend` event, which is
strictly edge-triggered and unbuffered. Searching the whole index for `tween|spring|ease|complet`
returns nothing.

`dioxus-motion` 0.3.5 has a pre-registered callback plus pollable flags:
`type OnComplete = Arc<Mutex<dyn FnMut() + Send + 'static>>`, `AnimationConfig::on_complete` /
`with_on_complete`, `AnimationManager::is_running()`, `AnimationSequence::{on_complete, is_complete}`.
Searching its full index for `future|await|poll|task` returns **nothing** — the callback is not a
future, must be registered before `animate_to`, and a late attach never fires. `stop()` also drives
`is_running()` to `false`, so cancelled and completed are indistinguishable from the flag. One
documented sharp edge: *"Cloning `AnimationSequence` … resets the inner `SequenceState::on_complete`
callback to `None`. Callers that clone an `AnimationSequence` must re-register `on_complete`."*
(<https://docs.rs/dioxus-motion/0.3.5/dioxus_motion/animations/core/struct.AnimationConfig.html>)

### slint 1.17.1 — **(N)**, and the absence is enforced by the compiler

From the official language reference on animations:

> "The body may contain only bindings of the form `field: value;`. **Any other statement is a compile
> error.**"
> "The only fields are `delay`, `duration`, `easing`, `iteration-count`, `direction`, and `enabled`.
> **Binding any other name is a compile error.** Every field is optional."

This is the strongest absence finding in the survey: a completion callback is not merely missing from
Slint's animation syntax, it is **unrepresentable**. The Rust side matches — slint 1.17.1's complete
public index contains **no animation type whatsoever**; the only adjacent items are
`platform::update_timers_and_animations` and `platform::duration_until_next_timer_update`, both
backend-integration hooks that tell you nothing about a specific animation. `Timer`/`TimerMode` are
wall-clock timers unconnected to animations. Timing an animation means duplicating its `duration` in a
`Timer` — and for `spring` easing the reference says that will not work: *"the animation is not
guaranteed to be done after `duration` elapses"*, with a settling clamp of up to 10× `duration`.
(<https://raw.githubusercontent.com/slint-ui/slint/master/docs/astro/src/content/docs/reference/language/animations.mdx>)

### GPUI (Zed) — **(N)**, verified from the public repository

**`/mnt/data/dev/flui/.gpui` does not exist in this checkout** — not empty, absent (`ls` returns
`No such file or directory`). Nothing below is from memory; every claim is from
`zed-industries/zed` `main` with the path cited, and I re-ran the decisive greps myself against the
fetched file.

```rust
pub struct Animation { pub duration: Duration, pub oneshot: bool, pub synced: bool,
                       pub easing: Rc<dyn Fn(f32) -> f32>, pub max_fps: Option<f32> }
pub trait AnimationExt {
    fn with_animation(self, id: impl Into<ElementId>, animation: Animation,
                      animator: impl Fn(Self, f32) -> Self + 'static) -> AnimationElement<Self>;
    fn with_animations(self, id, animations: Vec<Animation>,
                       animator: impl Fn(Self, usize, f32) -> Self + 'static) -> AnimationElement<Self>;
}
```

The entire completion mechanism, inside `AnimationElement::request_layout`, is a **local variable**:

```rust
let mut done = false;
if delta > 1.0 {
    if self.animations[animation_ix].oneshot {
        if animation_ix >= self.animations.len() - 1 { done = true; }
        else { state.start = Instant::now(); state.animation_ix += 1; }
        delta = 1.0;
    } else { delta %= 1.0; }
}
…
if !done { /* schedule another frame */ }
```

My own grep of `crates/gpui/src/elements/animation.rs` confirms `done` appears at exactly four lines
(`let mut done = false`, `done = true`, and two `if !done` guards) and never escapes the function;
the only `pub fn`s on `Animation` are constructors and config (`new`, `repeat`, `repeat_synced`,
`with_easing`, `with_max_fps`), with no `Task<…>` return and no completion API. `request_animation_frame`
is itself a one-shot re-render: `let entity = self.current_view(); self.on_next_frame(move |_, cx| cx.notify(entity));`.

**The `SpringPlayback` trap.** `crates/gpui/src/spring.rs` defines
`pub enum SpringPlayback { Running, Paused, Stopped, Completed, Cancelled }`. These look like status
reports. **They are not** — `SpringPlayback` is a *caller-supplied input* set via the builder
`pub fn playback(mut self, playback: SpringPlayback) -> Self`, and the element does
`state.playback = self.playback;`, adopting what the caller told it. `Completed` and `Cancelled` are
commands you issue, not outcomes you read. The genuine settle test,
`SpringConfig::is_settled(state, target, epsilon) -> bool`, is a pure function consumed internally and
never surfaced.
(<https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/elements/animation.rs>,
<https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/spring.rs>)

### Rust summary

| Project | Scope | Completion mechanism | Late arrival gets | Class |
|---|---|---|---|---|
| bevy `bevy_animation` 0.19.1 | core | `is_finished()`, `completions()`; `just_completed()` 1-tick edge | durable flag — **unless** `stop()` erased it | **C** |
| bevy_tweening 0.16.0 | ecosystem | `AnimCompletedEvent` (EntityEvent + Message); `tween_state()` | nothing by default (component despawned) | **C**, B-by-default |
| iced 0.14.0 | core | none; `is_animating(at)` computed | recomputed predicate, no outcome record | **N** |
| egui 0.36.2 | core | none; `animate_*() -> f32` per frame | the value, by re-sampling | **N** |
| masonry 0.4.0 | core | `on_anim_frame` + `request_anim_frame` only | nothing | **N** |
| xilem 0.4.0 | core | **none at all** (0 anim items in public API) | nothing | **N** |
| dioxus 0.7.10 | core | none; forwards CSS `onanimationend` | nothing (DOM edge) | **N** (delegated B) |
| dioxus-motion 0.3.5 | ecosystem | `with_on_complete` + `is_running()`/`is_complete()` | pollable flag; callback must pre-exist | **C** |
| slint 1.17.1 | core | **none — binding one is a compile error** | nothing | **N** |
| GPUI (zed `main`) | core | none; `done` is a local that stops frame requests | nothing | **N** |

**Six of seven core projects are (N).** Only bevy core ships a completion signal, and it is a pollable
flag rather than an awaitable. Both (C) entries are ecosystem crates, both callback-based.

**OPEN (Rust).** No closed-as-wontfix issue or design doc arguing *against* an await-animation API was
located in bevy, iced, egui, xilem, slint, or zed. iced #2757 is the closest positive evidence of
intent but is silent on completion rather than rejecting it. Title-only issue search is a weak
instrument; absence of a rejection is not evidence of one. Also: bevy behavioural source quotes are
from `main` (`0.20.0-dev`) with every cited API name confirmed present on released 0.19.1 via the
docs.rs method index — signatures verified, bodies not diffed against the 0.19.1 tarball.
---

## The verdict

### Full classification

| Toolkit | API | Class |
|---|---|---|
| Web Animations | `Animation.finished` + `playState` + `finish`/`cancel` events | **C** (the awaited half is **A** for the *finish* outcome; the *cancel* outcome is not carried) |
| Flutter / Dart | `TickerFuture` on `Completer` | **A** |
| Jetpack Compose | `Animatable.animateTo` → `AnimationResult` / `CancellationException` | **A, single-consumer** (no handle is vended) |
| Android | `Animator.AnimatorListener` | **B** |
| Android | `androidx.core.animation` backport | **B** |
| Android | `ViewPropertyAnimator.withEndAction` | **B** |
| Apple | `CAAnimationDelegate.animationDidStop` | **B** (strictest — one delegate slot, zero status properties) |
| Apple | `UIViewPropertyAnimator.addCompletion` | **B**, narrow **C** window in `.stopped` |
| Apple | `UIView.animate(…completion:)` | **B** (degenerate — no handle) |
| Apple | SwiftUI `withAnimation(…completion:)` | axis N/A; **exactly-once delivery guaranteed** |
| Qt | `QAbstractAnimation::finished()` / `stateChanged()` | **B** |
| Qt | QML `Animation`; QML `Behavior` | **B**; **no channel at all** |
| WPF | `Timeline.Completed` + `Clock.CurrentState` | **C**, weak |
| WinUI | `Storyboard.Completed`; `CompositionScopedBatch.Completed` | **B**; **B** |
| Rust | bevy core; bevy_tweening, dioxus-motion (ecosystem) | **C**; **C** (B-by-default) |
| Rust | iced, egui, xilem/masonry, dioxus core, slint, **GPUI** | **N** — no such API |

### Which class is the majority — and the question that actually decides the fix

**By raw count, (B) is the majority**: every retained-mode native stack that ships a completion
callback — Android, Core Animation, UIKit, Qt, WinUI — is (B).

**That count is the wrong population for this decision, and the right one gives the opposite answer.**
(B) is exclusively the class of *callback and signal* APIs, where the caller registers a listener
*before* starting the animation and a late arrival is therefore out-of-contract by construction. FLUI's
`TickerFuture` is not that shape: it is an **awaitable handle that outlives the call and is handed to
an arbitrary number of consumers at arbitrary times**. Restricted to toolkits that ship an awaitable of
that shape, the tally is unanimous:

| Awaitable handle | Class |
|---|---|
| Web Animations `Animation.finished` (a `Promise`) | **A** for the finish outcome, inside a **C** design |
| Flutter / Dart `TickerFuture` (a `Completer`) | **A** |
| Jetpack Compose `animateTo` (a `suspend fun`) | **A**, single-consumer |

**Not one awaitable animation API in this survey is (B). Zero out of three.** And Qt — the only stack
that ships a generic signal→future adapter — makes the point from the other side: `QtFuture::connect`
is documented as subscribe-time (*"fulfilled only once, when the signal is emitted for the first
time"*), so wrapping a (B) signal in a future does **not** produce a working awaitable; it produces
exactly FLUI's bug with a `QFuture` on the front. That is the closest thing to a controlled experiment
the survey contains.

### Did anyone deliberately choose (B) and document why?

**No — and the stronger finding is that nobody documented the question at all.**

- **Android**: `addListener`'s javadoc says only "through the life of an animation". The late-arrival
  rule is settled purely from AOSP source. **No documented choice.**
- **Apple**: `addCompletion(_:)` is the only doc in the survey that comes close — *"You may add
  completion blocks to an animator at any time, including while it is stopped"* — and it addresses the
  transient `.stopped` window, **not** the finished case, which remains **OPEN**. **No documented choice.**
- **Qt**: the signals-and-slots page documents only forward delivery and never states the no-replay
  rule; it must be read out of `doActivate`. **No documented choice.**
- **WPF / WinUI**: `Timeline.Completed` and `Storyboard.Completed` say nothing about late handlers.
  WPF's per-tick `CompletedEventRaised` flag, cleared in a `finally`, is an implementation detail with
  no documented contract. **No documented choice.**
- **Rust**: no closed-as-wontfix or design doc rejecting an await-animation API was found in bevy,
  iced, egui, xilem, slint, or zed (**OPEN** — title-only search is weak). iced's own introducing PR
  (#2757) is the only positive statement of intent, and it is silent on completion rather than
  rejecting it.

So (B) is never a *recorded decision* here. It is what you get from a listener-list or signal
implementation when nobody asked the question — which is also how FLUI arrived at it.

**The strongest available argument for the fix, stated plainly: every toolkit that ships an awaitable
"tell me when this animation ends" handle stores the outcome durably and resolves a late awaiter from
that store. None makes the awaiter catch an edge. No toolkit anywhere documents choosing otherwise.**

### Three transferable findings the survey produced that the brief did not ask for

1. **Nearly every (B) toolkit makes the same independent mistake: the terminal state is spelled
   identically to the initial state.** Android's `ValueAnimator.endAnimation` sets `mRunning = false;
   mStarted = false;`, byte-identical to a fresh object. UIKit documents that a finished animator
   "returns to the inactive state" while `.inactive` means "the animations have not yet started
   executing". Qt's `Stopped` is documented as both initial and terminal, as is `QTimeLine`'s
   `NotRunning`. WPF lands in `Stopped` under `FillBehavior.Stop`. bevy's `all_finished()` is vacuously
   `true` on an empty map. **This single decision is what forecloses class (C) in each case** — no
   amount of readable status helps a late observer when the terminal value equals the initial one.
   FLUI's `Pending`/`Complete`/`Canceled` tri-state does **not** have this defect and must not be
   collapsed into an `is_running` bool in the course of fixing the wakeup.
2. **A durable state that is deleted on cancel is not durable.** bevy's `stop()` removes the animation
   from the map; bevy_tweening despawns the component by default; WAAPI's `cancel()` replaces the
   rejected promise with a fresh pending one. All three make the *completion* outcome readable and the
   *cancellation* outcome unreadable. FLUI must keep `Canceled` as a retained value, not a removal.
3. **Two shipped APIs document permanent awaiter starvation as normal behaviour** — UIKit's
   `stopAnimation(true)` (completion blocks never run, animator returns to `.inactive`) and
   `pausesOnCompletion == true` (edge switched off, docs tell you to poll `isRunning` instead). So
   "an awaiter is never starved" is not a universal law. But in both cases the starvation is
   *documented* and an alternative is *named*. FLUI's is neither — which is what makes it a defect
   rather than a contract.
## Appendix — the fix shape the evidence supports, and its primary source

Nothing in this survey argues for abandoning `event_listener`; it argues for **inverting the order**
so the durable state FLUI already keeps is the source of truth and the event is only a hint to
re-read it. The crate's own crate-level documentation ships exactly that loop, and the comment on
the second check is the whole point:

```rust
// docs.rs/event-listener/5.4.2/event_listener/ — crate-level example
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
<https://docs.rs/event-listener/5.4.2/event_listener/>

`TickerFuture::poll` today implements the first check and the `listen()` and omits the second check.
That is the entire defect, and the crate documents the omission as the hazard the second check
exists to close.

The survey's independent contribution is the *shape of the state* that second check should read:

- The finished/cancelled outcome must be **durable and monotone** — WAAPI keeps the resolved promise
  for as long as the animation stays finished (§4.5.11–4.5.12); Dart stores the result in the same
  field that used to hold the listener list (`_Future._resultOrListeners`). FLUI already satisfies
  this with `Mutex<TickerFutureState>`; the fix does not need new state, only a second read.
- The **cancel path must tolerate having no awaiter**. WAAPI marks its rejection
  `[[PromiseIsHandled]] = true` for precisely this reason (§4.5.14). The Rust analogue is that
  `set_canceled` with zero registered listeners must be an ordinary no-op, not a warn or a leak.
- **Liveness flags are not outcomes.** Every (B)-class toolkit in this survey has a readable
  running/stopped flag, and in every one of them that flag collapses "never started", "ran to
  completion" and "was stopped early" into one value. FLUI's `is_complete`/`is_canceled`/`is_pending`
  trio does not have this weakness and should not be replaced by a single `is_running`.

---

*Retrieved 2026-09-15. Web Animations quoted from the CSSWG editor’s draft and the W3C
Working Draft of 5 June 2023 (identical text, different section numbering). Qt read at branch 6.8;
WPF and WinUI at `main`; androidx at `androidx-main`; AOSP at `main`; Zed at `main`. Rust crate
versions are named per project in §7.*
