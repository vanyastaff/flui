# Geometry and value-layer census

- **Date:** 2026-09-28, at `39e138762` (main).
- **For:** [ADR-0098](../adr/ADR-0098-owned-f64-geometry-values.md) and
  [issue #1352](https://github.com/vanyastaff/flui/issues/1352), which asks for this census
  before any consumer migrates, so the final state can be compared against it.
- **Host:** Windows 11, 32 logical CPUs, rustc 1.98.1, default `dev` profile, empty target
  directory before the cold run.

## What was counted

"Production" means every `.rs` file under `crates/*/src`, `packages/*/src`, `src/` and `tools/`,
excluding `tests/`, `benches/`, `examples/`, files named `tests.rs`, `*_tests.rs` or `test_*.rs`,
and each file from its first top-level `#[cfg(test)]` followed by `mod` to the end of the file.
The last rule removes the conventional trailing test module and nothing else; a test module
placed mid-file, or behind a second attribute, is still counted. That makes every number below
an upper bound, and the name-matched rows (a type named `Line` or `Transform` elsewhere matches
too) looser still.

The mirror and the counts come from these commands, run from the repository root:

```bash
fd -e rs . crates packages src tools -E '**/tests/**' -E '**/benches/**' -E '**/examples/**' \
   -E '**/test_*.rs' -E '**/tests.rs' -E '**/*_tests.rs' -E '**/target/**' |
while read f; do
  mkdir -p "$OUT/$(dirname "$f")"
  awk 'BEGIN{cut=0} { if (!cut && $0 ~ /^#\[cfg\(test\)\]/) { hold=$0; getline nxt;
       if (nxt ~ /^(pub(\(crate\))? )?mod /) { cut=1; next } else { print hold; print nxt; next } }
       if (!cut) print }' "$f" > "$OUT/$f"
done
cd "$OUT"
# consumer sites: every crate except flui-geometry and flui-types
rg -o -t rust '\bPixels\b' crates packages src | grep -v -E '^crates.(flui-geometry|flui-types)' | wc -l
```

The mirror holds 1,221 files and 455,988 lines.

## Size of the value layer

| Crate | `src/` lines, tests included | Production lines |
|---|---:|---:|
| `flui-geometry` | 19,722 | 16,441 |
| `flui-types` | 24,889 | 17,394 |

`flui-types` production lines by family: styling 4,905, painting 4,173, layout 2,399,
typography 2,080, gestures 1,370, physics 1,308, platform 748, `lib.rs` 202, `ime.rs` 94,
`haptics.rs` 70, `lerp_impls.rs` 45.

## Scalar units

Sites outside `flui-geometry` and `flui-types`:

| Token | Sites | Files |
|---|---:|---:|
| `Pixels` | 1,814 | 223 |
| `Pixels(` (tuple constructor) | 234 | 38 |
| `px(` | 782 | 147 |
| `DevicePixels` | 84 | 21 |
| `device_px(` | 46 | 15 |
| `PixelDelta` | 58 | 10 |

`Pixels` by crate, all production files: flui-geometry 708, flui-interaction 344, flui-types 325,
flui-objects 287, flui-engine 234, flui-rendering 147, flui-widgets 142, flui-painting 137,
flui-platform 111, flui-layer 86, flui-material 46, flui-platform-api 45, flui-testing 20,
flui-app 12, flui-animation 12, flui-semantics 9, flui-runtime 8, flui-cupertino 8, flui-view 2,
flui-foundation 1.

The typed conversions `Pixels::to_device`, `DevicePixels::to_logical` and `ScaleFactor` have **no
production caller** outside `flui-geometry`. Logical-to-physical conversion is done by hand with a
raw `f32` scale factor (`scale_factor` appears 204 times in flui-platform, 48 in flui-app, 15 in
flui-platform-api).

## Geometry field access

All production files. ADR-0098 keeps every field name, so a field access keeps its spelling and
changes type from `Pixels` to `f64`:

| Field | Sites | FLUI type |
|---|---:|---|
| `.x` / `.y` | 780 / 747 | `Point` |
| `.width` / `.height` | 626 / 631 | `Size` |
| `.left` / `.top` / `.right` / `.bottom` | 260 / 279 / 185 / 229 | `Edges`/`EdgeInsets` |
| `.min` / `.max` | — | `Rect` (min/max corners) |
| `.dx` / `.dy` | 347 / 367 (207 / 230 outside the value crates) | `Offset` |

The [euclid spike](2026-05-29-u17-euclid-spike-report.md) counted 2,379 field sites that a
`Pixels`-preserving wrapper would turn into method calls. Plain `f64` fields turn none of them
into calls. The compiler reports each site where a `Pixels` or `f32` value was expected.

Name-matched uses of the geometry types outside the two value crates: `Offset` 1,202, `Size`
1,127, `Rect` 697, `Matrix4` 379, `Point` 342, `EdgeInsets` 234, `RRect` 159, `Bounds` 137,
`Transform` 106, `Circle` 47, `RSuperellipse` 32, `Vec2` 23, `Line` 12, `Edges` 6, `Corners` 2,
`RelativeRect` 2, `Percentage` 1. **Zero** uses: `Transform2D`, `Rems`, `DefiniteLength`,
`AbsoluteLength`, `Radians`, `ScaleFactor`, `QuadBez`, `CubicBez`, `CharTransform`.

Two rectangle types exist (`Rect { min, max }` and `Bounds { origin, size }`), three
displacement-like types (`Offset`, `Vec2`, and `Point` used as one), and two `Axis` enums
(`crates/flui-geometry/src/traits.rs:19`, `crates/flui-types/src/layout/axis.rs`).

## Equality and hashing

- `Pixels` derives `PartialEq` on `f32` but implements `Eq` through `total_cmp` and `Hash`
  through `to_bits` (`crates/flui-geometry/src/units.rs:91`, `:575`, `:592-596`), so `px(0.0)`
  and `px(-0.0)` are equal and hash differently.
- `BoxConstraints` repeats the pattern: derived `PartialEq`, `impl Eq`, and a `Hash` over
  `to_bits` (`crates/flui-rendering/src/constraints/box_constraints.rs:40`, `:56-66`).
  Constraints whose bounds differ only by the sign of zero compare equal and hash apart.
- 72 production `to_bits()` calls outside `flui-geometry` build hash keys from floats, 70 of
  them in flui-rendering (parent data, sliver and box constraints, layout cache) and
  flui-engine's `path_cache.rs`. One site canonicalises `-0.0` explicitly
  (`crates/flui-painting/src/parley_text/key.rs:161-164`).
- Four production types derive `Eq` while holding float geometry: `LongPressDownDetails`,
  `TapDragDownDetails` (flui-interaction), `ChildState` (flui-rendering `box_protocol.rs`),
  `DraggableDetails` (flui-widgets).

## Rounding to device pixels

Float-to-integer casts after `round`/`floor`/`ceil` outside `flui-geometry`: 38, of which 28 sit
on the raster or platform boundary and 10 are unrelated (animation tweens, curve sampling, grid
delegates, a text-store hit test).

| Where | Operation | Policy today |
|---|---|---|
| `flui-app/src/app/runner/web.rs:170-171`, `flui-platform` web `window.rs:57-58,258-259`, iOS `window.rs:764-765`, iOS/macOS `display.rs`, Windows `util.rs:43` | surface and display sizes | `f32::round` (ties away from zero) |
| `flui-engine/src/layer_dispatcher.rs:377-380`, `advanced_blend/mod.rs:114-117` | viewport and copy sizes | `f32::round` |
| `flui-engine/src/painter/layer.rs:74-80`, `ssaa.rs:329-330`, `effects/blur.rs:43`, `offscreen/blur.rs:221` | covering bounds, kernels | `floor` of the minimum, `ceil` of the maximum |
| `flui-painting/src/text_layout/layout.rs:915` | text baseline snap | `f32::round` |

No site handles negative ties or non-finite input by an explicit rule; `as` saturates and maps
NaN to 0. In flui-engine, device-space rectangles are `Rect<Pixels>` (for example
`op.device_bounds` in `ssaa.rs`), so the type does not distinguish logical from physical
coordinates there.

## Paths, kurbo and lyon

- `kurbo` 0.13 is an optional dependency of `flui-geometry` behind its `kurbo` feature
  (`crates/flui-geometry/src/bridges/kurbo.rs`, 236 lines); no workspace member enables the
  feature, so kurbo is not compiled in any normal build.
- `Path` is FLUI's own command list (`crates/flui-types/src/painting/path.rs`, 2,126 lines,
  `PathCommand`), converted to lyon in `crates/flui-engine/src/tessellator.rs` (1,727 lines,
  134 `lyon` references). Other lyon references are in `path_cache.rs`, `superellipse.rs`,
  `painter/`, `replay/` and `lib.rs`.
