# View, Element, RenderObject

FLUI's pipeline is three trees, each rebuilt from the last:

```text
View (config)  →  Element (lifecycle)  →  RenderObject (layout/paint)  →  Layer (retained)
```

## `View`

```rust,ignore
pub trait View: Downcast + DynClone + 'static { /* ... */ }
```

*(`crates/flui-view/src/view/view.rs`)* — an immutable, cheap-to-clone description of a piece of
UI, exactly like a Flutter `Widget`. Two flavors, matching Flutter's own split:

- `StatelessView` (`crates/flui-view/src/view/stateless.rs`) — `fn build(&self, ctx: &dyn
  BuildContext) -> impl IntoView`, no persistent state.
- `StatefulView` + `ViewState<V>` (`crates/flui-view/src/view/stateful.rs`) — `create_state()`
  produces a `ViewState` that owns mutable data and rebuilds independently of its parent.

Both are usually written via `#[derive(StatelessView)]` / the `StatefulView`+`ViewState` pair
rather than by hand — see the [Flutter → FLUI mapping](../mapping.md) table and
`examples/counter.rs` for the shape.

## `Element`

```rust,ignore
pub struct Element<V, A, B> { /* ... */ }
```

*(`crates/flui-view/src/element/unified.rs`)* — the mutable, long-lived counterpart to a `View`: it
holds the `View` it was built from, its position in the tree, and its
[lifecycle state](lifecycle.md). Rebuilding the tree reconciles new `View`s against existing
`Element`s by [key](keys.md) and type, reusing an `Element` (and the `State`/`RenderObject` it
owns) wherever the reconciliation matches — the same "elements are the retained half, views are
the ephemeral half" split as Flutter's `Element`/`Widget`.

## `RenderObject`

```rust,ignore
pub trait RenderObject<P: Protocol>: Diagnosticable + Downcast + 'static { /* ... */ }
```

*(`crates/flui-rendering/src/traits/render_object.rs`)* — performs layout and paint. Two concrete
sub-traits cover the two layout protocols FLUI ships: `RenderBox` (box protocol — most widgets) and
`RenderSliver` (sliver protocol — scrollable/virtualized content), in
`crates/flui-rendering/src/traits/{render_box,render_sliver}.rs`. See
[Layout: constraints down, sizes up](layout.md) for the protocol itself.

An `Element` owns at most one `RenderObject`; `StatelessView`/`StatefulView` that only compose
other views don't need one at all — only the leaf render-backed widgets do. See "Add a render
object" in AGENTS.md's Extending FLUI table for the checklist to implement a new one.

## `Layer`

```rust,ignore
pub enum Layer { /* Canvas, Picture, Texture, PlatformView, ... */ }
```

*(`crates/flui-layer/src/layer/mod.rs`)* — the retained compositing tree that paint produces,
consumed by `flui-engine`'s compositor. Unlike the other three, `Layer` is a closed `enum` rather
than a trait — FLUI's layer set is a known, fixed vocabulary rather than an extensible hierarchy,
which is one of the deliberate divergences from Flutter's `Layer` class hierarchy (see the
crate's own `ARCHITECTURE.md` for the reasoning).
