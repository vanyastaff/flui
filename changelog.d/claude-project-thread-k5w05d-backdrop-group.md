### Fixed

- A partial frame that redraws one grouped backdrop filter (`BackdropGroup`, `BackdropFilter::grouped`) reads the backdrop where the group's first filter paints, as a full frame does. Before, when the damage left the first filter out, the capture moved to the first filter inside the damage, which then blurred content painted after the first one, so partial and full frames showed different pictures.

### Changed

- `flui_layer::close_over` takes a `Closures` set in place of an iterator of `Closure`s. `Closures::push_effect` adds an effect's `read_closure` and joins it to its backdrop group, and damage that meets one member of a group takes every member's footprint.
