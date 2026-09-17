# Flutter-tracker triage ledger — flui-animation Vsync/controller batch (2026-09-16)

Each issue below was read in full (body AND every comment; the dumps sit beside this file) and
triaged against FLUI's `AnimationController`/`Vsync`. "Solved in" names the FLUI PR that folded it.

| flutter/flutter | state | what it establishes | FLUI disposition |
|---|---|---|---|
| #67507 (6 comments) | open, docs | `repeat()` at 3.44 starts from the CURRENT value's phase (`_RepeatingSimulation._initialT`); the old "min + value" formula the report describes is gone; maintainers never explained the why | **Solved in #1182**: phase-continuity adopted; `repeat`/`repeat_with` docs answer the reporter's confusion ("to start at `min`, `set_value(min)` first") |
| #158233 (7 comments) | open, proposal | `value=` always reports `forward`; the community workaround is `animateTo(x, duration: zero)` / `animateBack`, which works BECAUSE direction is chosen by the method and the zero path settles synchronously | **Solved in #1181**: zero-duration runs settle at the call; `animate_to`/`animate_back` direction by method — the workaround is now a first-class contract in FLUI |
| #106277 (94 comments) | open, engine | 120 Hz Android delivers frame timestamps that go BACKWARDS; Flutter asserted `elapsedInSeconds >= 0`, engine#55310 clamps ("time traveling frame times") | **Addressed in #1179/#1182**: FLUI's clocks are `Instant`-based (monotonic); `Vsync::tick_all` documents the non-decreasing precondition and that a backwards step re-samples the pure time function; repeat sampling is f(t), so a rewind is well-defined (the ported rewind test) |
| #190372 (2 comments) | open, P3 proposal (2026-08) | asks Flutter for "declarative (idempotent) f(t) animations immune to time moving backwards" and safeguards for accumulation animations under VRR | **Already the shape #1182 shipped** for repeats (pure sampling in integer ns); time-based runs were already f(t); simulations sample `x(t)`; recorded in the market survey |
| #1913 (12 comments) | closed (2016) | `forward()` then `reverse()` at 0 before a tick lost the intermediate status; fix was to deliver intermediate statuses | **Pinned in #1181**: `forward_then_reverse_with_no_tick_delivers_the_intermediate_status` |
| #1911 (5 comments) | closed | `value = 0.0` inside a `dismissed` listener recursed to stack overflow | **Already safe**: `take_status_change` dedups same-status writes; covered by `status_listener_is_not_refired_for_an_unchanged_status` |
| #11445 (1 comment) | closed | `animateTo(duration: Duration.zero)` hit an obscure assert; fixed by the synchronous zero branch | **Solved in #1181** (same branch shape) |
| #37685 (2 comments) | closed, user error | `repeat()` called from `build` every frame; in Flutter it still progresses because the repeat continues from the current phase | **Solved in #1182**: FLUI's old snap-to-`min` would have frozen that pattern; phase continuity fixes it |
| #149573 (1 comment) | closed, unreproduced | "animation jumps on `forward()` in `initState`" | **Not applicable**: `Vsync` anchors `t = 0` on the first observed tick, so a long first frame cannot produce the jump |
| #156120 (4 comments) | closed dup of #106277 | negative ticker time | see #106277 |
| #181699 (1 comment) | closed | `resync()` asserted right after `start()` | **Not applicable**: FLUI has no `resync` (the scheduler is per realm; a controller never changes provider) |
| #15630 (10 comments) | closed, user error | status listener "fires continuously" (bad code); thread also shows `await forward().orCancel` chaining | **Covered**: `TickerFuture::when_complete_or_cancel` (ADR-0064) is the equivalent; the cupertino button now chains on it (#1181) |
| #76014 (0 comments) | open | `AnimationController.unbounded`: `forward()` jumps to ∞, `reset()` → −∞, behavior of bound-targeting methods undefined | **Filed #1183**: FLUI has the same ∞ jumps and, by reading `tick_time_based`, reads NaN at the anchor tick (`∞ * 0.0`); degenerate use, follow-up rather than this batch |

Searches run (titles and bodies, open and closed): `AnimationController repeat`, `AnimationController.repeat`,
`TickerFuture`, `AnimationController animateTo`, `AnimationController zero duration`, `Ticker muted`,
`AnimationController status`, `AnimationController fling`, `AnimationController reverse`,
`AnimationController is:open`, `Ticker is:open animation`, `AnimationController dispose`,
`AnimationController duration`, `TickerProvider`, `AnimationController value`, `whenCompleteOrCancel`,
`TickerCanceled`, `AnimationController repeat count`, `animateTo status forward`, `Ticker dispose active`,
`AnimationController unbounded`, `AnimationController resync`, `non-monotonic frame timestamp animation`.
Not applicable by inspection (widget-layer or platform, not the controller): #39495, #27842, #70614,
#168344, #14019, #31385, #148379, #6792, #179337, #167899, #131357, #117539, #115901.
