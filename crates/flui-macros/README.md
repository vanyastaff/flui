# flui-macros

The procedural macros of FLUI. An author never names this crate: each macro is
re-exported beside the trait or function it serves, at the path the table
gives. An application reaches the same items through the facade:
`flui::prelude` carries the view derives, `InheritedData` and `Routable`,
`Inspect` is `flui::Inspect`, `MotionValue` is in `flui::animation`, `Lerp` is in
`flui::geometry`, and
the entry attribute is `flui::main`.

| Macro | Writes | Import it from |
|-------|--------|----------------|
| `#[derive(StatelessView)]`, `#[derive(StatefulView)]` | `impl View` for a type that implements the authoring trait, with its key and creation site from the fields marked `#[view(key)]` and `#[view(site)]` | `flui_view` |
| `#[derive(InheritedData)]` | one `FIELD_<NAME>` mask per field and `impl InheritedData` | `flui_view` |
| `#[derive(Inspect)]` | `impl Inspect` for a struct or an enum | `flui_foundation::inspect` |
| `#[derive(MotionValue)]` | `impl MotionValue` from each field's vector and rest thresholds | `flui_animation` |
| `#[derive(Lerp)]` | a fieldwise `impl Lerp` | `flui_foundation::geometry` |
| `#[derive(Routable)]` | `impl Routable` from `#[route("…")]` patterns | `flui_widgets` |
| `#[flui::main]` | the entry every platform starts an application from | `flui` |

The attributes each macro takes, what it refuses and why are in its rustdoc
(`cargo doc -p flui-macros --open`). [`ARCHITECTURE.md`](ARCHITECTURE.md) records the
decisions behind them: how an expansion finds its runtime crate, how `Inspect`
maps a field, how `Routable` orders its patterns.

A view, as an application writes one:

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

## Checking a change

```bash
cargo heli clippy -p flui-macros --all-targets -- -D warnings
cargo heli nextest run -p flui-macros
```

A refusal a derive makes before it needs a runtime path (an attribute out of place, repeated
or empty, a shape it does not support) is a unit test of its `expand` here. What an accepted
derive expands to is tested where the runtime is: `flui-view`, `flui-foundation`,
`flui-animation` and `flui-widgets` for behaviour, and the trybuild suites of `flui-sdk`,
`flui-view`, `flui-widgets` and the facade (`tests/ui/entry` for `#[flui::main]`) for what an
author sees, a refusal included.
