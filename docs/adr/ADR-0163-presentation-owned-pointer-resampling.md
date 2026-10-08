# ADR-0163: Presentation-owned pointer resampling

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0037](ADR-0037-presentation-ownership-domains.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)

## Context

Frame-aligned motion needs both measured hardware timestamps and the frame
timestamp that the presentation actually publishes. Reading a separate clock
at dispatch or flush can move the sampling window onto another timeline,
particularly under a manual clock. Buffering motion also creates work for a
future frame: acknowledging the current rendered tree cannot erase that work.

The presentation therefore owns an explicit resampling policy and the runtime
supplies its clocks and continuation demand. This makes the latency trade-off
authored at the host boundary while retaining the existing pointer vocabulary,
binding, cached contact route and frame transaction.

## Decision

### Explicit policy per presentation

`PointerResampling::Disabled` is the default and retains frame coalescing.
`PointerResampling::FrameAligned` opts a presentation into measured-motion
interpolation. `PresentationWindow::with_pointer_resampling` supplies its
initial policy; app configuration forwards it during presentation assembly.
`UiRealm::set_pointer_resampling` addresses an existing presentation, and
`HeadlessHost` forwards the same runtime operation.

A contact keeps the policy under which its Down was admitted. Changing policy
while contact sequences are active returns the binding's mode-change error;
setting the same policy remains idempotent. A missing presentation returns a
typed unavailable error. A sibling presentation has its own binding, queues
and policy even when both presentations share a realm clock.

This is an application choice, not an inferred system preference. It does not
introduce process-wide configuration or fabricate display-refresh metadata.

### Clock authority and measured samples

The binding supplies arrival from the realm's `ClockSource`. The resampler
maps owned `EventTime` onto that Instant timeline using the earliest consistent
clock base observed, preserving measured spacing when packets arrive together.
Clock-base installation and enqueue share one guarded operation; diagnostics
and delivery run after the guard is released. Unrepresentable mappings use the
existing arrival fallback, and zero remains a valid hardware timestamp.

`UiRealm::pump` publishes the one frame Instant read from its
`FrameClockSource`. The frame-aligned path samples at that Instant minus the
38 ms lookback and supplies an explicit next-sample window using the binding's
configured positive period. It does not independently read the wall clock to
choose this window. A manual host supplies the same clock to frame production
and input admission, so advancing that clock advances both timelines.

Interpolation uses the last measured point and the next buffered measured
point. The synthesized sample carries its interpolated time and position;
predicted hardware samples are not substituted for measured history. The
lookback is a deliberate latency cost, and the existing default period is
16,667 microseconds rather than an assertion about the host display.

### Measured motion before observing input

Keyboard and IME observe their resolved presentation's accepted motion before
their own dispatch. The runtime calls `GestureBinding::flush_pending_input`,
which drains measured packets without advancing frame time, interpolating a
synthetic frame sample or ending contacts. Default coalescing retains its latest
pending move; opted-in resampling retains its measured prefix. Tracking,
interpolation anchors, exact contact generations and capture selection remain
owned by the admitted contact.

The binding freezes every contact prefix before the first callback. Reentrant
motion belongs to a later operation or frame; it cannot be pulled into another
contact's frozen prefix. A newer coalesced marker cannot erase an already
frozen Contact payload while its exact contact generation remains live; old
cleanup cannot remove the newer marker. Native termination or replacement
still invalidates the old contact. Capture delivery guards protect accepted
measured and coalesced prefixes until delivery finishes, including when a
callback queues newer motion and releases another contact's token. Loss
settlement then delivers its accepted tail and one Cancel before the observing
input. The binding retains existing deterministic
pointer iteration; this contract does not establish global timestamp ordering
across independent devices.

Keyboard resolves the focus coordinator's active presentation before the
barrier, even when the native event names another window. That accepted input
keeps its resolved presentation if a motion callback changes active focus. The
next input resolves focus again. IME retains its addressed presentation and
projects onto its actual attached text store. Motion, deferred arena settlement
and the following observing input each have containment: an earlier failure
cannot suppress accepted healthy tails or the following edit, and competing
failures cannot replace the first one. Rendering geometry still changes at the
ordinary frame boundary; this is an input-state observation barrier.

