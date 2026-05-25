# Core.2 Wave 3a — Clip Render-Object Family

**Phase:** Core.2 (Render-Object Catalog) — Wave 3a
**Date:** 2026-05-24
**Scope:** one new file (`crates/flui-rendering/src/objects/clip.rs`)
hosting the entire clip family
**Result:** ✅ all gates green

---

## Goal

Land the `RenderClipRect` / `RenderClipRRect` / `RenderClipOval` /
`RenderClipPath` family — needed by `Container`, `Card`, `Chip`,
`CircleAvatar`, and every Material/Cupertino surface that does visual
clipping — **as a single generic implementation** rather than four
hand-written render objects.

This wave is the showcase for the Rust-side architectural improvement
flagged in Wave 1's phase notes: collapsing Flutter's 4-class private
`_RenderCustomClip<T>` mixin chain into one monomorphisable struct.

## What was built

Single file `crates/flui-rendering/src/objects/clip.rs` containing:

| Item | Role |
|---|---|
| `Oval` (newtype around `Rect<Pixels>`) | distinguishes "rectangle bounding an ellipse" from a rectangular clip at the type level |
| `mod sealed` + `Sealed` trait | gate the `ClipGeometry` trait so downstream crates cannot add shapes the engine cannot render |
| `trait ClipGeometry` | per-shape behavior: `default_for_size`, `contains`, `apply_to_canvas` |
| `impl ClipGeometry for Rect<Pixels>` | rectangular clip |
| `impl ClipGeometry for RRect` | rounded-rect with full per-corner ellipse hit-test (4 elliptical corners with independent X/Y radii) |
| `impl ClipGeometry for Oval` | inscribed-ellipse clip and hit-test |
| `impl ClipGeometry for Path` | path clip; canvas-side clip via `clip_path`, hit-test currently permissive (defer to child) — matches Flutter |
| `type CustomClipper<S>` | `Arc<dyn Fn(Size) -> S + Send + Sync + 'static>` — Flutter's `CustomClipper<T>.getClip(size)` analog, type-erased over the shape |
| `struct RenderClip<S: ClipGeometry>` | single generic render object — paint/hit-test bodies never branch on shape, calls dispatch through the sealed trait |
| `type RenderClipRect = RenderClip<Rect<Pixels>>;` | ergonomic name parity with Flutter |
| `type RenderClipRRect = RenderClip<RRect>;` | … |
| `type RenderClipOval = RenderClip<Oval>;` | … |
| `type RenderClipPath = RenderClip<Path>;` | … |

Plus wiring: `objects/mod.rs` registers `clip` and re-exports
`ClipGeometry`, `CustomClipper`, `Oval`, `RenderClip`, `RenderClipRect`,
`RenderClipRRect`, `RenderClipOval`, `RenderClipPath`.

LOC: **~810 (file)** / **15 new tests**.

## Rust-native improvement (the headline)

Flutter shape:

```text
_RenderCustomClip<T> (abstract, library-private)
├── RenderClipRect    extends _RenderCustomClip<Rect>
├── RenderClipRRect   extends _RenderCustomClip<RRect>
├── RenderClipOval    extends _RenderCustomClip<Rect>
└── RenderClipPath    extends _RenderCustomClip<Path>
```

Each subclass:

* Duplicates the same `_clipper` / `_clip` / `clipBehavior` field
  cluster (inherited from the abstract base).
* Overrides `_defaultClip` (the only thing that genuinely varies for
  the no-custom-clipper case).
* Overrides `hitTest` for the contain-test (`Rect.contains` vs ellipse
  hit-test for `RenderClipOval` vs path containment).
* Overrides `paint` to pick the right `canvas.clipRect` /
  `canvas.clipRRect` / `canvas.clipPath` call.

Rust shape after Wave 3a:

```text
trait ClipGeometry (sealed)
├── impl for Rect<Pixels>
├── impl for RRect
├── impl for Oval        ← newtype around Rect, distinguishes intent
└── impl for Path

struct RenderClip<S: ClipGeometry>   ← single, generic, monomorphised
type RenderClipRect   = RenderClip<Rect<Pixels>>;
type RenderClipRRect  = RenderClip<RRect>;
type RenderClipOval   = RenderClip<Oval>;
type RenderClipPath   = RenderClip<Path>;
```

Wins:

1. **Field cluster lives in one place** (`RenderClip<S>`).
   Adding a new field — say `clip_op` — touches one line, not four.
2. **No vtable dispatch on the hot path.** Each instantiation
   monomorphises to a dedicated type; the generic body's calls to
   `S::default_for_size`, `S::contains`, `S::apply_to_canvas` are
   direct calls after monomorphisation. No `Box<dyn>`, no `dyn`
   anywhere in the paint/hit-test path.
