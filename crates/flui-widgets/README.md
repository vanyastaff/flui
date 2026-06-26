# flui-widgets

The user-facing, Flutter-style **widget catalog** for [FLUI](../../README.md) —
the layer an app author composes. Every widget is a small, immutable
*configuration object* over the `flui-objects` render catalog: declarative on the
outside, the gold-standard render machine underneath.

```rust
use flui_widgets::prelude::*;
use flui_widgets::{column, row}; // the ViewSeq macros (shadow std's same-named)

Container::new()
    .color(Color::rgb(18, 18, 24))
    .padding(EdgeInsets::all(px(24.0)))
    .child(Column::new(column![
        Text::new("Hello, FLUI"),
        SizedBox::height(12.0),
        Row::new(row![
            ColoredBox::new(Color::rgb(229, 57, 53)).child(SizedBox::square(64.0)),
            SizedBox::width(12.0),
            ClipOval::new().child(SizedBox::square(64.0)),
        ]),
    ]))
# ;
```

See [`examples/widgets_gallery.rs`](../../examples/widgets_gallery.rs) for a
runnable demo (`cargo run -p flui --example widgets_gallery`).

## What's in the box

| Family | Widgets |
|---|---|
| **Layout** | `Padding` · `Align` · `Center` · `SizedBox` · `ConstrainedBox` · `LimitedBox` · `AspectRatio` · `FittedBox` · `FractionallySizedBox` · `Transform` · `FractionalTranslation` |
| **Flex / Stack** | `Flex` · `Row` · `Column` · `Stack` |
| **Paint** | `ColoredBox` · `DecoratedBox` · `Opacity` · `RepaintBoundary` |
| **Clip** | `ClipRect` · `ClipOval` |
| **Interaction** | `IgnorePointer` · `AbsorbPointer` · `Offstage` |
| **Composition** | `Container` |
| **Text** | `Text` |

Each is **behavior-loyal to Flutter** (same layout/paint algorithm) with a
**Rust-native** surface: compile-time child-arity safety, `f32` at the call site
(`Pixels` conversion is internal), and a chainable `#[must_use]` builder API.

## How it composes (the three shapes)

- **Render-object widget** — wraps one render box (`Padding`, `Text`, …): a
  `RenderView` + `impl_render_view!`.
- **Multi-child widget** — `Row`/`Column`/`Stack`: generic over
  `C: ViewSeq` (default `Vec<BoxedView>`), so the static `column!`/`row!` tuple
  path (monomorphic per child) and the dynamic `Vec<BoxedView>` path are *one
  type* (contract C2).
- **Composition widget** — `Container`: a `StatelessView` that builds a stack of
  other widgets in Flutter's exact order.

It is **reactive**: a `setState`/rebuild that changes a widget's configuration
updates its render object in place (no remount), exactly as Flutter does.

## Status

This is the [Core.1 vertical slice](../../docs/ROADMAP.md) — it proves the whole
`build → layout → paint → composite → reconcile` pipeline on live widget code.
Not yet shipped (tracked): `Flexible`/`Expanded`/`Positioned` (parent-data),
`ClipRRect`/`ClipPath`, `Image`, scrolling, implicit animations, and gesture
widgets. See [`AGENTS.md`](AGENTS.md) for the authoring pattern and
[`docs/adr/ADR-0009`](../../docs/adr/ADR-0009-flui-widgets-authoring-catalog.md)
for the design rationale.

## Testing

Integration tests in [`tests/`](tests/) drive a **headless view-level harness**
(`tests/common/mod.rs`): mount a widget tree, run a real frame, and assert the
computed `Size`/`Offset` against Flutter's layout algorithm — no GPU, no window,
no singleton, so they run in parallel. Every widget carries a parity test that
would fail if it mis-wired its render object.

```
cargo test  -p flui-widgets
cargo clippy -p flui-widgets --all-targets -- -D warnings
```

## License

MIT OR Apache-2.0, matching the workspace.
