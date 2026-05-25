# Core.2 Wave 4 — Pointer / Visibility / Transform Proxy Family

**Phase:** Core.2 (Render-Object Catalog) — Wave 4
**Date:** 2026-05-24
**Scope:** six new files in `crates/flui-rendering/src/objects/`
**Delivery:** worker → reviewer chain (forked from parent context)
**Result:** ✅ all gates green; reviewer verdict `ready_to_commit`

---

## Goal

Land the pointer / visibility / transform proxy family for the box
catalog. Six small, Single-arity render objects that unblock every
Material / Cupertino composite that needs to **hide**, **intercept**,
**ignore**, **scale**, or **attach payload to** a subtree.

This wave was the first delegated through subagents — worker (forked
context) implemented, reviewer (fresh context) audited the diff.
Parent applied one cosmetic nit fix, re-ran all gates locally,
committed. Pattern documented for future waves.

## What was built

Six new files, all Single arity, BoxParentData:

| File | Render object | LOC | Tests |
|---|---|---:|---:|
| `objects/offstage.rs` | `RenderOffstage` | 234 | 6 |
| `objects/absorb_pointer.rs` | `RenderAbsorbPointer` | 201 | 5 |
| `objects/ignore_pointer.rs` | `RenderIgnorePointer` | 196 | 5 |
| `objects/meta_data.rs` | `RenderMetaData` (+ `MetaDataPayload`) | 349 | 11 |
| `objects/fractional_translation.rs` | `RenderFractionalTranslation` (+ `TranslationFraction`) | 336 | 9 |
| `objects/fitted_box.rs` | `RenderFittedBox` | 478 | 13 |
| `objects/mod.rs` (modified) | declarations + re-exports + doc | +24 lines | n/a |
| **Total new code** | | **~1,794** | **49** |

## Rust-native improvements introduced

Carrying the Wave 1 / 2a / 3a discipline forward, and adding two new
patterns specific to Wave 4:

### 1. `MetaDataPayload = Arc<dyn Any + Send + Sync + 'static>`

Flutter's `RenderMetaData.metadata` is `Object?` — anything goes,
runtime-typed, no Clone guarantee. The Rust port lifts the same
intent to `Arc<dyn Any + Send + Sync + 'static>`:

* **`Send + Sync` enforced at the API boundary.** Flutter relies on
  the framework being single-threaded; FLUI's `RenderObject: Send +
  Sync` requirement makes the bound a compile-side requirement.
* **`Clone` preserved on `RenderMetaData`** because `Arc::clone` is
  pointer-bump only.
* Same discipline as Wave 3a's `CustomClipper<S>`: type-erase the
  payload behind `Arc` so the containing render object stays
  ergonomic.

### 2. `TranslationFraction` newtype

Flutter's `RenderFractionalTranslation.translation` is `Offset` — but
`Offset` carries *pixel* values everywhere else in the framework, and
`RenderFractionalTranslation` overloads it to carry *fractions of
child size*. The convention is implicit and only enforced by the
class name / docs.

The Rust port lifts the intent to a dedicated newtype:

```rust
pub struct TranslationFraction { pub dx: f32, pub dy: f32 }

