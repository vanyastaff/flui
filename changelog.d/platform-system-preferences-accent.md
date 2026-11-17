### Added

- `SystemPreferences::accent_color` reports the accent colour the user chose, as a checked sRGB
  `SrgbColor`, on Windows, macOS, Linux desktops that publish it through the Settings portal
  (GNOME 47, KDE Plasma 6) and browsers that know the `AccentColor` system colour; iOS and
  Android report none. `MediaQueryData::accent_color` and `MediaQuery::accent_color_of` carry it
  to widgets.
- `SystemPreferences::scroll_bars` reports whether scroll bars always show (`ScrollBarVisibility`)
  and what a click in the track does (`ScrollTrackClick`): Windows' "Always show scrollbars",
  macOS's "Show scroll bars" and "Click in the scroll bar to", GNOME's overlay scrolling.
  `MediaQueryData::scroll_bars` and `MediaQuery::scroll_bars_of` carry it to widgets.
- Input and native recordings carry both preferences (agent protocol 0.20).

### Changed

- A `Scrollbar` whose author did not call `thumb_visibility` shows its thumb at rest where the
  system asks for scroll bars that always show, and a tap on its track moves the thumb to the
  spot tapped where the system asks for that; elsewhere it fades and pages as before.
- A `CupertinoTheme` without an authored primary colour takes the system's accent colour where
  the host reports one, and system blue elsewhere.
