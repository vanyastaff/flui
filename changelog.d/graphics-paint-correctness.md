### Fixed

- Honor Solid shader RGBA on rectangle, rounded-rectangle and circle fills while preserving transforms, clips, explicit blend modes and parent group opacity.
- Isolate clips when drawing a balanced recorded picture into a canvas, preserving the caller's clip and subsequent drawing.
- Preserve analytical rounded-rectangle shadows under axis-aligned reflections and half-turns.
- Circular decoration hit-testing and circle containment preserve inside, boundary, and outside classification for very large and very small finite radii.
