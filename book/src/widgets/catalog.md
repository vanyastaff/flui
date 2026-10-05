# Widget catalog

The framework crates are not yet published to crates.io (only the `flui-cli` tool is), so there
is no docs.rs page for the widgets to link to yet — until there is, this page points at the source
tree and the runnable catalog instead of listing every widget by name here (a hand-maintained list
would go stale the moment a widget is added; the source directories and the gallery example do
not). This page has no screenshots yet.

## See it running

```bash
cargo run --example widgets_gallery
```

opens a window walking the catalog live — see [Getting Started](../getting-started/installation.md) for
how to build and run it. [`examples/README.md`](https://github.com/vanyastaff/flui/blob/main/examples/README.md)
indexes every bundled example by category, several of which exercise a specific widget or widget
group in isolation.

## Controlled values: Slider and Disclosure

`Slider` and `Disclosure` report proposed values through `on_changed`; the parent decides
whether to commit them. Their displayed and accessible state changes when the parent
rebuilds with the new value. Without a callback, the control is disabled.

This illustrative excerpt belongs inside a `ViewState::build` method receiving `ctx`.
Assume `self.volume: Signal<f64>` and `self.expansion: Signal<ExpansionState>` were created
in `init_state`, initially `50.0` and `ExpansionState::Collapsed`. Read them in `build` and
write from the event callback, as in [Counter → Todo](../getting-started/tutorial-todo.md).
It is not a complete application:

```rust,ignore
use flui::prelude::*;
use flui::widgets::column;

let volume = self.volume;
let expansion = self.expansion;
let range = match NumericRange::new(volume.get(ctx), 0.0, 100.0, 5.0) {
    Ok(range) => range,
    Err(error) => return Text::new(format!("Invalid volume range: {error}")).boxed(),
};

Column::new(column![
    Slider::new(range)
        .label("Volume")
        .on_changed(move |cx, proposed| volume.set(cx, proposed)),
    Disclosure::new(
        expansion.get(ctx),
        Text::new("Details"),
        Text::new("Additional settings"),
    )
    .on_changed(move |cx, proposed| expansion.set(cx, proposed)),
])
.boxed()
```

`NumericRange::new(value, min, max, step)` returns a `Result`: every argument must be
finite, the bounds must admit the current value, and `step` must be positive. The step
governs keyboard and increment/decrement proposals; an admitted exact numeric value from
an assistive setter is not rounded to a step boundary. A zero-span range is valid but inert
for adjustments.

Give Disclosure a passive header such as `Text`; Disclosure owns header activation.
Collapsing unmounts the body, removing its layout, input, focus and accessibility
contributions. It does not guarantee retention of child-local state; keep data that must
survive collapse in the parent. See [Lifecycle](../concepts/lifecycle.md) for ownership.

## Source, by crate

- **`flui-widgets`** — the core, theme-independent catalog: layout (flex, stack, wrap, clip,
  physical model), scrolling, text, images, icons, animation/transitions, overlays, navigation, and
  the app-level `container`/`app` scaffolding.
  [`crates/flui-widgets/src/`](https://github.com/vanyastaff/flui/tree/main/crates/flui-widgets/src)
- **`flui-material`** — Material Design widgets (buttons, app bar, scaffold, cards, dialogs,
  navigation, form controls) built on `flui-widgets` plus `ColorScheme`/`ThemeData`, an
  official package on `flui-sdk`.
  [`packages/flui-material/src/`](https://github.com/vanyastaff/flui/tree/main/packages/flui-material/src)
- **`flui-cupertino`** — iOS-style widgets (buttons, nav bar, page/tab scaffold, theming) built on
  `flui-widgets` plus `CupertinoTheme`, an official package on `flui-sdk`.
  [`packages/flui-cupertino/src/`](https://github.com/vanyastaff/flui/tree/main/packages/flui-cupertino/src)

Once the framework crates publish (see `docs/ROADMAP.md`), this page becomes a thin index into
docs.rs instead of the source tree directly.
