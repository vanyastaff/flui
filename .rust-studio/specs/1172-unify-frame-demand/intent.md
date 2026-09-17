# Intent — #1172 Unify frame demand

## Asked for

Frame demand reaches the runner's `wake_action`
(`crates/flui-app/src/app/runner/frame_pacing.rs`) through two independent
carriers OR'd together:

1. **scheduler-owned** — `UpdateScheduler::frame_scheduled`, set by
   `ensure_visual_update` / `schedule_frame_if_enabled` (tickers,
   `end_of_frame`, `request_frame`).
2. **realm-owned** — `PipelineOwner::request_visual_update`
   (`crates/flui-rendering/src/pipeline/owner/accessors.rs`) →
   `fire_need_visual_update()` → the presentation's wake closure
   (`crates/flui-app/src/app/presentation.rs`, the `set_on_need_visual_update`
   callback calling `capabilities.wake` + `window.request_redraw()`), which
   never touches `UpdateScheduler`.

`ensure_visual_update` (`crates/flui-scheduler/src/scheduler.rs`) carries
Flutter's phase gate (schedule from Idle/PostFrameCallbacks, no-op mid-frame on
the driving thread), but that gate governs ONLY the scheduler carrier. A
pipeline visual update issued mid-frame still goes straight to
`request_redraw`.

## What fixed looks like

- A pipeline visual update issued while the scheduler is mid-frame on the
  driving thread does **not** immediately `request_redraw` (gated).
- One issued from Idle/PostFrameCallbacks still results in a scheduled
  frame/wake.
- The realm-owned carrier flips the scheduler's `frame_scheduled` edge — one
  carrier reads both; `frame_is_dirty` / `mark_rendered` keep their
  surplus-frame-guard role.

## Non-goals

- Touching the ticker/animation carrier (`Ticker::schedule_tick_if_active` →
  `schedule_frame_callback` → ungated `request_frame`). Out of scope.
- Changing the multi-window *routing* model (`SeparateRealms` vs
  `SharedRealm`). The per-window poke is preserved, only gated.
- Reworking `frame_is_dirty` / `keeps_frame_gate_open` / `mark_rendered`.

## The one tension resolved in spec.md

The per-window `request_redraw()` poke (issue #555's addressed-routing slice,
pinned by `redraw_request_from_a_does_not_wake_bs_window`) must survive
routing through the scheduler — but must be gated behind the SAME phase
decision so a mid-frame mark no longer pokes the window. The resolution is a
`bool` return from `ensure_visual_update` that the closure uses to gate the
poke, without duplicating the phase/`frame_thread` logic.
