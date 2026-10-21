# ADR-0210: Haptics has a semantic tier and a pattern tier, both addressed and both reporting an outcome

- **Status:** Accepted (2026-10-13); implemented. The native macOS, iOS, Android and Web paths are
  compiled but have not run on a device; the Windows pen path has run only on a host with no
  haptic pen. §5 amended (2026-10-17): the window's default actuator on Windows is the pen in
  range of it.
- **Date:** 2026-10-13
- **Supersedes:** [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md)
  §1–§4 only. ADR-0031 §5, the system chrome deferral, stays in force.
- **Related:** [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) (`InputSequenceId`,
  the `EventTime` scale, owner-thread haptics),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) (the contract crate),
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) (lifecycle capabilities),
  [ADR-0212](ADR-0212-system-preferences-for-input.md) (the user haptics preference),
  [ADR-0213](ADR-0213-input-capabilities.md) (`Availability`, `unstable-input`),
  [ADR-0027](ADR-0027-owner-affine-ui-realms.md); the
  [interaction completeness spec](../plans/specs/flui-interaction/completeness/design.md) §8.

## Context

`PlatformHaptics::perform(HapticFeedback)` is fire-and-forget over Flutter's eight kinds. It
cannot say which actuator plays (a pen and a touchpad are both actuators on Windows), cannot
align feedback with the frame that shows the change (AppKit's `drawCompleted`, UIKit's
`impactOccurred(at:)`), cannot express a Core Haptics or Android envelope pattern, and gives a
test nothing to assert. AppKit (`alignment`, `levelChange`), Android `HapticFeedbackConstants`
(`CONFIRM`, `REJECT`, `DRAG_START`) and SwiftUI `SensoryFeedback` name kinds Flutter lacks.
Android API 36 envelopes and Core Haptics share one model: normalized intensity and sharpness
with control points ([followups §3.1](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-12-interaction-input-followups.md)).
No widget can reach haptics today.

## Decision

1. **Tier 1, semantic.** `HapticFeedback` (Stable, `flui-platform-api`) keeps its variants and
   gains `Alignment`, `LevelChange`, `DragLift`, `Drop`, `Confirm` and `Reject`. A kind the
   device lacks maps to the nearest one the backend has and reports `Approximated`.
2. **Tier 2, patterns.** `HapticPattern` is an ordered list of `HapticEvent`s, built only by a
   consuming, validating builder (no typestate) and frozen as `Arc<[HapticEvent]>`, an immutable
   value that crosses the `Send + Sync` platform boundary:

   ```rust
   pub enum HapticEvent {
       Transient { at: Duration, intensity: Intensity, sharpness: Sharpness },
       // ParameterCurve: ordered ControlPoint { offset, intensity, sharpness }
       Continuous { at: Duration, curve: ParameterCurve },
   }
   #[non_exhaustive]
   pub enum HapticPatternError {
       Unordered { index: usize }, NonFinite, OutOfRange, EnvelopeNotClosed, TooLong { max: Duration },
   }
   ```

   `Intensity` and `Sharpness` are newtypes over a finite `f32` in `0.0..=1.0`
   (`const fn new -> Option<Self>`, `TryFrom<f32, Error = UnitRangeError>`). A curve's control points are ordered with
   positive offsets and end at zero intensity; the pattern's length is capped. A bad pattern is
   an `Err` at construction, never at play time. `HapticCapabilities` describes one actuator
   and is read when the device set changes, never per call.
3. **Request and outcome.** Both tiers take a `HapticRequest { source: Option<DeviceId>, timing:
   HapticTiming, sequence: Option<InputSequenceId> }` with
   `HapticTiming::{Immediate, FramePresented}`. `InputSequenceId` lives in `flui-foundation`
   (beside `DataTransferId`), so this contract crate names it without depending upward; the
   binding mints it ([ADR-0207](ADR-0207-host-input-boundary-and-timeline.md)). The
   widget-facing handle fills `sequence` from the current dispatch and leaves it `None` outside
   one: the sequence is diagnostic context, not the correlation key, so a request outside
   dispatch mints nothing and cannot fail on sequence exhaustion. `play` also takes the tier-1
   `HapticFeedback` it falls back to. Both return `HapticOutcome`, an enum, not a `Result`,
   `#[must_use]` and `#[non_exhaustive]`:
   `Submitted` (handed to the actuator as asked), `Queued(HapticTicket)`, `Approximated` (played
   with lost fidelity), `FellBack` (the pattern could not play; its fallback did),
   `Suppressed` (deliberately not played) and `Unavailable` (no actuator). A backend never
   returns `Queued`; only the frame queue does.
