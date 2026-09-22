# Layout: constraints down, sizes up

FLUI's box layout protocol is Flutter's "constraints go down, sizes go up, parent sets position":
a parent passes its child a `BoxConstraints`, the child picks its own `Size` within those
constraints and returns it, and the parent then positions the child — a child never sees its
parent's size or position directly, and a parent never dictates an exact size without also
allowing the child to choose within a range.

```rust,ignore
pub struct BoxConstraints {
    // min_width <= width <= max_width
    // min_height <= height <= max_height
    // ...
}
```

*(`crates/flui-rendering/src/constraints/box_constraints.rs`)* — a size satisfies a `BoxConstraints`
if and only if `min_width <= width <= max_width` and `min_height <= height <= max_height`, exactly
Flutter's own `BoxConstraints` contract.

A `RenderBox` implements the protocol by overriding `perform_layout`:

```rust,ignore
fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) -> Size {
    // measure/lay out children against ctx's constraints, return this render object's own size
}
```

*(`crates/flui-rendering/src/traits/render_box.rs`, doc example — the exact context type
(`Leaf`/`Single`/`Variable`) and parent-data type vary by how many children the render object
has.)*

For virtualized/scrollable content, `RenderSliver` implements a second, parallel protocol instead
of the box protocol — see [View, Element, RenderObject](view-element-render.md) and
`crates/flui-rendering/src/traits/render_sliver.rs`.
