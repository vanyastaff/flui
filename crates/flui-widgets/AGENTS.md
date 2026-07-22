# AGENTS.md — flui-widgets

> The user-facing, Flutter-style widget catalog (Business layer, L6). What an app
> author composes. Read this before adding or changing a widget.

## What this crate is

`flui-widgets` is the declarative surface over the render machine. Every widget
is a **small immutable configuration object** in one of these shapes:

| Shape | Trait | When | Example |
|---|---|---|---|
| Render-object widget | `RenderView` + `impl_render_view!` | wraps one `flui-objects` render box | `Padding`, `ColoredBox`, `Text` |
| Multi-child render widget | `RenderView` (hand-written `impl View`, generic over `C: ViewSeq`) | lays out a child sequence | `Flex`/`Row`/`Column` |
| Composition widget | `StatelessView` + `#[derive(StatelessView)]` | builds other widgets | `Container` |
| Parent-data widget | `ParentDataView` + `impl_parent_data_view!` | configures a child's parent-layout data | `Flexible`/`Expanded`/`Positioned` |
| Transition widget | `AnimatedView` + `impl_animated_view!` | rebuilds each `Animation` tick | `FadeTransition` |

The render *machine* (layout/paint/compositing) lives in `flui-rendering` +
`flui-objects`. **Do not** put layout math here — a widget only *configures* a
render object. If a needed render object is missing or a Leaf where Flutter is a
proxy, fix/extend `flui-objects`, then wrap it here.

## The authoring pattern (copy this for a new single-child widget)

```rust
#[derive(Clone, Debug)]
pub struct Foo { /* config */, child: Child }

impl Foo {
    pub fn new(/* config */) -> Self { Self { /* … */, child: Child::empty() } }
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view()); self
    }
}

impl RenderView for Foo {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFoo;
    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject { /* build from config */ }
    fn update_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>, ro: &mut Self::RenderObject) { /* set_* or `*ro = …` */ }
    fn has_children(&self) -> bool { self.child.is_some() }
    fn visit_child_views(&self, v: &mut dyn FnMut(&dyn View)) {
        if let Some(c) = self.child.as_ref() { v(c); }
    }
}
impl_render_view!(Foo);
```

- Public constructors take **`f32`** for dimensions and convert to `Pixels` with
  `px()` internally — keep `Pixels` out of the common call site.
- Single child: store `Child`; `.child(impl IntoView)`.
- Multi-child: be generic over `C: ViewSeq` (default `Vec<BoxedView>`) so both the
  static `column!`/`row!` tuple path and the dynamic `Vec<BoxedView>` path work
  (contract **C2**). Generic widgets can't use `impl_render_view!`; use the
  crate-local `generic_render_view_element!` macro in `support.rs`.
- If a render object exposes no setter, rebuild it in `update_render_object`
  (`*ro = …`) — the render-tree links live in the arena, not in the object.

## Flutter-parity gotchas already hit

- `RenderSizedBox` / `RenderColoredBox` in `flui-objects` are **Leaf** simplifications
  (no child). For Flutter parity, `SizedBox` wraps **`RenderConstrainedBox`** (tight
  constraints) and `ColoredBox` wraps **`RenderDecoratedBox`** (color decoration) —
  both Single-child, matching Flutter exactly.
- `Container::build` composes, child-outward: `Align → Padding → ColoredBox →
  DecoratedBox → ConstrainedBox → Padding(margin) → Transform` — Flutter's exact
  order. `width`/`height` fold into constraints via `tighten`/`tightFor`.
- `BoxDecoration` does not expose border insets, so `Container` does **not** fold a
  border's thickness into layout padding (documented on `Container`). Set `padding`
  explicitly if a bordered box must reserve the border.

## Testing — the parity oracle

Integration tests in `tests/` use the headless view-level harness
(`tests/common/mod.rs`): it mounts a root widget, runs `build_scope`, finds the
render-tree root (the parentless render node — works for `RenderView` and
`StatelessView` roots alike), drives a real `run_frame`, and reads back
`Size`/`Offset`. **No `WidgetsBinding` singleton** — tests are parallel-safe.
Owner-side fixture actions that can schedule local post-frame work must run through
the harness's owner scope; direct navigation/animation mutations outside that scope
correctly receive `InactiveLane` rather than silently queueing work.

Every new widget needs a test asserting a **computed** size/offset that would be
wrong without the widget (a regression that fails on a `Size::ZERO`/mis-wire).
Expected values come from Flutter's documented layout algorithm (the oracle), not
from running the code first. Text asserts non-degeneracy (positive w/h) because
exact glyph metrics are font-dependent.

## Gate before commit

```
cargo fmt -p flui-widgets -- --check
cargo clippy -p flui-widgets --all-targets -- -D warnings
cargo test  -p flui-widgets
```

## Design record

[`docs/adr/ADR-0009`](../../docs/adr/ADR-0009-flui-widgets-authoring-catalog.md) —
why the catalog is configuration-objects-over-render-objects, the C2/C3 surface,
and the Core.1 vertical-slice scope.
