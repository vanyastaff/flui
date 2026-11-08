### Changed

- Pointer, gesture and hit-test contracts now have their canonical facade and SDK paths under `interaction`; painting, layout-delegate, scroll-state and semantics contracts are under `painting` or `rendering`. Package-only render-catalog configuration is under the SDK's Evolving `pipeline` module. The widgets prelude keeps its curated convenience imports directly from those owners.

### Removed

- Removed cross-layer support re-exports from the `flui-widgets` root and catalog modules, including pointer/gesture/semantics contracts, layout delegates, route values, animation futures and render-catalog configuration. Import them from their owning facade or SDK modules instead; catalog-owned views and the curated widgets prelude are unchanged.
- Moved `flui::rendering::{CursorIcon, HitTestBehavior}` and their SDK counterparts to `interaction`.
- Removed the public `SemanticsScrollUnit` and `SemanticsTextDirection` aliases. Use `rendering::ScrollUnit` and `rendering::TextDirection`; private import aliases remain available for disambiguation inside implementations.
