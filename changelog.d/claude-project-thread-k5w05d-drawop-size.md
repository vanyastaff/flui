### Changed

- A `DisplayList` keeps paths, `DRRect`'s rounded rectangles, image-region placements and colour filters in a buffer beside its commands, which `clear` keeps: `DrawOp::{Path, ClipPath, Shadow, DRRect, ImageRegion, ImageFiltered}` hold a typed `PayloadId<T>`, read with `DrawCommand::payload`, and a command is at most 120 bytes instead of 224. A list recorded again into its own room allocates nothing for them, and a shape shadowed and then filled is stored once.
- `DrawOp::DRRect` holds one `NestedRRects { outer, inner }` and `DrawOp::ImageRegion` one `ImageRegionPlacement { src, dst, tile, repeat, filter }`.
- `flui_testing`'s `LaidOut::draw_ops` returns `DrawCommandBuf` values, owned commands that carry their payloads.

### Added

- `flui_painting::{PayloadId, Payload, NestedRRects, ImageRegionPlacement, DrawCommandBuf}`, `DrawCommand::{payload, try_payload, to_buf}` and `DrawCommandBuf::{payload, command}`.
