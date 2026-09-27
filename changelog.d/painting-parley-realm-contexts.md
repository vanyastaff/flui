### Added

- **`flui-painting`**: `FontCollection`, the app's shared, add-only font collection, and
  `TextContext`, one realm's text service built from it. Behind the `parley` feature a context
  shapes a `parley_text::ParagraphSpec` into a `ParagraphLayout` through `&mut`, with no FLUI
  lock, and `FontCollection::register_font` adds a face every context sees, including ones built
  earlier. Without `parley` both types exist and hold nothing. No production caller yet; the
  cosmic-text path and its process font system are unchanged
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §10 step 2).

### Changed

- **`flui-painting`**: the `parley` feature now brings Parley as a normal optional dependency,
  and clippy's `disallowed_types` rejects a `Mutex` or `RwLock` in the crate outside the
  cosmic-text font system.
