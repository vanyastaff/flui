[← Roadmap Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md) · [Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md)

# U17 — euclid-wrapper spike: Option C cost measurement + PR-2 decision

> **Status:** spike complete / decision gate. Executes the pre-agreed N-geom **U17** risk gate from
> [`2026-05-24-flui-geometry-polish-pass-research.md` §VIII](2026-05-24-flui-geometry-polish-pass-research.md).
> **Date:** 2026-05-29. **Author role:** senior Rust engineer, flui port discipline.
> **Drives:** ROADMAP-TRACKER rows `N-geom.U17` (this), `N-geom.U14C` (Option C), `N-geom.U14` (Option D).

---

## Executive summary — recommend **Option D**

The U17 spike built a faithful newtype wrapper over `euclid 0.22` for the core coordinate types
(`Pixels`/`Offset`/`Point`/`Size`/`Rect`/`Edges`/`Transform2D`), reimposed the PR-1 unit-barrier
invariants on top, and migrated `flui-rendering::RenderPadding`'s geometry surface onto it. Both the
wrapper and the port compile and are clippy-clean under `-D warnings`.

**Per the pre-agreed decision rule (`wrapper_LOC + per_widget_LOC × 80` vs `3 × Option_D = 2,250`), the
result selects Option D.** The faithful-wrapper extrapolation lands at **~4,120 LOC** — and that number
is itself an *undercount*, because the rule's `per_widget × 80` term does not capture Option C's dominant
real cost: a **codebase-wide field→method conversion of ~2,379 geometry field-access sites** that the
euclid newtype makes unavoidable. Option D (wrap `glam` *under* the current public API) keeps that surface
— including public fields — byte-stable, so **zero** of those sites change, while still banking the SIMD,
`bytemuck::Pod`, and `mint`-bridge wins.

| | **Option C** (euclid wrapper) | **Option D** (glam under current API) |
|---|---|---|
| Wrapper / delegation LOC (measured→extrapolated) | **~3,640** code-only | ~500–850 (research estimate) |
| Per-widget migration (Padding, measured) | **6 lines** | **0 lines** (API stable) |
| Codebase field→method conversions | **~2,379 sites** | **0** |
| Decision-rule total | **~4,120 (rule) / ~6,000 (census-corrected)** | ~750 |
| vs 2,250 ceiling | **✗ exceeds (≈1.8–2.7×)** | ✓ |
| SIMD / Pod / mint wins | yes | yes |

---

## What was built (reproducible artifact)

A throwaway crate `crates/flui-geometry-euclid-spike/` (removed after this report; see §Appendix for the
anchor type). Each public flui type is a `#[repr(transparent)]` newtype over the matching euclid type:

```rust
#[repr(transparent)] pub struct Pixels(euclid::Length<f32, PixelsUnit>);   // units.rs
#[repr(transparent)] pub struct Offset(euclid::Vector2D<f32, PixelsUnit>); // vector.rs
#[repr(transparent)] pub struct Size(euclid::Size2D<f32, PixelsUnit>);     // size.rs
#[repr(transparent)] pub struct Point(euclid::Point2D<f32, PixelsUnit>);   // point.rs
#[repr(transparent)] pub struct Rect(euclid::Box2D<f32, PixelsUnit>);      // rect.rs
#[repr(transparent)] pub struct Edges(euclid::SideOffsets2D<f32, PixelsUnit>); // edges.rs
#[repr(transparent)] pub struct Transform2D(euclid::Transform2D<f32, _, _>);   // transform2d.rs
```

The newtype is what makes Option C *clean* — euclid ships `From<f32>`, `Length + Length`, `f64 * Point`,
etc., and the newtype hides all of euclid's inherent/derived impls, so the PR-1 barrier (U1/U2/U4) is
reimposed simply by choosing which operators to re-expose. `bytemuck::Pod` mirror structs were added on
`Offset`/`Transform2D` (the engine-upload need), and the `FloatUnit` bridge trait was carried over.

**The same newtype is also what makes Option C expensive** — because it hides euclid's fields too.

## The structural finding: field access becomes method calls

