### Added

- **Partial repaint**: frames now repaint only what changed. `flui_layer::LayerDiffer` compares
  consecutive scenes' repaint-boundary stamps and produces `DamageRegion::Partial(DamageRect)` or
  `DamageRegion::Unchanged`; `flui-app`'s raster lane sends it, and `flui-engine` renders a partial
  frame into a retained target and blits it, so an unchanged frame does not present at all
  ([ADR-0087](/docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) §3–§4). Damage is on by
  default; `FLUI_DAMAGE=off` switches it off for every window the process opens.
- **`flui-layer`**: `BoundaryStamp`, `ContentToken`, `DamageRect`, `DamageMode` and
  `DamageRegion::union`.
- **`flui-painting`**: `DisplayList::damage_extent` and `DamageExtent`, a conservative ink
  extent (glyph ink from each face's bounds, stroke joins, a shadow's blur as a spread that
  scales with the transform's largest stretch, `Unbounded` for full-canvas fills), and
  `DisplayList::volatile_extent`, the texture draws whose pixels change behind an unchanged
  list.
- **`flui-painting`**: `BlendMode::keeps_destination_under_transparent_source`,
  `ColorFilter::modifies_transparent_black`, `ImageFilter::modifies_transparent_black` and
  `ColorMatrix::modifies_transparent_black`, which tell a damage producer when a layer's composite
  reaches pixels its children never inked.
- **`cargo xtask bench-collect --with-features`** runs the feature-gated bench targets too.

### Changed

- **`flui-layer`** (breaking): `LayerNode::with_render_id` is replaced by
  `LayerNode::with_boundary(render_id, content_token)`; `DamageRegion` gains the `Partial` and
  `Unchanged` variants, so a match on it needs arms for them or a wildcard.
- **`flui-engine`**: `RasterOwner` applies each frame's damage to the backend (`mark_dirty` for a
  partial region) instead of always calling `mark_full_repaint`, including for frames it rejects
  or that a newer submit supersedes; a failed render marks the next frame full.
- **`flui-engine`**: `Renderer::render_scene` is now the entry point for frames outside a raster
  owner (direct mode, a hot-reload plugin's scene): it always renders in full and makes the next
  frame full too. `RasterBackend::render_scene` renders the damage the owner applied.
- **`flui-engine`**: the performance overlay clips its readouts to its bounds; an overlay
  narrower or shorter than its text no longer draws past them.
