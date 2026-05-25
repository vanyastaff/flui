[← Roadmap Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md) · [Foundations](../FOUNDATIONS.md) · [Port Methodology](../PORT.md)

# `flui-geometry` Polish Pass + Math-Stack Architecture — Senior Rust Research

> **Status:** research / pre-commit. Validates the seven proposed `U1–U7` units against the 2026 Rust UI ecosystem (`kurbo`, GPUI, Slint, Jetpack Compose, taffy, Bevy), surfaces six additional findings the task spec didn't enumerate, and proposes a migration order that accounts for **future Vello/kurbo interop**.
>
> **Part VIII (added 2026-05-24 in response to feedback, revised same day):** extends the research to the **fundamental math-stack question** — `glam`, `euclid`, `nalgebra`, `cgmath`, `mint` — and shows that flui-geometry has 3,833 LOC of pure linear algebra duplicating glam, and an undocumented Cargo.toml `glam`/`mint` integration. **Recommended sequence under foundation-quality + breaking-allowed mandate:** polish (PR 1) → U17 euclid spike (risk gate) → PR 2 = **Option C by default** (full euclid + glam + kurbo + mint hybrid), Option D only as fallback if spike measures C > 3× D cost → kurbo bridge → done.
>
> **Author role:** senior Rust engineer review under flui port discipline.
> **Date:** 2026-05-24.
> **Drives:** ROADMAP-TRACKER row group N-geom (new sub-block of Core.0).

---

## Executive summary

The seven `U1–U7` units in the task spec are **directionally correct** and **converge with where the wider Rust UI ecosystem is heading in 2026**. Specifically:

