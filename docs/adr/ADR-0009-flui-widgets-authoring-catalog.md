# ADR-0009: flui-widgets is a catalog of configuration objects

- **Status:** Accepted
- **Date:** 2026-06-25

## Context

The render machine and the View/Element spine existed before any user-facing widget did. An
authoring-API flaw found halfway through the catalog would be a catalog-wide rewrite, so the
first widgets (box layout, flex, `Container`, `Text`) were built as a thin end-to-end slice to
fix the widget shape early.

Two facts shaped it. The authoring API was already in place: `build` returns `impl IntoView`,
`RenderView` exposes `create_render_object`/`update_render_object`/`visit_child_views`, and
`ViewSeq` covers tuples up to 16 and `Vec<BoxedView>`. And some `flui-objects` render boxes were
childless leaves where Flutter has proxies (`RenderSizedBox`, `RenderColoredBox`), so a widget
built on the convenient primitive would silently lose its child.

## Decision

**A widget is a small immutable configuration object**, over a render object or composed of
other widgets. Three shapes, no inheritance simulation:

- **Render-object widget.** Implements `RenderView`, wraps one render box; a single child is
  stored as `Child` and set with `.child(impl IntoView)`.
- **Multi-child render widget.** `Flex`/`Row`/`Column` are generic over `C: ViewSeq` with
  default `Vec<BoxedView>`, so one type serves both child paths: `row!`/`column!` produce a
  monomorphic tuple, a `Vec<BoxedView>` carries a dynamic list.
- **Composition widget.** A `StatelessView` whose `build` composes other widgets. `SafeArea` is
  the exemplar.

**Build on the primitive that has Flutter's semantics, not the convenient one.** `SizedBox`
wraps `RenderConstrainedBox` with tight constraints; `ColoredBox` wraps `RenderDecoratedBox`.

**Authoring surface.** A constructor plus chainable `#[must_use]` config methods; `f32` at the
call site, `Pixels` internally. Children are erased to `BoxedView`/`ViewSeq` at the widget
boundary, so widget types are non-generic (single child) or take one type parameter
(multi-child).

**Widgets are tested at view level**: mount a root widget into an `ElementTree` +
`PipelineOwner`, run a real frame, assert `Size`/`Offset`. No process singleton, so tests run
in parallel.

### A composition widget may collapse into one render object — narrowly

`Container` is a `RenderView` over one `RenderContainer` that carries alignment, padding,
margin, color, decoration, constraints and transform as fields. Flutter's `Container.build` is
a conditional stack (`Align` → `Padding` → `ColoredBox` → `DecoratedBox` → `ConstrainedBox` →
`Padding` → `Transform`), each level present only while its field is set; toggling a field
inserts or removes a level above the caller's child, so an unkeyed stateful child is rebuilt
from scratch (flutter/flutter#161698). One render object makes the child's slot fixed.

Composition stays the default. A composition widget may collapse into a single render object
only when:

(a) its optional levels depend on caller-visible properties **and** its child slot is the
caller's — toggling a property would otherwise insert or remove an element between the widget
and an unkeyed caller-supplied child; **and**

(b) every absorbed level is pure box-protocol geometry or paint, with no build, no parent data
and no element identity of its own.

`SafeArea` and `Card` fail (a): both are fixed depth. `ListTile` fails (b): its conditional
members are child widgets with their own identity. `Material` is already a single render
object.

## Flutter divergences

- `Container` is one render object, not a widget stack (above).
- Some render objects are rebuilt wholesale in `update_render_object` until they gain setters.

## Alternatives rejected

- **Generic-over-child single-child widgets (`Padding<W: View>`).** Nested concrete types
  (`Padding<Center<ColoredBox<…>>>`) explode compile times and type names; Flutter erases here
  too.
- **`SizedBox`/`ColoredBox` over the leaf render boxes.** No child support — a regression that
  looks finished.
- **Testing widgets at render-object level.** Re-tests `flui-objects`, not whether the widget
  wires its render object and child.
- **Driving widget tests through a binding singleton.** Forces serial tests.
