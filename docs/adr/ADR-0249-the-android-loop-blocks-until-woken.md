# ADR-0249: The Android event loop blocks until something wakes it

- **Status:** Accepted
- **Date:** 2026-10-19
- **Supersedes:** [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 7's Android
  sentence (an idle poll derived from the display period), and
  [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) decision 5 (Android keeps its
  sleep). The rest of both decisions is unchanged; iOS keeps its sleep.
- **Related:** [ADR-0247](ADR-0247-android-java-view.md) (the Java view whose events wake the
  loop), [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) (the surface a resize reaches)

## Context

`AndroidPlatform::run` polled `ALooper` with a timeout of one display period, because some work set
a flag without waking the looper (a window redraw request) and some was read on a timer (the
preferences every 500 ms). The loop therefore ran 60 to 120 times a second for as long as the app
was in the background, locking the window, driving the owner and making JNI calls. A wake deadline
that only a frame can consume (the device-recovery retry) came due while paused and turned the
poll into a 0 ms busy loop until `Resume`. The runner's backgrounded pump (frames disabled) slept
16 ms on the loop thread per wake, stalling input and lifecycle delivery.

## Decision

1. **Every producer of work wakes the looper.** `AndroidWindow::request_redraw` wakes it on the
   request that sets the flag, from any thread. The owner signal, the Java view's event queue, the
   inset reply and the display watch already did.
2. **The loop has no timer of its own.** `owner::android::poll::poll_timeout` decides how long
   `poll_events` blocks: not at all with a frame to deliver; while resumed, until the next wake
   deadline or the next preference sample; while paused, until a deadline that is still ahead, and
   otherwise until woken. A deadline already behind while paused was offered to the owner on that
   turn and is waiting for a frame, which nothing delivers before `Resume`.
3. **Preferences without a change notification are read every 500 ms only while resumed**, and on
   `Resume` and `ConfigChanged`.
4. **The backgrounded pump is paced by a deadline.** A pump that leaves work asking for another
   wake arms `BackgroundPace` one `BACKGROUNDED_PUMP_PACE` ahead; the wake hook reports it, a wake
   before it does not pump, the wake it brings consumes it, and a wake with frames enabled drops it.

## Consequences

A paused Android app costs no CPU between real events. A resumed, idle app wakes at most every
500 ms, for the preference read. Work that sets state without waking the looper now waits for the
next event; a new producer must wake it. The decision rests on `a_paused_loop_blocks_until_it_is_woken`'s
table and `a_background_pump_defers_the_next_by_one_pace`; the loop itself runs only on a device.
