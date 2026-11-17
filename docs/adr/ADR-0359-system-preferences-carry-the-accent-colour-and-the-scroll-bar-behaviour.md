# ADR-0359: System preferences carry the accent colour and the scroll-bar behaviour

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md) (the host owns the
  observations, their revision and their delivery; this record adds two fields under its rules),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (which upstream types a stable
  signature may name),
  [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (a design system reads the platform
  through one channel)

## Context and problem statement

`SystemPreferences` described text, motion, contrast, haptics, languages, regional formats,
gestures and the wheel, and nothing about colour or scroll bars. A user who picks an accent in
System Settings, Windows' Personalization or GNOME 47 saw FLUI's controls in the design system's
fixed blue; a macOS user who set "Show scroll bars: Always", or a Windows user who turned on
"Always show scrollbars", got bars that fade, and a macOS user who set "Click in the scroll bar
to: Jump to the spot that's clicked" got paging. Other toolkits read these settings (AppKit and
WinUI natively; Raven reads Windows' accent palette and GNOME's `accent-color`, Rune reads
`controlAccentColor` and the scroller style). The question is how the two settings enter the
host-wide snapshot, what each host reads, and which widget follows them.

## Decision drivers

- The host owns every system observation and delivers it as one revision (ADR-0172); a field a
  host cannot read is `None`, never a guessed default.
- A value from the OS is checked at the seam: a colour is finite, in a stated colour space, with
  its alpha.
- `flui-platform` sits below `flui-painting`, so the contract cannot name painting's `Color`.
- No polling: a change arrives by the OS's own notice, or at the next sample another notice starts.
- Every host either reads a setting or says it does not, with a row in the parity table.
- A new public field has a consumer in the same change, and no new widget family starts.

## Considered options

1. One accent colour as a checked sRGB value, and scroll-bar visibility and track click as two
   closed vocabularies resolved by each host; `MediaQuery` carries both, `Scrollbar` and the
   Cupertino theme follow them.
2. The accent as a palette: Windows' eight `AccentPalette` shades, or Android's Material You
   tonal palettes.
