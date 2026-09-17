# Spec — #1172 Route pipeline visual updates through `ensure_visual_update`

Problem and non-goals carried from `intent.md`. Base: `main`.

## The two carriers (verified, not recalled)

- **scheduler carrier** — `frame_scheduled: AtomicBool`
  (`scheduler.rs::FrameState`), set by `request_frame_impl` (fires
  `on_frame_scheduled` on false→true, *outside* the hook lock), cleared by
  `handle_begin_frame` and `finish_async_pump`.
- **realm carrier** — the presentation's `set_on_need_visual_update` closure
  (`presentation.rs`, ~line 525), which calls `capabilities.wake`
  (= the realm's `wake` = `FrameWakeHandle::wake_frame`) and pokes
  `window.request_redraw()`. The closure fires when the pipeline's internal
  `DirtyTracker` (`flui-rendering/src/pipeline/scheduler.rs`) fires
  `fire_need_visual_update` on a new dirty mark — including the mid-phase
  side-queue paths (`add_node_needing_layout`, `enqueue_paint`,
  `schedule_compositing_root`, `add_node_needing_semantics`).

The realm's `wake` is *already* registered as the scheduler's
`on_frame_scheduled` hook (`ui_realm.rs:808`
`scheduler.set_on_frame_scheduled(Some(Arc::clone(&wake)))`). So routing the
closure through `ensure_visual_update` makes `visual_wake()` redundant — the
hook fires it on the `frame_scheduled` false→true edge. The closure keeps only
the per-window poke.

## Design

### flui-scheduler — `ensure_visual_update` (and `schedule_frame_if_enabled`) return `bool`

```rust
pub fn schedule_frame_if_enabled(&self) -> bool {
    if self.inner.binding.frames_enabled.load(Ordering::Acquire) {
        self.request_frame();
        true
    } else {
        false
    }
}

pub fn ensure_visual_update(&self) -> bool {
    match self.phase() {
        SchedulerPhase::Idle | SchedulerPhase::PostFrameCallbacks => {
            self.schedule_frame_if_enabled()
        }
        SchedulerPhase::TransientCallbacks
        | SchedulerPhase::MidFrameMicrotasks
        | SchedulerPhase::PersistentCallbacks => {
            let frame_thread = *self.inner.frame.frame_thread.lock();
            if frame_thread != Some(std::thread::current().id()) {
                self.schedule_frame_if_enabled()
            } else {
                false
            }
        }
    }
}
```

**Return semantics:** `true` iff the phase gate passed AND frames are enabled
(a frame was actually requested); `false` iff the demand was dropped — the
same-thread mid-frame no-op, or `frames_enabled == false`. This is *not* the
`frame_scheduled` false→true edge: the per-window poke must still fire when a
frame is already scheduled (the multi-window test dirties B after A, both from
Idle, and expects B's window poked).

**No `#[must_use]`:** ignoring the return is a legitimate position (the
existing `RendererBinding::request_visual_update` and the integration test do
not need to know). `bool` is not `#[must_use]`; existing call sites stay green.

### flui-app — presentation closure gates the per-window poke

```rust
let scheduler = capabilities.scheduler.downgrade(); // WeakUpdateScheduler
let redraw_window = Arc::downgrade(&window);
pipeline.with_mut(|owner| {
    owner.set_on_need_visual_update(move || {
        if let Some(scheduler) = scheduler.upgrade()
            && scheduler.ensure_visual_update()
        {
            if let Some(window) = redraw_window.upgrade() {
                window.request_redraw();
            }
        }
    });
});
```

`visual_wake()` is dropped: the `on_frame_scheduled` hook (same `wake`)
already fires on the false→true edge. The per-window poke survives, gated on
the bool. `WeakUpdateScheduler` (not a strong `Arc`) is captured, matching
`RenderingFlutterBinding.scheduler`'s convention; upgrade failure (realm torn
down) is a silent no-op — nothing left to wake.

## Lock-order / re-entrancy proof (async-systems-lens)

The closure fires while the caller holds the pipeline cell checked out. It now
reaches `ensure_visual_update`, which locks `frame_thread` (mid-frame arm) and,
via `request_frame_impl`, `on_frame_scheduled` (clone-then-drop). Deadlock
requires a reverse path — a scheduler lock held across a call back into the
pipeline. None exists:

- `handle_begin_frame` stores `frame_thread` under a *temporary* guard
  (statement-scoped, `scheduler.rs:1309`) and releases it before the pipeline
  runs; the *value* stays `Some(driving thread)` through the frame, which is
  exactly what the same-thread no-op reads.
- `drive_frame_impl` runs the lane closure (`pipeline()`) with **no** scheduler
  lock held: `idle_deadline` is set-and-released, `handle_begin_frame`/
  `handle_draw_frame` release every lock before invoking callbacks, and the
  `IdleDeadlineGuard` is dropped before `pipeline()`.
- `request_frame_impl` clones the `on_frame_scheduled` hook, **drops the lock
  guard, then** calls the hook; the hook (`wake`) touches only `needs_redraw`
  and the `redraw_window` slot, never the pipeline or a scheduler lock.

Therefore a mid-frame pipeline mark calls `ensure_visual_update` from inside the
cell checkout, locks a *free* `frame_thread`, reads `Some(current)`, returns
`false`, and pokes nothing. No lock is held across the pipeline on any path, so
no re-entry and no inversion.

## Flutter cross-check (`.flutter` at tag 3.44.0)

`SchedulerBinding.ensureVisualUpdate` → `scheduleFrame()` →
`if (_hasScheduledFrame || !framesEnabled) return;`. The enablement gate is
*inside* the demand call — FLUI's `schedule_frame_if_enabled` is the same
shape. Routing the pipeline wake through it matches Flutter: a pipeline visual
update while frames are disabled is dropped (the dirty node stays in the
pipeline's dirty list, and the lifecycle re-enable edge re-requests a frame
that picks it up). This is a deliberate unification with the existing gated
`RendererBinding::request_visual_update` path, not a new divergence.

## Tests (red before green)

1. **flui-scheduler** (`visual_update_tests.rs`): assert the bool — the three
   mid-frame same-thread no-op pins return `false`; the Idle and
   PostFrameCallbacks pins return `true`; the cross-thread mid-frame pin
   returns `true`. Add a frames-disabled pin: `set_frames_enabled(false)` →
   `ensure_visual_update()` returns `false` and does not schedule.
2. **flui-app** (`presentation.rs` or `ui_realm.rs`): a presentation whose
   scheduler is mid-frame (driving-thread persistent callback) issues a
   pipeline visual update and observes its window's `request_redraw` count
   stay zero; the same update from Idle pokes the window. The existing
   `redraw_request_from_a_does_not_wake_bs_window` continues to pin per-window
   routing.

## Files

- `crates/flui-scheduler/src/scheduler.rs` — `ensure_visual_update`,
  `schedule_frame_if_enabled` return `bool`; doc rewritten (the "never through
  this method" paragraph is now stale).
- `crates/flui-scheduler/src/scheduler/visual_update_tests.rs` — bool pins.
- `crates/flui-app/src/app/presentation.rs` — closure routes through
  `ensure_visual_update`, drops `visual_wake`, gates the poke.