euclid's aggregate types expose their components as **`f32` fields** (`Point2D.x: f32`,
`Size2D.width: f32`, `SideOffsets2D.left: f32`). The production flui types expose them as **`Pixels`
fields** (`Size.width: Pixels`, `Edges.left: Pixels`, `Offset.dx: Pixels`). A newtype over euclid
**cannot** re-expose a `Pixels`-typed field over an `f32` component, so every component read must go
through an accessor **method** that wraps the `f32` back into `Pixels`:

```rust
// today (production, field):           // Option C wrapper (method):
size.width                              size.width()
edges.left                              edges.left()
offset.dx                               offset.dx()
```

This is intrinsic to Option C's value proposition: the point of using euclid is to reuse `Point2D` /
`Rect` / `Transform2D`'s methods, which forces euclid's `f32`-component aggregate types, which forces
accessor methods. (A hybrid that keeps own `{ x: Pixels, y: Pixels }` structs to preserve fields would
reimplement those methods itself — i.e. it stops being Option C.)

### Census of the affected surface

High-confidence geometry field-access sites in **production** code (`rg`, excludes tests; the clearest
geometry tokens only — `.dx .dy .width .height .left .top .right .bottom`):

| token | sites | token | sites |
|---|---:|---|---:|
| `.width` | 491 | `.dx` | 316 |
| `.height` | 468 | `.dy` | 283 |
| `.left` | 179 | `.right` | 120 |
| `.top` | 177 | `.bottom` | 120 |
| **total** | | | **2,379** |

(`+ .x/.y` adds ~131 more in rendering/painting/layer alone.) Some `.width`/`.height` hits are on
non-geometry types — call it an upper bound — but even discounting 30–40 % leaves **~1,500 mandatory
conversions**. Each is a one-token edit, but they are spread across every layout/paint/hit-test/widget
file and every one must be touched, reviewed, and re-tested.

## Measurements

**Wrapper LOC anchor** — the 79 `pub fn`s I implemented across the 7 wrapper modules:

| metric | value |
|---|---|
| wrapper LOC (with docs/blanks) | 858 |
| wrapper LOC (code-only) | 603 |
| `pub fn` implemented | 79 |
| **code-only LOC / fn** | **7.63** |
| total LOC / fn | 10.86 |

**Full-wrapper surface** — the core coordinate types a faithful Option C wrapper must cover
(`units + vector + point + size + rect + bounds + edges + offset + transform2d + matrix4 + transform`) =
**477 `pub fn`** in today's `flui-geometry`. Extrapolating at the measured 7.63 code-only LOC/fn →
**≈ 3,640 code-only LOC** (≈ 5,180 with docs). This is well above the research's pre-spike guess of
1,200–2,000 — confirming the research's own caveat that the wrapper estimate was an order-of-magnitude
guess to be replaced by measurement.

**Per-widget migration** — `RenderPadding` ported onto the wrapper (`padding_port.rs`, diffed against the
PR-1 original): **6 changed lines**, all of them field→method conversions (`.left/.top`, `child_size.width/.height`,
`size.width/.height`). Everything else — `px(...)`, `EdgeInsets::all`, `Offset::new`, `Size::new`,
`horizontal_total()`, `Pixels::ZERO`, `.max(...)`, `Rect::from_origin_size` — was **byte-identical**,
because the wrapper preserves the flui surface names. Padding is one of the *simplest* widgets (≈8 geometry
ops); heavier widgets (Flex/Stack/transform/painting) carry proportionally more conversions.

## Decision-rule application

```
Option D total ≈ 750 LOC          ceiling = 3 × 750 = 2,250
```

1. **Literal rule, measured partial wrapper:** `858 + 6×80 = 1,338` → would say C — but `858` is only the
   79-fn slice, *not* a complete wrapper, so this reading is not faithful.
2. **Faithful full wrapper (477 fns):** `3,640 + 6×80 = 4,120` → **> 2,250 → Option D.**
3. **Census-corrected** (replace the under-modelled `per_widget×80=480` term with the real
   ~2,379-site field-conversion surface): `3,640 + ~2,379 ≈ 6,000` → **strongly Option D.**

Readings 2 and 3 agree, and reading 1 only flips by using an admittedly-incomplete wrapper. **The gate
selects Option D**, and by a comfortable margin even under the generous 3× foundation-quality threshold.

## Why this is the right call (not just the cheap one)

- **The foundation-quality mandate tolerated up to 3× Option D's cost.** Option C measured at ~1.8–2.7× *before*
  counting the field-conversion churn, and well past 3× after. The mandate's own ceiling rejects it.
