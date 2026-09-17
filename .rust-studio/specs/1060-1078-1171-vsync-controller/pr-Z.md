## Summary

A zero-duration run (`AnimationController::new(Duration::ZERO, …)`, an explicit `Some(Duration::ZERO)`, or a zero-distance start) now settles **synchronously at the call**: value snaps to the target, the run's direction settles the status, value listeners fire once (only if the value moved), status listeners fire once, the displaced run's `TickerFuture` is canceled after the new status is observable, and the returned `TickerFuture` is already complete — one gate in `forward_from`/`reverse_from`/`drive_to`, the shape of Flutter's `_animateToInternal` `simulationDuration == Duration.zero` branch. Before, such a run stayed pending until the controller's first tick — a frame later on a scheduler-driven controller, and until some pump called `tick_at` on a `Vsync`-driven one.

Three further contracts were aligned or fixed on the way, each recorded in `docs/ARCHITECTURE.md` `## Mapping decisions` and in the crate CHANGELOG (they are invisible to `cargo public-api`/`semver-checks`, which report zero diff):

- **Direction is chosen by the method.** `animate_to`/`animate_to_curved` run `Forward` and end `Completed`, `animate_back`/`animate_back_curved` run `Reverse` and end `Dismissed`, regardless of whether the target is above or below the current value — Flutter's documented contract (`animateTo`/`animateBack`). FLUI derived direction from travel, an unrecorded divergence that made the two methods differ only in default duration and threw away the one bit the caller has (the substance of flutter/flutter#158233). Every run end now reports `direction.settled_status()` with no bound check (Flutter's `_tick` rule); `stop()`/`set_value` keep the bounds-first rule. Production consumers were read: the unbounded scroll controller's `animate_to` toward smaller pixels now reports `Forward`/`Completed` (its listener matches `Completed | Dismissed` alike), and the cupertino button — which chained its release fade on `status == Completed` and would have re-entered once per tap — now chains on the press fade's own `TickerFuture` (`when_complete_or_cancel`, the oracle's `ticker.then`).
- **`Vsync` polls for an installed run, not a running status.** `WalkProbe.live_running` and `tick_at`'s guard read `active_run.is_some()` (one controller lock per registry step instead of two — the perf follow-up from #1179). `dispose()` leaves `status` untouched (Flutter parity; a `hero_flight` proxy reads it on replay), yet a disposed-but-not-unregistered controller neither ticks nor holds the frame loop open. The same predicate fixes a pre-existing defect: `set_value` mid-run left `status().is_running()` true with no run installed, so the next `tick_all` overwrote the value from the stale run (`dismissible` worked around it by unregistering during drags).
- **`from` notifies.** A non-settling `forward_from(Some(x))`/`reverse_from(Some(x))` now notifies value listeners iff `from` moved the value (Flutter's `value=` setter); a zero-distance settle no longer fires a spurious value notification.

Also closed: the `settle_at_target` ticker-stop guard read `can_tick()` (Active only) where `restart_ticker` already knew a Muted ticker keeps its run — aligned to `is_running()` (unreachable through the public API today, said so at the guard); the repeat-exhaustion branch now sets `direction` to the final retired leg before settling (a bounce over an interior range with an even multi-cycle frame reported the wrong status), with a test.

## Tests

flui-animation: zero-duration `forward`/`reverse`/`animate_to`/`animate_back` settle at the call (future complete, value, status, listener counts, no frame needed, `run_generation` unchanged); method-chosen direction and run-end status (`animate_to` below the value runs `Forward`, `animate_to(lower_bound)` ends `Completed`); displaced-run cancellation after the new status; zero-distance settle notifies no value listener; `from` notifies once; `dispose` mid-run leaves status, `tick_at` no-op; `set_value` mid-run holds against the next `tick_all`; bounce exhaustion over an interior range; flutter/flutter#1913 pin (`forward()` then `reverse()` at 0 with no tick delivers `Forward` then `Dismissed`). Mutation probes by the reviewer (M1–M8) each redden a named test.

flui-widgets: a `Duration::ZERO` implicit retarget lays out the new target on the same pump (and pins the one redundant `AnimatedBuilder` rebuild the external inbox defers — #1180); a zero-`transition_duration` route through a real `TransitionRoute` settles its pop synchronously in `handle_pop` while push still needs one pump for `Pushing → Idle`.

flui-cupertino: the release fade starts exactly once per tap.

## Review

Plan reviewed before code by harsh-critic (two rounds), concurrency-specialist, api-design-lead and an outside glm-5.3 lens; the diff by rust-reviewer (mutation probes in a scratch worktree), an outside glm-5.3 agentic lens (second vendor after deepseek on #1179), and api-design-lead's API gate (`cargo public-api` / `semver-checks`: 0 diff). The outside lens caught the cupertino button's `Completed`-chained release; the reviewer caught the widgets test asserting a quiescence it had not measured and the `set_value`-mid-run overwrite.

Market survey behind the zero-duration decision (Web Animations, Android `ValueAnimator`, Compose `snap()`, SwiftUI): `.rust-studio/research/animation-repeat-zero-duration-registry-market-survey.md` (untracked planning artefact).

Closes #1171. Refs #1180.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
