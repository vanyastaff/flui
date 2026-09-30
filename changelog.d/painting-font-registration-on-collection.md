### Added

- **`flui::register_font`** and **`FontRegistrationError`** (`flui_app::register_font`): registers
  a font's faces with the app, before it starts or while it runs, for measurement and paint
  alike; every realm lays its text out again on its next frame. The same bytes registered twice
  are refused (`AlreadyRegistered`)
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §2).
- **`FontCollection::generation`** (`flui-painting`): how many registrations added a face.
- **`PipelineOwner::apply_font_change`** (`flui-rendering`): lays out again, and repaints, every
  node that measured text through the pipeline's context since the collection last changed.
- **`UiRealm::fonts_changed`** (`flui-runtime`): requests a frame for every presentation after a
  font registration.

### Changed

- `FontCollection::register_font` exists in every build, not only under `parley`. On a
  collection built by `FontCollection::with_host_faces` it loads the face into that process font
  system too, so the face paints as it measures.
- `PipelineOwner::drain_pending_dirty` applies a font collection change after the dirty
  requests it drains.

### Removed

- `SharedFontSystem::register_font` from the default build; it remains under the `testing`
  feature as a paint-only door for tests. Migrate `shared_font_system().register_font(&bytes)?`
  to `flui::register_font(&bytes)?`.

### Fixed

- A face registered after the app started changed painted text but not its measured size, and
  nothing laid the text out again: registration now reaches the app's font collection and the
  process font system together, and every realm re-lays out the text it measured.
