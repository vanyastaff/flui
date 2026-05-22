---
date: 2026-05-22
topic: flui-painting-view-cycle5
scope: deep
audit_source: docs/research/2026-05-22-flui-painting-view-audit.md
---

# flui-painting × flui-view — Cycle 5 audit execution

## Summary

Cycle 5 of the audit-execute series closes the 45 findings in the flui-painting × flui-view audit. The brainstorm ratifies a single classification policy for the audit's ~4,585 LOC of zero-consumer surface — genuine zombies and parallel-type drift are deleted, forward-looking Flutter port-targets stay compiled with a port-completion ledger, and the two half-implemented modules whose consumers already exist are wired up. It also reframes finding V-1: the audit's proposed fix would have introduced an `InheritedView` scoping bug, so cycle 5 deletes the dead registry instead of wiring it.

---

## Problem Frame

The audit ([docs/research/2026-05-22-flui-painting-view-audit.md](../research/2026-05-22-flui-painting-view-audit.md)) catalogs 45 findings across the canvas-recorder + view-tree pair, including ~4,585 LOC verified to have zero external consumers. It is thorough and well-evidenced, but it carries a structural bias: it lumps two materially different things under one "zero-consumer" label, and its severity tags inherit that bias.

One group is **genuine rot** — `tessellation` duplicates the engine's canonical tessellator, `hit_region` is a parallel `PointerEvent` surface that routes nowhere, the fallback `TextLayout` is a parallel impl, `NotificationNode` is parallel dispatch the unified protocol replaced, `SharedWidgetsBinding` is deprecated. These are real removals.

The other group is **forward-looking Flutter port infrastructure** — `AnimatedView`, `ParentDataView`, `ErrorView`, `RootRenderView`, `TextPainter`. These are faithful 1:1 ports of real Flutter constructs. They have zero consumers only because the widgets that would consume them (`Flexible`, `RichText`, animated widgets, the app bootstrap) live in `flui-widgets` — a crate that does **not exist in the workspace at all** — and `flui-animation` is disabled. The audit's Part IV puts "feature-gate this group default-off" in the **P0 critical-correctness wave**, treating correct port infrastructure as critical-to-remove.

This collides head-on with `STRATEGY.md`'s stated bet — "Порт, не редизайн" — and with the recorded principle that zero-consumer port infrastructure is a *migration gap*, not a deletion signal: the migration moves consumers toward the abstraction, not the reverse. Feature-gating these modules default-off exiles correct, tested code from `just ci`, guaranteeing it bitrots before `flui-widgets` is ever created. The cost of *not* having a ratified policy is visible in the audit itself: it re-flags an 18-month-old finding from the 2026-05-20 audit because nothing ever recorded the decision — every audit re-litigates the same surface.

Separately, the audit's headline view finding — V-1, "wire `BuildOwner::inherited_elements` for O(1) lookup" — rests on a flat `HashMap<TypeId, ElementId>`. A `.flutter/` cross-check shows this is not a faithful port of Flutter's per-element `_inheritedElements` persistent map. A flat process-global map holds one element per type; wiring it returns the wrong ancestor whenever same-type `InheritedView`s nest (a sub-`Theme` inside a `Theme`). The audit's proposed fix would trade an O(depth) correct lookup for an O(1) incorrect one.

---

## Requirements

**Line-number policy:** this doc cites paths and symbol names. Line numbers in the audit are illustrative — the implementer greps the symbol at edit time. LOC counts are approximate.

The audit document is the authoritative per-finding catalog. This doc is the **policy layer** on top of it: it ratifies, reclassifies, or reframes findings and sets the execution contract. Where a requirement says "per the audit," the audit's fix shape stands.

**Classification policy**

- R1. Every zero-consumer item in the audit is classified into exactly one of three buckets — *genuine zombie* (delete), *forward-looking port-target* (keep + ledger), or *half-impl with an existing consumer* (wire up). The classification table below is authoritative; no zero-consumer finding is executed against the audit's verdict where this doc overrides it.
- R2. No forward-looking port-target is feature-gated default-off. Feature-gating default-off removes the code from the default build, so it loses `cargo build` / `clippy` / `test` coverage and bitrots before its consumer crate exists.
- R3. Each forward-looking port-target keeps a `// PORT-TARGET:` ledger comment at its module head naming the consumer it waits on and the crate that must exist first (e.g. `flui-widgets::Flexible`). The ledger makes "zero-consumer" legible as intentional, so future audits stop re-flagging it.

