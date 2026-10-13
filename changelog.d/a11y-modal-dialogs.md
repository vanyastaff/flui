### Added

- **Modal semantics** (`flui-semantics`, `flui-widgets`): `SemanticsFlag::IsModal`,
  `SemanticsConfiguration::set_modal` and `Semantics::modal` mark a node modal, published as
  AccessKit's `modal`. `NavigatorRoute::is_modal` (true for `PopupRoute`) makes a
  route's overlay entry hide the entries below it from semantics while they stay painted;
  `RenderTheater::set_semantics_blocked_count` carries it to the render tree.

### Fixed

- **Pages under a dialog were announced** (`flui-widgets`, `flui-material`): a screen reader could
  move from a dialog into the page behind it. Routes beneath a modal route are now hidden from
  assistive technology until it is dismissed, and Material's `Dialog` and `AlertDialog` publish
  one modal `Dialog`/`AlertDialog` node.
