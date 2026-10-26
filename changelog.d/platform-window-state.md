### Added

- `WindowState` holds every fact of the window: `metrics` (the drawable area and scale; a report
  that is not a drawable area keeps the last), `appearance`, `safe_area`, `execution`, `hovered`
  and `capabilities`, with a `with_*` builder each. Every host publishes them in one revision and
  raises `on_state_changed` before the fact's own callback.
- `Availability::is_available` and `WindowCapabilities::availability`.
- `flui_testing::window_conformance` checks that the snapshot agrees with what the window system
  reports: a `WindowFixture` reads it back (`Readback`).

### Changed

- `WindowCapabilities` fields are `Availability` instead of `bool`: `Emulated` marks a command the
  host builds itself (the Win32 fullscreen); `permits` stays and answers whether a command is
  available at all.
- A Win32 window's snapshot is not visible while a cloak or a locked session hides it, as its
  visibility callback already reported.
