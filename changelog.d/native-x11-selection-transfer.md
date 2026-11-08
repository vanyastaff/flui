### Changed

- Unified X11 clipboard and XDND whole/`INCR` selection reads and writes on shared protocol-only state machines while preserving feature-specific ownership, queues, payloads and errors.

### Fixed

- Bounded `INCR` announcement reservations and rejected announced or accumulated payloads past reader limits, including lying chunks and oversized whole properties.
- Added bounded, deadline-driven outgoing XDND `INCR` transfers so large drag payloads complete with a terminal empty chunk instead of being refused.