- **Option D is not "own math forever."** It deletes flui-geometry's 3,833 LOC of hand-rolled matrix/vector
  math + the hand-written SSE/NEON behind a stable public API, replacing it with `glam` (SIMD by default),
  gains `glam`'s `bytemuck::Pod` (deletes the engine's per-call Pod shims) and the `mint` cascade (shrinks
  the kurbo bridge to a pass-through). The wins the research attributed to "the big refactor" land under D.
- **Option C stays available as a *future, decoupled* step.** Its blocker is the field→method surface, not
  euclid itself. If that surface is ever wanted, the cheap prep is a mechanical *field→accessor-method* pass
  on the existing types (no euclid involved), after which a euclid swap would be low-churn. That pass can be
  scheduled independently if/when typed-euclid genericity becomes valuable — it is not a PR-2 prerequisite.

## Recommendation

1. **Adopt Option D as the PR-2 default** (`N-geom.U14`): wrap `glam::Mat4`/`Affine2`/`Vec2` under the
   current `flui-geometry` public API; `glam = { features = ["bytemuck", "mint"] }`, `kurbo = { features = ["mint"] }`;
   delete the `simd` feature + hand-rolled SIMD; keep `inverse() -> Option<_>` semantics; add the
   `size_of`/`align_of` `repr(transparent)` assertions (research R9).
2. **Re-classify `N-geom.U14C` (Option C) as deferred**, gated on a future (optional) field→accessor-method
   refactor — not part of PR 2.
3. **PR 3 (kurbo bridge, U8)** is unaffected and still rides the `mint` cascade under Option D.
4. Remove the throwaway spike crate (done in this change); measurements above are reproducible from the
   §Appendix anchor + the `rg` census commands.

## Threats to validity

- **Padding is a simple widget.** Its 6-line cost is a *floor*, not an average — which only strengthens the
  case against C (the multiplied term is larger in reality).
- **The census over-counts** non-geometry `.width`/`.height`; discounted, it is still ~1,500 sites — far
  above the ceiling.
- **The wrapper extrapolation assumes uniform LOC/fn.** Simple accessors are cheaper than operator/Flutter-parity
  methods; the 79-fn slice deliberately mixed both (Pixels operators + Rect/Edges constructors + accessors) to
  keep 7.63 representative.
- **euclid genericity not fully exercised.** The slice specialised to `f32, PixelsUnit`; a fully generic
  `Length<T, U>` wrapper adds where-clause overhead per type, i.e. *more* wrapper LOC — again favouring D.

---

## Appendix — wrapper anchor (`units.rs`, abridged)

```rust
#[repr(transparent)]
pub struct Pixels(euclid::Length<f32, PixelsUnit>);

pub const fn px(value: f32) -> Pixels { Pixels(euclid::Length::new(value)) }

impl Pixels {
    pub const ZERO: Pixels = px(0.0);
    pub const fn new(value: f32) -> Self { px(value) }
    pub const fn get(self) -> f32 { self.0.0 }      // euclid Length is `Length(pub T, _)`
    pub fn min(self, o: Self) -> Self { px(self.get().min(o.get())) }
    pub fn max(self, o: Self) -> Self { px(self.get().max(o.get())) }
    // … floor/ceil/round/abs/sqrt/clamp/lerp/is_finite/to_device …
}
impl Add for Pixels { /* same-unit add kept */ }
impl Mul<f32> for Pixels { /* scaling kept */ }
impl Div<Pixels> for Pixels { type Output = f32; /* ratio */ }
// U1: no From<f32>.  U2: no PartialEq<f32>/Add<f32>.  U4: no Mul<Pixels> for Pixels.
// All three are *automatically* absent because the newtype hides euclid's defaults.
```

Census commands (reproducible):

```bash
rg -t rust --glob '!**/tests/**' --glob '!**/test*.rs' \
  -o -e '\.dx\b' -e '\.dy\b' -e '\.width\b' -e '\.height\b' \
     -e '\.left\b' -e '\.top\b' -e '\.right\b' -e '\.bottom\b' crates/ | wc -l   # 2379
```

---

[← Roadmap Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md) · [Polish-pass research](2026-05-24-flui-geometry-polish-pass-research.md)
