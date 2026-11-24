### Added

- **The iOS Simulator lane reads the app as VoiceOver on iOS does** (`flui-driver`): on macOS
  it reads the iOS app's accessibility elements through the Mac's accessibility API on the
  Simulator's window (Simulator.app, or DeviceHub from Xcode 27), matches them to the app's own
  tree by role, label, state and bounds, and presses an element through `AXPress`
  (`lane::ios_simulator::accessibility`, `tree`, `roles`; `IosSimulatorLane::read_through`). The
  lane reports `AccessibilityApi::SimulatorAx`, and the `roles` and `accessibility` scenarios run
  through it. `testing::ScriptedSimulatorScreen` scripts the Simulator's window.

### Changed

- **`Lane::invoke` takes the app's tree** (`flui-driver`), as `Lane::read_tree` does, so a lane
  that matches the OS tree to the app's finds the element to activate; `Window::os_invoke` hands
  it over when the lane's API matches against it.
