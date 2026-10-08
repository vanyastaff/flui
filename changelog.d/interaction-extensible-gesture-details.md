### Changed

- Gesture callback detail payloads are non-exhaustive, allowing new observations without requiring exhaustive consumer construction. Existing factories remain available; callback consumers can read fields or destructure with `..`.
