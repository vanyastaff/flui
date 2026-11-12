### Changed

- `DisplayList::append_isolated` splices a run that only draws (no clip, save, restore or layer scope) as it stands, without a `Save`/`Restore` pair around it, and wraps a first run that does need one in its own buffer. A repaint of 1 000 plain leaves records 4 050 allocations instead of 6 050 and runs about 17 % faster.
- `Canvas` keeps its paint interning pool inline for up to four paints.
