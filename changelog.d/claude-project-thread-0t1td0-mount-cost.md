### Changed

- A `RawButton` over a `Text` mounts with 84 allocations and 9.7 KB (was 94
  and 13.9 KB), and every widget gains from the same changes: a mounted
  stateful element no longer reserves room for a second copy of its view, an
  `Actions` scope shares its bindings with the chain it publishes instead of
  copying every enclosing binding into a map, a focus widget keeps one shared
  cell instead of seven, and a `SemanticsConfiguration` is 488 bytes instead
  of 1144 (the attributes few nodes declare sit behind one pointer).
- `SemanticsConfiguration::numeric_range` is no longer a `const fn`.
