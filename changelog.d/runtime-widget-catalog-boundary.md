### Added

- `flui_view::environment::FocusTreeRoot` establishes the presentation-local focus parent below the widget catalog. `flui_widgets::app_root` adds the standard keyboard policy; `flui-app` and `HeadlessHost` compose it for every attached root, including secondary windows.

### Changed

- `flui-runtime` no longer has a normal dependency on `flui-widgets`. Its `reach-forbid` rule keeps the frame runtime and its production dependency closure independent of the catalog; catalog views used by runtime tests remain dev-only.

### Removed

- `FocusRoot` and `FocusRootState` are removed. Use `FocusTreeRoot` for the focus-tree contract and `flui_widgets::app_root` for standard traversal, activation, dismissal and clipboard bindings.