**Genuine zombie removal**

- R4. Delete, per the audit's fix shapes: `tessellation` module + `lyon` dependency + `tessellation` feature (P-1); `display_list::hit_region` + `Canvas::add_hit_region` + `DisplayList::hit_regions` + `DisplayListStats::hit_regions` (P-2, P-19); the `text_layout::fallback` parallel `TextLayout` arm, making the `text` feature non-optional (P-3); `NotificationNode` / `NotificationHandler` / `BoxedNotification` / `NotificationCallback` (V-9); the `#[deprecated]` `SharedWidgetsBinding` + `create_shared_binding` (V-10); the `Picture` type alias (P-9).
- R5. `canvas::sugar` (the ~720 LOC of `draw_pill` / `draw_ring` / `debug_grid` / fluent combinators, P-4 / P-14 / P-15) is **deleted, not feature-gated**. It is invented ergonomics with no Flutter analogue — not port infrastructure — so the keep-and-ledger policy does not apply to it; YAGNI does.

**Forward-looking port-targets — keep + ledger**

- R6. Keep compiled in the default build and add the R3 ledger comment to: `AnimatedView` + `AnimationBehavior` + `AnimatedElement` (V-3); `ParentDataView` + `ParentDataConfig` + `ParentDataElement` (V-4); `TextPainter` and the cosmic-text `TextLayout` it wraps (P-5). Their consumers are widgets that would live in `flui-widgets`, which does not yet exist — wiring is impossible, not deferred by preference.
- R7. Rename `flui-view`'s `AnimationBehavior` struct to eliminate the name collision with `flui-animation`'s `AnimationBehavior` enum. This is a real defect independent of consumer status; do it regardless of R6.

**Wire-up — half-impls whose consumer exists today**

- R8. Wire `WidgetsBinding::attach_root_widget` to bootstrap the element tree through `RootRenderView` / `RootRenderElement`, and remove the parallel direct-mount path (V-6). This mirrors Flutter's `attachRootWidget` → `RenderObjectToWidgetAdapter.attachToRenderTree` bootstrap (`.flutter/flutter-master/packages/flutter/lib/src/widgets/binding.dart`). The change is intra-`flui-view`; the `flui-app` call site is unchanged.
- R9. Wire `ErrorView`: `Element::perform_build` catches a panicking `build()` and substitutes the registered error view instead of unwinding the frame (V-5). This mirrors Flutter's `ComponentElement.performRebuild`, which wraps both `build()` and the child update in `try/catch` → `ErrorWidget.builder` (`framework.dart:5810-5859`).

**V-1 reframe — delete, do not wire**

- R10. Do **not** wire `BuildOwner::inherited_elements` as V-1 proposes. The flat `HashMap<TypeId, ElementId>` is not a faithful port of Flutter's per-element `_inheritedElements` persistent map and returns the wrong ancestor when same-type `InheritedView`s nest. Cycle 5 deletes the flat registry: the `inherited_elements` field, `register_inherited` / `unregister_inherited` / `inherited_element`, the `Debug` field entry, and the test surface. The existing `walk_ancestors_for_inherited` O(depth) lookup in `ElementBuildContext::depend_on_inherited` is correct and stays.
- R11. The faithful O(1) optimization — a per-element persistent inherited map, structurally shared from parent — is recorded as a future-cycle finding. It is not built speculatively in cycle 5; the O(depth) walk is correct and adequate until profiling says otherwise.

**Correctness & Flutter-parity (independent of the zombie policy)**

