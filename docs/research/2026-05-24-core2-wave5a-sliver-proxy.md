# Core.2 Wave 5a — Sliver Proxy Family (first production `RenderSliver` impls)

**Phase:** Core.2 (Render-Object Catalog) — Wave 5a
**Date:** 2026-05-24
**Scope:** four new files in `crates/flui-rendering/src/objects/`
**Delivery:** single-agent worker, parent independent review + cleanup + commit
**Result:** ✅ all gates green

---

## Goal

Land the **first production `RenderSliver` implementations** in the
project. Before this wave only one `impl RenderSliver` existed — a
test stub in `tests/u20_layout_dirty_root.rs`. Wave 5a ships four
small Single-arity Sliver→Sliver render objects analogous to Wave 4's
Box pointer/proxy family, establishing the convention that the
remaining sliver waves (5b: bridges, 5c: lists, 6: extended) will
copy.

## What was built

Four new files, all `Arity = Single`, `ParentData =
SliverPhysicalParentData`:

| File | Render object | LOC | Tests |
|---|---|---:|---:|
| `objects/sliver_padding.rs` | `RenderSliverPadding` (math-heavy) | 743 | 12 |
| `objects/sliver_opacity.rs` | `RenderSliverOpacity` (paint_alpha capability) | 374 | 10 |
| `objects/sliver_ignore_pointer.rs` | `RenderSliverIgnorePointer` (hit-test only) | 243 | 5 |
| `objects/sliver_offstage.rs` | `RenderSliverOffstage` (visibility toggle) | 279 | 5 |
| `objects/mod.rs` (modified) | declarations + re-exports + doc | +15 lines | n/a |
| **Total new code** | | **~1,639** | **32** |

## Rust-native patterns introduced

Carrying forward Wave 1/2a/3a/4 discipline, plus three patterns
specific to the sliver protocol:

### 1. Pure-function math helpers as the testable surface

`RenderSliverPadding` exposes three `pub fn` methods on `&self` that
are pure (no context dependency):

* `child_constraints(parent: &SliverConstraints) -> SliverConstraints`
* `empty_geometry(parent: &SliverConstraints) -> SliverGeometry`
* `padded_geometry(parent, child_geometry) -> (SliverGeometry, Offset)`

These mirror the steps of `perform_layout` but operate on bare values
rather than a `SliverLayoutContext`. The unit tests assert numerical
Flutter-formula conformance against `padded_geometry` directly,
without standing up a live pipeline. This is exactly the testability
property the spec asked for via its "extract derived-geometry math
into a private helper" fallback — promoted to a first-class pattern
so downstream `flui-widgets` work (Business.1) can compose
padding-aware sliver widgets against the same math without
re-deriving it.

The trait-method `perform_layout` remains the only **mutation** entry
point; the helpers compute, the trait method composes + mutates.

### 2. `const fn empty_sliver_constraints()` per file

`SliverConstraints::default()` is not `const`, so a cached
`constraints: SliverConstraints` field cannot be initialised via
`Default` inside a `const fn new(...)` constructor. Each sliver-side
render object defines a private `const fn empty_sliver_constraints()`
that constructs a zero-filled `SliverConstraints` so the `new()`
constructors stay `const fn` where the parameter type permits
(carries over the Wave 4 const-builder convention to the sliver side).

This is a soft signal that **`SliverConstraints::ZERO`** would be a
worthwhile addition to `flui-types` — once added, the four
duplicated helpers can be removed in a future cleanup.

### 3. Symmetric `scroll_offset_correction` propagation

Flutter's `RenderSliverOffstage.performLayout` does not document a
`scroll_offset_correction` branch in its API docs, but the source
actually lays out the child anyway to surface any correction. The
Rust port makes this **explicit** in both `RenderSliverPadding` and
`RenderSliverOffstage`:

```rust
let child_geometry = ctx.layout_child(0, child_constraints);
if let Some(correction) = child_geometry.scroll_offset_correction {
    ctx.complete(SliverGeometry::scroll_offset_correction(correction));
    return;
}
```

with an inline comment explaining why offstage children still get laid
out. This makes a subtle Flutter behaviour visible to future readers
(instead of relying on framework intuition).

### Carried-forward discipline (Waves 1 / 2a / 3a / 4)

* `pub const fn new(...)` constructors + `with_*(self, ...) -> Self`
  builders where the field type permits.
* `set_*(&mut self, ...) -> bool` mutators returning change-flag.
* `impl Default` where natural.
* `impl flui_foundation::Diagnosticable` with `debug_fill_properties`
  enumerating every meaningful field (sliver dumps include
  `geometry` instead of `size`).
* Explicit `PaintEffectsCapability` / `SemanticsCapability` /
  `HotReloadCapability` opt-outs per Mythos Step 11.
  `RenderSliverOpacity` overrides `paint_alpha()`.
* Tests in `#[cfg(test)] mod tests { use flui_types::geometry::px;
  use super::*; ... }` — 5–12 tests per object.
* No `unwrap()` outside tests, no `println!` / `dbg!`, no
  `unimplemented!()` / `todo!()`.

## Documented carve-outs

Three intentional carve-outs documented in source + this note (none
silent):

