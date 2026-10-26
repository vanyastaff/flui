### Changed

- **Owner-thread traits drop `Send + Sync`** (`flui-view`, `flui-foundation`): `Notification` and
  `ViewKey` (with `ValueKey<T>`) no longer require `Send + Sync`. Notifications dispatch and keys
  reconcile on the owner thread, so a payload or key may now hold an `Rc`.
- **`RendererBinding` lends render views** (`flui-rendering`, `flui-runtime`):
  `with_render_view(id, visit)` runs a closure on the view in place of
  `render_view(id) -> Arc<RwLock<RenderView>>`, and `insert_render_view`,
  `remove_render_view_by_id` and `add_render_view_with_config` take and return `RenderView` by
  value. A view lent to a visitor that panics stays in the binding.
- **Build-during-layout cells are owner-local** (`flui-objects`, `flui-view`):
  `LayoutConstraintsCell` and `HeaderShrinkCell` keep their state in a `Cell` and are shared as
  `Rc` (`RenderLayoutBuilder::new`, `set_shrink_cell`), so `perform_layout` publishes without a
  lock; `BuildDuringLayoutCell` no longer requires `Send + Sync`.

### Removed

- **`ElementBuildContext::tree` and `ElementBuildContext::build_owner`** (`flui-view`): build code
  no longer gets the element tree's or build owner's `Arc<RwLock<…>>`, so it cannot hold one's
  guard across a call that locks the other.

### Fixed

- **Headless frame requests ran on the requesting thread** (`flui-native`):
  `MockWindow::request_redraw` called from another thread now queues the frame callback for the
  owner's next turn, coalesced, instead of running owner code on the caller's thread.