4. **Frame queue.** A `FramePresented` request joins the presentation's owner-local queue, which
   requests a frame and returns `Queued(HapticTicket)`. The ticket is an ordinal
   (`NonZeroU64`) unique in its presentation, never reused; at its terminal value the queue
   refuses permanently and requests degrade to `Immediate`, which returns a final outcome
   directly. The final outcome is delivered with the same ticket to the
   capability's observers after dispatch, with panics contained. Pending delivery is kept apart
   from the frame queue: resolving a request moves `(ticket, outcome)` into the presentation's
   outcome mailbox before any observer runs, and the queue entry is retired only then. The
   mailbox is drained to the observers registered at delivery time with no borrow held. The
   mailbox is bounded (a fixed capacity per presentation, 64 entries): when it is full, a new
   `FramePresented` request is not queued and degrades to `Immediate`, which returns its final
   outcome directly, so admission is refused before a ticket exists and memory stays bounded
   with an absent or failing observer (row
   `sustained_requests_with_no_observer_degrade_to_immediate_when_the_mailbox_is_full`). With no
   observer registered the outcomes stay in the mailbox and go to the next observer that
   registers; an observer replaced during delivery hands the rest of the batch to its
   replacement. An entry leaves the mailbox only when an observer call for it returns normally;
   an observer that panics is contained (the first failure authoritative and reported), and the
   entry it panicked on stays at the head of the mailbox with the tail behind it. If it
   installed a replacement before panicking, the committed replacement wins: the panicking
   observer is retired after the catch, never restored over it, and the replacement receives the
   retained head and tail (row `an_observer_that_installs_a_replacement_then_panics_hands_over_the_head`).
   Otherwise the panicking observer keeps its registration. Every outgoing observer (replaced
   after a normal return, replaced and then panicking, or removed) is retired after its call
   returns or is caught, under the same containment custody: a panic while destroying its
   captures is contained separately, never replaces the first failure and never double-panics,
   and the replacement still drains the retained entries and any retry is still scheduled (row
   `a_replaced_observer_whose_drop_panics_does_not_stop_delivery`). That delivery pass ends, and a
   failed pass schedules one owner-thread wake through the runtime scheduler for the next pass,
   so recovery does not wait for unrelated dispatch or frames (a replacement observer also
   starts a pass at once); each pass retries from that entry, at most once per pass. Wake-driven
   retries of one head by one registration are bounded (three consecutive failed passes); after
   that the head waits, with no further wakes, until the observer is replaced or removed, a new
   observer registers, or the presentation closes, so a deterministically panicking observer
   cannot keep the runtime busy (rows `an_observer_panic_on_an_idle_presentation_is_retried_by_a_wake`,
   `a_permanently_panicking_observer_stops_waking_after_the_bound`).
   Closing the presentation first resolves every queued request (`Suppressed`) and runs one
   final delivery pass; entries that pass cannot deliver (no observer registered, or the
   observer panicked) are reported with their tickets to the owner's diagnostics and retired
   with the presentation. Tickets are scoped to their presentation, and the elements that hold
   them unmount with it, so no consumer remains to receive them later (row
   `closing_delivers_pending_outcomes_in_a_final_pass`). Every accepted ticket therefore reaches
   an observer once one accepts it (rows: `queued_outcomes_wait_for_an_observer`,
   `an_observer_replaced_mid_delivery_hands_over_the_rest`,
   `an_observer_panic_keeps_the_outcome_for_the_next_pass`). The request plays when that
   frame is presented. Its deadline is snapshotted at enqueue: enqueue time plus
   the presentation's `haptic_frame_deadline`, held in its presentation metrics (ADR-0212; not a
   pointer-kind preset; recomputed when the window changes display, a queued request keeping
   the value snapshotted at its enqueue; not keyed by pointer kind, so
   keyboard, assistive and programmatic requests use it too; a sourced number such as two
   refresh intervals of the window's display), on the `EventTime` timeline. The sum is a checked
   add: if it overflows, or if it yields the terminal `EventTime` (no representable time lies
   strictly after it, so expiry could never be scheduled), the request (a synthetic or replayed
   time near the end of the range) is never queued and resolves to `Suppressed` at once; row
   `deadline_overflow_resolves_without_queueing`, with cases for overflow and for a deadline
   equal to the terminal `EventTime`.
   Enqueueing also
   schedules an owner-thread deadline wake through the runtime scheduler, independent of frame
   presentation and of further input, for the first `EventTime` strictly after the deadline
   (a wake that observes `now == deadline` reschedules itself once past it); presentation
   cancels that wake. A frame presented at a
   time `<= deadline` plays the request (inclusive); expiry fires only when `now > deadline`, so
   the order in which the two wakes are processed never changes the outcome. At expiry the
   request resolves to `Suppressed`; it never plays late. Hiding, occluding or closing the
   window resolves it earlier, draining the queue with every entry `Suppressed`. Every accepted
   request therefore reaches exactly one final outcome. Nothing plays while the window is hidden.
5. **Source.** `source: None`, or a `DeviceId` the backend no longer knows (a pen put away, a
   device unplugged), plays on the window's default actuator. A stale source is not an error.
   On Windows the window has no actuator of its own (§8), so its default actuator is the pen in
   range of the window, touching or hovering: the pen a request names while it is in range,
   otherwise the pen read most recently. With no pen in range nothing plays and the request is
   `Unavailable`. Widgets fill no source today, so this is what makes a pen vibrate for them.
6. **User preference.** When the observed system haptics setting is off (ADR-0212), both tiers
   resolve to `Suppressed` before reaching the backend. Built-in widgets fire haptics only
   together with a visible change, so turning haptics off loses no information.