3. Each design system reads the OS itself through a capability of its own.
4. The raw settings as the hosts spell them (macOS's `AppleShowScrollBars` string, Windows'
   `AutoHideScrollBars`, GNOME's `overlay-scrolling`) in the snapshot, resolved by the widget.

Option 2 carries what most hosts do not have: macOS, the portal and the browser give one colour,
and the shades Windows derives are its own tonal policy, which a design system would derive again
in its own way. Option 3 gives every design system its own observer of one setting, which
ADR-0172 removed for every other preference. Option 4 makes every consumer know every host's
spelling, and macOS's "automatically" needs the pointing device the host already resolves through
`NSScroller.preferredScrollerStyle`.

## Decision

Chosen option: 1.

### §1 The accent colour

`SystemPreferences::accent_color() -> Option<SrgbColor>`. `SrgbColor` holds red, green, blue and
alpha as `f32`, sRGB-encoded with straight alpha, each finite and within `0..=1`;
`SrgbColor::try_new(f64, f64, f64, f64)` refuses anything else as `InvalidPreference::Color`
(no clamping) and reads negative zero as zero, and `SrgbColor::from_rgba8` takes 8-bit channels.
A host converts the system's colour into sRGB before it builds the value, so a wide-gamut colour
arrives clipped to sRGB rather than misread.

### §2 Scroll-bar behaviour

`SystemPreferences::scroll_bars() -> &ScrollBarPreferences`, two independent observations:

- `visibility: Option<ScrollBarVisibility>`, `WhenScrolling` (an overlay scroller that fades
  once the content rests) or `Always`, as the host resolves its own setting;
- `track_click: Option<ScrollTrackClick>`, `Page` or `JumpToSpot`.

Both enums are `#[non_exhaustive]`.

### §3 What each host reads

| Host | Accent colour | Visibility | Track click |
|------|---------------|------------|-------------|
| Windows | `UISettings.GetColorValue(Accent)` | "Always show scrollbars" (`UISettings.AutoHideScrollBars`) | none: Windows pages, Shift-click jumps, and has no setting |
| macOS | `NSColor.controlAccentColor` in `sRGBColorSpace` | `NSScroller.preferredScrollerStyle` | `AppleScrollerPagingBehavior` (absent: page) |
| Wayland, X11 | the Settings portal's `org.freedesktop.appearance accent-color` | GNOME's `overlay-scrolling` through the portal | none: GTK's `gtk-primary-button-warps-slider` is an XSETTING the portal does not carry |
| Web | the computed CSS system colour `AccentColor` | none | none |
| iOS | none: the tint belongs to each application | none | none |
| Android | none (§4) | none | none |
| Headless | what the test sets | what the test sets | what the test sets |

A portal `accent-color` with a component outside `0..=1` is the portal specification's "unset":
no accent, which clears one accepted before. A browser that does not know `AccentColor` refuses
the declaration; the host reads that as no observation, not as the inherited colour.

### §4 Android reports no accent

Material You's dynamic colour is five tonal palettes, and the colour a control takes from them
depends on the role and on light or dark (`system_accent1_600` for a light primary,
`system_accent1_200` for a dark one). One accent picked from them would be FLUI's guess at a role,
not the user's setting. The consumer that would read Android's palette is a dynamic colour scheme
built from all of it, a preference of its own; until one exists, `accent_color` is `None` there.

### §5 Delivery

Each source follows ADR-0172's rules and adds no mechanism. Windows registers
`ColorValuesChanged` and `AutoHideScrollBarsChanged` beside its text-scale and animation events,
and its broadcast receiver hears `WM_DWMCOLORIZATIONCOLORCHANGED` beside `WM_SETTINGCHANGE`; an
interface an older Windows lacks is no observation. macOS observes
`NSSystemColorsDidChangeNotification` and `NSPreferredScrollerStyleDidChangeNotification`; the
paging default posts nothing and is read again when the application is active again, as the
double-click interval is. The Linux watch reads the two keys with the others and retries them on
the same bounded schedule, whose pauses come from `flui_platform::Backoff`. The browser announces
no accent change, so the web host reads the accent at every sample another notice starts.

### §6 Consumers

`MediaQueryData` gains `accent_color: Option<Color>` (painting's colour, converted once by the
runtime) and `scroll_bars: ScrollBarPreferences`, with the field-scoped readers
`MediaQuery::accent_color_of` and `MediaQuery::scroll_bars_of`.

- `Scrollbar::thumb_visibility` is optional: unset, the thumb shows at rest where the system asks
  for `Always` and only while scrolling otherwise; an authored value wins. A tap on the track,
  off the thumb, moves the thumb's middle to the spot at once where the system asks for
  `JumpToSpot`, and pages otherwise.
- An unset `CupertinoThemeData` primary colour resolves to the accent when the host observes one,
  as AppKit tints its controls, and to system blue otherwise; an authored primary wins.

### Consequences

- Good, because a user's accent and scroll-bar settings reach FLUI's own scroll bars and the
  Cupertino controls on every host that has them, live, without a restart.
- Good, because the recordings carry both fields, so a replay sees what the run saw.
- Bad, because a Material application does not follow the accent yet: a Material colour scheme
  seeded from it is a design decision of its own.
- Bad, because the web host shows an accent change only at its next sample.
- Neutral, because Windows' derived accent shades stay Windows' policy; a design system derives
  its own.

### Confirmation

- `an_accent_colour_enters_only_as_finite_srgb_in_range` and
  `scroll_bar_observations_stay_apart_and_unknown_until_read` in `flui-platform`'s
  `preferences_contract`; `preferences_round_trip_through_their_parts`.
- Windows: `windows_reads_preferences_before_a_user_window_exists` compares the snapshot with
  `UISettings`' own answers; `a_colorization_change_wakes_the_owner_without_a_user_window`.
- Linux: `the_portal_accent_is_a_colour_in_range_and_unset_outside_it`,
  `overlay_scrolling_is_scroll_bars_that_show_while_scrolling`, and the stand-in portal cases
  `accent-color` and `overlay-scrolling` of `linux_preferences`.
- Web: `a_computed_colour_reads_back_only_in_srgb`.
- Recordings: `preference_rows_keep_their_json_spelling`,
  `every_vocabulary_variant_round_trips_through_the_recording_dto`.
- Consumers: `a_track_tap_jumps_to_the_spot_where_the_system_asks` and
  `an_unset_thumb_follows_the_systems_scroll_bar_visibility` in `flui-widgets`' `scrollbar`
  table; `an_unset_primary_follows_the_system_accent` in `flui-cupertino`.
- Native runs owed on each host are notes of the "Accent colour" and "Scroll-bar behaviour" rows
  of [`docs/platform-parity.md`](../platform-parity.md#system-preferences).
