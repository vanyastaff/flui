# flui-macros

The procedural macros of FLUI. An author never names this crate: each macro is
re-exported beside the trait or function it serves, and an application gets
all of them from `flui::prelude`.

| Macro | Writes | Import it from |
|-------|--------|----------------|
| `#[derive(StatelessView)]`, `#[derive(StatefulView)]` | `impl View` for a type that implements the authoring trait | `flui_view::prelude` |
| `#[derive(InheritedData)]` | one `FIELD_<NAME>` mask per field and `impl InheritedData` | `flui_view::prelude` |
| `#[derive(Inspect)]` | `impl Inspect` for a struct or an enum | `flui_foundation::inspect` |
| `#[derive(TwoWayConverter)]` | `impl TwoWayConverter` and a fieldwise `impl Lerp` | `flui_animation` |
| `#[derive(Routable)]` | `impl Routable` from `#[route("…")]` patterns | `flui_widgets::router` |
| `#[flui::main]` | the entry every platform starts an application from | `flui` |

The attributes each macro takes, what it refuses and why are in its rustdoc
(`cargo doc -p flui-macros --open`). [`ARCHITECTURE.md`](ARCHITECTURE.md) records the
decisions behind them: how an expansion finds its runtime crate, how `Inspect`
maps a field, how `Routable` orders its patterns.

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

The refusals each derive makes are unit-tested here. What an accepted derive
expands to is tested where the runtime is: `flui-view`, `flui-foundation`,
`flui-animation` and `flui-widgets` for behaviour, and the trybuild suites of
`flui-sdk`, `flui-view` and `flui-widgets` for diagnostics.
