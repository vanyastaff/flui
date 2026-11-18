# Layout: constraints down, sizes up

Flutter counterpart: `BoxConstraints`, `RenderBox.performLayout`.

FLUI's box layout protocol is "constraints go down, sizes go up, parent sets position". A parent
passes its child a `BoxConstraints`, the child picks its own `Size` within those constraints and
returns it, and the parent then positions the child. A child never sees its parent's size or
position directly, and a parent never dictates an exact size without also allowing the child to
choose within a range.

A size satisfies a `BoxConstraints` if and only if `min_width <= width <= max_width` and
`min_height <= height <= max_height`. A parent derives its child's constraints from its own: a
`Padding` deflates them by its insets, so a tight window stays tight for its child, and a `Center`
or an `Align` loosens them, so its child may pick any size up to the parent's:

```rust
use flui::geometry::{EdgeInsets, Size};
use flui::rendering::BoxConstraints;

// The window gives the root a tight 400 × 300.
let window = BoxConstraints::tight(Size::new(400.0, 300.0));
assert!(window.is_satisfied_by(Size::new(400.0, 300.0)));

// A Padding takes 16 from each side: its child must be exactly the rest.
let padded = window.deflate(EdgeInsets::all(16.0));
assert_eq!(padded.biggest(), Size::new(368.0, 268.0));
assert_eq!(padded.smallest(), Size::new(368.0, 268.0));

// A Center inside it lets its child pick any size up to that.
let centered = padded.loosen();
assert_eq!(centered.smallest(), Size::new(0.0, 0.0));

// A child that asks for more than it may have gets the closest size that fits.
assert_eq!(centered.constrain(Size::new(500.0, 20.0)), Size::new(368.0, 20.0));
```

A render object implements the protocol in `perform_layout`: it lays out each child against the
constraints it chooses for that child, positions the children, and returns its own size. Layout
and child measurements return typed errors; propagate them with `?`, since a rejected
measurement supplies no size to position or paint. See
[ADR-0181](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0181-fallible-text-and-layout-measurement.md)
for ordinary text rejection and changed-input recovery, and
[Extending FLUI](https://github.com/vanyastaff/flui/blob/main/docs/extending.md) for writing a
render object of your own.

For virtualized and scrollable content, `RenderSliver` implements a second, parallel protocol
instead of the box protocol: see [View, Element, RenderObject](view-element-render.md).