- R12. Hoist `reconcile_children` (the keyed O(N) 5-phase reconciliation already implemented in `crates/flui-view/src/tree/reconciliation.rs`) into `VariableChildStorage::update_with_views`, replacing the index-based loop (V-2). This unblocks `Hero` / `Reorderable` / `GlobalKey` reparenting. Folds in: `View::can_update` consulting keys (V-25), and resolving `ReconcileAction` — either restructure the algorithm to return it or delete it (V-11).
- R13. `ElementTree` implements `TreeRead` / `TreeNav` / `TreeWrite` from `flui-tree` (V-7), making the element tree DAG-uniform with `RenderTree` and `LayerTree` — the flui-tree unified-interface intent applied to the view layer.
  - **Planning update (2026-05-22):** R13 / V-7 was reshaped to a **Cycle 6 deferral** during planning. `ElementTree` is vestigial — production inserts only the root — so faithful execution requires a foundational element-ownership unification, not the audit's mechanical "+200 LOC" trait impl. See [docs/plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md](../plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md) (Key Technical Decisions, Scope Boundaries).
- R14. `attach_root_widget` returns a `Result` instead of `assert!`-panicking on double-attach (V-12), per Constitution Principle 6.

**Hot-path performance**

- R15. Cycle 5 includes the audit's hot-path findings: eliminate the per-draw `Paint` clone (P-7); make `append_display_list_at_offset` O(1) via a paint-time transform instead of an O(N) bake (P-11); constant-time `DrawCommand::kind()` (P-13); `draw_polyline` `windows(2)` idiom (P-6); replace the `ClipShape` data-carrying enum with a depth counter (P-8); eliminate the per-build dummy-context allocation (V-13); make `collect_all_elements` O(N) (V-16). Each performance finding is benchmarked before and after.

**Hygiene & API discipline**

- R16. Cycle 5 includes the audit's P2/P3 hygiene findings as specified in Part IV rows 23–45 — `#[non_exhaustive]` additions, `pub(crate)` visibility trims, `REMOVE_BY` / doc-cadence markers, snapshot-then-fire on `WidgetsBinding::handle_*`, `#[allow(dead_code)]` accessor cleanups, doc-lie fixes — **except** V-20 and V-23 (see R20).

**Cross-crate**

- R17. Delete the parallel `Color` / `ColorScheme` in `crates/flui-app/src/theme/colors.rs` and migrate to `flui_types::Color` (V-14). This is the one cross-crate touch; it is folded into cycle 5 rather than deferred because it is zero-consumer, small, and a parallel-type drift the audit already verified.

**Execution shape & scope**

- R18. Findings land as atomic commits — one self-contained finding per commit — in conventional-commit format with the `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` trailer, matching the PR #81–#117 precedent. Wave grouping and ordering are `/ce-plan`'s decision.
- R19. Each wave passes a verification gate before it is considered done: `cargo build --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p <touched-crate> --lib`; `bash scripts/port-check.sh -v`. All green, or blocking issues reported explicitly.
- R20. Scope is all 45 audit findings. V-20 (`ElementBase` sub-trait split) and V-23 (`WidgetsBindingInner` per-field locks) are deferred to cycle 6 — the audit itself marks both "future wave."

---

## Cycle 5 verdict table

The brainstorm's substance is the verdict column. ✔ marks findings where the brainstorm **ratifies** the audit; a bold verdict with no ✔ marks where the brainstorm **changed** it.

| Finding | Module / symbol | Audit verdict | Cycle 5 verdict | Why |
|---|---|---|---|---|
| P-1 | `tessellation` + `lyon` dep | delete | delete ✔ | engine owns the canonical tessellator |
| P-2 / V-8 / P-19 | `display_list::hit_region`, parallel `PointerEvent` | delete | delete ✔ | parallel-type drift; `flui-interaction` owns `PointerEvent` |
| P-3 | `text_layout::fallback` | delete fallback | delete fallback ✔ | parallel impl; `text` feature becomes non-optional |
| P-4 / P-14 / P-15 | `canvas::sugar` (30+ helpers) | feature-gate **or** delete | **delete** | invented ergonomics, no Flutter analogue — not port infra |
| P-5 | `TextPainter` + cosmic-text `TextLayout` | feature-gate default-off | **keep + ledger** | port-target; consumers (`RichText`/`TextField`) need `flui-widgets` |
| P-9 | `Picture` type alias | delete | delete ✔ | cosmetic duplicate name |
| V-1 | `BuildOwner::inherited_elements` flat registry | wire for O(1) | **delete registry, keep O(depth) walk** | flat map ≠ faithful port of Flutter `_inheritedElements`; wiring adds a nested-scope bug |
| V-3 | `AnimatedView` + `AnimationBehavior` | feature-gate default-off | **keep + ledger** (+ rename) | port-target; needs `flui-animation` + `flui-widgets` |
| V-4 | `ParentDataView` + `ParentDataConfig` | feature-gate default-off | **keep + ledger** | port-target; needs `flui-widgets` (`Flexible`/`Positioned`) |
| V-5 | `ErrorView` + `FlutterError` | wire **or** feature-gate | **wire up** | half-impl; producer (`perform_build`) exists intra-crate |
| V-6 | `RootRenderView` + `RootRenderElement` | wire **or** feature-gate | **wire up** | half-impl; bootstrap consumer exists intra-crate |
| V-9 | `NotificationNode` & friends | delete | delete ✔ | parallel dispatch; unified protocol is the live path |
| V-10 | `SharedWidgetsBinding` | delete | delete ✔ | deprecated since 0.2.0 |