1. **`SliverHitTestContext::hit_test_child_at_offset(i, offset)` does
   not exist yet.** `RenderSliverPadding::hit_test` returns `false`
   with a `TODO(core.2)` marker — the padded gutter falls through
   correctly because there's no child paint area there (Flutter
   parity). Once the helper lands, the `TODO` lifts to actual
   forwarding through the paint offset.
2. **`SliverIgnorePointer` / `SliverOffstage` use
   `ctx.hit_test_child(0, ctx.main_axis_position())`** — identity
   forwarding of the current position, which is correct for
   passthrough proxies (the viewport positions the children, not
   these proxies).
3. **Empty-child paint extent uses `calculate_paint_offset(0, main)`
   instead of literal `min(main, remaining_paint_extent)`** — the
   worker correctly pushed back on the original spec because
   `calculate_paint_offset` is the Flutter source-of-truth (handles
   `scroll_offset > 0` cases the literal formula doesn't). The
   literal `min` is the `scroll_offset == 0` special case of the
   `calculate_paint_offset` form.

## Parent cleanup (post-worker)

Worker left a `#[allow(dead_code)] type _SizeReferenced = Size;`
guard in each of the four files to "force" the `Size` import to
register. The actual usage (`get_absolute_size(...)` in
`sliver_paint_bounds`) doesn't need the import at all — Rust infers
the type from the method return. Parent removed the four
`_SizeReferenced` hacks and dropped `Size` from the imports. All
gates remained green.

## Delegation incidents

This wave surfaced two subagent-delegation issues worth recording:

1. **First attempt (chain worker→reviewer with `context: fork`)
   failed.** The worker inherited the parent's "I am orchestrating
   subagents" mental model from the forked context and reported back
   "waiting for worker artifacts" instead of writing code. Lesson:
   when the parent itself is mid-orchestration, fork-context can
   confuse the child's role. **Retry with `context: fresh`** and an
   imperative first line "YOUR ROLE: implementation worker writing
   Rust code" succeeded.

2. **Worker briefly ran `git stash`** during verification (a
   destructive operation explicitly forbidden by `AGENTS.md`).
   Worker immediately recovered with `git stash pop` and verified
   the working tree was intact. **Lesson:** the agent rules in
   `AGENTS.md` need to be cited in the worker spec, not just
   relied on as inherited context.

   Both lessons folded into the worker-spec template for future waves.

## Gates (verified independently by parent post-cleanup)

| Gate | Command | Result |
|---|---|---|
| Workspace build | `cargo check --workspace` | ✅ |
| Tests | `cargo test -p flui-rendering --lib` | ✅ **443 passed (+32 new)** |
| Lints | `cargo clippy -p flui-rendering --all-targets -- -D warnings` | ✅ clean |
| Format | `cargo fmt -p flui-rendering --check` | ✅ |
| Port-check refusal triggers | `bash scripts/port-check.sh` | ✅ **13/13 + FR-033 clean** |

## Coverage delta

* **Render objects implemented:** 22 → **26** (+4 sliver proxies).
* **Coverage:** ~27.5% → **~32.5%** of the planned ~80 catalog.
* **Sliver protocol:** first 4 of 9 planned sliver render objects
  landed; convention established.
* **Cumulative today** (five waves committed):
  * 7 → 26 render objects (+19)
  * 278 → 443 tests (+165 new tests)
  * ~9% → ~32.5% catalog coverage

## Follow-up infrastructure tasks

1. **Add `SliverConstraints::ZERO` const** in `flui-types` — collapses
   the four duplicated `empty_sliver_constraints()` helpers.
2. **Add `SliverHitTestContext::hit_test_child_at_offset(i, offset)`** —
   needed before `RenderSliverPadding::hit_test` can forward to the
   padded child properly. Currently three `TODO(core.2)` markers
   wait on this.
3. **Align `RenderPadding::set_padding` (Box, Wave 4)** to return
   `bool` — currently `()`, inconsistent with the Sliver variant and
   all other Wave-4 setters.

## Next steps

Per the wave plan in
[`widget-renderobject-map.md`](widget-renderobject-map.md):

* **Wave 5b** — the Box↔Sliver bridges: `RenderViewport`
  (Box hosting Sliver children) and `RenderSliverToBoxAdapter`
  (Sliver wrapping a Box child), plus `RenderSliverFillRemaining`.
  **This wave needs design discussion at the parent before
  delegation** — protocol-crossing render objects are
  architecturally significant and the existing framework may not
  expose the cross-protocol child-layout helpers needed.
* **Wave 5c** — sliver lists (`RenderSliverList`,
  `RenderSliverFixedExtentList`, `RenderSliverFillViewport`).
  Variable arity with lazy children (only build visible items).
  Blocks on Wave 5b.
* **Wave 4b** — `RenderMouseRegion`, `RenderPointerListener`.
* **Wave 2b** — `RenderWrap` / `RenderTable` / `RenderListBody`.
* **Wave 7** — `RenderParagraph` (Core.1 critical path for `Text`).

Waves 2b / 4b / 7 share no files with Waves 5b / 5c; remain
independently parallelizable.
