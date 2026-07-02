# flui-macros

Procedural macros for FLUI.

This crate is a proc-macro leaf crate. It emits code into the consuming crate
using absolute `::flui_view::...` paths, so every consumer must depend on
`flui-view` directly. Most authors get the derives through:

```rust,ignore
use flui_view::prelude::*;
```

## Derives

### `#[derive(StatelessView)]`

Generates the object-safe `impl View for T` boilerplate for a type that already
implements the typed authoring trait `StatelessView`:

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

The generated implementation calls:

```rust,ignore
::flui_view::element::ElementKind::stateless(self)
```

and preserves the user's generic parameters and where clauses.

### `#[derive(StatefulView)]`

Generates the matching `impl View for T` boilerplate for a type that implements
`StatefulView`:

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

The generated implementation calls:

```rust,ignore
::flui_view::element::ElementKind::stateful(self)
```

## Keys

The derives intentionally do not generate a custom `View::key()` method; the
default key is `None`. A widget that participates in keyed reconciliation must
write the `impl View` block manually so `create_element()` and `key()` live in a
single coherent implementation:

```rust,ignore
impl View for KeyedRow {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}
```

A future derive attribute for field-backed keys is an authoring improvement, not
part of the current Phase 3 surface.

## Recursive And Conditional Returns

The derives operate on the struct declaration only; they do not inspect the body
of `build()`. When a `build()` body has branches with different concrete return
types, or a recursive widget needs an erased boundary, use `.boxed()` at that
branch:

```rust,ignore
if has_children {
    TreeNode::new(children).boxed()
} else {
    LeafNode.boxed()
}
```

This keeps the public authoring trait on `impl IntoView` while making the
dynamic boundary explicit and local.

## Other Macros

`Animatable` and `Diagnosticable` are also implemented in this crate. They are
independent of the view derives and are documented in the generated rustdoc for
their proc-macro entry points.

## Verification

Focused checks:

```bash
cargo test -p flui-view --test derive_smoke --features test-utils
cargo test -p flui-view --test derive_bon_stack --features test-utils
cargo clippy -p flui-macros --all-targets -- -D warnings
```
