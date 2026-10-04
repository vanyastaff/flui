# ADR-0120: Glyph images establish an immutable byte-layout invariant at construction

- **Status:** Accepted
- **Date:** 2026-10-04
- **Supersedes:** [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md), only the
  producer/atlas boundary for validating the `GlyphImage` byte layout.
- **Supersedes:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §5, only the
  representation and admission of the `GlyphImage` returned by the rasterizer.
  Its trait, font identity, registry ownership and shaping decisions remain in force.

## Context

`GlyphImage` exposed six mutable public fields. Its documented relationship
between dimensions, content and byte length was a convention a safe Rust
producer could violate. The engine therefore checked the relationship before
initial upload and again while rebuilding an atlas page. Multiplying a color
bitmap's texel count by four also required overflow handling.

The bitmap crosses the painting/engine boundary through `GlyphRasterizer`.
Validation belongs to the value's construction rather than each downstream
upload. Re-rasterizing a key can still fail or return a different valid image:
one image's byte-layout invariant does not prove same-key determinism.

## Decision

`flui_painting::GlyphImage` is an immutable owned bitmap. Its `left`, `top`,
`width`, `height`, `content` and `data` fields are private. Construction uses
`GlyphImage::try_new(left, top, width, height, content, data)`, returning
`Result<GlyphImage, GlyphImageError>`. It takes ownership of the supplied
`Vec<u8>` and checks the expected byte count before admitting the value.

- The byte count is `width × height × content.bytes_per_texel()` using checked
  arithmetic and a representable byte length for the target.
- An unrepresentable count returns `GlyphImageError::SizeOverflow`.
- A representable count unequal to the supplied data length returns
  `GlyphImageError::InvalidDataLength { expected, actual }`, with both byte
  lengths represented as `usize`.
- Zero width or height is legal when the byte vector is empty, including when
  the other dimension is nonzero. Bearings remain signed device-pixel offsets.
- Read-only `left()`, `top()`, `width()`, `height()`, `content()` and `data()`
  accessors expose the admitted value. `data()` returns a borrowed byte slice.
  No mutable access or unchecked constructor can invalidate the relationship.

The producer constructs the complete bitmap once. `SwashRasterizer` validates
its converted Swash output through this constructor. If construction fails,
its existing `rasterize` method returns `None`; the trait remains
`rasterize(key) -> Option<GlyphImage>`. External producers may propagate the
constructor error at their own boundary or map it to the same `None` outcome.

The engine consumes the established byte-layout invariant and removes its
duplicate data-length check. It still owns device limits, packer capacity,
upload placement and page lifetime. Atlas growth still rejects a re-rasterized
image whose dimensions or content differ from the recorded slot, even though
that image is individually valid. `None` and mismatched replay remove the
cache entry and allow a later attempt. Allocations referenced by recorded
draws remain retired until the frame ends, rather than being reused early.

All unaffected decisions in ADR-0067 and ADR-0092 remain binding: painting
shapes into neutral runs, the engine shapes no text, the raster side owns its
registry and atlas, keys carry font identity, and masks/color bitmaps retain
their existing content interpretation. This change adds no shaper, renderer,
thread, lock or display-list protocol.

## Consumer migration

Replace a struct literal with the fallible constructor:

```rust,ignore
let image = GlyphImage::try_new(left, top, width, height, content, data)?;
```

A `GlyphRasterizer` implementation returning `Option` may use `.ok()` when
invalid output has the existing "cannot rasterize this key" meaning. Consumers
whose API returns errors should preserve the `GlyphImageError` instead.

Replace field reads such as `image.width` and `&image.data` with `image.width()`
and `image.data()`. Code that changes dimensions, content, bearings or bytes
must prepare the new fields and construct a new image. For byte edits, copy
the borrowed slice into an owned vector or retain the producer's source data,
perform the edits, then validate the replacement. No field assignment or
mutable byte borrow remains available on an admitted image.

The constructor does not resize, pad, truncate or reinterpret supplied data.
Callers must handle refusal; an empty bitmap and failed rasterization retain
their different meanings (`Some` zero-area image versus `None`).

## Verification

Public painting consumer cases cover mask/color admission, exact length errors,
overflow, both zero-area orientations, nonempty zero-area rejection and the
immutable API. Constructor cases must distinguish omitted validation and
unchecked arithmetic. Compile-fail consumer cases establish that a struct
literal or direct field mutation cannot bypass construction.

Existing raster determinism and recorded bitmap oracles use read-only accessors.
The engine's replay recovery family uses valid alternate dimensions/content and
`None`, covering retry and retention of regions referenced by the current frame.
Malformed bitmap cases belong at the constructor boundary because a safe
rasterizer can no longer return one. Execution and production-control results
are recorded in the task's validation report; this decision asserts no measured
rendering speedup.
