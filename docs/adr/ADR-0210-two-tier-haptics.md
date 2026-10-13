# ADR-0210: Haptics has a semantic tier and a pattern tier, both addressed and both reporting an outcome

- **Status:** Accepted (2026-10-13); implemented by the interaction completeness spec
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
with control points ([followups §3.1](../research/2026-10-12-interaction-input-followups.md)).
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
   HapticTiming }` with `HapticTiming::{Immediate, FramePresented}`; `play` also takes the tier-1
   `HapticFeedback` it falls back to. Both return `HapticOutcome`, an enum, not a `Result`,
   `#[must_use]` and `#[non_exhaustive]`:
   `Submitted` (handed to the actuator as asked), `Queued(HapticTicket)`, `Approximated` (played
   with lost fidelity), `FellBack` (the pattern could not play; its fallback did),
   `Suppressed` (deliberately not played) and `Unavailable` (no actuator). A backend never
   returns `Queued`; only the frame queue does.
4. **Frame queue.** A `FramePresented` request joins the presentation's owner-local queue, which
   requests a frame and returns `Queued(HapticTicket)`. The ticket carries the `InputSequenceId`
   of the dispatch that issued it and an ordinal unique in its presentation, so two requests of
   one sequence stay distinct; the final outcome is delivered with the same ticket to the
   capability's observers after dispatch, with panics contained. The request plays when that
   frame is presented. A request still queued at its deadline (on the `EventTime` scale)
   resolves to `Suppressed`; it never plays late. Hiding, occluding or closing the window drains
   the queue, every entry resolving to `Suppressed`. Nothing plays while the window is hidden.
5. **Source.** `source: None`, or a `DeviceId` the backend no longer knows (a pen put away, a
   device unplugged), plays on the window's default actuator. A stale source is not an error.
6. **User preference.** When the observed system haptics setting is off (ADR-0212), both tiers
   resolve to `Suppressed` before reaching the backend. Built-in widgets fire haptics only
   together with a visible change, so turning haptics off loses no information.
7. **Degradation, per backend** (followups §3.3, one table pinned per backend):

   | Backend | Tier 1 | Tier 2 |
   |---|---|---|
   | iOS | `UIFeedbackGenerator(view:)` | Core Haptics: exact |
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
   ([dependency selection](../research/2026-10-13-interaction-dependency-selection.md)). It routes
   per thread, so requests run on the owner thread of the window that received the input.
   Core Haptics stops its engine on interruption: the iOS backend installs a `resetHandler` that
   only records a restart obligation and wakes the owner, which restarts the engine before the
   next play; a pattern requested while it is down falls back to tier 1.
9. **Gate.** `HapticPattern` and its parts, `HapticRequest`, `HapticOutcome`, `HapticTicket` and
   `HapticCapabilities` sit behind the additive `unstable-input` feature until every backend
   passes the degradation table and the conformance rows. Under it, `PlatformHaptics` gains
   methods with default bodies (so enabling the feature never breaks an implementor); the
   Stable `perform(HapticFeedback)` is unchanged until the gate is lifted.
10. **Widget path.** Widgets reach haptics through a capability acquired in `init_state` or
    `did_change_dependencies` (`LifecycleContext`, ADR-0078), bound to the element's
    presentation and owner thread. An inherited subtree switch turns it off: requests under it
    resolve to `Suppressed`. Headless resolves `FramePresented` at the harness's frame boundary.

## Consequences and verification

- Tests through public API: `haptic_pattern_builder_rejects_invalid_patterns` (each error
  variant and the boundary values of `Intensity`/`Sharpness`);
  `frame_presented_haptics_play_with_the_frame` (frame requested, ticket matched, deadline →
  `Suppressed`, drain on hide, occlusion and close, never played late);
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
