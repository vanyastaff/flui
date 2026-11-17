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

The accent a host publishes is the colour resolved for the appearance it publishes beside it:
where the system's accent depends on light or dark and on increased contrast (AppKit's catalog
colours), the host resolves it under the application's appearance and reads it again when that
appearance or the contrast setting changes, so one revision carries the accent and the appearance
it belongs to. Where the accent does not depend on them (Windows' `Accent`, the portal's
`accent-color`), the user's colour is published as it is. An accent a host cannot read is no
observation; it does not fail the rest of the snapshot.

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
| macOS | `NSColor.controlAccentColor` under the application's effective appearance, in `sRGBColorSpace` | `NSScroller.preferredScrollerStyle` | `AppleScrollerPagingBehavior` (absent: page) |
| Wayland, X11 | the Settings portal's `org.freedesktop.appearance accent-color` | GNOME's `overlay-scrolling` through the portal | none: GTK's `gtk-primary-button-warps-slider` is an XSETTING the portal does not carry |
| Web | none: a browser resisting fingerprinting resolves the CSS `AccentColor` to a fixed colour a page cannot tell from the user's | none | none |
| iOS | none: the tint belongs to each application | none | none |
| Android | none (§4) | none | none |
| Headless | what the test sets | what the test sets | what the test sets |

A portal `accent-color` with a component outside `0..=1` is the portal specification's "unset":
no accent, which clears one accepted before. The web host reads no accent until a run in each
browser shows which ones resolve `AccentColor` to the user's colour; that run is owed in the parity
table.

### §4 Android reports no accent

Material You's dynamic colour is five tonal palettes, and the colour a control takes from them
depends on the role and on light or dark (`system_accent1_600` for a light primary,
`system_accent1_200` for a dark one). One accent picked from them would be FLUI's guess at a role,
not the user's setting. The consumer that would read Android's palette is a dynamic colour scheme
built from all of it, a preference of its own; until one exists, `accent_color` is `None` there.

### §5 Delivery

Each source follows ADR-0172's rules. Every value of a snapshot is read alone: a getter that
fails keeps the value accepted before it and owes a retry, and the others stand. A read that
fails or leaves a value unread is retried on a bounded schedule of `flui_platform::Backoff`
pauses (`Retries`: Windows 0.1 to 3.2 s, six retries; the Linux watch 1 to 16 s, five), then not
until the system next reports a change; nothing polls.

Windows registers `ColorValuesChanged` and `AutoHideScrollBarsChanged` beside its text-scale and
animation events, and its broadcast receiver hears `WM_DWMCOLORIZATIONCOLORCHANGED` beside
`WM_SETTINGCHANGE`; an interface an older Windows lacks is no observation. macOS observes
`NSSystemColorsDidChangeNotification`, `NSPreferredScrollerStyleDidChangeNotification` and the
distributed `AppleInterfaceThemeChangedNotification` (a switch between light and dark), beside
the accessibility display options it already observes; the paging default posts nothing and is
read again when the application is active again, as the double-click interval is. The Linux
watch reads the two keys with the others. A key the portal answers it does not have keeps its
value while the same portal answers, and withdraws it once the portal changed hands or the watch
connected anew: the backend answering now is the one to believe (for GNOME's `clock-format`, the
environment's hour cycle stands again).

### §6 Consumers

`MediaQueryData` gains `accent_color: Option<Color>` (painting's colour, converted once by the
runtime) and `scroll_bars: ScrollBarPreferences`, with the field-scoped readers
`MediaQuery::accent_color_of` and `MediaQuery::scroll_bars_of`.

- `Scrollbar::thumb_visibility` is optional: unset, the thumb shows at rest where the system asks
  for `Always` and only while scrolling otherwise; an authored value wins. Where the system asks
  for `JumpToSpot`, a press on the track moves the thumb's middle under the pointer at once and
  moving on drags the thumb from there, as AppKit's scroller does; otherwise a tap on the track
  pages.
- An unset `CupertinoThemeData` primary colour resolves to the accent when the host observes one
  and the theme's brightness is the platform's, as AppKit tints its controls. The accent is
  resolved for the platform's appearance (§1), so a theme forced to the other brightness keeps the
  dynamic system blue with its dark and high-contrast variants, as it does without an accent. An
  unset contrasting colour on the accent is white or black, whichever contrasts more with it
  (`Color::contrasting_text_color`). An authored primary or contrasting colour wins.

### Consequences

- Good, because a user's accent and scroll-bar settings reach FLUI's own scroll bars and the
  Cupertino controls on every host that has them, live, without a restart.
- Good, because the recordings carry both fields, so a replay sees what the run saw.
- Bad, because a Material application does not follow the accent yet: a Material colour scheme
  seeded from it is a design decision of its own.
- Bad, because a web application follows no accent until a browser run shows which browsers
  report the user's.
- Neutral, because Windows' derived accent shades stay Windows' policy; a design system derives
  its own.

### Confirmation

- `an_accent_colour_enters_only_as_finite_srgb_in_range` and
  `scroll_bar_observations_stay_apart_and_unknown_until_read` in `flui-platform`'s
  `preferences_contract`; `preferences_round_trip_through_their_parts`.
- Windows: `windows_reads_preferences_before_a_user_window_exists` compares the snapshot with
  `UISettings`' own answers; `a_colorization_change_wakes_the_owner_without_a_user_window`;
  `failed_reads_retry_on_a_doubling_schedule_and_stop`,
  `an_unreadable_value_keeps_the_others_and_owes_a_retry` and `every_settings_event_is_observed`
  in `native_preference_read_recovery`.
- Linux: `the_portal_accent_is_a_colour_in_range_and_unset_outside_it`,
  `overlay_scrolling_is_scroll_bars_that_show_while_scrolling`,
  `a_missing_key_withdraws_its_field_only_after_the_portal_changed_hands`, and the stand-in portal
  cases `accent-color`, `overlay-scrolling` and `accent-withdrawn` of `linux_preferences`.
- The schedule: `a_retry_schedule_doubles_and_runs_out`, `the_watchs_retries_are_bounded`.
- Recordings: `preference_rows_keep_their_json_spelling`,
  `every_vocabulary_variant_round_trips_through_the_recording_dto`,
  `platform_recordings_replay_on_the_headless_host` (each new field in its own spelling) and
  `native_recordings_write_back_byte_for_byte`.
- Consumers: `a_track_tap_jumps_to_the_spot_where_the_system_asks` and
  `an_unset_thumb_follows_the_systems_scroll_bar_visibility` in `flui-widgets`' `scrollbar`
  table; `an_unset_primary_follows_the_system_accent` and
  `a_forced_brightness_or_a_light_accent_keeps_the_theme_legible` in `flui-cupertino`.
- Native runs owed on each host are notes of the "Accent colour" and "Scroll-bar behaviour" rows
  of [`docs/platform-parity.md`](../platform-parity.md#system-preferences).
