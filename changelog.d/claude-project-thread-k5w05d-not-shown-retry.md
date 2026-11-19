### Changed

- **`flui-runtime`**: a frame its surface withheld (`SubmitVerdict::NotShown`) is retried by
  publishing the scene version it already holds, with nothing built, laid out or painted, where it
  repainted the whole tree. The engine draws every such retry in full, replaying from its recording
  cache the chunks it kept. A retry that finds another surface size or device pixel ratio, a newer
  version applied since, or a painter the executor let go (a device rebuilt, a window's render that
  panicked) repaints in full
  ([ADR-0304](/docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) §7).

### Added

- **`flui-engine`**: `RasterSender::painter_resets` and `PainterResets`, how many times the
  executor let go of the painter its slots share; a window's host repaints in full when the count
  moved since its last frame.
- **`flui-testing`**: `LaidOut::leave_unshown`, the widget harness's way to leave frames unshown as
  `HeadlessHost::leave_unshown` does.
