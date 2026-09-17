# Intent — #1055 `end_of_frame` demand and cancellation

## Asked for

From issue #1055 (bug, priority: medium), "scheduler: make `end_of_frame` demand-driven and
cancellation-safe", filed against an audited revision of `crates/flui-scheduler/src/scheduler.rs`:

> `UpdateScheduler::end_of_frame` registers an awaiter WITHOUT requesting a frame — registration
> only pushes a notifier; it never calls the coalesced frame-demand path — so an idle caller can
> wait indefinitely with no unrelated frame demand; and the future stores a `Waker` in a
> strongly-shared `Arc` with no cancellation cleanup on drop, so a cancelled wait retains
> executor-owned resources until the next completion.

The issue carries a public-API reproducer: 3 tests, 1 passing, 2 failing, using a custom `Wake`
impl to detect resource retention; `execute_frame()` completes the future and releases the waker.

## Two defects, not one

1. **No demand.** A caller that awaits `end_of_frame()` while the scheduler is idle waits forever
   unless something *else* requests a frame.
2. **No cancellation.** Dropping the future leaves its `Waker` (and the `Arc` behind it) alive
   inside the scheduler's waiter list until the next frame completion drains it. An executor's
   task resources are pinned by a wait nobody is waiting on.

## What fixed looks like

- Awaiting `end_of_frame()` from an idle scheduler resolves on the next frame, with no other
  demand in the system.
- Dropping a pending `end_of_frame()` future releases every resource it held, immediately and
  observably — provable with the issue's `Wake`-impl probe, without waiting for a frame.
- Neither fix costs the guarantees `#1156`/`#1158`/`#1160` just landed in this crate: the
  drain-then-wake lock ordering, the per-waker panic containment, and the "an aborted frame still
  resolves `end_of_frame`" behavior pinned by `tests/frame_panic_recovery.rs`.

## Non-goals

- Reworking `FrameTiming`, the frame phases, or `drive_frame`'s deadline model.
- Making `end_of_frame` usable as a general-purpose "await N frames" API.
- Fixing `flush_microtasks`' unbounded drain (#1159) — adjacent, separately filed.

## User amendments

- *"поищи так же похожие проблемы в flutter issues"* — sweep Flutter's tracker for the same
  failure mode, not just the reference source. Recorded in `survey.md` §Flutter tracker.
- *"или в библиотеках которые используют wgpu и прочитай там тоже комменты"* — extend the sweep to
  the surrounding ecosystem, reading what those projects say, not only what they ship.
- *"всегда через полный /dev-task"* — every issue enters through the full skill discipline
  (scout → plan → adversarial plan review → approval → build → two-stage review), before any
  research or branch of my own.
- *"так же вопрос как бы про комментарии в issue и там может быть полезное или надо использовать
  какую то крутую библиотеку может которая решит или надо поднять версию или какие то фичи
  включить библиотеки"* — read the issue's own comment thread for signal, and treat "an existing
  library / a version bump / a feature flag already solves this" as a first-class candidate,
  ahead of hand-rolling. This amendment is what produced the reshape recorded in `survey.md`
  §Library survey and the design in `spec.md`.

## Corrections

- The plan's first draft cited Flutter's `ensureVisualUpdate` as the demand path `endOfFrame`
  takes. It does not — `endOfFrame` calls `scheduleFrame()` directly, and only when the phase is
  `idle`. Caught by `harsh-critic` before approval; the wrong citation would have bought a
  surplus frame per registration and turned the advertised "re-register in the post-frame
  callback" idiom into a self-sustaining frame loop.