impl TranslationFraction {
    pub const fn new(dx: f32, dy: f32) -> Self { ... }
    /// Resolve against a child size to produce a pixel-typed `Offset`.
    pub fn resolve(self, size: Size) -> Offset { ... }
}
```

Trying to pass `TranslationFraction` where `Offset` is expected (or
vice-versa) is a compile error. The unit-mismatch class of bug is
unrepresentable at the API boundary.

The rename from the originally-spec'd `FractionalOffset` happened
because `flui_types::layout::FractionalOffset` already exists with
*incompatible semantics* (`[0, 1]` anchor-point space with
`TOP_LEFT` / `BOTTOM_RIGHT` constants). `port-check.sh` trigger 10
(SP-3 parallel cross-crate type definitions) caught the conflict
during worker execution; the worker renamed to `TranslationFraction`
and documented the rename — exactly the kind of distinction the
refusal trigger exists to surface.

### 3. `HitTestBehavior` consumed as a state table, not boolean pair

`RenderMetaData::hit_test` reads:

```rust
match self.behavior {
    HitTestBehavior::Opaque        => { /* self-hit + block-below */ }
    HitTestBehavior::Translucent   => { /* self-hit + propagate */ }
    HitTestBehavior::DeferToChild  => { /* child only */ }
}
```

via existing `HitTestBehavior::registers_self()` /
`blocks_below()` helpers. Flutter handles the same logic with a
pair of booleans (`opaque`, `translucent`) and conditional branches
on each.

### 4. `BoxFit::apply()` delegation

`RenderFittedBox` does not re-implement the seven-case scaling
switch. The math lives once in `flui_types::layout::BoxFit::apply`
and is consumed by `RenderFittedBox` as:

```rust
let fitted = self.fit.apply(child_size, self.size);
let sx = fitted.destination.width / fitted.source.width;
let sy = fitted.destination.height / fitted.source.height;
```

Flutter inlines the seven-case switch inside `RenderFittedBox`
itself; if a new fit mode lands or a fit-mode bug is found,
Flutter needs to be patched in two places (the enum definition
and every consumer). FLUI patches one (`BoxFit`).

### 5. `paint_transform()` capability over `paint()` override

`RenderFittedBox` exposes its composed
`Matrix4::translation × scale` via the
`PaintEffectsCapability::paint_transform()` capability return, **not**
via a direct `RenderBox::paint()` override. The pipeline already
consumes `paint_alpha()` / `paint_transform()` returns to wrap
children in `OpacityLayer` / `TransformLayer` automatically; the
typed `RenderBox::paint` body bridge from the protocol layer is
currently a no-op stub. The capability-return path is both
architecturally correct (no double-translation when the pipeline
eventually wires `RenderBox::paint`) and works today.

This is the same forward-compatible pattern Wave 3a applied to
`RenderClip<S>::paint`: write the body, but rely on the capability
return for the actually-invoked path until `RenderBox::paint` is
wired.

### Carried-forward discipline (Waves 1 / 2a / 3a)

* `pub const fn new(...)` constructors + `with_*(self, ...) -> Self`
  builders, all `const fn` where the field type permits.
* `set_*(&mut self, ...) -> bool` mutators returning change-flag
  for pipeline `mark_needs_layout` short-circuit.
* `impl Default` where natural.
* `impl flui_foundation::Diagnosticable` with `debug_fill_properties`
  enumerating every meaningful field.
* Explicit `PaintEffectsCapability` / `SemanticsCapability` /
  `HotReloadCapability` opt-outs (default-impl) per Mythos Step 11.
* Tests in `#[cfg(test)] mod tests { use flui_types::geometry::px;
  use super::*; ... }` — 5-13 tests per object.
* No `unwrap()` outside tests, no `println!` / `dbg!`, no
  `unimplemented!()` / `todo!()`.

## Documented deviations

The worker flagged five deviations from the original task spec; the
reviewer independently validated each as justified. They are
documented in the source (module doc-comments + inline notes) as
intentional carve-outs, not silent gaps:

| # | Deviation | Why it's correct |
|---|---|---|
| 1 | `FractionalOffset` → `TranslationFraction` rename | `flui_types::layout::FractionalOffset` already exists with `[0,1]` anchor semantics; sharing the name would collide. `port-check.sh` trigger 10 catches this exact class of bug. |
| 2 | `RenderFittedBox` uses `paint_transform()` capability, not `paint()` override | Pipeline currently consumes `paint_transform()` and stubs `RenderBox::paint` body bridge. Future-proof: when bridge wires, no double-translation. |
| 3 | `RenderFittedBox` active clipping deferred to Wave 3b | Default `Clip::None` is honoured exactly; real layer-clip wiring lands with `RenderRepaintBoundary` in Wave 3b. |
| 4 | `RenderFittedBox::hit_test` is scale-approximate | Alignment shift applied, scale divide not. Accurate for `BoxFit::Fill` / unit-scale `Contain`. Per-axis scale-aware path needs `BoxHitTestContext::position_divide` helper that doesn't exist yet. |
| 5 | `RenderMetaData::set_metadata` reports change on `Some→Some` always | `dyn Any` cannot be structurally compared; conservatively reports change. Matches Flutter (which unconditionally `markNeedsPaint`s on any setter call). |

## Subagent delegation pattern

This wave was the first delegated through subagents instead of
implemented inline. The pattern:

1. **Parent** scopes the wave + writes the worker task spec
   (including reference files to read, conventions to match, gotchas
   from earlier waves, deliverable list, gates).
2. **Worker** (builtin, forked context) inherits parent's pattern
   knowledge from the Wave 1 / 2a / 3a session history; writes all
   files, runs gates iteratively until clean; reports back via
   structured artifact in `chain_dir`.
3. **Reviewer** (builtin, fresh context) re-runs gates from a clean
   session, reads the diff, audits per convention checklist + Flutter
   parity, reports verdict + findings.
4. **Parent** reads both artifacts, applies any blocker fixes (none
   here), applies optional nit fixes (one rename), re-runs gates
   locally, updates map + writes phase notes + commits.

**What worked:**
* Forking parent context to the worker pre-loaded all established
  patterns — the worker matched Wave 1 / 2a / 3a discipline exactly.
* Fresh-context reviewer caught no false positives because it
  re-derived correctness from the codebase, not from memory.
* Explicit pointer to `port-check.sh` triggers in the worker spec
  meant the worker hit refusal triggers during development and
  corrected (rename #1) instead of merging into trunk.

**What to refine for future waves:**
* The single nit (N1: misleadingly-named test) could have been
  prevented by a more explicit "test-name = behavior under test"
  rubric in the worker spec.
* For waves that touch shared infrastructure (e.g., adding a method
  to `BoxHitTestContext`), the worker spec should call out the
  cross-file surface explicitly.

## Gates (verified independently by parent post-N1-fix)

| Gate | Command | Result |
|---|---|---|
| Workspace build | `cargo check --workspace` | ✅ |
| Tests | `cargo test -p flui-rendering --lib` | ✅ **411 passed (+49 new)** |
| Lints | `cargo clippy -p flui-rendering --all-targets -- -D warnings` | ✅ clean |
| Format | `cargo fmt -p flui-rendering --check` | ✅ |
| Port-check refusal triggers | `bash scripts/port-check.sh` | ✅ **13/13 + FR-033 clean** |

## Coverage delta

* **Render objects implemented:** 16 → **22** (+6).
* **Coverage:** ~20% → **~27.5%** of the planned ~80 catalog.
* **Cumulative today** (four waves committed):
  * 7 → 22 render objects (+15)
  * 278 → 411 tests (+133 new tests)
  * ~9% → ~27.5% catalog coverage

## Caveats / follow-up notes

* `RenderFittedBox::hit_test` is alignment-aware but not scale-aware
  — track this for a future enhancement when the pipeline wires a
  proper `BoxHitTestContext::position_divide` helper.
* `RenderFittedBox` active clip lifts in Wave 3b alongside
  `RenderRepaintBoundary` (both need layer-tree integration).
* `RenderAbsorbPointer` / `RenderMetaData` carry `TODO(core.1)`
  comments pointing to the gesture system's `target_id` integration
  — comments, not `todo!()` macros, so port-check stays clean.

## Next steps

Per the wave plan in
[`widget-renderobject-map.md`](widget-renderobject-map.md):

* **Wave 4b** — `RenderMouseRegion` / `RenderPointerListener`
  (needs mouse-tracker + pointer-event routing infra).
* **Wave 2b** — `RenderWrap` / `RenderTable` / `RenderListBody`.
* **Wave 3b** — `RenderDecoratedBox` + `RenderRepaintBoundary`
  (needs painting/layer infra).
* **Wave 7** — `RenderParagraph` (Core.1 critical path).
* **Wave 5** — sliver baseline (`RenderViewport` + adapter +
  `RenderSliverList`).

Waves 2b, 3b, 5, 7 share no files with Wave 4; remain independently
parallelizable.