V-2, V-7, V-12 and the P1/P2/P3 performance + hygiene findings are correctness/quality work, not zero-consumer classification — they execute per the audit and are not in this table.

---

## Acceptance Examples

- AE1. **Covers R10.** Given an element tree with a `Theme` `InheritedView` and, deeper, a nested `Theme` of the same view type, when a descendant below the inner `Theme` calls `depend_on::<Theme>()`, it resolves to the **inner** `Theme`. The `walk_ancestors_for_inherited` path returns this correctly; a flat `TypeId → ElementId` registry would return whichever `Theme` registered last — which is why the registry is deleted, not wired.
- AE2. **Covers R9.** Given a registered error-view builder, when a view's `build()` panics, `perform_build` catches the panic and the element renders the error view in that slot — the frame does not unwind.
- AE3. **Covers R8.** Given a fresh `WidgetsBinding`, when `attach_root_widget` is called, the root element is created and mounted through `RootRenderView`; grepping the crate afterward finds no second direct-mount code path.
- AE4. **Covers R12.** Given a `Variable`-arity element whose children carry keys, when the child list is reordered between builds, children are matched by key and their element state is preserved — not rebuilt by index position.
- AE5. **Covers R3, R6.** Given `ParentDataView` has zero consumers, when a future audit greps for zero-consumer surface, the `// PORT-TARGET: flui-widgets::Flexible` comment identifies it as intentional port infrastructure rather than a zombie.

---

## Success Criteria

- A re-audit of flui-painting / flui-view does not re-flag the kept port-targets as zombies — the `// PORT-TARGET:` ledger ends the re-flag churn that this audit itself demonstrates.
- `depend_on` stays correct for nested same-type `InheritedView`s; no scoping regression is introduced in the name of an O(1) lookup.
- Keyed reconciliation is live in the production child-update path, so `Hero` / `Reorderable` / `GlobalKey` reparenting become implementable.
- `/ce-plan` can sequence waves without re-deciding delete-vs-keep-vs-wire for any finding — the verdict table is complete and every one of the 45 findings has a disposition.
- The workspace builds, clippy-clean, and tests green after every atomic commit; `port-check.sh` passes per wave.
- Net effect: roughly −2,500 LOC of genuine-zombie removal, with the forward-looking port surface retained, compiled, and tested rather than exiled.

---

## Scope Boundaries

- **V-20 and V-23 are deferred to cycle 6.** Both are architectural splits the audit marks "future wave"; they are not blocked by anything in cycle 5.
- **The faithful per-element persistent inherited map is out of scope.** Cycle 5 removes the broken flat registry and keeps the correct O(depth) walk. The real O(1) port is a future-cycle finding, pursued only when profiling justifies it.
- **Creating the `flui-widgets` crate is out of scope.** Cycle 5 keeps the port-targets ready for it; it does not create it or its widgets.
- **No new abstractions, no Flutter redesign.** Cycle 5 is port-fidelity work — delete drift, wire half-impls to their Flutter shape, ratify what is correct. Anything that "improves on" Flutter semantics is rejected per `STRATEGY.md`.
- **Wave sequencing and the V-2 store-by-id vs store-by-value implementation choice belong to `/ce-plan`**, not this doc.
- **Cycle 6 target selection** (platform × app, foundation × types, or an interaction × scheduler second pass) is out of scope.

---

## Key Decisions

