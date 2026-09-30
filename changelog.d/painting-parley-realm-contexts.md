### Added

- **`flui-painting`**: `FontCollection`, the app's shared, add-only font collection, and
  `TextContext`, one realm's text service built from it. A context shapes a
  `parley_text::ParagraphSpec` into a `ParagraphLayout` through `&mut`, with no FLUI lock, and
  `FontCollection::register_font` adds a face every context sees, including ones built earlier.
  `ParagraphSpec::direction` only aligns lines: Parley 0.11.1 takes the bidi base direction from
  the text ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §10 step 2a).

### Changed

- **`flui-painting`**: clippy's `disallowed_types` rejects a `Mutex` or `RwLock` in the crate
  outside the cosmic-text font system.
