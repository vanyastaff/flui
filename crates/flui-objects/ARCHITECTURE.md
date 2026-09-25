# flui-objects Architecture

Per-crate ledger for the concrete `RenderBox` / `RenderSliver` catalog.
Mapping decisions that span more than one module in this crate land
here so later parity work does not treat a deliberate divergence as accidental
drift. Module-level `//! # Mapping decisions` comments may repeat a local note
and should cite this file when the contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far; Flutter source mapping for the full catalog grows as objects
are touched.

---

## Flutter source mapping

| Flutter source | FLUI module | Notes |
|---|---|---|
| `rendering/sliver_fixed_extent_list.dart` `RenderSliverFixedExtentBoxAdaptor` | [`src/sliver/sliver_fixed_extent_list.rs`](src/sliver/sliver_fixed_extent_list.rs) | Index math + request-strategy retain band; see mapping decision below. |
| `rendering/sliver_grid.dart` `RenderSliverGrid` | [`src/sliver/sliver_grid.rs`](src/sliver/sliver_grid.rs) | Delegate-windowed geometry + the same finite-domain scroll-window policy. |

---

## Mapping decisions

### Non-finite scroll-window edges never reach `f32 as usize`

**Rule:** Flutter's fixed-extent adaptor converts scroll/cache floats with
`toInt`, which throws on `Infinity` / `NaN` (flutter/flutter#105630). Rust's
`f32 as usize` saturates instead (`+∞ → usize::MAX`, `NaN → 0`), so the same
malformed constraints can silently mint a retain band or build window at
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
That is louder than silent saturation and quieter than Flutter's release
crash; diagnostics name the sliver type and the offending fields so production
reports identify the bad window.

**Replacement coverage:** unit tests on
`sliver_fixed_extent_list::window` / index helpers and on
`sliver_grid::classify_cache_window` / poison geometry prove the contract;
a healthy viewport cannot inject non-finite scroll edges into the harness.

---

### `RenderParagraph` publishes no semantics node for empty text

**Rule:** Flutter's `RenderParagraph.describeSemanticsConfiguration`
(`rendering/paragraph.dart`, oracle tag `3.44.0`) sets `config.attributedLabel`
and `config.textDirection` unconditionally in its plain-text branch (no inline
recognizers/placeholders — the only branch this object's V1 scope supports;
see `src/text/paragraph.rs`'s module doc "Out of scope"), including for an
empty string.

**Choice:** `RenderParagraph::describe_semantics_configuration` sets neither
`label` nor `text_direction` when `TextPainter`'s plain text is empty, leaving
the configuration un-annotated.

**Why the divergence:** `SemanticsConfiguration::set_text_direction` (like
every other setter in `flui-semantics`) calls `mark_annotated()` on its own,
with no emptiness check — so mirroring the oracle exactly would mark *every*
text-less paragraph in a tree as contributing semantics (`has_been_annotated
== true`), which forms or merges an empty, unlabelled node wherever a
`RenderParagraph` sits directly under a semantics boundary or an
explicit-child-node ancestor. Flutter's own `SemanticsConfiguration` has the
same unconditional setters; whether the oracle ever surfaces an empty node
this way was not traced — its `Text` widgets normally carry text, and this
document makes no claim about the rest of its call graph. FLUI's headless
test harness does construct the empty case (see
`crates/flui-widgets/tests/semantics.rs`), so the divergence is
made explicit here instead of leaking into the merge pipeline as an
undocumented empty-label node.

**Alternatives considered:**

- Set `label`/`text_direction` unconditionally, matching the oracle
  byte-for-byte — rejected: publishes a spurious node (or spurious merge
  input) for any empty-text paragraph, which is strictly worse for an
  assistive-technology consumer than publishing nothing.
- Fix `set_text_direction` to skip `mark_annotated()` for a "no-op" value —
  rejected: `text_direction` has no such value (`Ltr`/`Rtl` are both
  meaningful), and the fix would have to special-case this one call site
  rather than the setter, which is what this decision does instead, kept
  local to `RenderParagraph`.

**Trade-off:** an empty-text `RenderParagraph` that Flutter would still tag
with a (redundant) text direction publishes nothing at all here. No known
consumer depends on an empty-labelled paragraph node's `text_direction`
alone; the alternative (a phantom node with no label) is the actively worse
default.

**Replacement coverage:** `crates/flui-widgets/tests/semantics.rs` mounts a
`Text` with empty content and asserts no node forms; a `Text("hello")` mount
asserts the label does.

---

## Thread safety

No locks. Catalog objects are mutated on the UI realm's layout/paint thread
through `PipelineOwner`; they hold no shared mutable state of their own.

---

## Friction log

| Item | Notes |
|------|-------|
| Full Flutter source mapping table | Deferred; fill as individual objects are re-touched. |
| Shared `classify_cache_window` helper across list + grid | Grid owns its classify today; list uses `finite_leading_cache_edge`. Consolidating into one module is optional follow-up once a third consumer appears. |

### Subtree anchor rebinding preserves mounted identity

`RenderSubtreeAnchor::set_anchor` transfers its mounted `RenderId` to a new
`SubtreeAnchor`, clearing the old slot. Before attach or after detach, rebinding
publishes nothing. Reusing the same slot is a no-op. This identity-only operation
does not invalidate geometry or replace the child. `AnchoredBox` calls it during
render-object updates, so reconciliation preserves child state when its anchor changes.
This extends FLUI's identity proxy contract; there is no direct Flutter render-object
counterpart. The `harness_subtree_anchor_rebinds_while_mounted_and_after_detach` test
covers publication lifetime, and the widgets `anchored_box` regression rebuilds the
real element tree and checks preserved render IDs and child state.
