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
- **How the iOS Simulator lane takes its window, its screen and its element** (`flui-driver`).
  A window is taken by an allow-list that every host goes through (`tree::choose_window`): the
  one window naming the device's UDID, whatever the others name, else the one naming no UUID
  while no other simulator of the default device set, in any state, shares its title, and never
  one naming any other UUID. Every run of a UUID's 8-4-4-4-12 form in the window's
  `AXIdentifier` or `AXDocument`, read whole up to `accessibility::IDENTITY_TEXT_LIMIT`, is read
  as a UUID whatever stands beside it, and a longer text names something else. The device is
  looked up by its UDID in the listing read before each read, and one renamed or removed since
  the lane connected is refused. The screen search reads the window's chrome breadth first, then
  goes into each plain group of the app window's shape to look for an `iOSContentGroup` under it
  (`tree::find_screen`): an `iOSContentGroup` is taken wherever it was found, else the shallowest
  plain group, which a cut of the chrome (an element gone, or the budget, refused with no retry)
  refuses (`TreeError::SearchCut`); what cuts the search inside a plain group is that group's.
  The screen's walk and the search list children through one bounded listing
  (`accessibility::list_children`), a level of exactly the budget's children read whole. A level
  a vanished element or the budget cuts is marked unread on its parent, the screen included,
  and a probe past the depth that fails fails the read; a screen moved, resized or gone during
  the read answers `Error::Busy` (`tree::read_screen`, the Mac's read and the tests' over their
  own trees; the scripted window shares `choose_window` and `locate`), and an element gone
  anywhere in the read while the process is no longer trusted answers the Accessibility
  permission refused. A press refuses an
  element that two read as, or one on a read that stopped short; a press sent and not answered,
  or whose element went away, keeps `may_have_run` when the Accessibility permission is gone
  afterwards, as the macOS lane's `AXPress` does (`lane::ax::pressed`, the answer both lanes
  give); and a timeout names the deadline of the element its call was asked of, or says that
  element refused it.
- **The accessibility walks mark a vanished element's level cut** (`flui-driver`): on Windows,
  macOS and Linux, a step to a next child that finds an element gone (the parent, or the sibling
  the step is asked from) leaves the parent's children marked unread (`children_unread`) and the
  read truncated (`Tree::truncated`, whose remedy then is to read again), and a search that
  meets one says so.
- **`AccessibilityApi::activates`** (`flui-driver`): whether a lane's API performs an element's
  action; UI Automator's does not.

### Changed

- **`Lane::invoke` takes the app's tree** (`flui-driver`), as `Lane::read_tree` does, so a lane
  that matches the OS tree to the app's finds the element to activate; `Window::os_invoke` hands
  it over when the lane's API matches against it and acts, and otherwise asks the lane without
  reading the app first.