- **Three-bucket classification (delete / keep + ledger / wire), not the audit's delete-or-gate.** The audit's binary collapses genuine rot and forward-looking port infrastructure into one treatment. Splitting them is what makes the policy honor `STRATEGY.md`'s port bet and the migration-gap principle while still removing real drift.
- **No feature-gate-default-off for port-targets.** Gating default-off removes code from `just ci`; correct, tested 1:1 ports would silently bitrot before `flui-widgets` is created. Keep-compiled + a ledger comment costs only a slightly larger default API and a small compile-time delta — both acceptable, since that API surface *is* the intended one.
- **`canvas::sugar` is deleted, not kept.** It is the one zero-consumer item that is neither drift nor a Flutter port-target — it is invented ergonomics. The keep-and-ledger policy is for port infrastructure; YAGNI governs speculative ergonomics.
- **V-1 reframed from "wire" to "delete."** A `.flutter/` cross-check (`framework.dart` `_inheritedElements`, a per-element `PersistentHashMap` populated by structural sharing from the parent) shows the flat `BuildOwner` registry is not a faithful port. Wiring it would buy O(1) at the cost of correctness for nested `InheritedView`s. Deleting it leaves the codebase simple and correct; the audit's own "worst of both worlds" framing supports this.
- **Wire `RootRenderView` and `ErrorView` now rather than gating them.** Their consumers/producers exist *inside* `flui-view` today — the root-bootstrap path and `perform_build`. Gating them would be the defer-with-excuse pattern the project explicitly rejects; the half-impl is closed by consolidating the parallel path, which is the real work.
- **Full 45-finding scope.** Cycles 1–4 each closed their complete finding set across multiple waves; cycle 5 follows that precedent. Only the two findings the audit itself marks "future wave" are deferred.

---

## Dependencies / Assumptions

- **`flui-widgets` does not exist in the workspace** — it is absent from `Cargo.toml` `[workspace.members]` and from `AGENTS.md`'s crate list (verified). **`flui-animation` is disabled** — commented out at `Cargo.toml:46` (verified). The keep-and-ledger verdict for V-3 / V-4 / P-5 rests on this: their consumers cannot be wired because the consumer crate has not been created.
- **The inherited-dependency *system* is complete and correct** — the ancestor walk, `InheritedBehavior::record_dependent`, and the `on_view_updated` notify path all work (verified in `element/behavior.rs` and `context/element_build_context.rs`). Only the O(1) lookup *cache* (the flat registry) is dead. Deleting the registry per R10 does not break `InheritedView` behavior.
- **`reconcile_children` already implements the correct 5-phase keyed algorithm** — V-2 / R12 is a hoist into the production path, not a new implementation (per the audit, which quotes both the live index loop and the dead algorithm).
- **Flutter parity anchors are verified against `.flutter/`:** root bootstrap = `attachRootWidget` → `RenderObjectToWidgetAdapter` (`binding.dart`); build-error handling = `ComponentElement.performRebuild` dual `try/catch` → `ErrorWidget.builder` (`framework.dart:5810-5859`); keyed reconciliation = `Element.updateChildren` (`framework.dart:4125`).
- **Wiring `ErrorView` (R9) carries a Rust-specific design question** that planning must resolve — `catch_unwind` requires an `UnwindSafe` boundary, the element being built is in an indeterminate state after a panic, and a `panic = "abort"` profile would defeat the catch entirely. This is a design task inside the finding, not a blocker.

---

## Outstanding Questions

### Resolve Before Planning

None. The user delegated all product decisions; the classification policy, scope, and every verdict are set in this doc.

### Deferred to Planning

- [Affects R9][Technical] How `Element::perform_build` establishes an `UnwindSafe` boundary around `build()`, what element-tree state is guaranteed consistent after a caught panic, and whether any active build profile sets `panic = "abort"`.
- [Affects R12][Technical] Store-by-id vs store-by-value for `VariableChildStorage` once keyed reconciliation is hoisted — the audit flags this as the architectural sub-decision of V-2.
- [Affects R13][Technical] Which `TreeWrite` cascade semantics `ElementTree` adopts relative to its current `remove` / `remove_finalized`, so the trait impl does not change observable removal behavior.
- [Affects R10][Needs research] Confirm nothing outside the audit's named tests reads a value from the flat `inherited_elements` registry before it is deleted.
