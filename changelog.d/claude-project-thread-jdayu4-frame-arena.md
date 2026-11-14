### Added

- `flui::foundation::arena::FrameArena` and `FrameVec`: scratch memory that one owner's frame walks share and free at once, through the standard `Allocator` trait (ADR-0328).

### Changed

- The semantics pass takes its per-node fragment lists and conflict marks from the pipeline owner's frame arena: a list-heavy scroll frame makes about 135 fewer global allocations.
