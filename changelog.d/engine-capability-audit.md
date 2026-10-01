### Fixed
- Serialize concurrent captures on one headless renderer through readback, and preserve the spare target when an invalidated partial frame needs a full retry.
- Merge compatible adjacent tessellated batches while preserving intervening primitive order and clip boundaries.
- Refresh immutable viewport bindings for resized offscreen-only frames, expose quarantined domains to native recovery, and discard failed GPU-profiler frames before retry.
- Preserve nested opacity and clipped children, and earlier siblings, inside blur, morphology and composed image-filter layers.
- Keep image allocation identities alive while cached or recorded, and distinguish filtered images with equal pixels but different dimensions.
- Preserve low-alpha texture pixels instead of discarding visible contributions below one percent alpha.
- Request adapter-specific texture format features only on GPU adapters that advertise them.
- Preserve painter order across primitive families, gradient tables beyond eight stops, distinct encoded viewport/buffer contents, and cached-image scissors.
- Preserve committed retained pixels after a candidate frame fails, and propagate offscreen failures to the frame owner.

### Added
- Expose embedder-owned painter frame boundaries and demonstrate same-device depth-tested GPU content composed with FLUI painting, including analytical capture verification.
- Demonstrate bounded provider SSE streaming through application services and StreamBuilder, with subscription cancellation and explicit offline mode.
- Add consuming painter submission, bounded recording arenas and prepared-resource admission with completion retirement and recovery after refusal.

### Changed
- Painter frame begin is fallible and bounds cumulative submissions until finish; gradient draws reject more than 256 stops with an explicit error to bound fragment-loop work.
- Recording admission removes per-element mutex acquisition and batches exact-size index charges while retaining thread-transferable ownership.
- Offscreen render operations return errors; managed frame submission work has an explicit bound and distinguishes current-frame limits from earlier GPU backpressure.
