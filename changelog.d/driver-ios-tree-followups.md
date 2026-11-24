### Fixed

- **The iOS Simulator lane's window and read** (`flui-driver`): a window is taken by an
  allow-list, the device's UDID, or no UUID at all while no other listed simulator (in any
  state, available or not) shares its title, and never on any other UUID; the device is looked
  up in the listing read before each read by its UDID, and one renamed or removed since the lane
  connected is refused; a level of the screen's elements that the node budget or a vanished
  parent cuts is marked unread on its parent, the screen included, and a group gone mid-search
  cuts the screen search short; a timeout names the deadline the read set on each element, or
  says an element refused it.

### Changed

- **The macOS lane's accessibility walk marks a vanished parent** (`flui-driver`): a container
  that goes away after some of its children were read leaves them marked unread
  (`children_unread`) and the read truncated, where its list was ended as whole
  (`walk::ProviderError::ParentGone`, which `ax::child_at` answers for a parent gone).
