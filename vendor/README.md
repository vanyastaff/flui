# Vendored crate patches

## `glyphon-0.11.0`

Upstream `glyphon` depends on `cosmic-text` with default features, which enables
`fontconfig` (Linux-only). FLUI pins `cosmic-text` without `default` so Windows
builds avoid the extra feature union and match `flui-painting`'s text stack.

Remove this patch when upstream glyphon adopts `cosmic-text` with
`default-features = false`, or when we drop glyphon.