7. **Degradation, per backend** (followups §3.3, one table pinned per backend):

   | Backend | Tier 1 | Tier 2 |
   |---|---|---|
   | iOS | `UIImpactFeedbackGenerator(style:view:)`, `UISelectionFeedbackGenerator(view:)`, `UINotificationFeedbackGenerator(view:)` (iOS 17.5+); before 17.5 the view-less initializers, `source` ignored: `Approximated` | Core Haptics: exact |
   | Android 36+ | `performHapticFeedback` | envelopes where `areEnvelopeEffectsSupported`: exact |
   | Android 30–35 | `performHapticFeedback` | transients to primitives by sharpness and intensity; one unsupported primitive → `FellBack`; continuous → amplitude waveform (sharpness dropped) or `FellBack` |
   | Windows | pen `SimpleHapticsController`: nearest waveform | nearest waveform, intensity only if `IsIntensitySupported`, sharpness dropped: `Approximated` |
   | macOS | `NSHapticFeedbackManager` (`alignment`, `levelChange`, else `generic`) | always `FellBack` |
   | Web | Vibration API, Chromium, with user activation | pulses only: `Approximated` or `FellBack` |
   | Linux | `Unavailable` | `Unavailable` |

   No backend stretches a duration, raises an intensity or plays for a hidden window.
8. **Platform notes.** Windows `InputHapticsManager` (touchpad and mouse haptics) is in no
   published `windows` crate (0.62.2 has only `SimpleHapticsController`), so it is
   `Unavailable` until a release ships it or a narrow `windows-bindgen` binding is generated
   ([dependency selection](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-13-interaction-dependency-selection.md)). It routes
   per thread, so requests run on the owner thread of the window that received the input.
   The pen's `SimpleHapticsController` (from `PenDevice::GetFromPointerId`) plays one waveform at a
   time, so a pattern's calls are made at their times off the owner thread, a newer request
   replacing what is left of an older one; a continuous waveform is handed its length where the
   controller takes a play duration and stopped at its end otherwise, so none outlasts its step.
   Android pattern vibrations carry the touch usage (`VibrationAttributes.USAGE_TOUCH` from API
   33, sonification audio attributes before it), and need the install-time `VIBRATE`
   permission: without it the backend offers no pattern playback, so patterns fall back to
   `performHapticFeedback`, which needs none.
   The Core Haptics backend restarts its engine after interruption, suspension or idle shutdown
   (`stoppedHandler`) and after a haptic-server error (`resetHandler`). Both handlers only
   record a restart obligation and wake the owner, which restarts the engine before the next
   play; a pattern requested while it is down falls back to tier 1.
9. **Gate.** `flui-platform-api` always builds `HapticPattern` and its parts, `HapticRequest`,
   `HapticOutcome`, `HapticTicket`, `HapticCapabilities` and the `PlatformHaptics` methods that
   take them (with default bodies, so a new method never breaks an implementor), so Cargo's
   feature unification cannot change them. The user-facing gate is the additive
   `unstable-input` feature on the `flui-sdk` and facade re-exports, kept until every backend
   passes the degradation table and the conformance rows. The Stable
   `perform(HapticFeedback)` is unchanged until the gate is lifted.
10. **Widget path.** Widgets reach haptics through a capability acquired in `init_state` or
    `did_change_dependencies` (`LifecycleContext`, ADR-0078), bound to the element's
    presentation and owner thread. An inherited subtree switch turns it off: requests under it
    resolve to `Suppressed`. Headless resolves `FramePresented` at the harness's frame boundary.

## Consequences and verification

- Tests through public API: `haptic_pattern_builder_rejects_invalid_patterns` (each error
  variant and the boundary values of `Intensity`/`Sharpness`);
  `frame_presented_haptics_play_with_the_frame` (frame requested, ticket matched, deadline →
  `Suppressed`, drain on hide, occlusion and close, never played late, a visible surface that
  never presents a frame, a request made outside dispatch with no sequence, presentation
  exactly at the deadline plays, ticket exhaustion degrades to `Immediate`);
  `stale_haptic_source_plays_on_the_window_actuator`;
  `haptics_preference_and_subtree_switch_suppress_both_tiers`; `haptic_degradation_table`, one row
  set per backend over its pure mapping functions; haptics rows in `assert_backend_conforms`.
- CI executes the headless backend, the queue, the builder and every backend's mapping table
  (pure functions). Actual vibration runs on a native host only: Windows pen, iOS Core Haptics
  (including reset), Android, macOS and Web, each through `cargo xtask device <os>-input`, and a
  PR touching them shows that run.
- `FakeHaptics` records requests and the outcomes it returned.

## Alternatives rejected

- **A second tier-1 enum** (`HapticIntent` in the followups note): two vocabularies for one tier.
- **`Result` for the outcome.** `Suppressed` and `FellBack` are normal results, not errors.
- **Playing a late frame-aligned request.** Feedback detached from its visible change misleads.
- **A git dependency on windows-rs** for `InputHapticsManager`: `deny.toml` allows no git
  sources.
