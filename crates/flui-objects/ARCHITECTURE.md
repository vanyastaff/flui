# flui-objects Architecture

Per-crate ledger for the concrete `RenderBox` / `RenderSliver` catalog.
Decisions that span more than one module in this crate land
here. Module-level `//! # Mapping decisions` comments may repeat a local note
and should cite this file when the contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far.

---

## Module map

| Object | FLUI module | Notes |
|---|---|---|
| `RenderSliverFixedExtentList` | [`src/sliver/sliver_fixed_extent_list.rs`](src/sliver/sliver_fixed_extent_list.rs) | Index math + request-strategy retain band; see mapping decision below. |
| `RenderSliverGrid` | [`src/sliver/sliver_grid.rs`](src/sliver/sliver_grid.rs) | Delegate-windowed geometry + the same finite-domain scroll-window policy. |

---

## Mapping decisions

### Non-finite scroll-window edges never reach `f32 as usize`

**Rule:** Rust's `f32 as usize` saturates (`+∞ → usize::MAX`, `NaN → 0`), so
malformed scroll/cache constraints can silently mint a retain band or build window at
`usize::MAX` and freeze the viewport without a crisp error.

**Choice:** `RenderSliverFixedExtentList` and `RenderSliverGrid` share one
finite-domain policy for the cache-extended window:

| Edge | Value | Behavior |
|------|-------|----------|
| Leading (`scroll_offset + cache_origin`) | `NaN` or `+∞` | Reject: empty retain band `(0, 0)`, report the declared scroll extent, `tracing::error!` naming the render object and constraint fields. |
| Leading | `−∞` or negative finite | Clamp to `0` via `.max(0.0)` — same as a negative finite offset; layout proceeds from the origin. |
| Trailing (`leading + remaining_cache_extent`) | Finite | Normal index conversion, clamped to `item_count − 1`. |
| Trailing | `+∞` | Intentional unbounded window (shrink-wrap); existing `MAX_UNBOUNDED_WINDOW_CHILDREN` / `UNBOUNDED_SENTINEL_WINDOW` truncation. |
| Trailing | `NaN` or `−∞` | Reject: same empty-band fallback as a poisonous leading edge — not an unbounded window. |

Public index helpers on the fixed-extent list apply the same poison test to a
single offset (`NaN`/`+∞` → `0` + error; `−∞` → `0` without error).

**Alternatives considered:**

- Rely on `SliverConstraints::is_normalized()` / debug asserts alone — too late
  and not a production guard on the hot layout path.
- Panic / return a layout `Result` — the sliver protocol today commits geometry
  infallibly; a local empty window is the recoverable fallback.
- Clamp `+∞` leading to `0` like `−∞` — would hide a genuine overflow and lay
  out the wrong end of a large list.

**Trade-off:** malformed constraints that previously continued with a
saturated index now produce an empty window and a structured error log.
That is louder than silent saturation and quieter than a crash; diagnostics name the sliver type and the offending fields so production
reports identify the bad window.

**Tests:** unit tests on
`sliver_fixed_extent_list::window` / index helpers and on
`sliver_grid::classify_cache_window` / poison geometry prove the contract;
a healthy viewport cannot inject non-finite scroll edges into the harness.

---

### `RenderParagraph` publishes no semantics node for empty text

**Rule:** a text-less paragraph contributes no semantics (plain-text branch only,
the only one this object's V1 scope supports; see `src/text/paragraph.rs`'s
module doc "Out of scope").

**Choice:** `RenderParagraph::describe_semantics_configuration` sets neither
`label` nor `text_direction` when `TextPainter`'s plain text is empty, leaving
the configuration un-annotated.

**Why:** `SemanticsConfiguration::set_text_direction` (like
every other setter in `flui-semantics`) calls `mark_annotated()` on its own,
with no emptiness check — so setting them unconditionally would mark *every*
text-less paragraph in a tree as contributing semantics (`has_been_annotated
== true`), which forms or merges an empty, unlabelled node wherever a
`RenderParagraph` sits directly under a semantics boundary or an
explicit-child-node ancestor. The headless test harness constructs the empty
case (see `crates/flui-widgets/tests/semantics.rs`).

**Alternatives considered:**

- Set `label`/`text_direction` unconditionally — rejected: publishes a spurious node (or spurious merge
  input) for any empty-text paragraph, which is strictly worse for an
  assistive-technology consumer than publishing nothing.
- Fix `set_text_direction` to skip `mark_annotated()` for a "no-op" value —
  rejected: `text_direction` has no such value (`Ltr`/`Rtl` are both
  meaningful), and the fix would have to special-case this one call site
  rather than the setter, which is what this decision does instead, kept
  local to `RenderParagraph`.

**Trade-off:** an empty-text `RenderParagraph` publishes nothing at all, not even a (redundant) text direction. No known
consumer depends on an empty-labelled paragraph node's `text_direction`
alone; the alternative (a phantom node with no label) is the actively worse
default.

**Tests:** `crates/flui-widgets/tests/semantics.rs` mounts a
`Text` with empty content and asserts no node forms; a `Text("hello")` mount
asserts the label does.

---

### Subtree anchor rebinding preserves mounted identity

`RenderSubtreeAnchor::set_anchor` transfers its mounted `RenderId` to a new
`SubtreeAnchor`, clearing the old slot only if it still contains this node's ID.
Detach uses the same ownership check, so a sibling swapping slots or taking over
a slot cannot have its publication erased by the previous publisher. Before attach or after detach, rebinding
publishes nothing. Reusing the same slot is a no-op. This identity-only operation
does not invalidate geometry or replace the child. `AnchoredBox` calls it during
render-object updates, so reconciliation preserves child state when its anchor changes.
This extends FLUI's identity proxy contract. The `harness_subtree_anchor_attach_*` and `harness_subtree_anchor_detach_*` rows of
`family_layer_links` cover publication lifetime, and the widgets `anchored_box` regression rebuilds the
real element tree and checks preserved render IDs and child state.

---

### Text objects measure through the context their layout lends

`RenderParagraph` and `RenderEditable` pass `&mut ctx.text()` to every
`TextPainter` measurement: `perform_layout`, the four intrinsics,
`compute_dry_layout` and `compute_dry_baseline`. The context is the realm's
`TextContext`, lent by the pipeline (flui-rendering's "Layout contexts lend the
realm's text context"), so a paragraph measures with its own realm's fonts
rather than an ambient collection (ADR-0092 §10 step 3; flui-painting mapping decision 14), and
paints the runs of the layout that measured (step 4). Every
`harness_*` test for both objects runs through the lent context. flui-runtime's
`two_realms_measure_text_through_their_own_contexts` shows the loan reaches a
realm's context through a mounted `Text`.

---

## Thread safety

No locks. Catalog objects are mutated on the UI realm's layout/paint thread
through `PipelineOwner`; they hold no shared mutable state of their own.

---

## Friction log

| Item | Notes |
|------|-------|
| Shared `classify_cache_window` helper across list + grid | Grid owns its classify today; list uses `finite_leading_cache_edge`. Consolidating into one module is optional follow-up once a third consumer appears. |
