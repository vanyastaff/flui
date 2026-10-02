### Changed

- Compose geometric clips as immutable expressions with exact Intersect/Difference membership, elliptical corner radii, bounded filled paths and common-sample antialiasing.
- Apply inherited group clips once after opacity and image filters, with coverage-correct destination-sensitive compositing.
- Reject invalid or non-representable 2D clip geometry and unsupported direct destructive AA explicitly instead of approximating the requested operation.
- Reuse headless capture pipelines across successful captures, resetting viewport and frame state; failed or unwound captures discard their painter before recovery.
