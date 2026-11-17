### Added

- `SystemPreferences::accent_color` reports the accent colour the user chose, as a checked sRGB
  `SrgbColor` resolved for the appearance the host reports beside it, on Windows, macOS and Linux
  desktops that publish it through the Settings portal (GNOME 47, KDE Plasma 6); iOS, Android and
  the Web report none. `MediaQueryData::accent_color` and `MediaQuery::accent_color_of` carry it
  to widgets.
- `SystemPreferences::scroll_bars` reports whether scroll bars always show (`ScrollBarVisibility`)
  and what a click in the track does (`ScrollTrackClick`): Windows' "Always show scrollbars",
  macOS's "Show scroll bars" and "Click in the scroll bar to", GNOME's overlay scrolling.
  `MediaQueryData::scroll_bars` and `MediaQuery::scroll_bars_of` carry it to widgets.
- Input and native recordings carry both preferences (agent protocol 0.23).

### Changed

- A `Scrollbar` whose author did not call `thumb_visibility` shows its thumb at rest where the
  system asks for scroll bars that always show; where the system asks a click in the track to
  jump, a press on the track moves the thumb under the pointer and moving on drags it. Elsewhere
  it fades and pages as before.
- A `CupertinoTheme` without an authored primary colour takes the system's accent where the host
  reports one and the theme's brightness is the platform's, with white or black on it, whichever
  contrasts more; elsewhere it keeps system blue.

### Fixed

- On Windows, a preference Windows cannot read no longer discards the others: each value is read
  alone, and a failed read is retried a bounded number of times, 0.1 to 3.2 s apart, instead of
  every 100 ms for good.
- On Linux, a Settings portal that changes hands to a backend without a setting no longer leaves
  the old value published.
