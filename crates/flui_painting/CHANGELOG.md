# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Tracing Instrumentation**
  - Added `#[tracing::instrument]` to performance-critical operations
  - Canvas::finish(), extend_from(), save_layer()
  - DisplayList::append(), to_opacity()
  - Structured logging with spans and fields for production debugging

- **Canvas Composition Methods**
  - `Canvas::extend(impl IntoIterator<Item = Canvas>)` - extends from multiple canvases (follows std pattern)
  - `Canvas::merge(self, other) -> Self` - functional-style merge of two canvases
  - Both methods use zero-copy move semantics

- **Common Traits (C-COMMON-TRAITS)**
  - Added `Clone` implementation for `Canvas` for better ergonomics

- **Optional Serde Support (C-SERDE)**
  - Added optional `serde` feature for serialization
  - `DisplayList`, `DrawCommand`, `DisplayListStats` support Serde
  - HitRegion skipped during serialization (contains function pointers)

- **API Guidelines Compliance Audit**
  - Comprehensive audit document (API_GUIDELINES_AUDIT.md)
  - 98% compliance with Rust API Guidelines (47/48 applicable)
  - Production-ready status confirmed

### Changed

- **API Naming Consistency (RFC 430)**
  - Renamed error variants for consistent word order:
    - `DecorationFailed` → `PaintDecorationFailed`
    - `TextPaintingFailed` → `PaintTextFailed`
    - `ImageFailed` → `PaintImageFailed`
  - Updated helper methods to match: `paint_decoration_failed()`, `paint_text_failed()`, `paint_image_failed()`
  - Follows Rust API Guidelines C-WORD-ORDER for error types

- **API Method Naming (C-CONV, C-METHOD)**
  - Renamed `DisplayList::with_opacity()` → `to_opacity()`
    - Uses `to_` prefix for expensive borrowed-to-owned conversions
  - Renamed `Canvas::append_canvas()` → `extend_from()`
    - Uses `extend_from` for consuming ownership (vs. `append(&mut)` which drains)
    - Follows Rust standard library conventions
  - All methods follow Rust API Guidelines naming conventions

## [0.1.0] - 2024-11-28

### Added

- **Extension Traits Pattern**
  - Split DisplayList API into `DisplayListCore` (sealed) and `DisplayListExt` (public)
  - Enables future API additions without breaking changes
  - Users can add custom extension traits

- **Iterator Methods**
  - Added `DisplayList::iter()` and `iter_mut()` for clippy convention
  - Implemented `IntoIterator` for `&DisplayList` and `&mut DisplayList`
  - Enables ergonomic iteration: `for cmd in &display_list { }`

- **Debug Trait**
  - Added `Debug` implementation for `Canvas`
  - All public types now implement Debug

- **Safe restore() Behavior**
  - `Canvas::restore()` no longer panics when called without matching `save()`
  - Now a safe no-op if save stack is empty
  - Matches behavior of common 2D graphics APIs

- **Comprehensive Documentation**
  - Added detailed crate-level documentation with examples
  - Added `docs/` directory with guides:
    - Architecture Guide (internal design and patterns)
    - Performance Guide (optimization techniques)
    - Migration Guide (version upgrade instructions)
    - Contributing Guide (contributor guidelines)
  - Enhanced prelude documentation
  - Improved trait documentation with examples

- **Modern Rust Patterns**
  - Sealed traits for API stability
  - Extension traits for modularity
  - Smart pointer support (Arc, Box, &)
  - Standard traits (AsRef, AsMut, Index, IndexMut)
  - `#[must_use]` on filtering methods
  - `#[non_exhaustive]` on error types
  - Const constructors where applicable

- **Quality Control**
  - Enabled comprehensive clippy lints (all, pedantic)
  - Added documentation lints (broken links, missing docs)
  - Zero clippy warnings with strict checks
  - Added missing Debug implementations

### Changed

- **Canvas::restore()** - No longer panics, now a no-op when called without save()
- **DisplayList API** - Methods split into core and extension traits (requires trait import)
- **Documentation** - Removed all Flutter-specific references, focusing on FLUI's own design

### Fixed

- Corrected documentation links
- Fixed clippy warnings for IntoIterator convention
- Updated test for safe restore() behavior

### Performance

- No performance regressions
- All optimizations preserved (zero-copy composition, transform baking, etc.)

## [0.0.1] - Initial Development

### Added

- Core Canvas API for recording 2D drawing commands
- DisplayList for immutable command sequences
- DrawCommand enum with all primitive types
- Transform stack with save/restore
- Clipping support (rect, rounded rect, path)
- Paint configuration (fill, stroke, blend modes)
- Path drawing with Bezier curves
- Image drawing (simple, repeat, nine-slice, filtered)
- Text rendering support
- Layer composition with effects
- Shader masks and backdrop filters
- Thread-safe design (Send for Canvas, Send + Clone for DisplayList)
- Zero-copy canvas composition
- Batch drawing methods
- Scoped operations (closure-based API)
- Chaining API (builder pattern)
- Debug helpers

[Unreleased]: https://github.com/flui-org/flui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/flui-org/flui/releases/tag/v0.1.0
[0.0.1]: https://github.com/flui-org/flui/releases/tag/v0.0.1