1. **`kurbo` (Linebender)** enforces exactly the same operator hygiene the spec wants — `Point + Point` and `f64 * Point` are deliberately NOT implemented because they are "geometrically ill-defined" ([deepwiki/linebender/kurbo](https://deepwiki.com/linebender/kurbo)).
2. **GPUI** is in the middle of an analogous purge ([zed#32339](https://github.com/zed-industries/zed/pull/32339)) merging `DevicePixels + ScaledPixels → PhysicalPixels<S>` and removing "lossy/useless trait impls."
3. **Slint** migrated its whole core API surface from raw scalar `Coord` to typed `LogicalLength`/`PhysicalLength` over four PRs in late 2022 ([slint#1697](https://github.com/slint-ui/slint/pull/1697), [#1729](https://github.com/slint-ui/slint/pull/1729), [#1731](https://github.com/slint-ui/slint/pull/1731), [#1620](https://github.com/slint-ui/slint/pull/1620)) — the exact arc this task is starting on.
4. **Jetpack Compose** has been on this design since 1.0: `@JvmInline value class Dp(val value: Float)` (the JVM equivalent of `#[repr(transparent)]`), conversion via `Density` scope, separate `IntOffset`/`DpOffset` for integer vs float UI dimensions.

The plan is sound. This report extends it with:

- **6 new findings** the spec didn't cover (asymmetric assignment ops, `MulAssign` follow-through, `From<Pixels> for i32` lossy conversion, GPUI 2026 unification trajectory, `transform_scalar` semantic bug, kurbo bridge prep).
- **`U7` is worse than "broken"** — the function semantically lies (Src/Dst phantoms unused, doc-example shows wrong type), and the published doc-example contradicts the API's own type-safety guarantee. **Recommendation: delete, not fix.**
- **`U6` is SP-4 speculative scaffolding** — `FloatPoint`/`FloatVec2`/`FloatSize`/`FloatOffset` have **zero production usages** (grep confirmed). Removal is unconditional.
- **Migration order matters.** `U3` (EdgeInsets) has ~50 production call sites and must follow `U1`+`U2` (which make `px(16.0)` the only legal literal); otherwise the EdgeInsets migration looks like cosmetic churn instead of safety improvement.
- **kurbo interop is a real future requirement** (Vello, parley, Xilem ecosystem all sit on kurbo). kurbo is **`f64`** and **also** restricts `Point + Point` — the design philosophies align, but the scalar widths don't. A `feature = "kurbo"` bridge layer should be queued for Core.2 (when `flui-painting`/`flui-rendering` start consuming path primitives).

---

## Part I — Context: why this surface matters

`flui-geometry` is **18,945 LOC, 25 files** — not a small types crate. Every render object, every widget, every paint command, every gesture event flows through these types. The polish pass touches a small fraction of the code (~300 lines across units.rs and lib.rs), but the **semantic contract** it establishes propagates to every downstream call site.

The current design is GPUI-vintage 2024: `Pixels(f32) / DevicePixels(i32) / ScaledPixels(f32)`, generic `Point<T: Unit>`/`Size<T>`/`Rect<T>`, `ScaleFactor<Src, Dst>` phantom-typed conversion. This is a sound foundation. The polish pass closes the **escape hatches** that defeat its own invariants.

### Why GPUI/Compose/Slint chose newtype over raw scalar

The single best summary is from kurbo's design rationale:

> Kurbo deliberately distinguishes between `Point` and `Vec2` to enforce semantic clarity in geometric operations. `Point` represents a specific location in space, while `Vec2` represents a displacement or a vector. This design choice prevents mathematically ambiguous operations like `Point + Point` (adding two locations doesn't yield a meaningful location) or `f64 * Point` (scaling a location without a reference point is ill-defined).
>
> The design rationale is to ensure that geometric operations are semantically correct and prevent common errors. ([source](https://deepwiki.com/linebender/kurbo))

The same logic generalizes from `Point/Vec2` to `Pixels/PhysicalPixels/ScaleFactor`: **the type system catches an entire class of unit-mixing bugs at compile time that runtime testing cannot catch reliably.** Flutter has open issues from 2016 to today on coordinate-mixing bugs that Dart's `double`-everywhere policy cannot prevent ([flutter#5873](https://github.com/flutter/flutter/issues/5873), [#41328](https://github.com/flutter/flutter/issues/41328), [#116278](https://github.com/flutter/flutter/issues/116278)).

The task spec's principle is correct: **the unit barrier exists; escape hatches make it ornamental.**

---

## Part II — Ecosystem evidence table

How each peer framework handles the questions this polish pass asks:

| Question | kurbo (Linebender) | GPUI (Zed 2026) | Slint | Jetpack Compose | taffy |
|---|---|---|---|---|---|
| **Scalar** | `f64` everywhere | `f32` (logical), `i32`/`f32` (physical) | `f32` via `euclid::Length` | `Float` via `Dp` value class | `f32` |
| **Newtype for length?** | **No** (raw `f64`) | **Yes** — `Pixels(f32)` etc. | **Yes** — `LogicalLength`/`PhysicalLength` | **Yes** — `@JvmInline Dp(Float)` | **No** (raw `f32`) |
| **`From<scalar> for length`?** | n/a | Existed; PR #32339 is removing | **No** — explicit `LogicalLength::new()` | **No** — `.dp` extension fn (explicit) | n/a |
| **`Point + Point`?** | **Deliberately no** | Provided via `derive_more::Add` (allowed) | Allowed (`euclid` allows) | n/a (uses `Offset`/`IntOffset`) | n/a |
| **`Length * Length` returns?** | n/a | f32 (dot/cross product mode) | n/a | **Compile error** for `Dp * Dp` | n/a |
| **Cross-scalar comparison?** | **No** — `Point == (f64, f64)` not provided | Removing in PR #32339 | **No** | **No** | n/a |
| **Conversion path** | `.to_point()` / `.to_vec2()` explicit methods | `pixels.to_device(ScaleFactor)` typed | `LogicalLength::new(x)` + `to_physical(scale)` | `with(LocalDensity.current) { dp.toPx() }` scope | n/a |
| **Separate int variant?** | No (always f64) | `DevicePixels(i32)` for pixel-grid ops | `PhysicalLength<i32>` available | **`IntOffset`/`IntSize`/`IntRect`** explicit | n/a |
| **Bridge to scalar?** | `Point::new(x, y).x` field access | `pixels.0` or `.into()` | `LogicalLength::get()` | `dp.value` (intentional friction) | n/a |

**Reading:**

- **Among UI frameworks** (Slint, GPUI, Compose) the answer is unanimously **newtype with explicit conversion at the boundary**. taffy and kurbo skip the newtype because they are *infrastructure* libraries (layout engine, curve library), not user-facing UI frameworks — the question doesn't apply.
- **flui's current state** is GPUI-vintage 2024 with some 2024-era escape hatches still in place. The polish pass moves flui to GPUI-2026-direction.
- The **future-proof shape** for flui = **newtype + explicit bridge to kurbo `f64`** via dedicated `From<flui::Point<Pixels>> for kurbo::Point` impls, gated behind `feature = "kurbo"`.

---

## Part III — Per-unit validation (U1–U7) + extensions (U8–U13)

For each unit: **what the spec asks**, **ecosystem evidence for/against**, **scope-of-damage in flui**, **verdict**.

### U1 — Remove `From<f32/f64/i32/u32/usize> for Pixels`

| Aspect | Detail |
|---|---|
| **What** | Delete 5 `From<scalar> for Pixels` impls at `units.rs:580-618`. Keep `Pixels::new(x: f32)`, `px(x: f32)`, `Pixels::from_i32(x: i32)`. |
| **Ecosystem evidence** | **Compose:** `Int.dp` is an extension function (explicit), no `Dp::from(Int)`. **Slint:** `LogicalLength::new(x)` explicit constructor. **GPUI:** PR #32339 removes "lossy/useless trait impls" identical to this set. **kurbo:** no `From<f64> for Point`, only `From<(f64, f64)>`. |
| **flui downstream blast** | Any `.into()` writing into a `Pixels` context fails to compile. 301 `.into()` occurrences in `flui-geometry/rendering/painting/view` — most are non-Pixels generic; estimate ~30 real Pixels fallouts (largely in `*Pixels::from` patterns + EdgeInsets construction). |
| **Risk** | Low. Compiler tells you every site exactly. |
| **Verdict** | ✓ **Approve.** Direct match with GPUI 2026 direction. |
| **Refinement** | Doc-comment on `Pixels::new` should explicitly warn this is the *only* `f32 → Pixels` blessed path. Add a `port-check.sh` trigger that rejects `From<f32>` / `From<f64>` impls on any wrapper in `flui-geometry/src/units.rs` to prevent regression. |

### U2 — Remove `PartialEq<f32>` / `PartialOrd<f32>` / `Add<f32>` / `Sub<f32>` for `Pixels`

| Aspect | Detail |
|---|---|
| **What** | Delete cross-type `PartialEq`/`PartialOrd`/`Add`/`Sub` impls (12 impls total, `units.rs:491-585`). Keep `Mul<f32>` / `Div<f32>` (scaling is dimensionally valid). |
| **Ecosystem evidence** | **kurbo:** explicitly cites `f64 * Point` as ill-defined (deepwiki quote above). **Compose:** `Dp == Float` does not exist. **Slint:** `LogicalLength + f32` does not compile. |
| **Semantic justification** | `px(10.0) == 10.0` and `px(10.0) + 5.0` compile today. A forgotten `.0` literal anywhere in the codebase silently loses unit safety. This is exactly the bug class that Flutter cannot type-out and FLUI exists to prevent. |
| **flui downstream blast** | Every site comparing or adding bare `f32` to `Pixels` breaks. Fix with `px(literal)` or `pixels.get()`. Estimate ~20 fallout sites based on consumer scan. |
| **Verdict** | ✓ **Approve.** |
| **Refinement** | Add a `compile_fail` doctest as proof-of-removal — the spec says the same. The CI doctest is the regression gate. |

### U3 — `EdgeInsets = Edges<f32>` → `Edges<Pixels>`

| Aspect | Detail |
|---|---|
| **What** | Change the type alias in `lib.rs:313`. Verify `Edges<T>` generic bounds at `edges.rs`. |
| **Ecosystem evidence** | **Compose:** `PaddingValues` uses `Dp`. **GPUI:** `Edges<Pixels>` direct. **Slint:** typed insets via `euclid::SideOffsets2D<LogicalPx>`. |
| **flui downstream blast** | ~50 production usages (concentrated in `flui-rendering/src/objects/padding.rs`, `sliver_padding.rs`, `box_constraints.rs`). Every `EdgeInsets { left: 16.0, ... }` literal becomes `EdgeInsets { left: px(16.0), ... }`. |
| **Risk** | Low *if* U1+U2 land first — by then `16.0` won't compile into `EdgeInsets` either, making the rewrite trivial. **High if U3 lands first** — call sites can be silently rewritten incorrectly. **Order matters.** |
| **Verdict** | ✓ **Approve, after U1+U2.** |
| **Refinement** | Add `EdgeInsets::all(px(8.0))`/`symmetric`/`only`/`zero` constructors so common literals stay one-liners. Document call-site migration pattern in CHANGELOG. |

### U4 — Remove `Mul<Pixels> for Pixels → Pixels` (semantic bug)

| Aspect | Detail |
|---|---|
| **What** | Delete `impl Mul<Pixels> for Pixels` at `units.rs:347-353`. Also delete `MulAssign<Pixels> for Pixels` (`units.rs:412-417`) — see **U9 extension** below. |
| **Ecosystem evidence** | **kurbo:** no `Point * Point` impl (geometrically `area`, not `location`). **Compose:** `Dp * Dp` is a compile error. |
| **Semantic justification** | `px × px = px²` (area), not `px` (length). Returning `Pixels` mislabels area as length. `Size::area() -> f32` already exists at `size.rs:190` as the explicit area path. |
| **flui downstream blast** | Any site doing `px_a * px_b` and expecting `Pixels` breaks. These are **likely all incorrect today** (area silently typed as length). Replace with `.get()` extraction or `Size::area()`. |
| **Verdict** | ✓ **Approve.** |
| **Refinement** | While at it — also see U10 about `DivAssign<Pixels> for Pixels`, which has the same asymmetry-with-Div bug. |

### U5 — Deprecate legacy `to_device_pixels(f32)` / `from_*_pixels(_, f32)`

| Aspect | Detail |
|---|---|
| **What** | `#[deprecated(since = "0.X.0", note = "use to_device(ScaleFactor) for compile-time DPI safety")]` on 3 functions at `units.rs:237-256`. Migrate internal callers. |
| **Ecosystem evidence** | **GPUI:** `Pixels::to_device(ScaleFactor)` is exactly the pattern. **Slint:** logical/physical sites are method-named (not parameter-distinguished). |
| **flui downstream blast** | The `to_device_pixels` pattern has metastasized — it now exists as wrapper-cascade in `size.rs`, `point.rs`, `bounds.rs` (each delegating to per-field calls). Tests in `flui-types/tests/device_pixels_geometry_tests.rs` use it directly. Migrating all to typed form = ~15 changed sites. |
| **Risk** | Very low — deprecation only, no breakage in this PR. |
| **Verdict** | ✓ **Approve.** Combine with internal call-site migration in same commit. |
| **Refinement** | **Don't only deprecate the unit functions — also deprecate the `Size::to_device_pixels()` / `Point::to_device_pixels()` / `Bounds::to_device_pixels()` wrappers** that propagate the untyped scale factor downstream. Otherwise the cascade survives. |

### U6 — Remove `FloatPoint`/`FloatVec2`/`FloatSize`/`FloatOffset` aliases

| Aspect | Detail |
|---|---|
| **What** | Delete 4 type aliases in `lib.rs:276/288/300/309`. |
| **Verification** | `rg 'FloatPoint|FloatVec2|FloatSize|FloatOffset' crates/ examples/` returns **only the 4 definitions, zero usages**. |
| **Semantic justification** | The doc claims "GPU-ready" — but `Point<Pixels>` (which is what `FloatPoint` aliases to) is **not** GPU-friendly. GPU wants `[f32; 2]`, which is `point.to_array()`, not a `Point<T>` type. The aliases are technically wrong **and** unused. |
| **Refusal trigger** | **SP-4 speculative scaffolding** under PORT.md. Removal is mandatory, not optional. |
| **Verdict** | ✓ **Approve, unconditional.** |

### U7 — Fix or remove broken `ScaleFactor::transform_scalar<T>`

| Aspect | Detail |
|---|---|
| **What spec asks** | "Fix or remove broken `ScaleFactor::transform_scalar<T>`" at `units.rs:2156-2162`. |
| **Investigation finding** | **This is worse than "broken" — the function semantically lies.** Signature `<T>(value: T) -> T` ignores `Src`/`Dst` phantom parameters (they are unused in the body). The function's own doc-example shows: `let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0); let physical = scale.transform_scalar(logical);` — but `physical` is typed `Pixels` (same as `logical`), **not** `DevicePixels`. The doc-example contradicts the function's apparent type-safety guarantee. |
| **Recommendation** | **Delete, do not fix.** Typed conversions already exist as `Pixels::to_device(ScaleFactor<Pixels, DevicePixels>) -> DevicePixels` and `DevicePixels::to_logical(ScaleFactor) -> Pixels`. Adding a generic `transform_scalar<T>` is dead weight that misrepresents what `ScaleFactor` is for. |
| **If kept** | Correct signature must be `fn apply<Src, Dst>(&self, value: Src) -> Dst where Src: Unit<Scalar=f32>, Dst: Unit<Scalar=f32>` — but this collides with `DevicePixels(i32)` needing `round() as i32`. A clean fix requires a `ScaleApply<Dst>` trait per destination type, which is overkill when the explicit `to_device`/`to_logical` methods already cover the cases. |
| **Verdict** | ✓ **Approve as "remove" — strongest version of U7.** |

### U8 (NEW) — Plan kurbo bridge layer for Core.2

| Aspect | Detail |
|---|---|
| **Context** | The roadmap's **Core.2 — Render-object catalog** introduces ~73 render objects, many of which need Bézier path operations, curve flattening, hit-testing on shapes — exactly kurbo's domain. Vello (the kurbo-based GPU renderer) is also the leading candidate for the future `RasterBackend` swap from lyon. |
| **Concrete prep** | Add `feature = "kurbo"` to `flui-geometry/Cargo.toml`. Add `crates/flui-geometry/src/bridges/kurbo.rs` with:<br/>• `impl From<Point<Pixels>> for kurbo::Point { fn from(p) -> Self { Self::new(p.x.get() as f64, p.y.get() as f64) } }` (lossless f32→f64 promotion)<br/>• `impl TryFrom<kurbo::Point> for Point<Pixels>` (fallible f64→f32 with `KurboBridgeError::OutOfRange` for non-finite or out-of-f32-range)<br/>• Same for `Size`, `Rect`, `Affine`/`Matrix4` (3x3 vs 4x4 — note discrepancy and document)<br/>• PORT-CHECK-OK-SP3 markers on each `as f64`/`as f32` site. |
| **Why now** | Filing the bridge as a **U-future** queued for Core.2 entry **but planned now**, prevents a later panic-port: if the painting/rendering layer matures without the bridge, kurbo conversion ends up sprinkled inline in object-by-object code, defeating the type barrier exactly where it matters most. |
| **Verdict** | ✓ **Approve as queued.** Not part of the polish-pass PR, but should be a tracker row tied to N12 (widget→render-object map) entry into Core.2. |

### U9 (NEW) — Remove `MulAssign<Pixels> for Pixels`

| Aspect | Detail |
|---|---|
| **What** | `units.rs:412-417` — `impl MulAssign<Pixels> for Pixels { fn mul_assign(&mut self, rhs: Pixels) { self.0 *= rhs.0; } }`. |
| **Why** | If U4 removes `Mul<Pixels> for Pixels → Pixels` because the result is area-not-length, **`a *= b` has exactly the same problem** — it lands the area back in a length variable. Cannot orphan this. |
| **Risk** | Co-removal with U4 in the same commit. |
| **Verdict** | ✓ **Approve, bundle with U4.** |

### U10 (NEW) — Fix `DivAssign<Pixels> for Pixels` asymmetry

| Aspect | Detail |
|---|---|
| **What** | `units.rs:447-452` defines `impl DivAssign<Pixels> for Pixels { ... self.0 /= rhs.0 ... }`. But `Div<Pixels> for Pixels` returns `f32` (correct — length/length is dimensionless). So `a / b` is `f32`, but `a /= b` keeps `a` as `Pixels`. Inconsistent. |
| **Recommendation** | **Remove** `DivAssign<Pixels> for Pixels`. Dimensionless ratio is the right semantic, so `let ratio = a / b` is correct, and `a /= b` would put a dimensionless value back into a length, which is the same class of bug as U4 (area-into-length). |
| **Verdict** | ✓ **Approve, bundle with U4/U9.** |

### U11 (NEW) — Audit `From<Pixels> for i32/u32/usize` lossy conversions

| Aspect | Detail |
|---|---|
| **What** | `units.rs:660-687` provides `From<Pixels> for f32` (lossless), `From<Pixels> for f64` (lossless promotion), AND `From<Pixels> for i32` / `for u32` / `for usize` (LOSSY: `as i32` truncates the fractional part, `as u32`/`as usize` also clamp negative-to-zero or wrap, depending on platform). |
| **Concern** | Rust 2026 idiom (per Rust API Guidelines and `cast` lints) is to make lossy conversions **explicit-method-named**, not provided via `From`/`Into`. The `From<Pixels> for f64` is fine (lossless promotion). The integer ones are lossy and silent. |
| **Spec position** | Task spec keeps these. |
| **My counter** | I recommend **replacing the three integer `From` impls** with explicit methods: `Pixels::to_i32_round() -> i32`, `Pixels::to_u32_round_clamped() -> u32`, `Pixels::to_usize_round_clamped() -> usize`. The method names announce the rounding/clamping behavior. The existing `From<Pixels> for f32`/`f64` stay. |
| **Cost** | Lower than U1 — these conversions are rarer at call sites (integer pixel extraction is usually framebuffer code). |
| **Verdict** | ◐ **Recommend, but lower priority.** Can be a follow-up commit. Not blocking the polish-pass PR. |

### U12 (NEW) — Add `port-check.sh` trigger to prevent regression

| Aspect | Detail |
|---|---|
| **What** | After U1/U2/U6/U7 land, add a new refusal trigger #14 (or amend an existing one) to `scripts/port-check.sh` that rejects:<br/>• `impl From<f32> for Pixels` (or any wrapper marked `#[repr(transparent)] pub struct ..(pub f32)` in `flui-geometry`)<br/>• `impl PartialEq<f32> for X` / `impl Add<f32> for X` where `X` is a length-unit wrapper<br/>• Any `Float*` type alias in `flui-geometry/src/lib.rs` |
| **Why** | The polish pass invests engineering effort in **establishing** the invariant. Without a CI gate, the next contributor who needs "just one quick conversion" re-introduces it. The bug class returns. |
| **Verdict** | ✓ **Approve, must accompany the polish-pass PR.** Without it, the polish is cosmetic. |

### U13 (FUTURE, not now) — Watch GPUI #32339 trajectory before unifying `DevicePixels + ScaledPixels`

| Aspect | Detail |
|---|---|
| **Context** | [zed#32339](https://github.com/zed-industries/zed/pull/32339) tried to merge `DevicePixels + ScaledPixels → PhysicalPixels<S>`. Closed as draft (not yet merged). Direction is set, but implementation isn't stable. |
| **flui current** | Has the same 3-tier shape GPUI is dismantling. |
| **Recommendation** | **Do NOT preempt the unification.** Track the upstream PR. When/if it merges, schedule a parallel migration as a separate U-future. The current 3-tier shape is honest (each tier has a distinct semantic role); collapsing two of them into a generic adds compile-time complexity that may or may not pay off — let upstream prove it first. |
| **Verdict** | ◐ **Queued for monitoring, not for this PR.** |

---

## Part IV — Migration order (the polish-pass PR sequence)

A sound order avoids cascading rewrites. Recommended commit order:

```
1. feat(geometry): tighten Pixels — remove cross-type ops              [U2]
   Cross-type PartialEq/PartialOrd/Add/Sub for f32 removed.
   Compile_fail doctest added.

2. refactor(geometry): drop Pixels From<scalar> conversions             [U1]
   5 From impls deleted. Constructors are now the only path.
   Doctest + port-check trigger candidate.

3. refactor(geometry): remove speculative Float* type aliases           [U6]
   4 dead aliases. SP-4 cited. Zero call-site fallout (verified).

4. fix(geometry): kill Pixels × Pixels semantic bug                     [U4 + U9 + U10]
   Mul / MulAssign / DivAssign that landed area-or-ratio in length removed.
   Compile_fail doctest. Same commit because they share the same defect class.

5. refactor(geometry): EdgeInsets becomes Edges<Pixels>                 [U3]
   Type alias change in lib.rs. All call sites updated to px(...).
   Constructors added: EdgeInsets::all(px) / symmetric / only / zero.

6. refactor(geometry): deprecate raw-scalar device conversions          [U5]
   3 functions + 3 wrapper-cascade siblings (Size/Point/Bounds versions)
   marked #[deprecated]. Internal callers migrated to typed ScaleFactor path.

7. refactor(geometry): remove broken transform_scalar                   [U7]
   Function deleted. Doc-example was lying about its own type guarantee.
   Migration: existing typed Pixels::to_device / DevicePixels::to_logical.

8. feat(geometry): install port-check refusal triggers for unit barrier [U12]
   Mechanical gates so the polish pass cannot regress.

9. (optional, follow-up PR)
   refactor(geometry): explicit lossy integer conversions               [U11]
```

**Dependency rationale:**

- **U2 first** because it has the cleanest semantic ("cross-type ops should never have existed") and gives the smallest fallout window. Sets the tone.
- **U1 second** — depends on U2 (without U2, `From<f32>` and `Add<f32>` are redundant in the same way, but the user-facing diagnostic is sharper if `==` and `+=` already fail).
- **U6 third** — independent dead code, free to drop at any point. Doing it early shrinks the surface for later steps.
- **U4+U9+U10 fourth** — same defect class (area-as-length, ratio-as-length), atomic.
- **U3 fifth** — **must come after** U1/U2 because the call-site rewrite from `EdgeInsets { left: 16.0 }` to `EdgeInsets { left: px(16.0) }` is mechanical *only if* `16.0` can no longer satisfy the field type. Otherwise call-site authors can drop bare `f32` again.
- **U5 sixth** — independent of U1–U4. Could go first or last; placed here to keep deprecations grouped with method-level rather than impl-level changes.
- **U7 seventh** — semi-independent; placed late because deletion may surface a final call-site that wants to use the typed conversion instead.
- **U12 last** — the refusal triggers can only land green when all the impls they ban are already gone.

**Atomic-commit shape** (per port discipline): each numbered step = one commit with `feat(geometry):` / `refactor(geometry):` / `fix(geometry):` prefix per Conventional Commits, the AGENTS.md decompose rule, and the in-spec U-ID requirement.

---

## Part V — Risks and counter-arguments

### R1 — "But removing `From<f32> for Pixels` breaks our serde flow."

**Check:** rg shows no serde usage on Pixels today (`flui-geometry/Cargo.toml` doesn't depend on serde; `Pixels` has no `Serialize`/`Deserialize`). Hypothetical risk, not present.

**Forward mitigation:** when serde is added later, derive `Serialize`/`Deserialize` and use `#[serde(transparent)]` — that uses the `f32` representation directly, doesn't need `From<f32>` for `Pixels`.

### R2 — "But we're losing ergonomics."

`px(16.0)` vs `16.0` is **3 extra characters per literal**. This is not an ergonomics fight; it's a "do you want unit safety or not" fight. Compose/Slint/GPUI all paid this cost and shipped. The mitigation is good constructor coverage on `EdgeInsets` (U3 refinement).

### R3 — "kurbo is `f64` and we are `f32` — won't lossy conversion hurt?"

**`f32 → f64` is lossless** (every `f32` is exactly representable in `f64`). The direction `flui::Pixels` → `kurbo::Point` is `f32` widened to `f64` — exact. Only the **return path** from kurbo to flui (`f64 → f32`) is lossy, and that's the path where we want explicit `TryFrom` with `Result<_, KurboBridgeError>`. The "lossy" risk is in exactly one direction and is handled by Rust's standard pattern.

### R4 — "GPUI is the reference, GPUI doesn't have all these guards — are we being more catholic than the pope?"

GPUI is **actively moving in this direction** (PR #32339 is the proof). It just hasn't merged the unification yet. flui is starting clean — it can adopt the destination shape without GPUI's migration backlog.

### R5 — "The 50-site EdgeInsets migration is risky."

The 50 sites are concentrated in 3 files (`padding.rs`, `sliver_padding.rs`, `box_constraints.rs`). Mechanical rewrite. The risk is **regression of behavior**, not compile correctness — `EdgeInsets::all(8.0)` and `EdgeInsets::all(px(8.0))` produce the same runtime value, so behavior is preserved. Tests catch any algebraic drift.

### R6 — "Why now, not later?"

Two reasons: **(a)** the polish pass is small enough (~300 lines) that it can be one focused review cycle, before `flui-widgets`/`flui-material` start importing `EdgeInsets` and amplifying the call-site count from 50 to 500+. **(b)** Core.2's render-object catalog needs the kurbo bridge plan (U8) on day one, and the bridge plan rides on the polish-pass discipline being in place.

### R7 — "We are changing semantics under our own consumers (`flui-rendering` etc.)."

True — but consumers don't have stable downstream consumers yet (Business.1 not built). This is **the cheapest moment** in the entire FLUI lifecycle to do this work. Each phase delayed makes the call-site count larger.

---

## Part VI — Recommendations (final)

1. **Approve U1, U2, U3, U4, U5, U6, U7 in the spec's stated form**, with the modifications recommended above:
   - **U7**: choose the "remove" variant, not the "fix" variant.
   - **U4**: bundle with **U9** (MulAssign removal) and **U10** (DivAssign removal) in the same commit — same defect class.
   - **U5**: also deprecate the `Size::to_device_pixels()`/`Point::to_device_pixels()`/`Bounds::to_device_pixels()` wrapper cascade, not only the unit-level functions.
   - **U3**: order *after* U1+U2 to maximize compile-time validation of the migration.

2. **Add U12 to the same PR**: a `port-check.sh` refusal trigger (or amendment) that bans regressing impls. Without this, the polish is cosmetic; with it, the invariant is enforced.

3. **File U8 as a follow-up tracked row** in `ROADMAP-TRACKER.md` under Core.2 entry preconditions. `feature = "kurbo"` bridge layer is a separate PR but should not be forgotten — kurbo's `f64`/`Point`-vs-`Vec2` design is the conceptual model flui is aligning toward. Set the bridge layer to land **before** Core.2's first object that needs path/curve operations.

4. **File U11 as a follow-up PR** (lossy integer conversions). Lower priority. Can be deferred to a code-hygiene sweep.

5. **Defer U13** (DevicePixels + ScaledPixels unification). Monitor [zed#32339](https://github.com/zed-industries/zed/pull/32339); revisit after upstream merges.

6. **Update `ROADMAP-TRACKER.md`** with a new Core.0 sub-section "N-geom" capturing U1–U7 + U9–U12 as a polish-pass block, status `◐ in-progress`, owner = this research doc.

7. **Update `docs/PORT.md`** translation manual with the kurbo bridge pattern (U8) as an idiom entry under "Boundary conversions": `f32 → f64` lossless promotion, `f64 → f32` fallible TryFrom returning `KurboBridgeError`.

8. **See Part VIII for the broader math-stack question** — the polish pass is necessary but not sufficient. The repo currently drifts between owning math and reusing glam/euclid/kurbo; that drift must be reconciled in PRs that follow this one. **Revised recommendation under foundation-quality + breaking-allowed mandate (2026-05-24):** polish (this PR) → U17 euclid spike (risk gate) → PR 2 = **Option C by default** (full hybrid euclid+glam+kurbo+mint), Option D only as documented fallback if spike measures Option C cost as catastrophic → kurbo bridge → done. No PR 4.

---

## Part VIII — The math-stack question (added 2026-05-24)

The initial research covered UI frameworks but skipped the **fundamental math crates** that sit beneath them. This is a gap, because flui-geometry has **3,833 LOC of pure linear algebra** (`matrix4.rs` 1,040 + `vector.rs` 1,489 + `transform.rs` 890 + `transform2d.rs` 414) **including its own SSE2/NEON SIMD implementation** (`matrix4.rs:797 fn mul_simd_sse`, `:843 fn mul_simd_neon`). That code lives in production-grade form in `glam` already, with broader hardware coverage, more reverse dependencies, and continuous SIMD updates.

### Existing architectural drift in the repo

Three facts surfaced during audit that the polish-pass spec didn't enumerate:

1. **`flui-geometry/Cargo.toml`** already declares optional `glam = "0.30"`, `mint = "0.5"`, and a `full = ["serde", "glam", "mint", "simd"]` feature group. **The integration infrastructure exists**, undocumented and unused.
2. **`flui-engine/src/wgpu/effects.rs`** imports `glam::Vec2` directly — bypassing the entire flui-geometry layer for engine-side code. The framework's own engine layer treats glam as authoritative while the geometry layer pretends glam isn't there.
3. **`flui-types/README.md` line 280** answers "Why not use glam or euclid?" with "FLUI needs Flutter-compatible APIs and unit type safety specific to UI layout" — a rationale that is **partially obsolete**. euclid provides exactly that unit-safety via `Length<T, Unit>` phantom-typed (Slint's foundation since 2022). Flutter-compatible methods are 50-line extension traits, not justification for a 3,833-LOC math reimplementation.

The repo is mid-decision and the decision has not closed. The polish pass should not deepen this drift.

### The Rust 2026 math-stack landscape

| Crate | Scalar | Typed units | SIMD | Use case | flui interop status |
|---|---|---|---|---|---|
| **`glam`** (Bevy default) | f32 / f64 / int variants | **No** — concrete `Vec2`/`Vec3A`/`Mat4` etc. | Native SSE2 / NEON / wasm-simd128 | Games, graphics, GPU math | `Cargo.toml` opt-in feature present ✓ |
| **`euclid`** (Mozilla/Servo/Slint) | Generic `T` | **Yes** — phantom-typed `Length<T, Unit>`, `Point2D<T, U>`, `Size2D<T, U>`, `Rect<T, U>` | No (scalar) | UI, browser geometry, typed coords | Indirect via Slint pattern only |
| **`nalgebra`** | Generic `T: RealField` | Partial (via `Unit<V>` wrapper) | Optional (slower than glam in mathbench) | Scientific / robotics / generic linalg | Cargo.lock has it transitively |
| **`cgmath`** | f32 / f64 generic | No | No | **Legacy, maintenance-only post-2015** | Skip — superseded |
| **`mint`** | Generic | No (interop only — no operations) | No | **Common-ground bridge layer** | `Cargo.toml` opt-in feature present ✓ |
| **`kurbo`** (Linebender / Vello / Xilem) | f64 only | No | No | Curves, Bézier paths, vector graphics math | `Cargo.lock` has it; queued for U8 bridge |
| **`bevy_math`** | Re-exports `glam` + adds `Curve`, `Affine3` reduced | No (delegates) | Yes (via glam) | Game engine ergonomics over glam | Reference pattern only |

**Reading the table:**

- **`glam`** is the modern default for `f32` graphics/UI math in Rust 2026 — 76M downloads, 1,005 reverse dependencies, SIMD-by-default, **outperforms `cgmath` 2-3× and `nalgebra` 5-100×** on common operations ([bitshifter/mathbench-rs](https://github.com/bitshifter/mathbench-rs)).
- **`euclid`** is the Rust 2026 default for **typed UI units** — phantom-typed `Length<T, Unit>` is exactly the problem flui-geometry's `Unit` trait is trying to solve, with 12 years of production hardening (Servo since 2014, Slint since 2022). It does NOT do SIMD — that's not its job.
- **`mint`** is the explicit *bridge* layer the ecosystem agreed on. kurbo supports `feature = "mint"`; glam supports it; nalgebra supports it. For cross-library normalization, mint is the lingua franca.
- **`nalgebra`/`cgmath`** are not relevant for flui — nalgebra is scientific-computing-oriented (and slower for graphics workloads), cgmath has been in maintenance mode since 2015.

### Authoritative voice — euclid's own design decision

The Servo team chose **phantom-typed `Length<T, Unit>`** over **newtype-per-unit** explicitly, after debate. Matt Brubeck (euclid author) on [servo/euclid#35](https://github.com/mozilla-servo/rust-geom/pull/35) responding to Patrick Walton's newtype proposal:

> My earlier drafts used newtypes, but I found that it made it harder to write generic code and ended up requiring a lot more boilerplate. The current design allows _all_ of the implementation to be generic, so adding a new unit requires only a single line of code [...]
>
> Au is hard-coded to an i32 representation. This seems bad because it would lead to an explosion of newtypes like `struct DevicePixel(f32)` and `struct DevicePixelInt(i32)`, each requiring its own trait implementations.

This is **directly relevant to flui** — we have `DevicePixels(i32)` and `Pixels(f32)` as **separate types**, exactly the "explosion" Brubeck warned against. euclid's `Length<f32, PixelsUnit>` + `Length<i32, DevicePixelsUnit>` solves both with one generic primitive.

### Four architectural options

**Option A — Status quo + drift (NOT viable as final state)**

Keep 3,833 LOC of own math + 4 feature flags (`glam`, `mint`, `serde`, `simd`) nobody uses + engine importing glam directly. The polish pass alone (U1–U13) **does not** resolve this; it only tightens the existing surface.

- **Cost:** unchanged maintenance burden, hand-written SIMD path, README contradicting Cargo.toml, drift continues.
- **Verdict:** ✗ Reject as final shape. Polish pass alone leaves drift in place.

**Option B — Full migration to `euclid` only**

Replace flui types with euclid re-exports under unit-typed aliases:

```rust
// flui-geometry/src/lib.rs after migration
pub enum PixelsUnit {}
pub enum DevicePixelsUnit {}
pub enum ScaledPixelsUnit {}

pub type Pixels = euclid::Length<f32, PixelsUnit>;
pub type DevicePixels = euclid::Length<i32, DevicePixelsUnit>;
pub type Point<U = PixelsUnit> = euclid::Point2D<f32, U>;
pub type Size<U = PixelsUnit> = euclid::Size2D<f32, U>;
pub type Rect<U = PixelsUnit> = euclid::Rect<f32, U>;
pub type Transform2D<Src, Dst> = euclid::Transform2D<f32, Src, Dst>;
// flui-specific additions stay as extension traits:
impl FlutterRectExt for euclid::Rect<f32, PixelsUnit> { fn from_ltrb(...) {...} }
```

- **Cost:** ~18,000 LOC deletion, ~2,000 LOC of extension traits for Flutter-API parity. Downstream consumers need import-path rewrites — mostly mechanical.
- **Win:** typed units battle-hardened (Servo 12y, Slint 4y). Single generic primitive. Slint's exact pattern. SIMD would still need a separate glam bridge for hot paths.
- **Verdict:** ◐ Strong long-term direction, but **dominated by Option C** because euclid alone doesn't give us SIMD or kurbo curves. If we're doing a big refactor, do it once and use all three crates in their native roles.

**Option C — Hybrid: `euclid` (UI typing) + `glam` (GPU/SIMD) + `kurbo` (curves)** — **DEFAULT after 2026-05-24 user direction (breaking allowed for foundation quality)**

Three libraries, each in its native role:

- **UI / layout layer** (`flui-widgets`, `flui-rendering`, `flui-geometry` public surface): `flui::Length<f32, Pixels>` (a thin newtype wrapper over `euclid::Length<f32, PixelsUnit>` to reimpose U1–U12 invariants over euclid's default `From<T>` looseness), `flui::Point<U>` over `euclid::Point2D<f32, U>`, `flui::Size<U>` over `euclid::Size2D<f32, U>`, `flui::Rect<U>` over `euclid::Rect<f32, U>`, `flui::Transform2D<Src, Dst>` over `euclid::Transform2D<f32, Src, Dst>` — typed safety + polish-pass discipline.
- **GPU / engine layer** (`flui-engine`, paint hot paths, `flui::Matrix4` for affine 4×4): `glam::Vec2`/`Mat4`/`Affine2`/`Affine3A` with `feature = "bytemuck"` for Pod, `feature = "mint"` for kurbo bridge — SIMD speed.
- **Curves / paths** (`flui-painting` Bézier operations, future Vello swap): `kurbo::Point`/`BezPath`/`Affine` with `feature = "mint"` — f64 accuracy.
- **Bridge layer:** `mint` types as the cross-library lingua franca — `kurbo` ↔ `glam` is zero-author. Only flui's typed `Length<f32, PixelsUnit>` → `glam::Vec2` boundary needs explicit `.to_vec2()` methods at hot-path boundaries (engine paint commands).

This is **what Slint does in production** (euclid + lyon + skia bindings) and what Bevy does (glam + bevy_math wrappers over it). It honors each library's native scalar type — euclid is generic f32, glam is SIMD f32, kurbo is f64 — and converts only at the layer boundaries.

- **Cost:** large refactor across rendering/painting/engine. ~80 widget call sites will need migration (when they exist; currently 24 EdgeInsets sites + a handful of Padding/Rect sites). Boundary conversions need ergonomic patterns (newtype methods + From impls).
- **Win:** best-in-class behavior per role. Maintenance burden drops sharply (no own SIMD, no own typed-length system, no own curve math, no own bridge mechanics — mint handles cross-library). 3,833 LOC of own math deleted entirely, not just wrapped.
- **Verdict:** ✓ **DEFAULT for PR 2 given foundation-quality + breaking-allowed mandate** (user direction 2026-05-24). Polish pass invariants ride on top of euclid via a thin wrapper, not on top of own newtypes. Option D moves to fallback (only if U17 spike measures Option C's call-site cost as catastrophic — >3× Option D LOC).

**Option D — Wrap glam under current API (pragmatic incremental)**

Keep the *public* `flui_geometry::Point<U>`, `Vec2<U>`, `Matrix4` API surface. Replace the *internal* math implementation with glam delegation:

```rust
#[repr(transparent)]
pub struct Matrix4(glam::Mat4);  // was 1,040 LOC of hand-written math + SIMD

impl Matrix4 {
    pub fn identity() -> Self { Self(glam::Mat4::IDENTITY) }
    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        Self(glam::Mat4::from_translation(glam::vec3(x, y, z)))
    }
    pub fn determinant(&self) -> f32 { self.0.determinant() }
    pub fn inverse(&self) -> Option<Self> {
        if self.0.determinant() != 0.0 { Some(Self(self.0.inverse())) } else { None }
    }
    pub fn transform_point(&self, x: Pixels, y: Pixels) -> (Pixels, Pixels) {
        let v = self.0.transform_point3(glam::vec3(x.get(), y.get(), 0.0));
        (px(v.x), px(v.y))
    }
    // ... ~3-4 delegation lines per public fn (count below)
}

#[repr(transparent)]
pub struct Vec2<U: Unit>(glam::Vec2, PhantomData<U>);
```

**Measured public surface to wrap** (`grep -cE '^\s*pub fn |^\s*pub const fn '`, 2026-05-24):

| File | `pub fn` / `pub const fn` count | Original LOC | Realistic delegation LOC |
|---|---:|---:|---:|
| `matrix4.rs` | 36 | 1,040 | 150–250 |
| `vector.rs` | 55 | 1,489 | 200–350 |
| `transform.rs` | 23 | 890 | 100–150 |
| `transform2d.rs` | 12 | 414 | 50–100 |
| **Total** | **126 public fns** | **3,833** | **500–850 LOC** |

- **Cost:** Internal rewrite of 126 public methods across 4 files. Each method is 3-4 lines of delegation (signature + glam call + return wrapping). **Realistic LOC: 500–850**, not 30 as the first draft suggested. Downstream consumers unaffected (public API stable).
- **Win 1 — SIMD free:** glam handles SSE2/NEON/wasm-simd128. Hand-rolled `mul_simd_sse`/`mul_simd_neon` deleted. Our `simd` Cargo feature deleted.
- **Win 2 — GPU buffer (Pod) compatibility, strict improvement.** Verified via `rg 'bytemuck::Pod|impl Pod' crates/flui-geometry crates/flui-engine`: `flui-geometry/Cargo.toml` has **0 bytemuck mentions**; `Matrix4` is NOT `Pod`. Today `flui-engine` works around this by maintaining its own Pod-derived vertex structs (5 in `effects.rs`, 4 in `instancing.rs`, 3 in `offscreen.rs`, 1 in `vertex.rs`) and **never feeds `Matrix4` to a wgpu buffer directly** — the renderer trait takes `&Matrix4` (~15 method signatures in `traits.rs`) and each call site manually copies into a local Pod struct. **Glam ships `feature = "bytemuck"`** that derives `Pod`/`Zeroable` on `Mat4`/`Vec2`/`Vec4`/etc. Option D + that feature flag = engine can `bytemuck::cast_slice(&[matrix4])` directly. Per-call Pod-conversion shim becomes deletable. **This is a strict improvement, not parity.**
- **Win 3 — mint bridge cascade, strict improvement.** Glam with `feature = "mint"` adds `From<mint::Vector2<f32>> for glam::Vec2` (and reverse). kurbo also supports `feature = "mint"`. **Enabling `glam = { features = ["bytemuck", "mint"] }` + `kurbo = { features = ["mint"] }` gives a cascading bridge** — the kurbo bridge (U8) becomes a one-line mint pass-through instead of hand-authored `as f64`/`as f32` casts. This *shrinks PR 3* (the kurbo bridge work).
- **Risks:** glam's `Mat4` is 64 bytes aligned to 16 (SIMD); our current `Matrix4` is also 64 bytes — `#[repr(transparent)]` should hold, verify with `assert_eq!(size_of::<Matrix4>(), size_of::<glam::Mat4>())` test. Edge cases where our determinant/inverse semantics differ from glam (e.g. glam doesn't return `Option<Mat4>` on inverse) need explicit handling — see Option D code above.
- **Verdict:** ◐ **Fallback only — use if U17 spike shows Option C migration cost is catastrophic (>3× D LOC).** Under the new breaking-allowed mandate, Option D is no longer the default. Its only role: clean retreat path if euclid migration surfaces ergonomic regressions on Flutter-API parity methods or type-parameter explosion in trait bounds. Still delivers SIMD + Pod + mint-bridge cascade if needed.

### Recommended sequencing (revised 2026-05-24 — breaking allowed + foundation quality mandate)

```
┌──────────────────────────────────────────────────────────────┐
│ PR 1 (now):           Polish pass — U1–U12                  │
│                        Invariants ride into Option C via   │
│                        the flui wrapper over euclid.       │
│                                                              │
│ SPIKE (~1 day):       U17 euclid Length+Point2D on Padding │
│                        Measure realistic Option C cost.    │
│                        **Risk gate, not go/no-go**.         │
│                                                              │
│ PR 2 (DEFAULT):       Option C — full hybrid migration     │
│                        euclid (UI typing) + glam (GPU/SIMD)│
│                        + kurbo (curves) + mint (bridge).   │
│                        Breaking change explicitly accepted.│
│                                                              │
│ PR 2 (FALLBACK):      Option D wrap-glam ONLY if spike     │
│                        shows Option C costs > 3× Option D   │
│                        LOC (i.e. > 2,500 LOC migration).   │
│                        Pragmatic mid-state — NOT preferred.│
│                                                              │
│ PR 3 (Core.2 entry):  kurbo bridge [U8]                     │
│                        Under Option C → only flui::Length → │
│                        kurbo::Point boundary; glam ↔ kurbo  │
│                        is free via mint.                    │
│                                                              │
│ PR 4: not needed.    Option C is the destination. No       │
│                        further migration. Maintenance phase.│
│                                                              │
│ Option B dominated by Option C — skip.                      │
│ Option A dominated by everything — reject.                  │
└──────────────────────────────────────────────────────────────┘
```

**Rationale for this order (revised under foundation-quality + breaking-allowed mandate):**

1. **Polish pass first** because it tightens the existing surface, and PR 1's invariants **become the wrapper discipline on top of euclid** in PR 2. Without polish, the flui wrapper over `euclid::Length<T, U>` inherits euclid's `From<T>` default — the U1 escape hatch returns through the back door. PR 1 lays the discipline; PR 2 reapplies it over euclid types.
2. **Spike U17 second, BEFORE PR 2 commit.** With breaking allowed, the spike is no longer "feasibility check for jumping to C" — it's a **risk gate** on Option C. Decision rule: spike LOC/widget × ~80 widgets vs Option D's 500–850 LOC. If C ≤ 3× D → take Option C. Only if C > 3× D (i.e. >2,500 LOC migration), fall back to D. The 1.5× threshold of the earlier draft assumed cost-parity discipline; foundation-quality discipline relaxes to 3× because the long-term win justifies short-term migration cost.
3. **kurbo bridge third (PR 3).** Core.2 needs it. **Under Option C**: only one boundary needs an explicit method — `flui::Length<f32, Pixels>` → `kurbo::Point` (lossless f32→f64 via mint). `glam::Vec2` ↔ `kurbo::Point` is zero-author via mint. The bridge module is small.
4. **PR 4 is not part of the plan.** Option C IS the destination. There is no "later phase decides to migrate further" — we are doing the full migration now, while the cost is at its lowest (no downstream widget catalog yet).

### Foundation-quality calculus (added 2026-05-24 after user direction)

User confirmed: **"breaking changes are allowed to build the right project so future contributors are pleased with a correctly laid foundation."** This is a significant direction signal that changes the default in three ways:

1. **Option D is no longer the pragmatic default.** It was justified by "public API stays stable." That justification weakens when breaking is explicitly permitted. Option D's only remaining role is as **fallback if Option C's measured cost is catastrophic** (spike LOC × 80 widgets > 2,500 LOC).
2. **Option C is no longer "the long-term destination, defer for now."** It is **the right shape to build first**, because:
   - The downstream widget catalog (`flui-widgets`, `flui-material`) is **not yet built**. Migration cost = ~24 EdgeInsets sites + a handful of Padding/Rect sites today; it will be ~200+ Padding sites alone after Business.1. **Migration cost monotonically increases with time.** Now is the lowest-cost moment.
   - **Future contributors arrive into a clean architecture**, not into `flui::Matrix4(glam::Mat4)` wrappers asking "why don't we just use glam directly?" The answer "we planned to migrate but got stuck" is a foundation flaw, not a feature.
   - Slint has proven this architecture works at production scale (22k+ stars, native UI toolkit for Rust/C++/JS/Python). It's not experimental.
3. **Option D as fallback retains value.** If U17 spike reveals that euclid migration has surprise costs (e.g. type-parameter explosion in trait bounds, ergonomics regressions on Flutter-API parity methods), Option D is a clean retreat path: wrap glam internally, ship working framework, revisit C later. The point of the spike is exactly to surface such surprises before committing.

**Net effect**: PR 2 default becomes Option C; Option D moves to a documented contingency. The U17 spike becomes a *risk gate* on Option C, not a *go/no-go* between two equally weighted alternatives.

### DevicePixels representation under Option C (decision, 2026-05-24 advisor R-PreFlight-2)

**Decision:** keep `DevicePixels` as `i32`-backed; **delete `ScaledPixels` entirely**. Final 2-tier shape: `Pixels(f32, PixelsUnit)` + `DevicePixels(i32, DevicePixelsUnit)`.

**Rationale:**

1. **Brubeck's "explosion" warning is about one *conceptual* quantity with two *scalar* representations** (Au i32 + DevicePixel f32 — both for CSS logical px). flui has the opposite: `DevicePixels` is semantically integer (framebuffer addresses, scissor regions, `wgpu::Origin3d { x: u32, y: u32, z: u32 }`), `Pixels` is semantically float (sub-pixel layout). **Two conceptual quantities, each with one canonical scalar type.** Not an explosion.
2. **`ScaledPixels` has 0 production usage** (grep verified — only `lib.rs` aliases + a few internal cast-target methods). It is SP-4 speculative scaffolding alongside the `Float*` aliases of U6. **Delete in PR 1 as an extension of U6**, or in PR 2 as part of the unit-system simplification.
3. **`wgpu` API is integer-native** for framebuffer ops: `set_scissor_rect(u32, u32, u32, u32)`, `Origin3d { x: u32, .. }`, `Extent3d { width: u32, .. }`. `DevicePixels(i32) as u32` is one explicit shim at the engine boundary, cleaner than `f32 → u32 round-then-cast`.
4. **Slint clamps at boundary** (their pattern). That's a per-call cost we avoid by integer-typing.
5. **GPUI #32339 was unifying `DevicePixels + ScaledPixels` to remove confusion** (which one when?), not because i32-vs-f32 was the problem. By **deleting `ScaledPixels`** we sidestep that exact confusion without giving up integer device coords.

**Consequence for tracker:** **add U6.1** (delete `ScaledPixels` and its aliases) to PR 1 polish pass alongside U6 (delete `Float*` aliases). Same justification (SP-4), zero production usage, mechanical removal.

### Production-constraint check: GPU buffer / Pod compatibility (added 2026-05-24 after advisor flagging)

A critical risk advisor flagged: `flui-engine` uses `wgpu`, which requires `bytemuck::Pod` for buffer fill. Before recommending Option D, we verified:

**Findings from `rg 'bytemuck::Pod|impl Pod' crates/flui-geometry crates/flui-engine`:**

- `crates/flui-geometry/Cargo.toml`: **0 mentions of bytemuck.** `Matrix4`, `Vec2<U>`, `Point<U>`, etc. are NOT `Pod`.
- `crates/flui-engine/Cargo.toml`: 2 bytemuck mentions — engine has its own Pod-derived vertex structs.
- **All Pod-derived types live in `flui-engine`**: `effects.rs` (5 structs), `instancing.rs` (4 structs), `offscreen.rs` (3 structs), `vertex.rs` (1 struct). Each is a *vertex layout struct specific to a pipeline*, not a wrapper around `flui_geometry::Matrix4`.
- `flui-engine`'s renderer trait (`crates/flui-engine/src/traits.rs`) takes `&Matrix4` by reference (~15 method signatures with `transform: &Matrix4`), but each implementation **manually copies into a local Pod struct** before `bytemuck::cast_slice` into a wgpu buffer. This is invisible per-call overhead and a maintenance hazard.
- `glam` ships with `feature = "bytemuck"` that derives Pod on `Mat4`, `Vec2`, `Vec3`, `Vec4`, `Affine2`, `Affine3A`, etc.

**Conclusion:** Option D doesn't just preserve the GPU story — it **improves** it. Today's engine has to round-trip `Matrix4 → local Pod struct → wgpu buffer`. Under Option D with `glam = { features = ["bytemuck"] }`, the engine can `bytemuck::cast_slice(&[matrix4])` directly. The Pod-conversion shim in engine becomes deletable. (Win 2 in Option D analysis.)

### Why Option C is the correct default now (under foundation-quality mandate)

The earlier draft of this research argued "Why not adopt Option C immediately" — listing three reasons to defer. Each of those reasons is now revisited given the breaking-allowed direction:

1. **"Polish-pass invariants are universal across backings."** Still true. But the implication flips: polish lays the discipline that the wrapper-over-euclid in PR 2 will inherit. Polish in PR 1 → wrapper in PR 2 reimposing discipline over `euclid::Length<T, U>::from(T)` is the canonical sequence. **Wrapper LOC revised after advisor R-PreFlight-1:** ~200 LOC was an order-of-magnitude undercount (same error as the original "30 LOC delegation" for Option D). The realistic wrapper needs to (a) block euclid's default `From<T>` impls, (b) reimplement Add/Sub/Mul/Div discipline (euclid provides these on raw Length, must be replaced/restricted), (c) keep Flutter-API parity methods (`Rect::from_ltrb`, `Size::area`, etc.), (d) add `bytemuck::Pod` derives where engine needs them, (e) provide type aliases preserving today's `Point<Pixels>` ergonomics. **Realistic wrapper LOC: 1,200–2,000**, still a strict win versus 18,945. This must be measured by U17 spike, not assumed.
2. **"Option D banks the SIMD win for free."** True under D; equally true under C — Option C still uses `glam::Vec2/Mat4/Affine2` for GPU paint operations. The SIMD win lands in PR 2 either way. Under Option C, glam is one of three libraries (each in its role) rather than the single backing.
3. **"Option C requires committing to verbose type-parameter syntax."** Mitigated by type aliases inside the flui wrapper crate:
   ```rust
   // flui-geometry/src/lib.rs under Option C
   pub type Length = flui::Length<f32, PixelsUnit>;      // wrapper, not raw euclid
   pub type Point<U = PixelsUnit> = flui::Point<f32, U>;  // wrapper over euclid::Point2D
   pub type Size<U = PixelsUnit>  = flui::Size<f32, U>;
   pub type Rect<U = PixelsUnit>  = flui::Rect<f32, U>;
   ```
   Public surface stays as concise as today's `Point<Pixels>`. Verbosity is contained to wrapper-crate internals.

**Conclusion:** the original three deferral reasons are weak under the new mandate. Option C is the correct PR 2 — conditional only on the U17 spike not surfacing catastrophic migration costs.

### U17 spike scope (revised after advisor R-PreFlight-3)

**Original spike scope (1 day):** migrate `flui-rendering::Padding` to `euclid::Length<f32, PixelsUnit>` + `euclid::Point2D<f32, U>`, measure per-widget LOC.

**Revised spike scope (2 days):** the spike must build the **wrapper crate itself**, not just measure call-site migration. Without measuring wrapper LOC, the decision rule `spike_LOC × 80 widgets` underestimates total Option C cost by ~1,500 LOC (the wrapper amortized once).

**Revised work breakdown:**

1. **Day 1 — build wrapper newtype crate scaffold** (~half day): `flui::Length<T, U>(euclid::Length<T, U>)`, `flui::Point<T, U>(euclid::Point2D<T, U>)`, `flui::Size<T, U>`, `flui::Rect<T, U>`, `flui::Transform2D<T, Src, Dst>`. Reimpose U1–U12 invariants. Add Flutter-API parity methods. Add `bytemuck::Pod` derives. Add type aliases.
2. **Day 1 (continued) — measure wrapper LOC**: report total wrapper crate size.
3. **Day 2 — migrate `flui-rendering::Padding`**: convert one widget's call sites to use the wrapper types. Measure per-widget migration LOC.
4. **Day 2 — produce decision report**: `wrapper_LOC + (per_widget_LOC × ~80 widgets) vs 3 × Option_D_LOC (which is 3 × 750 = 2,250 LOC)`.

**Revised decision rule:**

```
Option C total = wrapper_LOC + (per_widget_LOC × 80)
Option D total = ~750 LOC (mid-point of 500–850 estimate)

If Option C total ≤ 3 × Option D total (i.e. ≤ 2,250 LOC) → take Option C
If Option C total > 2,250 LOC → fall back to Option D
```

**Example outcomes:**

- Wrapper = 1,500 LOC, per-widget = 10 LOC → Option C total = 2,300 LOC → borderline, surface the trade-off explicitly.
- Wrapper = 1,200 LOC, per-widget = 5 LOC → Option C total = 1,600 LOC → take Option C.
- Wrapper = 2,000 LOC, per-widget = 25 LOC → Option C total = 4,000 LOC → fall back to Option D.

### U7 collision check (advisor R-PreFlight-3 measurement)

**Question:** does polish-pass U5 (deprecate `to_device_pixels(f32)`) and U7 (delete `transform_scalar`) collide with PR 2's wholesale swap of `flui::ScaleFactor` to `euclid::Scale`?

**Measurement (`rg 'ScaleFactor' crates/ examples/`, 2026-05-24):**

- 89 mentions total across the workspace.
- 31 in `flui-geometry/src/units.rs` (definitions + tests).
- 28 in `flui-geometry/src/size.rs`/`rect.rs`/`point.rs`/`offset.rs` (wrapper methods).
- 10 in `flui-types/tests/scale_conversion_tests.rs`.
- ~12 in `flui-platform` (window display info from OS).
- `transform_scalar` itself: **0 production callers** (only doc-example in its own definition).
- `.to_device()`/`.to_logical()`/`.to_scaled()` (the typed API): all in `units.rs` tests, **0 production callers outside geometry**.

**Conclusion:** U5 deprecation tags and U7 deletion are **forward-compatible with PR 2's `euclid::Scale` swap**. Deprecation tags in PR 1 explicitly signal next-step migration; PR 2 then removes the deprecated functions entirely as part of the `flui::ScaleFactor → euclid::Scale` swap. **Keep U5+U7 in PR 1 scope.** Not wasted work.

### New tracker rows (additions to `ROADMAP-TRACKER.md`)

| ID | Title | Status | Notes |
|---|---|---|---|
| **U14-glam** | Wrap `Matrix4` / `Vec2<U>` / `Transform*` over glam internals | ☐ todo (post-polish) | Option D from this research. Internal-only refactor; public API preserved. Gates: size/align tests, mathbench-equivalent for our transform pipeline, `inverse()` returns explicit `Option<Matrix4>` to keep flui semantics. |
| **U15-readme** | Update `flui-types/README.md:280` FAQ on glam/euclid | ☐ todo | Replace "Why not use glam or euclid?" answer with the current strategy: glam under the hood (Option D), euclid as long-term typing reference (Option C, future), Flutter-compat as extension traits, mint as cross-library bridge. |
| **U16-engine** | Audit `flui-engine` direct `glam::Vec2` imports; align with bridge policy | ☐ todo | After Option D, engine should either import via `flui_geometry::Vec2<...>` (typed) or via `flui_geometry::raw::Vec2` (explicit raw glam re-export) — not random direct glam imports. Mark boundary clearly. |
| **U17-euclid-spike** | 200-LOC spike: build a `Length` + `Point2D` migration prototype for `flui-rendering::Padding` widget | ☐ **todo (BEFORE PR 2, not Core.1)** | **Cheap experiment, decides PR 2.** Measures realistic call-site cost of Option C on one widget. Multiply spike LOC × ~80 widgets to estimate full Option C cost; compare against measured Option D LOC (500–850). Advisor correction — originally placed after PR 2 commit, now before. |

### Risks specific to Options D and C

**R7.5 — EdgeInsets call-site count corrected (advisor finding).** Original estimate of 50 was based on `rg -c 'EdgeInsets'` (line count per file). Real literal-construction count via `rg 'EdgeInsets\s*\{|EdgeInsets::(all|symmetric|only|fromLTRB|new|zero|...)\('` = **24 production sites** (15 in `sliver_padding.rs`, 6 in `padding.rs`, 3 elsewhere). U3 migration is *smaller* than the original research suggested, not larger. Risk multiplier downgraded.

**R8 — `glam::Mat4::inverse()` doesn't return `Option<Mat4>`** (it returns `Mat4`, and is undefined for non-invertible matrices). Our `Matrix4::inverse(&self) -> Option<Matrix4>` is the correct contract for a UI framework. Mitigation in Option D: wrap as shown in the Option D code block above. Cost is one branch per inverse call.

**R9 — glam's `repr` alignment is 16 bytes for SIMD types** (Mat4, Mat3A, Vec3A, Vec4, Quat). Our wrappers need `#[repr(transparent)]` to preserve. Verify with a `static_assertions::assert_eq_size!` test on each.

**R10 — euclid's `From<T> for Length<T, U>` is provided by default.** This conflicts with our U1 (no implicit `f32 → Pixels`). If we go Option C, we need to wrap euclid's `Length` in our own newtype to re-impose the unit barrier — at which point the wrapper is half the size of our current `Pixels`. This is the strongest argument to **defer Option C and do D first**: D doesn't reopen this fight.

**R11 — Two SIMD systems coexisting.** Current `flui-geometry` has its own `simd = []` feature flag with hand-rolled SSE/NEON. Option D removes the need for our SIMD entirely (glam does it). The `simd` feature must be **deleted** in U14, not preserved.

---

## Part VII — Sources

**Primary:**

- **kurbo design rationale** — Linebender (DeepWiki AI synthesis on the kurbo repository, queried 2026-05-24). Confirms deliberate restriction of `Point + Point`, `f64 * Point`, and the explicit `to_point()`/`to_vec2()` conversion model.
- **kurbo `f64` rationale** — Linebender (DeepWiki, same date). Accuracy-over-memory; explicit `(f32, f32) → Point` via `From` for the bridge case.
- **`crates/flui-geometry/src/units.rs`** (this repo, 2,461 LOC), `src/edges.rs`, `src/lib.rs`, `src/traits.rs` — ground-truth code reading.

**Cross-framework evidence:**

- [Zed#32339](https://github.com/zed-industries/zed/pull/32339) — GPUI Pixels unification, opened 2025-06-08, closed-draft 2025-08-18.
- [Slint#1697](https://github.com/slint-ui/slint/pull/1697), [#1729](https://github.com/slint-ui/slint/pull/1729), [#1731](https://github.com/slint-ui/slint/pull/1731), [#1620](https://github.com/slint-ui/slint/pull/1620) — the 2022 Slint logical-length migration arc.
- [Jetpack Compose `Dp` reference](https://developer.android.com/reference/kotlin/androidx/compose/ui/unit/Dp).
- [Kotlin KEEP-0104 inline classes](https://github.com/Kotlin/KEEP/blob/main/proposals/KEEP-0104-inline-classes.md) — `@JvmInline value class` design (the JVM-side equivalent of `#[repr(transparent)]`).
- [taffy docs](https://docs.rs/taffy/latest/taffy/), [Bevy coordinate system reference](https://bevy-cheatbook.github.io/fundamentals/coords.html).

**Math-stack ecosystem evidence (added Part VIII):**

- [bitshifter/glam-rs](https://github.com/bitshifter/glam-rs) — glam v0.32, 76M downloads, 1,005 reverse dependencies, SIMD-by-default for f32 graphics math.
- [bitshifter/mathbench-rs](https://github.com/bitshifter/mathbench-rs) — mathbench comparison: glam outperforms cgmath 2-3× and nalgebra 5-100× on common operations.
- [glam introduction blog post](https://bitshifter.github.io/2019/07/10/introducing-glam-and-mathbench/) — Cameron Hart's design rationale and benchmark methodology.
- [servo/euclid#35](https://github.com/mozilla-servo/rust-geom/pull/35) — Matt Brubeck (euclid author) on phantom-typed `Length<T, Unit>` design rationale, May 2014, the foundational decision that became Slint's typing model.
- [Mozilla Research: Static checking of units in Servo](https://research.mozilla.org/2014/06/23/static-checking-of-units-in-servo/) — Brubeck's 2014 announcement of euclid's typed length system.
- [euclid Length docs](https://docs.servo.org/euclid/struct.Length.html), [Point2D source](https://doc.servo.org/src/euclid/point.rs.html), [Size2D source](https://doc.servo.org/src/euclid/size.rs.html) — current production API.
- [bevy_math docs](https://docs.rs/bevy_math/latest/bevy_math/) — reference pattern for wrapping glam in a higher-level UI framework (`Affine3` reduced, `Curve` trait, etc.).
- [kurbo `feature = "mint"`](https://crates.io/crates/kurbo) — confirms mint as the agreed bridge layer.

**Flutter prior-art bugs that newtype catches:**

- [flutter#5873](https://github.com/flutter/flutter/issues/5873), [#41328](https://github.com/flutter/flutter/issues/41328), [#116278](https://github.com/flutter/flutter/issues/116278) — coordinate-system mixing bugs that `double` cannot catch.

---

[← Roadmap Tracker](../ROADMAP-TRACKER.md) · [← Roadmap](../ROADMAP.md) · [Foundations](../FOUNDATIONS.md) · [Port Methodology](../PORT.md)
