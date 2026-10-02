### Fixed

- Window and headless layer captures now share shader-mask, backdrop-filter and follower traversal, including nested effects and trailing siblings.
- Shader masks apply their requested blend operator between shader and child pixels, then composite the isolated result over the parent.
- Backdrop blur preserves independent axes, reads the containing attachment with its kernel halo, and reconstructs retained-frame input dependencies before filtering. Resize-transient reads and writes stay within the viewport/attachment intersection.
- Gradient geometry preserves affine transforms and local shader coordinates; full-turn sweep gradients no longer collapse to the first colour. Linear projection and sweep phase are computed in f64 before packing to preserve colours with distant endpoints or large angles.
- Quarter-turn backdrop rotations preserve anisotropic blur axes despite trigonometric roundoff.
- Reflected rectangles retain positive local bounds; rounded rectangles preserve scaled radii and reflected corner identities, including large local origins.

### Changed

- Unsupported mask shaders and backdrop filter/transform combinations return typed errors instead of silently rendering a different effect. Nested effect recording and cumulative backdrop sampling work are bounded.
- Gradient recording rejects nonfinite or unrepresentable numeric payloads before GPU upload, including ordinary and advanced blends.

### Removed

- The unused separate offscreen mask/Kawase backend and its testing-only texture-pool exports. Layer effects use the ordered painter path; presentation blitting remains available.
