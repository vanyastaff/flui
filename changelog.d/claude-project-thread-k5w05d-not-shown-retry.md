### Changed

- **`flui-runtime`**: a frame its surface withheld (`SubmitVerdict::NotShown`) is retried by
  publishing the scene version it already holds, with nothing built, laid out or painted, where it
  repainted the whole tree; the raster side replays the chunks it recorded. A retry that finds
  another surface size or device pixel ratio, or a newer version applied since, repaints in full
  ([ADR-0304](/docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) §7).

### Added

- **`flui-testing`**: `LaidOut::leave_unshown`, the widget harness's way to leave frames unshown as
  `HeadlessHost::leave_unshown` does.
