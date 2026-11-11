### Changed

- A mounted stateful element no longer reserves room for a second copy of its
  view, an `Actions` scope shares its bindings with the chain it publishes
  instead of copying every enclosing binding into a map, and a focus widget
  keeps one shared cell instead of seven. A `RawButton` over a `Text` mounts
  with 84 allocations and 11.7 KB (was 94 and 13.9 KB).
