# Core.2 Wave 1 — Constraint-Modifying Render-Object Family

**Phase:** Core.2 (Render-Object Catalog) — first wave
**Date:** 2026-05-24
**Scope:** 4 new render objects, all `Single` arity, `BoxParentData`
**Result:** ✅ all gates green

---

## Goal

Begin Core.2 (per [`docs/ROADMAP.md`](../ROADMAP.md#core2--render-object-catalog--was-phase-2))
with a coherent, parallelizable wave that:

1. Does not conflict with the in-progress contracts work in
   `specs/004-view-element-core` (different files, different crate).
2. Unblocks part of the Core.1 vertical slice (`Container.constraints`,
   `AspectRatio` widget, `FractionallySizedBox` widget).
3. Showcases Rust-native architectural improvements over Flutter's class
   hierarchy without breaking behavior loyalty
   ([`STRATEGY.md`](../../STRATEGY.md)).

## What was built

| Render object | File | LOC (incl. tests) | Tests |
|---|---|---:|---:|
| `RenderConstrainedBox` | `crates/flui-rendering/src/objects/constrained_box.rs` | 310 | 8 |
| `RenderLimitedBox` | `crates/flui-rendering/src/objects/limited_box.rs` | 317 | 12 |
| `RenderAspectRatio` | `crates/flui-rendering/src/objects/aspect_ratio.rs` | 437 | 14 |
| `RenderFractionallySizedBox` | `crates/flui-rendering/src/objects/fractionally_sized_box.rs` | 470 | 14 |
| **Total** | — | **~1,534** | **48** |

Plus:

* Updated `crates/flui-rendering/src/objects/mod.rs` to register and
  re-export all four new types.
* Created [`docs/research/widget-renderobject-map.md`](widget-renderobject-map.md) —
  the canonical mapping that gates Core.2 entry (Core.0 deliverable that
  was missing; seeded with this wave and a forward plan of 9 waves).

## Rust-native improvements over Flutter

Rather than a literal 1:1 port, each render object incorporates Rust-side
improvements that preserve behavior (per the parity oracle in
[`docs/ROADMAP.md`](../ROADMAP.md#what-parity-means-and-how-it-is-measured))
while eliminating classes of bugs unrepresentable in Rust:

### 1. Newtype-validated domain values

* **`AspectRatio(f32)`** in `aspect_ratio.rs` — cannot represent a
  non-positive or non-finite ratio. Flutter's `double aspectRatio`
  field accepts `NaN` and silently produces `NaN`-sized layouts;
  here that mistake is unrepresentable.
* **`FractionFactor(f32)`** in `fractionally_sized_box.rs` —
  rejects negative and non-finite at the API boundary. Flutter
  silently clamps at runtime.
* Both expose `new()` (Option-returning), `new_unchecked()`
  (debug-asserted const), and `from_size()` / arithmetic helpers
  for ergonomic chaining.

### 2. `Option<T>` instead of magic sentinels

* **`RenderLimitedBox`** uses `Option<Pixels>` for `max_width` /
  `max_height` (Flutter uses `double.infinity` as the "no cap"
  sentinel — works but conflates "infinite" with "unset").
* **`RenderFractionallySizedBox`** uses `Option<FractionFactor>`
  (Flutter uses `null` for "inherit parent constraint" — same
  semantics, but Rust's `Option` is type-checked at every use site).

### 3. Typed `Pixels` boundary

* All four render objects pass `Pixels` through every arithmetic
  step; raw `f32` only crosses the boundary at intrinsic-dimension
  trait methods (which the `RenderBox` trait signature forces) and
  ratio math inside `AspectRatio`/`FractionFactor`.

### 4. Compile-time arity

* All four use `Arity = Single` — the type system enforces
  "exactly zero or one child"; no runtime check needed. Flutter
  enforces this through inheritance from `RenderObjectWithChildMixin`
  + runtime assertions.

### 5. `const` constructors and builders

* `RenderLimitedBox::width(_)`, `RenderLimitedBox::both(_,_)`,
  `RenderFractionallySizedBox::new()` + `.with_*()` builders are
  all `const fn` — they compose into compile-time defaults.

### 6. `set_*` returns change flag

* Every mutator returns `bool` indicating whether the value
  actually changed. The pipeline can use this to skip
  `mark_needs_layout()` calls when nothing changed (Flutter
  performs the equality check inside the setter and unconditionally
  marks dirty when different — same logic, surfaced as an explicit
  return value for callers that want to batch).

### 7. `tracing` for soft assertions

* `RenderAspectRatio` emits `tracing::warn!` (not `panic!`) when
  both incoming constraint dimensions are unbounded — falls back to
  `Size::ZERO` rather than crashing. Flutter uses `assert(() { ... })`
  blocks that throw `FlutterError` only in debug builds. The
  `tracing` approach is observable in production and never
  destabilizes a release build.

### 8. Constraint composition through existing primitives

* No abstract base class for "constraint transformer" was introduced.
  Each render object uses the rich [`BoxConstraints`] API
  (`.enforce()`, `.tighten()`, `.constrain()`, `.has_bounded_*()`)
  that already exists. The shared algorithm in
  `_RenderCustomClip<T>` (Flutter) is *not* mirrored — its
  Rust-native shape is "a small `fn` per render object", which
  reads more clearly and avoids the diamond-inheritance ambiguity
  Flutter's `RenderProxyBox` ↔ `_RenderCustomClip` mixin chain
  carries.

## Behavior loyalty

The `_applyAspectRatio` algorithm in `RenderAspectRatio` is a
step-for-step port of Flutter's
`packages/flutter/lib/src/rendering/proxy_box.dart::RenderAspectRatio.
_applyAspectRatio` — same ordering (width-first → height clamp → min
push-ups), same tight-constraint short-circuit, same fallback when both
axes are unbounded. The 6 dedicated parity tests in
`aspect_ratio.rs` cover each branch.

`RenderLimitedBox::limit_constraints` mirrors Flutter's
`_limitConstraints` exactly — each axis is independently checked, and a
cap is applied *only* when that axis is unbounded.

`RenderConstrainedBox` uses `BoxConstraints::enforce()` which is
behaviorally equivalent to Flutter's `additionalConstraints.enforce()`.

`RenderFractionallySizedBox` matches Flutter's
`RenderFractionallySizedOverflowBox` restricted to the non-overflow
case (the `FractionallySizedBox` widget contract).

## Gates

All four standing-discipline gates ([`FOUNDATIONS.md` Part VI](../FOUNDATIONS.md#part-vi--the-standing-quality-discipline))
exit green:

| Gate | Command | Result |
|---|---|---|
| Build | `cargo check -p flui-rendering` | ✅ |
| Workspace build | `cargo check --workspace` | ✅ |
| Tests | `cargo test -p flui-rendering --lib` | ✅ **326 passed (+48 new)** |
| Lints | `cargo clippy -p flui-rendering --all-targets -- -D warnings` | ✅ clean |
| Format | `cargo fmt -p flui-rendering --check` | ✅ |
| Port-check refusal triggers | `bash scripts/port-check.sh` | ✅ **13/13 + FR-033 clean** |

No new `unimplemented!()` / `todo!()` introduced. No `println!`/`dbg!`
introduced. No `unwrap()` in non-test code. All new public API is
documented.

## Coverage delta

* **Render objects implemented:** 7 → 11 (+4, +57%)
* **`flui-rendering` parity:** the constraint family of the box
  catalog is now closed for L6 widget needs.
* **Widget catalog unblocked by this wave:** `ConstrainedBox`,
  `LimitedBox`, `AspectRatio`, `FractionallySizedBox`, and
  the `constraints:` leg of `Container`.

## Next wave

Per the wave plan in
[`widget-renderobject-map.md`](widget-renderobject-map.md), the next
parallel waves are:

* **Wave 2** — multi-child layout (`RenderStack`, `RenderWrap`,
  `RenderTable`, `RenderListBody`).
* **Wave 3** — clip + decoration (`RenderClipRect`/`RRect`/`Oval`/`Path`,
  `RenderDecoratedBox`, `RenderRepaintBoundary`) — strong candidate
  for a `RenderClip<S: ClipShape>` generic that collapses Flutter's
  4-class `_RenderCustomClip<T>` hierarchy into one type.
* **Wave 7** — `RenderParagraph` (the most urgent for Core.1's text
  needs).

These three waves do not share files and can ship as independent PRs.
