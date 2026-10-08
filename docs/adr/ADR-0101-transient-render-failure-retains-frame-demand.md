# ADR-0101: Transient render failure retains frame demand

- **Status:** Accepted
- **Date:** 2026-10-01
- **Related:** ADR-0045 (reliable raster completion), ADR-0068 (presentation
  disposition), ADR-0083 (host-free UI runtime), ADR-0100 (prepared GPU work).

## Context

Resource admission can defer a frame while earlier GPU work is outstanding. A
missing retained source also requires another attempt with full damage. Calling
these errors recoverable inside the engine does not schedule that attempt: mapping
them to the UI runtime's terminal failure verdict drains input epochs and loses the
last update of a static scene until unrelated input arrives.

## Decision

After surface and device recovery are classified, other recoverable engine errors
produce `FrameDropReason::RetryableRenderError`. The reliable completion slot also
publishes `retry_required` before retiring the ticket and waking the owner. A full
telemetry acknowledgment channel cannot erase that fact.

The application's raster lane and direct web sink map transient rendering failure
to the exhaustive, GPU-free `SubmitVerdict::Retry`. The UI runtime retains input epochs,
marks the presentation for full repaint, and requests its ordinary paced frame
wake. No new input, surface-generation change or device replacement is required.
A wake alone is insufficient because the failed attempt already consumed paint
demand. A successful retry accounts for the original input epoch.

An impossible current-frame resource footprint remains terminal. Earlier pending
work can cause transient backpressure. The engine must give native completion
callbacks a nonblocking progress opportunity before admission can refuse another
frame; otherwise repeated refusal can prevent the very retirement it awaits.
Browser completion progresses through the event loop instead of blocking paint.

`PresentDisposition` keeps its existing meaning. The bounded `NotShown` retry
policy remains distinct from resource backpressure. Transient retries use the
runner's existing no-present pacing, not an immediate inner retry loop. Repeated
recoverable failures can therefore keep requesting paced frames; this decision
does not introduce a timeout that silently drops owed content.

## Verification

- `every_failure_boundary_retires_its_ticket` fills the lossy ack channel, fails
  with backpressure or a missing retained source, observes reliable retry debt and
  wake delivery, then successfully renders the next frame.
- `raster_lane_outcome_matrix` checks transient, terminal and device failures in
  both application sinks, followed by successful submissions.
- `surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`
  covers both stale-surface and transient verdicts: the second attempt presents
  without another dirty event and still reports the original input epoch.

The engine tests inject errors at the backend boundary; they do not establish
driver-specific starvation or window-system timing on every platform.
