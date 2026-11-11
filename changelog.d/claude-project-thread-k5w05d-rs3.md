### Changed

- `DamageRegion` is exhaustive and says why a frame repaints in full: `Unchanged`, `Region(Region)` with up to eight disjoint rects, or `Full(FullReason)` (first frame, surface change, above the threshold, unbounded, non-finite, damage off, or a resync with its `ResyncReason`). `union` keeps the first `Full` reason and merges regions rect by rect (ADR-0304 §5).
- `DamageRect::covering` is now `DamageRect::try_covering`, which refuses a non-finite rect with `NonFinite` instead of reporting it as empty.
