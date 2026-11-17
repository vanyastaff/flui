### Added

- `WindowCapabilities::reports_occlusion` (and `CapabilityParts::reports_occlusion`): whether a
  covering by other windows reaches `WindowState::occluded`. Native on macOS, on X11 while no
  compositing manager runs and on Wayland compositors with `xdg_toplevel` version 6; absent on
  Windows, iOS, Android and the Web, where an unoccluded snapshot does not mean "seen". The native
  recording carries it (protocol 0.19).
- `flui_platform::Backoff`: the doubling wait, from a first value up to a ceiling, that a surface
  showing nothing backs off on.

### Changed

- A Wayland window whose compositor stops answering its frame callbacks, without suspending it,
  is asked for frames less and less often (a one-second grace doubling to eight seconds) instead
  of once a second forever, and is paced again by the compositor's next answer.
- A window whose surface keeps withholding frames past the retry budget logs one `flui.pace`
  warning, `surface_withholds_frames`, saying whether its host can tell a covering.

### Fixed

- A late frame-callback answer on Wayland no longer releases a newer callback, which let a frame
  go before the compositor showed the one before it.
