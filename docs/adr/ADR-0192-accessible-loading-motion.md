# ADR-0192: Loading feedback remains visible when motion stops

- **Status:** Accepted
- **Date:** 2026-10-10
- **Supersedes:** [ADR-0195](ADR-0195-presentation-motion-policy.md), loading indicator behavior only.
- **Related:** [ADR-0179](ADR-0179-controller-registration-ownership.md), [ADR-0191](ADR-0191-single-parent-animation-registries.md).

## Context

Loading conveys ongoing work, but rotation is not required to convey that state.
Preserving every loading repeat bypasses the user's motion preference. Merely
parking its controller is insufficient: the linear indicator's initial animated
frame contains no visible bar. An arbitrary animated frame also need not read as
unknown-total feedback.

[WCAG 2.2.2](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html)
requires an operable pause, stop or hide mechanism for qualifying automatic motion
alongside other content; essential motion is a contextual exception.
[WCAG 2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html)
is an additional AAA target for disabling nonessential interaction-triggered motion.
[WCAG2ICT](https://www.w3.org/TR/wcag2ict-22/) supplies informative A/AA guidance
for native software. These are specific design criteria, not a claim of whole-app
WCAG conformance or certification.

## Decision

`ActivityIndicator`, `CupertinoActivityIndicator` and indeterminate
`LinearProgressIndicator` use Normal repeating controllers. The existing registry
policy parks them under Reduce and resumes them under Full; it remains the sole
timing authority. Physical inertia and display/press-delay timers retain Preserve.
`RefreshIndicator` inherits the visual indicator's behavior without changing its
refresh completion protocol.

Indicators select visible static feedback during build when motion is reduced,
their controller is unbound, or enclosing declarative ticker modes disable motion.
The circular indicator paints its initial nonempty arc; Cupertino paints eight
initial ticks. Material paints one centered segment spanning 37.5% through 62.5%
of the track in both directions. Its semantics remain unknown-total loading, with
no percentage. Determinate progress still paints and announces its actual fraction.

`TickerMode::is_enabled` establishes a build dependency on the effective configured
permission: ancestor permission AND local permission, defaulting to true without
a mode. A stable private inherited wrapper carries that observation across enabled,
disabled, detached and bound states, preserving descendants and keyed state.
Registry topology continues to enforce tick delivery. This query is not proof of
time advancing: raw registry muting, an independently supplied registry, reduced
motion and clock playback remain separate controls.

Static painters hold no controller or repaint subscription. Repaint equality
includes representation changes. Registration ownership remains with the widget,
so re-enabling or changing policy resumes the existing controller; unmount releases
it. Full-mode tracks retain their original timing and empty cycle seam. Substituting
a fixed segment at that seam would introduce an unrelated visual pulse.

The motion lab provides a keyboard-operable Pause activity / Resume activity
button around its indicator's ticker mode. Pause persists after focus leaves the
button. Resume honors reduced motion; the ticker mode's existing catch-up semantics
apply under Full. Application authors must provide controls for their qualifying
automatic content; a library's reduced-motion support alone cannot establish that
an application satisfies WCAG 2.2.2.

## Alternatives and evidence

Keeping Preserve ignores the adopted preference. Duplicating pause/run state in
each widget would duplicate registry policy and lifecycle obligations. Replacing
every empty animated frame would change ordinary Full animation. Static
representation plus the existing Normal policy separates visual feedback from
timing without a second driver.

The public producer tests `activity_indicator_arc_follows_keyframes`,
`cupertino_activity_indicator_ticks_step` and
`linear_progress_indeterminate_keyframes` cover initial/live Reduce, visible
painting, unknown-total semantics and Full resumption. Material also covers
disabled/detached scopes, nested permission, registry replacement and switches to
actual determinate progress. `keyed_indicator_moves_between_configured_permissions_without_restarting`
pins controller transfer and retained phase. Cupertino's family also checks
disabled/detached feedback, binding, resumption and unmount.
`refresh_indicator_spins_while_refreshing`
checks visible feedback and cleanup under both policies.
`motion_lab_buttons_drive_independent_property_deadlines_and_reduce` drives the
actual example's Tab/Enter control, retained focus, persistent pause and resumption.
These headless producer observations do not establish native assistive-technology
announcements, platform preference delivery on every backend, or visual GPU output.
