# Market survey: repeating animations, zero-duration runs, per-frame animation registries

Date: 2026-09-16. For the flui-animation batch (#1060 Vsync lookup, #1078 repeat sampling,
#1171 zero-duration completion). Primary sources read directly (fetched files kept in the session
scratchpad); secondary/web findings from the kimi-k3 research lens are appended in the last
section and marked as such.

QUESTION 1: Is a repeating animation's value a pure function of elapsed time, and where does a
finite repeat land at its end?

ANSWER: Every standard and mainstream framework checked except Bevy samples the value from
absolute elapsed time (`overall progress = time / period`, iteration = floor, phase = fraction,
alternate direction = iteration parity). Three of them — the W3C Web Animations spec, Android
`ValueAnimator`, Jetpack Compose — land a finite repeat on the END of its last iteration; Flutter
3.44.0 is the outlier (`% 1.0` wraps a 1-count restart repeat back to `min`). Compose and GPUI do
the modulo in integer nanoseconds before any float conversion. Flutter's `_initialT` (start from
the current value's phase) has an exact analogue in Compose's `StartOffsetType.FastForward` and the
Web spec's `iteration start`; and Flutter's `_exitTimeInSeconds = count*period − initialT` equals
Compose's `getDurationNanos = iterations*duration − initialOffset` — "count boundaries from the
phase origin", not "count full periods from the call".

| Source | Value model | Dropped frames | Finite end | Alternate rule | Zero period |
|---|---|---|---|---|---|
| W3C Web Animations L1 §4.8.3.2/§4.8.3.3/§4.8.4/§4.9.1 | `overall progress = active time / iteration duration (+ iteration start)`; `simple iteration progress = overall % 1.0`; `current iteration = floor(overall)` | pure sampling — no state | "when an animation's active interval ends precisely at the end of an iteration, it fills by holding the endpoint of the final iteration rather than the start of the next" (simple iteration progress := 1.0; current iteration := floor − 1) | `alternate`: `d = current iteration`, `d % 2 == 0` → forwards else reverse; infinity → forwards | iteration duration zero: overall progress := iteration count (after phase) → finishes at once |
| Android `ValueAnimator.java` (`animateBasedOnTime`, `getCurrentIteration`, `getCurrentIterationFraction`, `shouldPlayBackward`, `clampFraction`) | `fraction = (currentTime − mStartTime) / scaledDuration`; `iteration = floor(fraction)`; per-iteration fraction = `fraction − iteration` | pure sampling; `newIteration` fires ON_REPEAT once even if several iterations elapsed | `if (fraction == iteration && fraction > 0) iteration--` → the current iteration's fraction is 1.0 at the exact end (end of the last iteration) | `REVERSE`: `iteration % 2 != 0` → backward | `scaledDuration == 0` → `fraction = 1f`, `done = true` on the first frame ("ignore the repeat count and skip to the end") |
| Compose `VectorizedRepeatableSpec` (`VectorizedAnimationSpec.kt`) | `repetitionPlayTimeNanos(playTimeNanos)`: integer nanos; `repeatsCount = min(post/duration, iterations − 1)`; `Reverse` and odd count → `(repeatsCount+1)*duration − post` | pure sampling | `min(…, iterations − 1)` clamps the last iteration so the exact end is `duration` into the last leg (its end) | `repeatsCount % 2` | `iterations < 1` throws; duration 0 not special-cased at this layer |
| Compose `StartOffset(FastForward)` | `initialOffsetNanos` added to play time → phase offset | — | `getDurationNanos = iterations*duration − initialOffsetNanos` | — | — |
| Flutter 3.44.0 `_RepeatingSimulation` (`animation_controller.dart:1005-1061`) | `t = ((time + _initialT)/period) % 1.0`; `isPlayingReverse = (total ~/ period).isOdd` | pure sampling | `isDone` at `count*period − _initialT`; value there is `% 1.0 = 0` → a restart repeat lands on `min` (test `animation_controller_test.dart:1291-1311` expects 0 at 100 ms); bounce lands on the NEXT leg's start and `_tick` reports the NEXT leg's direction as status | odd → reverse | `assert(_periodInSeconds > 0.0)` |
| GPUI `crates/gpui/src/elements/animation.rs` | `delta = elapsed / duration; if !oneshot { delta %= 1.0 }`; `repeat_synced` phase-locks to an app-wide epoch and reduces `elapsed.as_nanos() % duration.as_nanos()` BEFORE the f32 conversion ("loses sub-second precision at scale") | pure sampling | oneshot only clamps to 1.0 | no alternate mode | — |
| Bevy `bevy_animation` `ActiveAnimation::update` | delta accumulation: `seek_time += delta*speed; if seek_time >= clip_duration { completions += 1 } … seek_time %= clip_duration` | a frame spanning several clips counts ONE completion (comment: "assumes delta is never lower than -clip_duration") — the incremental camp's known undercount | `Count(n)` finishes at `completions >= n` | `PingPong` elsewhere | `clip_duration == 0` → `seek_time = 0`, one completion per update |

