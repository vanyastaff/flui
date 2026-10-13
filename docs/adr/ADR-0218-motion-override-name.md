# ADR-0218: The application motion setting is named `MotionOverride`

- **Status:** Accepted
- **Date:** 2026-10-13
- **Supersedes:** [ADR-0195](ADR-0195-presentation-motion-policy.md), the application setting's name only.
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md)

## Context

ADR-0195 named the application's motion setting `MotionPreference`
(FollowSystem, Reduce, Full). The host's observation is also
`flui_platform_api::MotionPreference` (NoPreference, Reduce, a duration scale),
and `MotionClock` takes both, through `set_preference` and `set_system_motion`.
Two public types with one name forced every consumer to qualify both, and the
name hid the distinction that matters: the platform type reports what the user
prefers, the animation type decides whether the application follows it.

## Decision

The application setting is `flui_animation::MotionOverride`, re-exported by
`flui-runtime` and `flui-app` under that name. Its variants keep their meaning:
`FollowSystem` resolves the host preference as ADR-0195 describes, `Reduce` and
`Full` override it. The setters follow the type: `MotionClock::set_override`,
`UiRuntime::set_motion_override`, `AppConfig::with_motion_override` and the
`motion_override` configuration field. The platform type keeps its name, which
mirrors the platforms' own "preference" vocabulary.

Every other part of ADR-0195 is unchanged.

## Consequences

- A breaking rename for applications that set the policy; no behaviour changes.
- Code that names both types no longer needs to qualify either.
