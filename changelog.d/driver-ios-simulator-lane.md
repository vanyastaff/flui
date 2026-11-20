### Added

- `flui-driver`'s iOS Simulator lane, `lane::ios_simulator`: `ios_simulator::launch` installs an
  app on a booted simulator named by its UDID (`FLUI_IOS_SIMULATOR_UDID`), starts it with its
  agent's Unix socket and token as `SIMCTL_CHILD_` launch values, drives it over that socket and
  ends it with `simctl terminate`; `Lane::capture_window` reads the simulator's screen through
  `simctl io screenshot`. The lane sends no input: every touch, key, text, scroll and button is
  refused before anything is sent as `not_supported` naming `ios_simulator_hid`, with the Xcode
  and runtime, until a supported channel exists. A session holds its simulator (`SimulatorLock`), and the lane ends only its own
  app's running process. `testing::ScriptedXcrun` scripts the Simulator for tests.
- `Launch::window_insets` opens the headless window with a safe area or a keyboard covering it, as
  a native host reports them.

### Changed

- `Driver::launch` of an `Artifact::AppBundle` answers `not_supported` naming `app_bundle_launch`,
  pointing at `lane::ios_simulator::launch`, instead of `apple_lane`.

### Security

- The Android lane's `Started` leaves the launch token out of its `Debug` form.