Consequences for FLUI:
- Adopt pure sampling (Web/Android/Compose/Flutter/GPUI agree); do the iteration/phase arithmetic
  in integer nanoseconds (Compose, GPUI) — the exact `total >= count*period` predicate cannot be
  trusted in f64 (`0.3 >= 3*0.1` is false).
- Land a finite repeat on the END of the last iteration (Web spec wording, Android, Compose);
  record the divergence from Flutter's `% 1.0` wrap with Flutter's own test as the replaced oracle.
- Start from the current value's phase (Flutter `_initialT`, Compose `FastForward`, Web
  `iteration start`) and count boundaries from that origin (Flutter, Compose agree).
- Zero period: no standard supports an infinite zero-period repeat; Android and Bevy retire one
  iteration per delivered frame; the Web spec and Android finish a FINITE zero-duration repeat at
  once. FLUI: finite → synchronous completion at the call; infinite → one cycle per tick.

QUESTION 2: What does a zero-duration run do — complete synchronously, on the next frame, or
never?

ANSWER: Flutter is synchronous (`_animateToInternal`: sets value, notifies, returns
`TickerFuture.complete()` with no tick, `animation_controller.dart:674-684`). The Web spec's zero
iteration duration puts the effect in the after phase immediately (finishes as soon as the timeline
is sampled; `finished` resolves on the next timeline update, not synchronously in the call).
Android's zero-duration animator ends on its FIRST frame (`animateBasedOnTime`: `fraction = 1f;
done = true`), but `start()` applies the initial value synchronously via `setCurrentPlayTime(0)`.
Compose's `snap()` spec is finished at play time 0 (`isFinishedFromNanos(0)` is `0 >= 0`) but
`Animatable.animateTo` still suspends for one frame; `snapTo` is Compose's synchronous path.
Nobody leaves a zero-duration run pending until an unrelated tick; FLUI today does for the
`without_ticker` + `Vsync`-driven population (the run resolves only when a pump calls `tick_at`;
a scheduler-driven controller's own `Ticker` completes it on the next frame), which is the class
#1161 closed for lost wakeups.
FLUI adopts Flutter's synchronous shape: it is the only one of the four with synchronous status
listeners and a returned completion handle, and the zero-DISTANCE settle already behaves that way.

QUESTION 3: How do frameworks avoid a per-frame tax for animations that are registered but idle?

ANSWER (Flutter, Android, Web verified; others from the lens): Flutter's `SchedulerBinding` only
holds ACTIVE tickers' transient callbacks (`scheduleFrameCallback` per tick; a stopped ticker is
not in the map). Android's `AnimationHandler` keeps only started animators in its frame-callback
list. Browsers keep per-timeline sets of current/in-effect animations. In all three the "registry"
IS the active set — there is no resident population of stopped animations to scan. FLUI's `Vsync`
is different by design (a registry of controllers per presentation, stopped ones included, so a
listener can start a later one and have it tick this frame — ADR-0020), so its per-pump cost is
Θ(N) in resident controllers whatever the lookup structure; #1060's O(N log N) removes the
quadratic term, and an active set (start/stop notifications from the controller) is the correctly
deferred next step.

Postscript (flutter/flutter#190372, open P3 proposal, 2026-08, read with comments): Flutter's own
VRR proposal asks for exactly this — "declarative (idempotent) animations … `f(t)` … inherently
immune to time moving backwards or out of order", because the engine's timestamp clamp (engine#55310)
now delivers several frames with the SAME timestamp and breaks delta-based simulations. #1182's repeat
model and FLUI's existing time-based runs are that shape; simulations sample `x(t)` and are the
"stateful / accumulation" class the proposal wants guarded.

QUESTION 4: Frame timestamps that go backwards?

ANSWER: flutter/flutter#106277 (open, engine): Android Choreographer at ≥120 Hz delivers frame
times older than the previous frame; Flutter's `AnimationController._tick` asserted
`elapsedInSeconds >= 0.0` in debug; flutter/engine#55310 now clamps ("Reported frame time is older
than the last one; clamping"). Flutter's own test harness (`scheduler_tester.dart` `tick(d)` =
`handleBeginFrame(d)`, an ABSOLUTE timestamp) deliberately rewinds time in
`animation_controller_test.dart:1348-1351` (100 ms → 60 ms → 0.6) — pure sampling makes a rewind
well-defined. FLUI's production clocks are `Instant`-based (`Ticker::start_time`,
`UiRealm::now_secs`) and so monotonic; `Vsync::tick_all(now_secs)` documents the non-decreasing
precondition and `tick_at` already clamps the run-relative time at 0.

VERSIONS: Web Animations Level 1 W3C TR as fetched 2026-09-16; `aosp-mirror/platform_frameworks_base`
`main` `core/java/android/animation/ValueAnimator.java` (1806 lines, fetched 2026-09-16);
`androidx/androidx` `androidx-main` `compose/animation/animation-core/.../VectorizedAnimationSpec.kt`
and `Animation.kt` (fetched 2026-09-16); `zed-industries/zed` `main` `crates/gpui/src/elements/animation.rs`
(1028 lines); `bevyengine/bevy` `main` `crates/bevy_animation/src/lib.rs` (1897 lines); Flutter
`.flutter` at tag 3.44.0 (verified with `git describe --tags`).

SOURCES:
- https://www.w3.org/TR/web-animations-1/ §4.8.3.2 "Calculating the overall progress", §4.8.3.3
  "Calculating the simple iteration progress", §4.8.4 "Calculating the current iteration", §4.9.1
  "Calculating the directed progress".
- https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/main/core/java/android/animation/ValueAnimator.java
  `getCurrentIteration` (~L779), `getCurrentIterationFraction` (~L796), `clampFraction` (~L810),
  `shouldPlayBackward` (~L824), `animateBasedOnTime` (~L1394).
- https://raw.githubusercontent.com/androidx/androidx/androidx-main/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/VectorizedAnimationSpec.kt
  `VectorizedRepeatableSpec` (~L702-790); `Animation.kt` `isFinishedFromNanos` (~L81),
  `TargetBasedAnimation.getValueFromNanos` (~L262); `AnimationSpec.kt` `StartOffsetType` (~L243-300), `SnapSpec` (~L463).
- https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/elements/animation.rs ~L405-445.
- https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_animation/src/lib.rs `ActiveAnimation::update` (~L576-608).
- `.flutter/packages/flutter/lib/src/animation/animation_controller.dart` 3.44.0: `animateTo`/`animateBack`
  docs and `_direction` assignment (L566-636), `_animateToInternal` (L640-690), `repeat` (L717-745),
  `_startSimulation` (L861-872), `_RepeatingSimulation` (L1005-1061);
  `.flutter/packages/flutter/test/animation/animation_controller_test.dart` L873-1000, L1269-1355;
  `.flutter/packages/flutter/test/scheduler/scheduler_tester.dart` L11-15.
- https://github.com/flutter/flutter/issues/67507 (repeat initial value; body + 6 comments),
  https://github.com/flutter/flutter/issues/158233 (status on `value=`; the `animateTo(duration: zero)`
  workaround in comments), https://github.com/flutter/flutter/issues/106277 (backwards frame times;
  engine clamp flutter/engine#55310 named in comments), https://github.com/flutter/flutter/issues/1913,
  https://github.com/flutter/flutter/issues/1911 (status coalescing / re-entrant `value=`).

## Appendix — web research lens (kimi-k3, secondary; each line is a claim, verified where marked)

Additions the lens found that the primary read above did not, with verification status:
- Compose `InfiniteRepeatableSpec` REJECTS a zero-duration inner spec (`IllegalArgumentException`
  "Animation to be infinitely repeated cannot have a 0-duration") — [unverified at source; from the
  developer.android.com reference]. Compose's `RepeatableSpec` docs recommend an ODD iteration count
  for `RepeatMode.Reverse` because `TargetBasedAnimation` returns `targetValue` once finished (a
  visible jump for an even count) — consistent with `Animation.kt:262` read above [verified in part].
- Android `AnimationHandler`: `ArrayList<AnimationFrameCallback>` of STARTED animators; the
  Choreographer callback re-posts only while the list is non-empty [unverified at source].
- Compose `BroadcastFrameClock`: a lock-guarded awaiter list; `onNewAwaiters` fires on 0 → 1, which
  is when a frame is requested [unverified at source].
- Slint: no registry — a thread-local `AnimationDriver` with `active_animations: Cell<bool>` re-set
  by any unfinished animated binding each frame; `update_animations` debug-asserts a monotonic tick;
  `iteration-count` accepts fractional values (2.5) [unverified at source].
- Masonry: `update_anim` pass prunes subtrees without the anim flag; widgets re-request each frame
  [matches the #1060 comment's reading of `passes/anim.rs`].
- Bevy: `advance_animations` iterates every `AnimationPlayer` every frame (no idle pruning)
  [consistent with the ECS shape; not re-verified].
- GPUI PR zed#62332 added `repeat_synced` because independently started spinners drifted out of
  phase — evidence that absolute-time anchoring matters in practice [the code is verified above].
- bevy_tweening: `RepeatStrategy::MirroredRepeat` counts a ping-pong as TWO cycles; Web/CSS count
  each direction as one iteration — same accounting as FLUI's `count` [docs.rs; unverified].
- Framer Motion: `repeatType: "mirror"` swaps origin/target rather than reversing playback — a third
  repeat semantics no other framework here has [docs; unverified].
- Web Animations 2015 WD §3.1.1 states the timing model is STATELESS by design: "the rate at which
  the model is sampled will not affect its progress", seeking is constant-time [spec text; unverified
  quote].
- Flutter: PR flutter#73129 (curves outside [0,1] for looping) was rejected on the Tween contract;
  `_RepeatingSimulation` originally (v0.9.1) took no initial value — the phase-continuity start was
  added later [unverified].
- Chromium Blink `animation_timeline.cc`: no backwards-time clamp found; only a `current_time ==
  last_current_time_` skip [lens read; not re-verified].
- CSS: `animation-duration: 0s` still fires `animationstart`/`animationend` [MDN; unverified].
- SwiftUI: `.linear(duration: 0)` is a real instant animation, distinct from `.animation(nil)`
  (Apple forums thread 728132) [unverified].

Decision impact: the zero-period policy moved from "one cycle per tick" to Android's "skip to the
end" (settle at the call for any count) — Compose rejects, Flutter asserts, nobody ticks an infinite
zero-period repeat once per frame; a frame loop held open forever doing nothing is the worst of the
options and FLUI's house rule is repair-not-reject.
