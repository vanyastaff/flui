# View, Element, RenderObject

Flutter counterpart: `Widget`, `Element`, `RenderObject`, `Layer`.

FLUI's pipeline is four trees, each built from the last, with a fifth, the semantics tree, built
alongside for accessibility:

```text
View (config)  →  Element (lifecycle)  →  RenderObject (layout/paint)  →  Layer (compositing)
```

## `View`

A view is an immutable, cheap-to-clone description of a piece of UI. An application writes two
kinds:

- a `StatelessView`, whose `build(&self, ctx)` returns the views it is made of;
- a `StatefulView` with its `ViewState`: `create_state(&self, ctx)` makes the state once, when the
  view is first mounted, and the state's `build` runs again whenever something it read changes.

```rust
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct Greeting {
    name: String,
}

impl StatelessView for Greeting {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(format!("Hello, {}!", self.name))
    }
}

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState {
    count: Signal<u32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> CounterState {
        CounterState { count: ctx.signal(0) }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        Column::new((
            Greeting { name: "FLUI".to_owned() },
            Text::new(count.get(ctx).to_string()),
            RawButton::new(Text::new("Increment"))
                .on_pressed(move |ctx| count.update(ctx, |n| *n += 1)),
        ))
    }
}
```

## `Element`

An element is the mutable, long-lived counterpart of a view: it holds the view it was built from,
its position in the tree, and its [lifecycle state](lifecycle.md). Rebuilding the tree reconciles
new views against existing elements by [key](keys.md) and type, and reuses an element, with the
state and render object it owns, wherever they match. Elements are the retained half, views the
ephemeral half. An application never names an element type; it reaches its element through the
`ctx` its hooks receive.

## `RenderObject`

A render object performs layout, paint and hit testing. Two protocols ship: the box protocol
(`RenderBox`, most widgets) and the sliver protocol (`RenderSliver`, scrollable and virtualized
content). See [Layout: constraints down, sizes up](layout.md) for the box protocol.

An element owns at most one render object. Views that only compose other views, like both views
above, have none; only the leaf render-backed widgets do. See "Add a render object" in
[Extending FLUI](https://github.com/vanyastaff/flui/blob/main/docs/extending.md) for the checklist
to implement a new one.

## `Layer`

Paint records into layers: a closed set of kinds (pictures, clips, transforms, opacity, textures,
platform views) that `flui-engine` composites and rasterizes. A layer is a value of one enum rather
than an open trait hierarchy; `crates/flui-layer/ARCHITECTURE.md` has the reasoning.