3. **Closed shape set.** The sealed trait prevents downstream crates
   from adding shapes the engine cannot render. Flutter's library-
   private base class enforces the same boundary but only within the
   `flutter/rendering` package; sealing is enforced compiler-side.
4. **Intent typing.** `Oval(Rect<Pixels>)` is a *distinct type* from
   `Rect<Pixels>`. Passing a bare `Rect` to a `RenderClipOval`
   constructor doesn't compile. In Flutter the oval / rect ambiguity
   lives at runtime; here it is unrepresentable.
5. **Custom clipper preserved as a behavioural extension point.**
   `Arc<dyn Fn(Size) -> S + Send + Sync + 'static>` keeps the
   Flutter `CustomClipper<T>.getClip(size)` ergonomics — same hook,
   typed per-shape rather than via an abstract class.
6. **`Clone` is preserved even with a closure clipper** because the
   closure lives behind `Arc`. Flutter's `customClipper` field is
   immutable and shared; the Rust shape mirrors that semantic
   exactly via `Arc::clone`.

## Behavior loyalty

* **`Rect::contains`** delegates to the geometry crate's existing
  rectangular contain-test.
* **`RRect::contains`** ports the per-corner elliptical hit-test
  algorithm from
  `crates/flui-rendering/migration/HIT_TEST.md::test_rrect_contains`,
  extended to handle independent x/y radii per corner (Flutter's
  `RRect` shape).
* **`Oval::contains`** uses the standard ellipse equation
  `((x − cx)/rx)² + ((y − cy)/ry)² ≤ 1` — equivalent to Flutter's
  `RenderClipOval._oval.contains(position)`.
* **`Path::contains`** is permissive (returns `true`) — matches
  Flutter's behaviour when no path-containment test is available
  in the framework layer. The corresponding backend-level check is
  the engine's responsibility.
* **`apply_to_canvas`** routes each shape through the existing
  `Canvas::clip_rect_ext` / `clip_rrect_ext` / `clip_path_ext`
  primitives with `ClipOp::Intersect` and the configured
  `clip_behavior`. Oval is rendered as an inscribed `RRect` with
  half-extent corner radii — exact for the inscribed-ellipse case;
  a future backend may specialise.

## Why decoration / repaint-boundary moved to Wave 3b

Wave 3 originally bundled `RenderDecoratedBox` and
`RenderRepaintBoundary` alongside the clip family. Both need
infrastructure that is out of scope for a tight Wave 3a PR:

* **`RenderDecoratedBox`** depends on a `paint_box_decoration`
  composition (color + gradient + image + border + shadow), which
  the painting layer does not currently expose as a single call.
  Wiring it requires touching `flui-painting` API surface.
* **`RenderRepaintBoundary`** needs to integrate with the layer
  tree (push a `RepaintBoundaryLayer`). The layer wiring is on the
  D-7 hardening list per [`docs/ROADMAP.md` Cross.H] and is
  scheduled separately.

Splitting them out keeps Wave 3a self-contained and reviewable.

## Gates

| Gate | Command | Result |
|---|---|---|
| Build | `cargo check -p flui-rendering` | ✅ |
| Workspace build | `cargo check --workspace` | ✅ |
| Tests | `cargo test -p flui-rendering --lib` | ✅ **341 passed (+15 new)** |
| Lints | `cargo clippy -p flui-rendering --all-targets -- -D warnings` | ✅ clean |
| Format | `cargo fmt -p flui-rendering --check` | ✅ |
| Port-check refusal triggers | `bash scripts/port-check.sh` | ✅ **13/13 + FR-033 clean** |

## Coverage delta

* **Render objects implemented:** 11 → **15** (4 clip variants, sharing
  one generic implementation).
* **`flui-rendering` paint-effects family:** the clip sub-family is
  closed for L6 widget needs.
* **Widget catalog unblocked:** any widget that asks for visual
  clipping — `ClipRect`, `ClipRRect`, `ClipOval`, `ClipPath`,
  `Card`, `Chip`, `CircleAvatar`, plus the visual-clipping leg of
  `Container.clip_behavior`.

## Next steps

Per the wave plan in
[`widget-renderobject-map.md`](widget-renderobject-map.md), the next
parallel waves are:

* **Wave 2** — multi-child layout (`RenderStack`, `RenderWrap`,
  `RenderTable`, `RenderListBody`).
* **Wave 3b** — `RenderDecoratedBox` + `RenderRepaintBoundary`
  (requires `flui-painting`/`flui-layer` surface work).
* **Wave 4** — pointer + simple proxy (`RenderAbsorbPointer`,
  `RenderIgnorePointer`, `RenderOffstage`, `RenderMetaData`,
  `RenderFittedBox`, `RenderFractionalTranslation`).
* **Wave 7** — `RenderParagraph` (Core.1 critical path).

Waves 2, 3b, and 4 share no files with Wave 3a; they remain
independently parallelizable.
