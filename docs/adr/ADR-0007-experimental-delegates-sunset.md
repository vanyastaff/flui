# ADR-0007: Keep `experimental-delegates` feature gated; define a hard sunset trigger

*Keep the six delegate-trait modules under the `experimental-delegates` feature flag — real implementations and 22 tests exist, but 0 driver render-objects have landed in 18 months — and define an explicit sunset: delete the feature and all six modules if no companion render-object (`RenderCustomPaint`, `RenderFlow`, `RenderSliverGrid`, or `RenderCustomMultiChildLayoutBox`) lands by Core.1 / Phase 4 of the `flui-rendering` audit plan.*

---

- **Status:** Accepted
- **Date:** 2026-06-24
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-rendering` — the `experimental-delegates` feature, six delegate-trait modules (`delegates/custom_painter.rs`, `delegates/flow.rs`, `delegates/multi_child_layout.rs`, `delegates/single_child_layout.rs`, `delegates/sliver_grid.rs`, `delegates/custom_clipper.rs`), and any render-object that would consume them (`RenderCustomPaint`, `RenderFlow`, `RenderSliverGrid`, `RenderCustomMultiChildLayoutBox`)
- **Relates to:** `crates/flui-rendering/docs/AUDIT-AND-PLAN-2026-06.md` (Phase 4 = Core.1 target); ADR-0003 (virtualization core — the sliver-grid render-object is one of the delegate consumers)

---

## Verdict

**KEEP** `experimental-delegates` gated under the off-by-default `experimental-delegates` feature. Do **not** delete now; do **not** un-gate now. Record a hard sunset trigger: if none of the four companion render-objects (`RenderCustomPaint`, `RenderFlow`, `RenderSliverGrid`, `RenderCustomMultiChildLayoutBox`) lands by Core.1 / Phase 4, delete the entire feature and all six delegate modules in one commit at that milestone.

---

## Context

### What the `experimental-delegates` feature contains

The `experimental-delegates` feature (added in Cycle 4 R-16) gates six delegate-trait modules, totalling approximately 1 800 LOC:

| Module | Delegate trait | Flutter equivalent |
|---|---|---|
| `delegates/custom_painter.rs` | `CustomPainter` | Flutter `CustomPainter` |
| `delegates/flow.rs` | `FlowDelegate` | Flutter `FlowDelegate` |
| `delegates/multi_child_layout.rs` | `MultiChildLayoutDelegate` | Flutter `MultiChildLayoutDelegate` |
| `delegates/single_child_layout.rs` | `SingleChildLayoutDelegate` | Flutter `SingleChildLayoutDelegate` |
| `delegates/sliver_grid.rs` | `SliverGridDelegate` | Flutter `SliverGridDelegate` |
| `delegates/custom_clipper.rs` | `CustomClipper` | Flutter `CustomClipper` |

Each module has a **real trait definition** with full documentation and a **working implementation** — these are not stubs. The feature currently includes 22 tests that pass when compiled with `--features experimental-delegates`.

### The problem: 0 driver render-objects in 18 months

Every delegate trait exists to be consumed by a specific render-object:

| Delegate | Requiring render-object | Status |
|---|---|---|
| `CustomPainter` | `RenderCustomPaint` | Not yet implemented |
| `FlowDelegate` | `RenderFlow` | Not yet implemented |
| `SliverGridDelegate` | `RenderSliverGrid` | Not yet implemented |
| `MultiChildLayoutDelegate` | `RenderCustomMultiChildLayoutBox` | Not yet implemented |
| `SingleChildLayoutDelegate` | `RenderCustomMultiChildLayoutBox` | Not yet implemented |
| `CustomClipper` | `RenderClipRect` / `RenderClipPath` (clip variants) | Not yet implemented as delegate-consuming path |

None of the four companion render-objects has landed. The delegate traits therefore have zero production call sites. The feature gate prevents them from appearing in the default-build surface, which is the right policy for dead code — but 18 months without a consumer is a signal that the delegate modules may never be activated, and keeping dead code indefinitely has costs (maintenance burden, doc-freshness drift, new-agent confusion about what is "real").

### Current phase

The `flui-rendering` audit plan (`docs/AUDIT-AND-PLAN-2026-06.md`) defines Phase 4 as Core.1: the render-object catalog fill-out pass where `RenderCustomPaint`, `RenderFlow`, `RenderSliverGrid`, and `RenderCustomMultiChildLayoutBox` are scheduled to land. This is the natural deadline for the "driver render-objects must exist" test.

---

## Decision

**Keep `experimental-delegates` gated (do not delete, do not un-gate) and establish the following sunset rule:**

> **Sunset trigger:** If none of `RenderCustomPaint`, `RenderFlow`, `RenderSliverGrid`, or `RenderCustomMultiChildLayoutBox` has landed in `crates/flui-rendering/src/objects/` by the time Core.1 / Phase 4 of the audit plan is declared complete, delete the `experimental-delegates` feature, all six delegate modules, and their 22 tests in a single commit at that milestone. Record the deletion in CHANGELOG and reference this ADR.

The trigger is intentionally **any** companion render-object, not **all** four — one landing render-object proves the pattern is active and justifies keeping the remaining delegate modules.

**Rationale for keeping rather than deleting now:**

- The 22 tests cover real behavior that would take non-trivial effort to reconstruct. Deleting working code one phase before the render-object fill-out pass that would activate it is premature.
- The feature gate already prevents the dead surface from appearing in default builds and in workspace compile times. The cost of keeping gated code is low.
- Phase 4 is the natural checkpoint. Deleting at the milestone (if not activated) is cheaper than deleting now and re-implementing later.

**Rationale for not un-gating now:**

- Un-gating with zero driver render-objects would add ~1 800 LOC of dead surface to every default build. This is the scenario the feature gate was designed to prevent (Cycle 4 R-16).
- Un-gated dead traits confuse new agents and reviewers: a trait with no production impl appears to be part of the API surface but is load-bearing for nothing.

---

## Consequences

- **`experimental-delegates` remains off-by-default.** No change to the default build surface or compile time.
- **22 existing tests continue to run** when compiled with `--features experimental-delegates`. They must not be silently dropped before the sunset milestone without recording why.
- **Phase 4 (Core.1) becomes a hard checkpoint.** At that milestone, the audit pass either (a) lands one or more of the four companion render-objects, in which case the delegate modules are kept and the feature may be un-gated, or (b) lands none, in which case the feature and all six modules are deleted per this ADR.
- **New render-object work before Phase 4 is unblocked.** The four companion render-objects can be implemented at any time; landing any one of them before Phase 4 satisfies the "not dead" test and cancels the sunset.
- **The sunset deletion, if triggered, is a single clean commit.** Six module files + the `Cargo.toml` feature entry + all `#[cfg(feature = "experimental-delegates")]` gates + 22 tests. No behavior change for downstream (no production callers exist). Reference this ADR in the commit message.

