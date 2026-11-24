### Fixed

- **The iOS Simulator lane's window and read** (`flui-driver`): a window is taken by an
  allow-list (`tree::choose_window`, which the scripted window goes through too): the one window
  naming the device's UDID, whatever the others name, else the one naming no UUID while no other
  listed simulator (in any state, available or not, in the default device set) shares its title,
  and never one naming any other UUID. Every run of a UUID's 8-4-4-4-12 form in the window's
  attributes is read as a UUID, whatever stands beside it: the device's UDID anywhere names the
  device, and any other run names another, so a UUID glued to the text around it never reads as
  naming none. The device is looked up in the listing read before each read by its UDID, and one
  renamed or removed since the lane connected is refused. A level of the screen's elements that
  the node budget or a vanished element cuts is marked unread on its parent, the screen included,
  and a probe past the depth that fails fails the read. A screen search that a group gone or its
  budget left partial refuses to take a plain group of the window's shape, since the marked
  screen may sit in the part not read; a level of exactly the budget's children is read whole. A
  press on a read that stopped short is refused, since one match there cannot be shown to be the
  only one. A press that was sent and not answered, or whose element went away, keeps
  `may_have_run` when the Accessibility permission is gone afterwards. A timeout names the
  deadline of the element its call was asked of, a press's included, or says that element
  refused it.

### Changed

- **The accessibility walks mark a vanished element's level cut** (`flui-driver`): on Windows,
  macOS and Linux, a step to a next child that finds an element gone (the parent, or the sibling
  the step is asked from) leaves the parent's children marked unread (`children_unread`) and the
  read truncated (`Tree::truncated`, whose remedy then is to read again), where the list was
  ended as whole, and a search that meets one says so.
