### Added

- **Partial repaint**: frames now repaint only what changed. `flui_layer::LayerDiffer` compares
  consecutive scenes' repaint-boundary stamps and produces `DamageRegion::Partial(DamageRect)` or
  `DamageRegion::Unchanged`; `flui-app`'s raster lane sends it, and `flui-engine` renders a partial
  frame into a retained target and blits it, so an unchanged frame does not present at all
  ([ADR-0087](/docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) §3–§4).
- **`flui-layer`**: `BoundaryStamp`, `ContentToken`, `DamageRect`, `DamageMode`,
  `DamageRegion::union` and `Layer::same_effect`.
- **`flui-painting`**: `DisplayList::damage_extent` and `DrawOp::damage_bounds`, a conservative
  ink extent (glyph overflow, stroke joins, shadow blur, `Unbounded` for full-canvas fills).
- **`cargo xtask bench-collect --with-features`** runs the feature-gated bench targets too.

### Changed

- **`flui-layer`** (breaking): `LayerNode::with_render_id` is replaced by
  `LayerNode::with_boundary(render_id, content_token)`; `DamageRegion` gains the `Partial` and
  `Unchanged` variants, so a match on it needs arms for them or a wildcard.
- **`flui-engine`**: `RasterOwner` applies each frame's damage to the backend (`mark_dirty` for a
  partial region) instead of always calling `mark_full_repaint`, including for frames it rejects
  or that a newer submit supersedes; a failed render marks the next frame full.
