### Added
- `PointerDispatch::with_root_to_local` names the transform that rewrote the event into the target's space.

### Fixed
- The ownership table measures an axial drag along its own axis: a horizontal drag inside a quarter-turn now wins a vertical motion of the root against an enclosing vertical scrollable, instead of losing it.
