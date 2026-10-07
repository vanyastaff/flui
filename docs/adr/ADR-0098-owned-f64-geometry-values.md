# ADR-0098: Geometry values are FLUI-owned `f64` structs; flui-geometry and flui-types dissolve

- **Status:** Proposed. The owner chose the direction on 2026-09-28: drop `px()` and the scalar
  unit types, and keep FLUI's own value types over kurbo and glam rather than exposing either.
  Acceptance waits on the review of this record.
- **Date:** 2026-09-28
- **Superseded-by:** [ADR-0149](ADR-0149-interpolation-contracts.md), in part: §7's `Color::lerp`
  bullet (colour now interpolates in Oklab, premultiplied)
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (kept as is: this
  record needs no exception to it), [ADR-0077](ADR-0077-migrate-to-parley.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md)
- **Refs:** [issue #1352](https://github.com/vanyastaff/flui/issues/1352); the
  [census](../research/2026-09-28-geometry-value-layer-census.md) (every count below); the
  [options research](../research/2026-09-28-geometry-options-research.md) (every external
  claim below); the earlier [euclid spike](../research/2026-05-29-u17-euclid-spike-report.md)

## Context

The value layer is two crates:

- **`flui-geometry`** (16.4k production lines) holds scalar unit types (`Pixels(f32)`,
  `DevicePixels(i32)`, `PixelDelta`, `Rems`, `Percentage`), geometry generic over them, its own
  Bézier and matrix math, and GPUI-era vocabulary with no consumer.
- **`flui-types`** (17.4k) mixes painting, typography, layout, gesture, physics and platform
  values that have other natural owners.

The census and the research found:

- **The unit types did not buy safety.**
  - `ScaleFactor` and `to_device` have no production caller.
  - flui-engine types physical rectangles as `Rect`, which says "logical".
  - 28 raster and platform sites convert by hand, each with its own rounding.
- **`f32` fails on the scroll axis.** Scroll offsets and sliver extents are `f32`; at 5e7
  logical px the step is 4 px, and small scroll deltas vanish (egui#1391 is the same failure).
  Flutter lays out in `double`.
- **`Pixels` breaks the Hash contract.** It is `PartialEq` on `f32` but hashes the bits. The
  `BoxConstraints` instance of this is fixed separately; this record removes the cause.
- **Exposing an upstream geometry crate costs more than it saves.**
  - **euclid:** same-named `Rect` methods have different semantics, patch releases have changed
    behaviour, and there are unit escape hatches.
  - **kurbo:** type identity changes with every major, its insets differ from Flutter's in
    direction and argument order, and it has no elliptical radius or perspective matrix.
  - **Both:** the orphan rule blocks FLUI's methods, constants, `Display` and `serde`.
  - **Precedent:** Slint and parley keep upstream geometry out of their APIs.
- **No newer crate replaces them.** kurbo is the 2026 standard for 2D-UI geometry in Rust, and
  glam for matrices. They are the right engines, just not the right public vocabulary for a
  Stable, Flutter-shaped facade.

## Decision

### 1. No scalar unit types

The following are deleted, together with their operator impls and compile-fail suites:

- `Pixels`, `DevicePixels`, `PixelDelta`, `Rems`, `Percentage`;
- the `Length`/`DefiniteLength`/`AbsoluteLength` family, `ScaleFactor`, `Radians`;
- the `Unit`/`NumericUnit`/`FloatUnit` traits;
- `px()` and `device_px()`.

A logical length is a plain number: `10.0` is ten logical pixels, as in Flutter, SwiftUI, egui
and Xilem. A newtype survives only where it enforces an invariant beyond "is a length", and its
doc comment names the invariant (§5's `DevicePixelRatio` is one).

### 2. One scalar: `f64`, narrowed once

- **`f64` everywhere logical.** Layout, constraints, sliver protocol, scroll positions and
  physics, paint offsets, hit-testing and semantics bounds all use `f64`.
- **One narrowing point.** The display list records `f64`; `flui-engine` narrows to `f32` where
  it ingests a command (the kurbo-to-lyon adapter, instance baking, uniform packing). By then
  coordinates are relative to the current layer, so they are small. The GPU stays `f32`. This
  mirrors Flutter's `double` framework over a `float` engine. Narrowing at record time instead
  would halve `DrawOp` (224 bytes now) but split the conversion across every recorder; measure
  before moving it.
- **One widening point.** Parley's `f32` metrics widen at the text boundary; widening is exact.
- **Tolerances follow Flutter's scale.** Scroll-axis comparisons use Flutter's
  `precisionErrorTolerance` (1e-10). Every `EPSILON_F32` and `1e-3` layout tolerance is
  re-derived.

### 3. Public value types

`flui-foundation` owns them in a `geometry` module:

| Type | Fields | Notes |
|---|---|---|
| `Point` | `x, y` | a position |
| `Offset` | `dx, dy` | a displacement; `Point ± Offset = Point`, and there is no `Point + Point`. `Point - Point` yields a `Vec2` until `Vec2` folds into `Offset` (Implementation status) |
| `Size` | `width, height` | never a displacement |
| `Rect` | `min: Point, max: Point` | with `left()`, `top()`, `right()`, `bottom()`, `width()`, `height()` and Flutter's constructors (`from_ltwh`, `from_ltrb`, `from_center`, `from_points`) |
| `EdgeInsets` | `left, top, right, bottom` | inward; the constructor order stays today's `new(top, right, bottom, left)` and `symmetric(vertical, horizontal)` |
| `Radius` | `x, y` | elliptical |
| `Matrix4` | private column-major storage | backed by `glam::DMat4` |
| `Axis` | enum | the one `Axis`, merging the two that exist |

Rules:

- **Layout.** All fields are `pub`, in kurbo's order. The types are generic over their scalar
  only, defaulting to `f64` (`Point<T = f64>`): `Point<i32>`, `Size<i32>` and `Rect<i32>` are
  the device-grid types, so the scalar alone keeps logical and device values apart and no
  space marker is needed. Constants (`Offset::ZERO`, `Size::INFINITY`) and inherent methods
  are FLUI's.
- **Semantics are chosen on their merits**, not copied from Flutter or from an upstream crate.
  Each rule is pinned by a test:
  - `Rect::contains` is half-open (`left <= x < right`), so a point on an edge shared by two
    rectangles hits exactly one of them. Today's inclusive version hits both.
  - `expand_to_include` counts empty rectangles, so a zero-size node still contributes its
    position to bounds.
  - `intersect` returns `Option<Rect>`, with `Some` for touching rectangles.
  - `is_empty` is `left >= right || top >= bottom`, so NaN counts as empty.
- **Where FLUI is stricter than Flutter.** `Point` and `Offset` are distinct types: Flutter uses
  `Offset` for both, which lets a position be added to a position.
- **Only what is used.** Methods are implemented only when the census shows a consumer. There is
  no `Vec2`, `Bounds`, `Line`, `Circle` or Bézier type here: shapes and curves belong to
  painting (§7).
- **kurbo interop stays inside.** Conversions to kurbo live in `flui-painting` as crate-internal
  functions, because the orphan rule forbids a `From` impl there. No kurbo or glam type appears
  in a public signature, so ADR-0089 needs no amendment.
- **Reversibility.** With the same scalar and field order as kurbo's `Point`, `Size` and `Rect`,
  a later move to kurbo aliases (after a kurbo 1.0, if ever) is a mechanical change. Exposing
  kurbo now and withdrawing it later would not be.

### 4. Float values are neither `Eq` nor `Hash`

- No type holding floats implements `Eq` or `Hash` over them.
- A cache keyed on float values uses `flui_foundation::geometry::canonical_bits`, which exists for
  `f32 → u32` and `f64 → u64`: `-0.0` becomes `+0.0`, and every NaN becomes one NaN.
- A bit-exact memo key (equality and hash over the raw bits) is allowed when it states that
  relation, as the layout-protocol cache keys do. Their existing tests pin it.
- `Color` is the one exception: it stays lawfully `Eq + Hash` by normalising at construction (§7).

### 5. Logical and physical meet only at the boundary

- **Physical types live only in platform and engine code.** Inside the framework everything is
  logical. The physical side has its own types in `flui_foundation::geometry`:
  - `DevicePoint`, `DeviceSize` and `DeviceRect`: `Point<i32>`, `Size<i32>` and `Rect<i32>`
    (signed extents, so a clamp to the attachment is a subtraction that cannot wrap);
  - `DevicePixelRatio`, a newtype whose constructor rejects non-finite, zero and negative values.

  Continuous physical coordinates (the engine's device-space maths) are private to `flui-engine`.
- **Each platform keeps its own source of truth:**
  - Windows reports the physical client size, and the logical size is `physical / ratio`, never
    rounded back.
  - Wayland fractional scale computes the buffer size in integers from the n/120 scale.
  - The web reads `devicePixelContentBoxSize` where available.
- **What types cannot catch.** Types cannot tell a local coordinate from a parent or global one;
  no framework's can. Hit-test and paint tests under non-identity transforms guard that.

### 6. Pixel snapping is an engine policy

`flui-engine` owns position snapping. Layout snaps one thing: border and stroke widths (last
rule below). Flutter's position that layout never snaps is what leaves its borders uneven at
fractional ratios ([flutter#151065](https://github.com/flutter/flutter/issues/151065),
[#90926](https://github.com/flutter/flutter/issues/90926)).

- **Rounding rule.** `snap(x)` is the nearest integer with ties toward +∞, as Skia's
  `sk_float_round` does. It is computed so that the float just below one half rounds to 0.
- **Edges, not sizes.** A snapped rectangle rounds `min` and `max` separately, so abutting
  rectangles stay abutting.
- **Covering bounds** (scissors, damage, layer and blur bounds) take the floor of the minimum
  and the ceiling of the maximum.
- **Where to snap.** Snapping happens in raster-target coordinates, after the ancestor
  transforms are composed, and only when the composed transform is a translation plus a positive
  axis-aligned scale. Under rotation, skew or perspective, content is antialiased and not
  snapped.
- **Border and stroke widths are resolved to whole device pixels during layout**, using the
  UI runtime's `DevicePixelRatio`. Firefox and GPUI do the same.
  - A width in (0, 1] device px becomes 1; anything wider takes the floor. A non-zero width
    therefore never disappears.
  - The content inset and the painted border agree exactly, because both use the resolved width.
  - A ratio change invalidates layout, as a window moving between monitors already does.
  - At paint, the inner edge is the snapped outer edge minus the resolved width.
  - An axis-aligned odd-width stroke centres on half pixels.
  - A hairline (width 0) is one device pixel under any transform.
- **Animated layers.** A layer whose transform is animating composites at its fractional offset
  and is not re-snapped each frame. Scroll offsets snap as translations.
- **Text** snaps each run's baseline origin once, on the cross axis only, and keeps subpixel
  glyph positions within the run.
- **Every pipeline goes through these functions:** solid, gradient, image, clip and blur.

### 7. Painting values belong to `flui-painting`

- **Painting values:** `Path`, `Paint`, shaders, gradients, images, decorations, borders,
  shadows and typography. `RRect` and `RSuperellipse` stay geometry values (§8): layers and
  the engine clip with them.
- **`Path` wraps a private `kurbo::BezPath` plus a shape hint.**
  - The hint is `Rect`, oval or `RRect`, set by the matching constructors and cleared by any
    other edit, so the analytic rounded-rect shadow keeps working.
  - Arcs are built with a fixed construction tolerance tight enough for the largest device
    scale. Containment is non-zero winding, and even-odd is `winding % 2`.
  - Strokes are outlined with kurbo and then filled.
- **`Color`** stores `f32` straight-alpha sRGB components privately and is `Eq + Hash`.
  - Its constructor rejects NaN and turns `-0.0` into `+0.0`, then compares bits.
  - It keeps `const fn from_argb(u32)` and the 8-bit constructors.
  - `Color::lerp` interpolates premultiplied components, then unpremultiplies. Fading a colour
    to transparent therefore keeps its hue instead of passing through dark grey, which Flutter's
    straight interpolation does ([flutter#48674](https://github.com/flutter/flutter/issues/48674)).
    CSS Color 4 interpolates premultiplied as well.
- **Gradients** carry an explicit interpolation colour space and alpha space.
  - The defaults are Oklab and premultiplied, as CSS Color 4 chose for modern colour syntax.
    Two-stop gradients then keep a perceptually even midpoint and no dark band.
  - sRGB and linear sRGB stay selectable, for matching designs made in tools that use them.
  - Both defaults differ from Flutter, and each is pinned by a test.
- **Upstream crates stay inside.**
  - `color` may back `Color`'s conversions internally.
  - peniko is added only where the display list stores its type as-is.
  - lyon remains an engine detail behind one kurbo-to-lyon adapter.
  - kurbo, color and peniko are bumped together as one version group.

### 8. Owners replace both crates

| Today | Owner |
|---|---|
| geometry values, `Matrix4`, device types, `DevicePixelRatio`, `canonical_bits` | `flui_foundation::geometry` (foundation and geometry both have 29 dependents, so rebuild fan-out does not grow; foundation gains `glam`) |
| `Path`, paint, shaders, gradients, images, colours, decorations, borders, shadows, typography | `flui-painting` |
| `RRect`, `RSuperellipse` | `flui_foundation::geometry`: rendering, layers and the engine clip with them, below painting's consumers |
| `Alignment`, `BoxFit`, `BoxShape`, `TextBaseline` | `flui-painting` |
| snapping, the physical-coordinate maths, the kurbo-to-lyon adapter | `flui-engine` |
| box and sliver constraints, `AxisDirection`, `TableCellVerticalAlignment`, `CacheExtentStyle` | `flui-rendering` (the protocol and parent data read them) |
| flex, stack, wrap and table-column enums, `VerticalDirection` | `flui-objects`, beside the render objects that read them; `flui-rendering` never does |
| simulations | `flui-animation` (duplicates deleted) |
| gesture details, `Velocity`, `PointerDeviceKind` | `flui-interaction` |
| IME, haptics, `Brightness`, `Locale`, `TargetPlatform` | `flui-platform-api` |
| `MaterialColors` and design-system colours | `flui-material` / `flui-cupertino` |
| `PointerData`, `OffsetPair`, `DeviceOrientation`, `FractionalOffset`, `Orientation`, `MaterialColors`, Bézier types, text-path types, and `flui-types`' duplicates of `BoxConstraints`, `FlexFit`, `CacheExtentStyle` and the simulations | deleted (no consumer, or an in-use counterpart) |
| `Line`, `Circle`, `Vec2`, `Bounds` | still in `flui_foundation::geometry`; pruning them to the census's consumers is follow-up work |

- Both crates leave the workspace once `cargo tree -i` and a source search find no dependent.
- No family becomes its own crate.
- The facade and `flui-sdk` re-export each type once, from its owner.

### 9. Order

Each step is one PR that builds and passes `cargo xtask check-changed`:

1. This record, the census and the research.
2. `flui_foundation::geometry` with §3–§5's types and their tests. Temporary `From` conversions
   to the old `flui-geometry` types, deleted in step 5.
3. Consumers, lowest tier first. Each crate moves to the new types and to `f64`, converting at
   its edge to crates not yet moved:
   - `flui-platform-api`, `flui-painting`, `flui-interaction`, `flui-semantics`,
     `flui-animation`;
   - `flui-layer`, `flui-rendering` (the sliver protocol's `f64` change lands here),
     `flui-objects`, `flui-engine` (§6 and the narrowing point);
   - the spine, the runtime, the packages and the facade.
4. §6's rules in the engine, with readback tests.
5. `flui-geometry` is deleted.
6. §7, then the `flui-types` families by §8, one family per PR. `flui-types` is deleted.
7. The census reruns with the same commands.

## Implementation status

The migration landed on one branch, in the steps of §9. What shipped:

- **§1–§2.** No unit types, no `px()`; every logical value is `f64`, including scroll and the
  sliver protocol. Scroll-axis tolerances use `flui_foundation::EPSILON` (1e-10).
- **§3–§5.** The values live in `flui_foundation::geometry`, with the one `Axis`. Float
  types are neither `Eq` nor `Hash`; `canonical_bits` covers `f32` and `f64`. `DevicePixelRatio`
  and the device aliases exist, and the compile-fail suite
  (`crates/flui-painting/tests/compile_fail/`) rejects `Point + Point`, a `Size` as an
  `Offset`, a `DevicePoint` as a `Point`, a literal `DevicePixelRatio`, and mixing `f64` with
  `i32` geometry.
- **§6.** `geometry::{snap, snap_edges, cover, device_rect_covering, device_size,
  resolve_stroke_width}`, pinned for negative, half-way and non-finite input. The engine's
  scissors, damage, backdrop and blend copy regions go through them: a hard rect clip snaps its
  edges under a translation plus a positive scale, and every bound in front of an SDF or a copy
  covers (`crates/flui-engine/ARCHITECTURE.md` mapping decision 17).
- **§7.** `Path` is a kurbo `BezPath` plus a shape hint, with exact winding, tight bounds and
  arcs built at a fixed tolerance, and one kurbo-to-lyon adapter in the engine. `Color::lerp`
  is premultiplied (`crates/flui-painting/ARCHITECTURE.md` mapping decision 13).
- **§8.** Both crates are deleted; the owner table above is what the code does.

What is deferred, each with its reason:

- **Content snapping** (solid, gradient and image quads; animated layers unsnapped), **border
  and stroke widths resolved in layout** (`resolve_stroke_width` exists, nothing calls it yet)
  and **text baseline snapping**. Together they move every readback and need the engine to
  know which layers animate; they land as one change with readbacks that tell a snapped edge
  from an antialiased one (`crates/flui-engine/ARCHITECTURE.md`, Open items).
- **Platform sizing formulas** (Windows physical-first, Wayland n/120, the web's
  `devicePixelContentBoxSize`). Backend changes that only the Linux path runs in CI; each needs
  its platform's smoke run.
- **§3's semantic changes**: a half-open `Rect::contains`, empty rectangles counted by
  `expand_to_include`, `intersect` returning `Option`. Each changes hit-testing or bounds
  results across the widget catalog and needs its own pass over the harness.
- **`Color` in `f32` storage behind the `color` crate, and gradient interpolation spaces**
  (Oklab, premultiplied). `Color` stays four `u8` channels, which are already lawfully
  `Eq + Hash`; wide-gamut and gradient-space work change the engine's shaders and belong
  together. peniko is not added: the display list stores no peniko type.
- **Pruning `Vec2`, `Bounds`, `Line`, `Circle`** and the scalar traits to their consumers.

## Alternatives considered

- **euclid aliases in the Stable facade**, this record's first draft. Rejected: same-named
  methods behave differently, patch releases have changed semantics, and the unit escape hatches
  mean the typed spaces are not sound. The orphan rule blocks FLUI's API on those types, and the
  f32 scalar still needs an f64 carve-out for scrolling. The research gives the evidence.
- **kurbo types directly**, as Xilem, Blitz and anyrender do. Rejected for a Stable,
  Flutter-shaped facade:
  - every kurbo major becomes a FLUI major, because type identity changes;
  - `Insets` differ in direction and argument order, 59 silent call-site bugs;
  - kurbo lacks elliptical radii, a perspective matrix and superellipses, so the vocabulary would
    be half kurbo and half FLUI;
  - methods and constants would need extension traits, which an upstream inherent method
    silently shadows.

  §3 keeps the move available later.
- **f32 box model with an f64 scroll carve-out.** Rejected: two scalars and a list of fields that
  change type, where Flutter uses one.
- **Fixed point (Blink `LayoutUnit`, Gecko `nscoord`).** Rejected: a range cap and saturating
  arithmetic, which f64 avoids.
- **Keep both crates and delete dead code.** Rejected: it keeps the unit types that bought
  nothing, the f32 scroll failure and a crate whose only reason to exist is the vocabulary
  foundation can hold.
- **Taffy as the layout foundation.** Out of scope: it does not replace the box and sliver
  protocols.

## Consequences

- **A pre-1.0 breaking change for every consumer.**
  - `px(10.0)` becomes `10.0`.
  - `Size` becomes `Size`; `Rect` becomes `Rect`.
  - Lengths become `f64`, and `f32` values need `f64::from` at the call site.
  - `flui::geometry` and `flui::types` are replaced by owner paths.
  - Float-holding types lose `Eq` and `Hash` (`Color` keeps them).
- **Behaviour changes.** Each has a test and, where it differs from Flutter, a mapping-decision
  entry:
  - a hard rect clip keeps exactly the pixels whose centres are inside, and every covering
    bound (damage, SDF scissors, backdrop copies) keeps every partly covered pixel;
  - `Color::lerp` is premultiplied;
  - `Path` bounds are tight (a curve's extremes, not its control points), and arcs are curves
    built to 1e-3 logical px;
  - completed animation runs end exactly on their last frame (elapsed time is taken on the
    clock's integer grid).

  §3's rectangle semantics, content snapping, layout-time stroke widths and Oklab gradients
  follow (Implementation status).
- **Memory.** Layout values double in size. Paths in `f64` are about twice their current
  memory, and so is the display list until narrowing moves to record time (§2).
- **Code size.** The target is about 5–6k lines of value code across foundation and painting,
  in place of 33.8k. As shipped, `flui_foundation::geometry` is 14.7k lines and painting's value
  modules 15.6k, tests included (`cat … | wc -l`); the pruning in Implementation status is what
  closes the gap.
- **Dependencies.** No upstream crate enters a Stable signature. Foundation gains `glam`, and
  painting gains `kurbo`.

## Verification

Exists with this record: the census and the research.

Lands with the steps of §9:

- **Value types.** Unit and property tests: `Rect` intersection, union and containment,
  including empty, touching and NaN cases; transform round trips through `Matrix4`; and
  `canonical_bits` agreeing with float equality for `f32` and `f64`.
- **Compile-fail cases:**
  - `Point + Point`;
  - a `Size` passed where an `Offset` is expected;
  - a `DevicePoint` passed where a `Point` is expected;
  - `DevicePixelRatio` constructed from a literal without the checked constructor.
- **Scroll precision.** A sliver list scrolled to 5e7 px places children at their exact offsets
  and responds to a 0.3 px delta.
- **Snapping, at DPR 1.25, 1.5, 1.75 and 2.625:**
  - uniform borders at fractional origins;
  - a 0.4 px divider that stays visible;
  - abutting `Row` children with no uncovered pixel (readback);
  - a 45°-rotated rectangle that bypasses snapping;
  - a slow fractional layer animation with no 1-pixel stepping;
  - unchanged glyph spacing within a run as the run moves.
- **Platform sizing.** Windows at 1001 px and 125% lays out at 800.8 logical; Wayland's buffer
  size for scale numerator 138 matches the integer formula.
- **Painting.**
  - The analytic shadow still fires for an `RRect` path.
  - A large scaled circle stays within 0.25 px.
  - `Color::lerp` from opaque red to transparent keeps red's hue at the midpoint.
  - A two-stop Oklab gradient has no dark band.
  - 8-bit colour round-trips all 256 values.
- **Closure.**
  - A source search finds no `Pixels`, `px(`, `flui_types` or `flui_geometry` after step 6.
  - `cargo xtask api-closure` (ADR-0089 §6) finds no kurbo, glam, color, peniko or lyon path.
  - The render-object harness stays green.
