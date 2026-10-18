# ADR-0212: Input behavior values resolve OS value, then platform preset, then subtree override, with accessibility timings as floors

- **Status:** Accepted (2026-10-13); implementation pending (docs/plans/specs/flui-interaction/completeness/tasks.md). Until it merges, `main` follows ADR-0172 and the current `GestureSettings` constructors.
- **Date:** 2026-10-13
- **Amends:** [ADR-0172](ADR-0172-host-owned-system-preferences.md): adds the haptics preference,
  per-display gesture values, accessibility timing floors and the resolution order. Its
  admission, delivery, retirement and snapshot rules are unchanged.
- **Amended-by:** [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) — decision 4 only: the
  refresh interval the haptic frame deadline multiplies is the presentation's refresh period it
  defines. The factor, the checked product and the 60 Hz fallback are unchanged.
- **Related:** [ADR-0210](ADR-0210-two-tier-haptics.md) (haptics suppression),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md) (the resolution-delay
  ceiling), [ADR-0195](ADR-0195-presentation-motion-policy.md); the
  [interaction completeness spec](../plans/specs/flui-interaction/completeness/design.md) §8, §12.

## Context

ADR-0172 makes the host the one owner of observed preferences and gives every new contact an
immutable settings snapshot. It leaves four gaps. There is no observed haptics setting, so a
user who turned system haptics off still gets FLUI feedback. Gesture geometry is projected for
"that presentation's DPI", but the OS defines it per display, and a window moved to another
display keeps the old values until a DPI change happens to fire. Nothing stops an authored scope
from shortening a long-press or double-click time the user lengthened for accessibility. And
the fallback values come from six ad hoc constructors (`for_platform`, `for_device`,
`android_defaults`, `ios_defaults`, `touch_defaults`, `mouse_defaults`) plus `Default`, whose
numbers differ from the platforms' own: Flutter's Android touch slop is 18 against AOSP's 8 dp
([best practices §2.4](../research/2026-10-12-interaction-input-best-practices.md)).

## Decision

1. **Haptics preference.** `SystemPreferences` gains an observed haptics setting,
   `haptics() -> Option<bool>`, alongside `high_contrast`. `None` means unobserved; the consumer
   policy then is enabled. `Some(false)` suppresses both haptic tiers (ADR-0210), including
   frame-queued requests resolved after the change. It is host-wide, not per window.
2. **Gesture values per display.** Geometry observations (touch slop, drag threshold,
   double-click rectangle) are held per `DisplayId` in the display's own pixel context. A
   presentation re-reads them when its display changes, when that display's scale changes, or
   when the host accepts a new observation. Timings stay host-wide. ADR-0172's rule holds:
   active sequences keep their admitted snapshot; only new sequences see the new values.
3. **Accessibility timings are floors.** The OS long-press (touch-and-hold) duration, double-tap
   and double-click intervals, and the hold that opens a `Sequenced` context menu are lower
   bounds. A preset or subtree override may lengthen them, never shorten them; a shorter
   authored value is raised to the floor, not refused, since it is valid on another machine.
   A floor exists only where the OS publishes the value: Android publishes its "touch & hold
   delay"; iOS Touch Accommodations has no public read API, so the long-press floor there is
   `None`. The resolution-delay ceiling (ADR-0208) is checked against authored and preset values, so
   raising a floor never turns a valid gesture tree into a composition error.
4. **Resolution order.** For every behavior value:

   ```text
   OS observation (SystemPreferences, when observed)
     → GestureSettings::preset(platform, pointer_kind)
         fallback: (platform, kind) → (platform, coarse | fine) → platform
     → explicit subtree override (GestureArenaScope settings, ADR-0172)
   floors from item 3 apply last
   ```

   A preset holds plain values only: numbers, and the remainder-handoff rule as a closed enum;
   no callbacks or estimator objects (velocity estimation stays consumer policy, ADR-0172). It
   covers slops, timeouts, the fling range, `min_target`, the spring-loading delay, the swipe
   edge width, the remainder-handoff rule, latch idle timeouts, `max_resolution_delay` and
   the factor for `haptic_frame_deadline` (ADR-0210). That factor is a platform-level value:
   it sits in the platform row at the root of the fallback chain, is read by platform alone
   (no pointer kind), and neither a kind row nor a subtree override (`GestureArenaScope`)
   applies to it, so every request on a presentation derives the same deadline. That deadline itself is not a pointer-kind preset: it lives in the presentation's metrics as factor × the current display's refresh
   interval, recomputed on a display change (a 60 Hz to 120 Hz move updates it for new
   requests; queued ones keep their snapshot), so keyboard, assistive and programmatic requests
   need no pointer kind. Where the presentation's refresh period is unknown (ADR-0236: the display reports
   none, or the window names no display), the metric uses a documented 60 Hz period (16.67 ms) until a period is
   observed, so the deadline is always defined (row
   `unknown_refresh_period_uses_the_60_hz_fallback`). The product factor × period is a checked
   multiplication (`Duration::checked_mul` over a finite factor); a period whose product
   overflows is treated as unknown, with a diagnostic, and the 60 Hz fallback applies (a case
   of the same row).
   Every number cites its source in its doc comment: AOSP `ViewConfiguration` for Android;
   documented Windows defaults, or an on-device measurement where none is published (Windows
   publishes no touch-slop constant); and an on-device measurement, recorded with the device
   and OS version, for Apple platforms, which publish no constants. The coarse class is touch;
   mouse and pen are fine; `Virtual` is coarse unless its preset row says otherwise. `preset`
   replaces the six constructors above.
5. **Headless tests choose a preset.** The headless harness takes the preset as an argument;
   nothing defaults to the host's platform, so a test's outcome does not depend on the OS that
   runs it.

## Consequences and verification

- Tests through public API: `behavior_values_resolve_os_then_preset_then_override` (one table,
  a row per value kind and source combination); `accessibility_timing_floors_hold_under_overrides`
  (an override below, at and above the floor; the ceiling check unaffected);
  `preset_fallback_chain` (a kind with its own row, a kind falling to its class, a platform with
  neither); `gesture_values_follow_the_presentation_display` in the runtime's owner-metrics
  contract (move between displays of equal and different scale, an active contact keeping its
  snapshot); `haptics_preference_suppresses_both_tiers`.
- CI executes all of these on the headless backend with synthetic observations. Reading the
  haptics setting and per-display geometry from a real OS runs on a native host only, as
  `preferences_contract` rows on Windows and through `cargo xtask device <os>-input` elsewhere;
  a backend that cannot read the haptics setting reports `None`.
- A changed preset number is a behavior change and needs its cited source in the same PR.

## Alternatives rejected

- **Flutter's single constants per platform.** They disagree with the platforms they name.
- **Letting an authored scope shorten an accessibility timing.** The user's setting exists
  because the default was too short for them.
- **A per-window haptics setting.** The OS setting is per user; a window-level switch is the
  subtree switch of ADR-0210.
