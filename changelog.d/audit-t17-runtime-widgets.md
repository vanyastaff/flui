### Added

- `flui_widgets::app_root(view)`: wraps a root in the catalog's standard keyboard bindings
  (`DefaultFocusTraversal`: Tab, Shift+Tab, activation, Back and the clipboard chords). Every
  `flui-app` runner calls it for each window it installs, and `flui_testing::HeadlessHost::attach`
  and `attach_to` call it for each root they attach.
- `flui_view::environment::FocusTreeRoot`, which publishes a presentation's root focus scope.
  The UI runtime installs it above each root; `FocusTreeRoot::maybe_of` reads the root node.

### Changed

- `flui-runtime` no longer depends on `flui-widgets`: the UI runtime installs only
  `flui_view::environment` scopes and no key bindings.

### Removed

- `flui_widgets::FocusRoot` and `FocusRootState`. A tree mounted on a bare binding wraps its root
  in `FocusTreeRoot::new(app_root(view))`.