- `euclid` 0.22.14 is already compiled in every engine build, through `lyon_geom` and
  `etagere`. `peniko` and `color` are not in `Cargo.lock`.

## Consumers of `flui-types`

`flui_types` references by consuming crate: flui-engine 284, flui-rendering 213, flui-widgets
161, flui-types itself 124, flui-objects 95, flui-platform 91, flui-interaction 51, flui-layer 48,
flui-painting 44, flui-platform-api 26, flui-app 25, flui-view 11, flui-runtime 9,
flui-animation 8, flui-testing 7, flui-semantics 4, flui-assets 4, facade 3, flui-sdk 1.

Module-qualified paths: `geometry` 286, `painting` 201, `layout` 69, `styling` 54,
`typography` 34, `platform` 15, `gestures` 5, `prelude` 2, `physics` 1; the rest go through
root re-exports.

Crates that name a family's types (by type name, outside flui-types):

- **physics** — flui-animation, flui-widgets (and `Tolerance` also in flui-engine).
- **gestures** — facade, flui-interaction, flui-widgets; `PointerDeviceKind` also in
  flui-rendering; `PointerData` has no consumer.
- **platform** — `Brightness`, `Locale` and `TargetPlatform` in flui-app, flui-widgets, flui-view,
  flui-semantics, flui-interaction and both design-system packages; `DeviceOrientation` has no
  consumer.
