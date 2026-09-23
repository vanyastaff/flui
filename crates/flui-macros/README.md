# flui-macros

Procedural macros for FLUI.

This crate emits code into the consuming crate. Derives resolve a direct runtime
dependency first (`flui-view`, `flui-foundation`, or `flui-animation`), then fall
back to the corresponding module in the `flui` facade. Cargo dependency aliases
are respected in either case. Applications can use a single `flui` dependency:

```rust,ignore
use flui::prelude::*;
```

## Derives

### `#[derive(StatelessView)]`

Generates the object-safe `impl View for T` boilerplate for a type that already
implements the typed authoring trait `StatelessView`:

```rust,ignore
use flui::prelude::*;

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

With only the facade dependency, the generated implementation calls:

```rust,ignore
::flui::view::element::ElementKind::stateless(self)
```

and preserves the user's generic parameters and where clauses.

### `#[derive(StatefulView)]`

Generates the matching `impl View for T` boilerplate for a type that implements
`StatefulView`:

```rust,ignore
use flui::prelude::*;

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

With only the facade dependency, the generated implementation calls:

```rust,ignore
::flui::view::element::ElementKind::stateful(self)
```

### `#[derive(InheritedData)]`

Field-granular inherited dependencies (issue #1090). For a non-generic struct with
named fields, generates one `pub const FIELD_<NAME>: FieldMask` per field (declaration
order, bit 0 first; `r#type` becomes `FIELD_TYPE`) and `impl InheritedData for T` whose
`field_mask_diff` unions the mask of every field that differs (`!=`, so each field must be
`PartialEq`). An `InheritedView` whose `Data` derives it overrides `changed_fields` with
`self.data().field_mask_diff(old.data())`, and a dependent that read one field through
`BuildContextExt::depend_on_field(T::FIELD_SIZE, ..)` rebuilds only when that field changes:

```rust,ignore
use flui::prelude::*;

#[derive(Clone, PartialEq, InheritedData)]
struct MediaQueryData {
    size: Size,
    text_scale_factor: f32,
}

// In a reader's build:
let size = ctx.depend_on_field::<MediaQueryData, _>(MediaQueryData::FIELD_SIZE, |d| d.size);
```

Refused at compile time, with the reason named: more than 64 fields (the mask is 64 bits;
split the provider data), tuple or unit structs, enums, unions, and generic structs.

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
