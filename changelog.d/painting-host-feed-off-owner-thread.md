### Changed

- **The host's fonts load off the owner thread** (`flui-app`, `flui-painting`): an app's first
  frame no longer waits for the host font scan and feed (about 43 ms on a Windows desktop). It
  renders with the bundled faces, and the host's faces arrive from a `flui-host-fonts` thread;
  text laid out in a fallback face (a family or script only the host carries) is laid out again
  at the next frame after they land, as after `register_font`. A build without `bundled-fonts`
  still feeds the host's faces before the first frame, since it has no face of its own
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §7).
- **`FontCollection::with_host_feed` and `HostFontFeed`** (`flui-painting`): a collection with
  the bundled faces now, and the feed that scans the host and adds its faces when run on
  another thread, raising the collection's generation once. `FontCollection::with_host_fonts`
  still feeds synchronously. Both register one font file at a time, so a host-fed collection
  keeps about 0.15 MiB of heap instead of 2.4 MiB.