- **layout** — splits in two. The box/sliver protocol types (`BoxConstraints`, `FlexFit`,
  `CacheExtentStyle`, `AxisDirection`, table and wrap enums) are used from flui-rendering
  upwards. `Alignment`, `Axis`, `BoxFit`, `BoxShape` and `TextBaseline` are used by flui-painting,
  flui-layer, flui-engine and flui-platform, which sit below flui-rendering, so they cannot move
  to flui-rendering. `FractionalOffset` and `Orientation` have no consumer.

## Rebuild fan-out and check time

`cargo check --workspace --lib --locked`, one run per row, same target directory:

| Change | Units re-checked | Wall time |
|---|---:|---|
| cold, empty target directory | 279 | 1 m 50 s |
| no change | 0 | 0.39 s |
| `touch` `flui-geometry/src/lib.rs` | 31 | 4.44 / 4.50 / 4.02 s |
| `touch` `flui-types/src/lib.rs` | 30 | 4.69 / 4.09 / 4.02 s |
| `touch` `flui-painting/src/lib.rs` | 22 | 4.05 / 4.12 / 4.04 s |
| `touch` `flui-rendering/src/lib.rs` | 18 | 3.48 / 3.43 / 3.47 s |
| add a `pub const` to `flui-geometry` | 31 | 4.93 / 5.43 / 5.57 s |
| add a `pub const` to `flui-types` | 30 | 4.91 / 4.64 / 5.10 s |
| add a `pub const` to `flui-painting` | 22 | 3.96 / 4.45 / 4.06 s |
| `flui-geometry` and `flui-types` alone, empty target directory | 12 | 15.62 s |

Workspace members that depend on each crate through normal or build edges
(`cargo tree -i <crate> --workspace -e normal,build`): flui-geometry 28, flui-types 27,
flui-painting 19.

On this host a check is dominated by the longest dependency chain, not the count, so the
wall-time difference between a 31-unit and a 22-unit rebuild is about one second. The unit
count is the number to compare after the migration; wall time matters more on a smaller host
and for test builds, which link.
