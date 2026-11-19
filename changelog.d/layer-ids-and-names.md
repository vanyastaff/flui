### Changed

- `LayerTree::get_layer` is `LayerTree::layer`.
- `LayerLink::new` never wraps: it mints from an `AtomicIdMint` and panics after 2^64 - 1 links
  instead of handing out an id a live link already holds.
- The damage types (`DamageRegion`, `DamageRect`, `FullReason`, `ResyncReason`, `NonFinite`,
  `Region`) live in `flui_layer::damage`'s module and keep their root paths.

### Removed

- `LayerTree::scene` and `LayerTree::with_scene`: no reader took the version id a flattened tree
  carried.
