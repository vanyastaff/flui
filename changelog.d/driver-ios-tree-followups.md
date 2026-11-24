### Fixed

- **The iOS Simulator lane's window and read** (`flui-driver`): a window is taken by an
  allow-list (`tree::choose_window`, which the scripted window goes through too): the one window
  naming the device's UDID, whatever the others name, else the one naming no UUID while no other
  listed simulator (in any state, available or not, in the default device set) shares its title,
  and never one naming any other UUID, found wherever it stands in the window's attributes; the
  device is looked up in the listing read before each read by its UDID, and one renamed or
  removed since the lane connected is refused; a level of the screen's elements that the node
  budget or a vanished element cuts is marked unread on its parent, the screen included, a probe
  past the depth that fails fails the read, and a group gone mid-search, or a level past the
  search's bound, cuts the screen search short; a timeout names the deadline of the element its
  call was asked of, a press's included, or says that element refused it.

### Changed

- **The accessibility walks mark a vanished element's level cut** (`flui-driver`): on Windows,
  macOS and Linux, a step to a next child that finds an element gone (the parent, or the sibling
  the step is asked from) leaves the parent's children marked unread (`children_unread`) and the
  read truncated, where the list was ended as whole, and a search that meets one says so.
