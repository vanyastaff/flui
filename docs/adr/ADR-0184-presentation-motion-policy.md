# ADR-0184: Presentation clocks resolve host motion and application policy

- **Status:** Accepted
- **Date:** 2026-10-09
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md),
  [ADR-0176](ADR-0176-presentation-animation-playback.md),
  [ADR-0179](ADR-0179-controller-registration-ownership.md)

## Decision

The existing host `SystemPreferences` snapshot is the sole motion observation.
There is no second window subscription or platform motion producer. Each UI
runtime retains an application `MotionPreference`: FollowSystem, Reduce or Full.
Missing host motion means NoPreference. FollowSystem resolves host Reduce to
Reduce and positive duration scales to Full with a scaled Normal timeline.
Application Full uses authored durations; application Reduce always reduces.

Each presentation owns its `MotionClock`. The clock preserves its debug/playback
timeline and projects a separate Normal timeline. Normal duration scales compose
with debug playback; Preserve ignores host scales and application policy while
still honoring debug playback. Changes rebase at the last accepted frame, without
sampling controllers or invoking user callbacks. Time remains monotonic and
saturates at `Duration::MAX`.

Every `FrameTick` carries both timelines and resolved policy. The Vsync traversal
forwards the same tick through nested registries. Under Reduce it settles Normal
work outside the registry borrow: finite runs complete at their terminal value;
finite repeats land at the last leg's endpoint; infinite repeats park at their
first leg's start and retain their future. Parked work requests no continuation.
Full resumes parked work from a fresh zero-time anchor. A reentrant replacement
waits until the next frame. Existing retirement and first-failure custody apply
to listeners, futures and simulation sources.

Runtime projection commits every presentation's clock before publication or wake
callbacks. Changed policy or scale requests a frame, including when a repeat is
parked or playback is paused. The resolved policy is published as
`MediaQueryData::motion`; repeated host observations do not republish it.
Application overrides apply before runners mount their first root and seed later
presentations in the same runtime.

Controllers default to Normal, including unbounded controllers. Physical scroll
and viewer inertia, loading indicators, snackbar display timers and press-delay
timers explicitly use Preserve. Cupertino route composition reads the inherited
policy and returns its static child under Reduce.

## Alternatives

Per-widget reduction would let a forgotten consumer keep moving and duplicate
policy resolution. Scaling all animation time would also scale essential timers
and physical inertia. A separate window producer would duplicate the accepted
host preference and its delivery/recovery protocol. Presentation clocks and the
registry traversal already own these responsibilities and their lifetime.

## Compatibility and evidence

`MediaQueryData` is publicly constructible. Adding `motion` breaks exhaustive
struct literals; literals using `..Default::default()` keep working. This is an
intentional pre-1.0 change. `AppConfig` is already non-exhaustive. Package users
reach AnimationBehavior and MotionPolicy through the SDK's animation module;
`tests/surface.rs` pins these actual consumer imports.

`motion_policy_resolves_preference_against_the_system` covers all nine policy
combinations. `normal_and_preserve_use_distinct_duration_timelines` covers nested
registries and both scale directions. `reduced_motion_settles_finite_and_paused_runs_once`
covers reverse and finite repeat endpoints, including paused clocks and runs.
`reduced_motion_admission_wakes_a_paused_run` pins new work admitted after paused
playback has already taken effect, through both direct and nested registries.
`parked_repeat_resumes_from_zero_under_full` pins parking and fresh resumption.
`reduced_settle_preserves_peer_delivery_and_reentrant_runs` checks competing
callback failures and the next frame after containment.
`system_motion_change_reaches_media_query_and_the_clock` drives host publication,
actual runtime frames, duplicate observations, late runtime seeds and later
presentation overrides. Removing clock projection makes that test fail.
`cupertino_route_does_not_slide_under_reduced_motion` checks that a pushed
page accepts input at its final position on its first frame.
