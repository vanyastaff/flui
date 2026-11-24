### Changed

- `SceneOwner::force_unshared_effect_updates` (feature `testing`) is now `SceneOwner::force_unshared_damage`, and it also makes a republished content land each of its entries through its own chain.

### Fixed

- A republished boundary content that damages many chunks (a list that dropped its first rows) folds its node's chain, and the effects above each run of chunks under one effect, once instead of once per chunk: the damage of a 3000-row filter frame costs 60% fewer instructions.
