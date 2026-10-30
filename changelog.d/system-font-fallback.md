### Added

- `SystemFallback`, `FaceRef` and `ScriptCode` in `flui-painting`: the host's own answer to which faces draw a character a style's family lacks, per script, for emoji, and for any script. `HostFontFeed::with_system_fallback` puts it ahead of FLUI's fallback tables, and `HostFonts::scan_with` does the same for a synchronous scan. A face the host names with colour glyph tables enters only the emoji generic (ADR-0271).
- `FallbackOrder`, which `HostFontFeed::run` now returns: the one writer of a collection's fallback order. Its `set_system_fallback` installs a new answer, such as one for a new locale, and raises the collection's generation once per change.
