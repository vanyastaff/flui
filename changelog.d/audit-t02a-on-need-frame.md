### Changed

- **`WidgetsBinding::set_on_need_frame` takes an owner-local callback** (`flui-view`): the callback
  no longer needs `Send + Sync`, so it may capture an `Rc`. It runs with no borrow of the binding
  held, so it may replace itself.
- **`ElementBuildContext` moved to `flui_view::testing`** (`flui-view`, behind the `testing`
  feature): `ElementBuildContext::for_element` takes the tree and owner as `Rc<RefCell<…>>` in
  place of `Arc<RwLock<…>>`.

### Deprecated

- **`flui_view::testing::ElementBuildContext`** (`flui-view`): a context detached from
  `BuildOwner::build_scope` records dependencies the production context never records. Read the
  `BuildContext` inside a view's `build` instead.

### Removed

- **`ElementBuildContextBuilder` and `ElementBuildContext::new`** (`flui-view`): use
  `ElementBuildContext::for_element`.

### Fixed

- **A frame-request callback that replaced itself hung** (`flui-view`): calling
  `WidgetsBinding::set_on_need_frame` from inside the callback took the lock the call already
  held; the callback now runs with nothing held.
