# flui-view

`flui-view` is FLUI's declarative tree layer: immutable `View` values are
mounted into mutable `Element` nodes, which in turn own or connect to render
objects.

The crate is intentionally close to Flutter's widget/element contract while
using Rust-native storage and dispatch:

- public view identity is `TypeId + Option<&dyn ViewKey>`;
- element storage is the closed `ElementKind` enum;
- public IDs use the workspace 1-based `NonZeroUsize` pattern;
- widget-author code returns `impl IntoView`, not `Box<dyn View>`;
- multi-child slots accept `ViewSeq` (`()` / tuples up to 16 / `Vec<V>` /
  `Vec<BoxedView>`);
- variable-arity reconciliation uses the keyed linear algorithm and emits
  `ReconcileEvent`s for tests and diagnostics.

## Authoring Shape

Most widget authors import the prelude, derive the `View` boilerplate, and
write the typed authoring trait implementation:

```rust,ignore
use flui_view::prelude::*;

#[derive(Clone, StatelessView)]
struct Greeting {
    name: String,
}

impl StatelessView for Greeting {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.name.clone())
    }
}
```

The derive is re-exported from `flui_view::prelude`, so there is no separate
`flui_macros` import. `StatelessView::build` and `ViewState::build` both return
`impl IntoView`; the framework normalizes that into a `View` at the build call
site.

Stateful widgets use the same derive for the object-safe `View` boilerplate and
keep state in a separate `ViewState` type:

```rust,ignore
use flui_view::prelude::*;

#[derive(Clone, StatefulView)]
struct Counter {
    initial: u32,
}

struct CounterState {
    count: u32,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(format!("count: {}", self.count))
    }
}
```

## Dynamic Returns

`impl IntoView` requires every branch of a `build` expression to have one hidden
return type. When branches naturally return different concrete views, box each
branch explicitly:

```rust,ignore
impl StatelessView for MaybePadded {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.padded {
            Padding::new(self.child.clone()).boxed()
        } else {
            self.child.clone().boxed()
        }
    }
}
```

This is the intentional dynamic boundary. `BoxedView` implements `View`, so it
also satisfies `IntoView` through the blanket implementation.

## Children

Static heterogeneous children use tuple-backed `ViewSeq` values. The `column!`
and `row!` macros are authoring helpers for the tuple shape:

```rust,ignore
let children = column![
    Header::new("Inbox"),
    MessageRow::new(first),
    Footer::default(),
];
```

Tuple `ViewSeq` implementations are generated for arities `0..=16`. At 17+
children, `column!` and `row!` deliberately emit a friendly compile error that
points to the dynamic fallback:

```rust,ignore
let children: Vec<BoxedView> = items
    .into_iter()
    .map(|item| MessageRow::new(item).boxed())
    .collect();
```

Use `Vec<V>` for homogeneous dynamic children and `Vec<BoxedView>` for
heterogeneous dynamic children.

## Keys And Reconciliation

`ObjectKey`, `ValueKey`, `UniqueKey`, and `GlobalKey` are stored on each
`ElementNode` as `Option<Box<dyn ViewKey>>`. Runtime update semantics follow
Flutter's `Widget.canUpdate`: same view type and semantically equal key.

The variable-arity reconciler preserves `ElementId` and state on keyed
reorders, tears down mismatched types, and emits structured `ReconcileEvent`s.
`GlobalKey` moves are routed through the same tree/reconciler machinery and
preserve state across inactive retake and active-to-active reparent paths.

## Important Modules

- `view/` - public `View`, `StatelessView`, `StatefulView`, `ViewState`,
  `RenderView`, `InheritedView`, `ProxyView`, `ParentDataView`, `IntoView`,
  `BoxedView`.
- `element/` - `ElementKind`, arity markers, generic element core, behavior
  dispatch, lifecycle, and render-object element adapters.
- `tree/` - slab-backed element tree, keyed reconciliation, global-key
  reparenting, and `ReconcileEvent`.
- `seq/` - `ViewSeq` tuple and `Vec` implementations.
- `macros/` - `column!` and `row!`.
- `key/` - `ObjectKey`, `GlobalKey`, and the process-wide global-key registry.
- `binding/` and `owner/` - build-frame coordination and split-borrow owner
  handles.

## Verification

For focused work in this crate:

```bash
cargo test -p flui-view --features test-utils --all-targets
cargo clippy -p flui-view --features test-utils --all-targets -- -D warnings
bash scripts/port-check.sh -v
```

In the Codex sandbox for this repository, use a writable target directory:

```bash
env CARGO_TARGET_DIR=/tmp/flui-target cargo test -p flui-view --features test-utils --all-targets
```
