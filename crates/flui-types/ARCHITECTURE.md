# flui-types Architecture

`flui-types` is the value vocabulary the rest of the framework speaks: colours and
decorations, the paint vocabulary (`Path`, `Shader`, `BlendMode`, filters), typography,
layout enums and constraints, gestures, physics simulations and platform values. Geometry is
`flui-geometry`, re-exported as `flui_types::geometry`. Nothing here draws or lays out; these
are the values that painting, layout and the widgets pass around.

The reference is Flutter's `dart:ui` and `painting` libraries. Where a type does something
better than a line-for-line port would, the choice is recorded under
[Mapping decisions](#mapping-decisions) and pinned by a test.

---

## Mapping decisions

### 1. `Color` stores 8-bit channels

`Color` is four `u8`s with straight alpha; Flutter 3.44's `Color` holds `f32` components in a
colour space. Everything that consumes a colour here (the display list, the GPU upload, the
CPU image filters) works in 8-bit sRGB, and four bytes keep `Color` `Copy`, `Eq` and `Hash`,
which a float colour cannot be. The conversions that produce a channel from a float round to
the nearest step (`with_opacity`, `lerp`, `blend`, `blend_over`, HSL/HSV), the way the GPU's
float-to-unorm8 conversion does, so the CPU and GPU paths agree.

### 2. `FontWeight::from_css` breaks ties the way CSS does

`from_css` maps a CSS numeric weight to the closest hundred, and an exact half goes the way
CSS Fonts 4's font matching searches for a missing weight: lighter below 400, heavier from 400
up (350 is `W300`, 450 is `W500`). Flutter has no such mapping, and the one place it snaps a
weight to the 100 scale, `FontWeight.lerp`, uses a plain `round()` that would send 350 to
`W400`, the opposite of what a browser picks. Pinned by `font_weight_from_css_buckets`
(`src/typography/text_style.rs`).

### 3. A one-sided focal point lerps to the other gradient's center

`RadialGradient::lerp` follows Flutter's `RadialGradient.lerp` (joint stops, sampled colours,
radii clamped at zero) except for a focal point set on one side only. A gradient without a
focal point is focused on its center, so the focal point moves to or from that center and the
result at `t = 1` paints exactly like the other gradient. Flutter lerps it toward
`Alignment(0, 0)`, which jumps at the end whenever the center is elsewhere. Pinned by
`radial_lerp` (`src/styling/gradient.rs`).

### 4. `Path::add_arc` continues an open contour

Flutter's `Path.addArc` always starts a new sub-path. Here an arc appended to an open contour
is joined to it by a chord, which is what lets `Path::from_rrect` build a rounded rectangle as
one contour that `rrect_hint` can recognize; an arc with nothing open starts its own. The
method's docs give the full reasoning, and the tessellator in `flui-engine` and
`Path::contains` implement the same rule. Pinned by
`an_arc_joins_an_open_contour_and_starts_a_closed_one_fresh` (`src/painting/path.rs`).

### 5. `Path::contains` flattens to the renderer's tolerance

Skia answers `contains` on the exact curves. `Path::contains` flattens arcs and Bézier curves
into chords within 0.1 px, the same bound `flui-engine`'s tessellator fills with, so a point
hit-tests as inside exactly when the painted shape covers it. The chord count follows each
curve's size (Wang's formula for Béziers, the sagitta bound for arcs) rather than being fixed.
Pinned by `curve_containment_matches_the_exact_region` and
`arc_chord_count_is_the_fewest_within_tolerance` (`src/painting/path.rs`).
