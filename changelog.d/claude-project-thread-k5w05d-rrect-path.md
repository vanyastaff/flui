### Changed

- A `Path` made as a rounded rectangle with circular corners that fit its sides, or an empty `Path`, builds its curves only when they are read, and a translate of such a path records the move, so a Material surface whose shape is recorded, moved, filled and shadowed allocates no path geometry while it is built and painted. An `ElevatedButton` holds 1 832 bytes, 6 allocations and 2 reallocations less per button.

### Removed

- `Path::bounds` and `Path::cached_bounds`: a path no longer caches its bounds, which kept every `Path` 16 bytes larger; `Path::compute_bounds` gives them.