### Pending delivery and containment

An accepted future sample is a delivery obligation distinct from an active
contact. Pending sampling work requests the owning presentation's next frame
both before propagating an input failure and after the current render
acknowledgement. Once the queue is empty, the contact alone creates no
resampling demand.

Sampling commits its queue changes before invoking callbacks. The binding and
runtime retain the first callback failure while completing the accepted batch,
deferred arena work, sibling presentation delivery and pending wake attempt.
They resume that first failure only after the applicable containment round.
A later healthy pump can deliver the retained future tail.

Up and Cancel remain synchronous. Terminal handling detaches the exact contact
generation, flushes its accepted buffered movement, delivers its terminal and
finishes arena retirement while preserving the first failure. A stale sampling
snapshot cannot publish into a replacement contact. Presentation suspension and
close retain their existing input cancellation ownership.

## Validation

The public widget row
`listener::presentation_resampling_uses_the_owner_frame_clock`, in
`contracts::pointer_and_gesture_recognition`, uses a real `HeadlessHost` and
Listener. A measured 100 ms segment with the 38 ms lookback emits x=72 at
62 ms, then a later owner frame delivers the accepted x=110 tail. The same row
pins the disabled control, active-policy refusal, idempotent policy and
synchronous Up.

The runtime table
`resampling_is_presentation_local_and_preserves_delivery_after_failure` uses
the private forest-installation seam because sibling bindings are internal to
the runtime. Each tree is acknowledged by a real sink pump before input; input
then uses addressed ingress and the actual owner pump. Its healthy, single and
competing callback-failure rows pin frame-aligned A versus default B, first
failure authority, sibling progress, next-frame redraw demand, tail recovery
and quiescence after the queue drains.

These behavioral tables establish owner-clock sampling and delivery behavior.
They do not establish a deterministic concurrent race proof for clock-base
installation against resampler stop. The one-guard implementation is the
ownership rule; a separate concurrency witness would be needed to claim that
specific race was reproduced. Native hardware timing and display cadence are
not implied by the headless tests.

The public `flui-testing::containment_and_isolation_matrix` also mounts
`mouse_motion_precedes_keyboard_without_a_frame`,
`touch_motion_precedes_keyboard_without_a_frame`, their resampled counterparts,
`motion_failure_keeps_following_keyboard_and_contact_terminal`,
`keyboard_failure_keeps_preceding_motion_and_contact_terminal` and
`motion_failure_precedes_competing_keyboard_failure_and_recovers`.
`ime_commit_observes_preceding_measured_motion` and
`ime_commit_survives_competing_motion_and_owner_failures` use an actual attached
EditableText and text-store projection.
`keyboard_reads_all_frozen_contacts_after_sibling_failure` and
`keyboard_barrier_keeps_reentrant_contact_motion_for_the_next_round` pin frozen
measured prefixes, Key's observation of motion-produced state and later debt.
`keyboard_barrier_keeps_frozen_coalesced_motion_before_reentrant_replacement`
pins the default-policy counterpart;
`keyboard_coalesced_prefix_survives_reentrant_capture_release` pins frozen
movement, release-tail delivery and one capture-loss terminal after replacement
of the queued marker.
`runtime_keyboard_barrier_preserves_scale_contacts_and_continuity` exercises
the actual GestureDetector consumer; and
`keyboard_motion_barrier_uses_resolved_focus_owner_during_reentrant_focus_change`
pins presentation resolution. These rows assert state before another frame,
then terminal delivery and healthy recovery, rather than only eventual counts.

## Consequences

Hosts can opt in without exposing substrate queues or inventing another input
transport. Replay and manual clocks use the same production path. Frame
acknowledgement, queued samples and wake demand stay separate, so successful
rendering or a failed observer cannot silently strand accepted motion.
