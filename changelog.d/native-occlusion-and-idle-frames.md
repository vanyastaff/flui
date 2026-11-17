### Added

- `WindowCapabilities::reports_occlusion` (and `CapabilityParts::reports_occlusion`): whether a
  covering by other windows reaches `WindowState::occluded`. Native on macOS and X11, emulated on
  Windows, absent on Wayland, iOS, Android and the Web, where an unoccluded snapshot no longer
  reads as "seen". The native recording carries it (protocol 0.19).

### Changed

- A Windows window that other windows cover now reads occluded (still shown), so it draws no
  frames until it is uncovered; it reads seen again as soon as the desktop uncovers it.
- A Wayland window whose compositor stops answering its frame callbacks, without suspending it,
  is asked for frames less and less often (a one-second grace doubling to eight seconds) instead
  of once a second forever, and is paced again by the compositor's next answer.
- A window whose surface keeps withholding frames past the retry budget logs one `flui.pace`
  warning, `surface_withholds_frames`, saying whether its host can tell a covering.

### Fixed

- A late frame-callback answer on Wayland no longer releases a newer callback, which let a frame
  go before the compositor showed the one before it.
