### Added

- `RenderObjectContext::collects_semantics` says whether the presentation collects its semantics tree; `WidgetsBinding::set_collects_semantics` and `BuildOwner::set_collects_semantics` record it, and a change pushes every render view's configuration again.
- `flui_testing::HeadlessHost::disable_semantics` detaches assistive technology from the window, as `enable_semantics` attaches it.

### Changed

- A `Semantics` registers the route of its actions only while its window collects semantics (assistive technology is attached, or an agent or a test holds the tree). A window nobody reads through mounts a node with actions without the route; turning semantics on publishes the same tree as before, with the same actions.
