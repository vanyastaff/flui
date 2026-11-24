### Added

- **The iOS Simulator lane reads the app's accessibility tree** (`flui-driver`): on macOS it
  reads the iOS app's elements through the Mac's accessibility API on the Simulator's window
  (Simulator.app, or DeviceHub from Xcode 27), matches them to the app's own tree by role,
  label, state and bounds, and presses an element through `AXPress`
  (`lane::ios_simulator::accessibility`, `tree`, `roles`; `IosSimulatorLane::read_through`). The
  lane reports `AccessibilityApi::SimulatorAx`, and the `roles` and `accessibility` scenarios run
  through it. Apple documents none of the Simulator's bridge: the roles it gives each UIKit trait
  are inferred, whether it carries the app's identifier is unconfirmed, and no run on a Mac has
  pinned either yet, so the read stands for VoiceOver on iOS only as far as those runs confirm
  it. `testing::ScriptedSimulatorScreen` scripts the Simulator's window.
- **`AccessibilityApi::activates`** (`flui-driver`): whether a lane's API performs an element's
  action; UI Automator's does not.

### Changed

- **`Lane::invoke` takes the app's tree** (`flui-driver`), as `Lane::read_tree` does, so a lane
  that matches the OS tree to the app's finds the element to activate; `Window::os_invoke` hands
  it over when the lane's API matches against it and acts, and otherwise asks the lane without
  reading the app first.