---

## Rejected alternatives

| Option | Why rejected |
|---|---|
| **(a) Delete now** | Loses 22 passing tests and ~1 800 LOC of real implementations one phase before the render-object fill-out pass. Premature — the cost of deletion exceeds the cost of keeping a gated feature for one more phase. If Phase 4 arrives and no companion render-object lands, delete then. |
| **(b) Un-gate now (make default)** | Adds dead surface to every default build. ~1 800 LOC of traits with zero production impls appear as public API. Confuses agents and reviewers. Undoes the Cycle 4 R-16 gate without any new evidence the delegates are being consumed. |
| **(c) Keep indefinitely with no sunset rule** | The current implicit policy. The problem is that 18 months of zero activity already indicates drift; keeping without a deadline invites perpetual deferral. A hard trigger converts an implicit "maybe later" into an accountable decision point. |
| **(d) Extract to a separate `flui-delegates` crate** | One consumer (the render-objects, not yet written) does not justify a crate boundary. Would be premature decomposition for the same reason rejected in ADR-0003 (Decision 1). Revisit if the delegates prove useful outside `flui-rendering`. |

---

## Amendment (2026-07-01): `CustomPainter` un-gated — sunset cancelled for that module

`RenderCustomPaint` landed in `crates/flui-objects/src/proxy/custom_paint.rs`, with the `CustomPaint` widget in `crates/flui-widgets/src/paint/custom_paint.rs`. Per this ADR's own rule ("one landing render-object proves the pattern is active"), this satisfies the sunset-cancelling condition for `delegates/custom_painter.rs` specifically — the same event that already cancelled it for `delegates/sliver_grid_delegate.rs` when `RenderSliverGrid` landed (Cycle 4 R-16, prior to this ADR being written).

**Consequence:** `delegates/custom_painter.rs` is un-gated (moved out of `#[cfg(feature = "experimental-delegates")]` in `delegates/mod.rs`) and ships unconditionally, alongside `sliver_grid_delegate.rs`. `CustomPainter::hit_test` was also corrected from `-> bool` to `-> Option<bool>` (Flutter's `bool?` tri-state — `None` means "use the caller's default") while making this change, since `RenderCustomPaint`'s hit-test order depends on the tri-state default.

**Still gated, unchanged by this amendment:** `delegates/flow_delegate.rs`, `delegates/multi_child_layout_delegate.rs`, `delegates/single_child_layout_delegate.rs`, and `delegates/custom_clipper.rs` — none of their companion render objects (`RenderFlow`, `RenderCustomMultiChildLayoutBox`) has landed. The sunset trigger in the Decision section above still applies to these four modules.
