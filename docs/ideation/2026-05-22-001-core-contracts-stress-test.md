---
date: 2026-05-22
focus: stress-test design commitments of specs/004-view-element-core/spec.md (round-3)
frames: [pain-friction, inversion, assumption-breaking, leverage, cross-domain-analogy, constraint-flipping]
axes: [A.element-tree-structure, B.children-and-authoring, C.reconciler, D.type-erasure-dispatch, E.migration]
agents: 6 ideation sub-agents (1 per frame), ~48 raw ideas, ~10 survivors
status: ideation
---

# Core Contracts Stress-Test — Ideation Survivors

Ideation pass on the unified Core Contracts spec (`specs/004-view-element-core/spec.md`, round-3 committed at `45bea6fb`). Six parallel frame-agents (pain-friction / inversion / assumption-breaking / leverage / cross-domain-analogy / constraint-flipping) generated ~48 alternative designs against the spec's 7 committed decisions. This document records the survivors that earned cross-frame agreement or named a concrete downstream consequence.

## Grounding context

The spec made these commitments to stress-test:

1. `ElementKind` closed enum with **4 separate `Render*` variants** (RenderLeaf/Single/Optional/Variable), each holding `Box<dyn RenderElementBase<A>>`
2. `StatelessView::build` / `ViewState::build` return `impl IntoView`
3. New `flui-macros` proc-macro crate with `#[derive(StatelessView)]` / `#[derive(StatefulView)]`
4. `ViewSeq` trait + tuple impls `0..=16` + `Vec<BoxedView>` dynamic-path
5. `column!` / `row!` macros in `flui-view::macros`
6. `key: Option<Box<dyn ViewKey>>` field on `ElementNode`
7. Keyed O(N) Flutter reconciler + `global_key_registry` as O(1) cross-tree index
8. 3-phase PR sequence (storage shape → reconciler+rewiring → IntoView+macros+downcast-elimination)

## Survivor table (ranked by cross-frame strength)

| # | Survivor | Axis | Frames agreeing | Verdict |
|---|---|---|---|---|
| **S1** | **KeyId interning** instead of `Option<Box<dyn ViewKey>>` per ElementNode | A/D | pain (F1) + assumption (F3) + leverage (F4) + constraint (F6) — **4-way** | **worth re-opening FR-022** before Phase 1 |
| **S2** | **Static path skips keyed reconciler entirely** (tuple-static has no permutation invariant; only dynamic path needs keyed algo) | C | inversion (F2) + assumption (F3 reconciliation-as-behavior + Salsa) + cross-domain (F5 Merkle) + constraint (F6 Rust-native) — **4-way** | **worth re-opening FR-016 / consider as Implementation Sequence Phase 1.5** |
| **S3** | **ViewSeq tuple cap = real cliff** — `column!` macro should auto-detect 17+ → `Vec<BoxedView>` with friendly compile error or codegen-to-HList | B | pain (F1) + assumption (F3 const-generic HList) + cross-domain (F5 music notation validates as feature-not-bug) + constraint (F6 100-author serialization) — **4-way** | **worth FR enrichment** (compile-time `column!` shim + friendly error) |
| **S4** | **AnimationListener closure capture** = 3 dyn-call indirection at 60fps per `AnimatedContainer` / `Hero` / `Ripple` — benchmark vs alternatives (type-id-keyed dispatch table) | A/D | pain (F1) — single-frame but **concrete consequence + perf-critical surface** | **worth re-opening FR-020 fold mechanism** OR mandatory Risk Register entry |
| **S5** | **Drop `flui-macros` — blanket-impl with defaults** (`impl<T: StatelessView + 'static + Clone> View for T`) replaces `#[derive(StatelessView)]` boilerplate; saves entire crate + syn/quote compile cost | B | inversion (F2) + constraint (F6 budget=0) — **2-way** | **worth Deferred-Open-Questions investigation** before Phase 3 commits |
| **S6** | **Recursive widget pattern needs derive-hint + docs warning** — `TreeView`/`MenuItem`/`JsonViewer` hit "unboundedly-deep impl Trait" rustc error; `#[derive(StatelessView)]` could detect via `#[view(recursive)]` attribute and auto-`.boxed()` at recursion edge | B/D | pain (F1) — single-frame but **every recursive widget author hits it** | **worth FR enrichment + Edge Case expansion** |
| **S7** | **Reconciler emits structured `ReconcileEvent` trace** (mount/unmount/reuse/reorder/reparent/type-mismatch) — instrumentation reuse: SC-002/SC-003 assertions become declarative, future profiler subscribes to same stream, devtools selection persistence | C | leverage (F4) — single-frame but **compounds across hundreds of catalog widget tests** | **worth FR enrichment** (~30 LOC in reconciler) |
| **S8** | **Sanctioned dyn boundary registry** as `port-check.sh` trigger #9 — encode FR-029's 3 sanctioned points (element storage + dynamic-children + platform backend) as allow-listed paths; any new `dyn` outside requires `// PORT-CHECK-OK-DYN: <reason>` marker | D | leverage (F4) — single-frame but **self-enforces Constitution Principle 4 across hundreds of future PRs** | **worth FR enrichment** (~10 LOC trigger + allow-list) |
| **S9** | **Phase 1 dead-code window** — between Phase 1 merge and Phase 3 merge, `ElementKind` exists + `downcast_ref` retained behind `#[deprecated]`; new ElementKind variants in this window need dual-path support. Time-box Phase 2/3 OR document acceptable window | E | pain (F1) + assumption (F3 ship-Phase-3-first inversion) — **2-way** | **worth Implementation Sequence enrichment** (named window + cap) |
| **S10** | **ElementId stability is a choice, not Flutter contract** — content-addressed identity (hash of `(TypeId, key, parent_path)`) eliminates Slab+NonZeroUsize machinery for keyed path, survives hot-reload more gracefully | A | assumption (F3) — single-frame but **reframes Constitution Principle 9 (ID offset pattern)** | **worth Deferred-Open-Questions / inspiration for FLUI 0.2+** |

## Critique-pass — also-ran but worth recording

These ideas surfaced but did not earn the survivor bar (single-frame, lower confidence, or covered by existing spec text):

- **`ViewSeq` `LEN_HINT` const** (F4) — reconciler fast-path when count matches. Folds into S7 instrumentation surface.
- **`#[derive(StatelessView)]` emits `VIEW_TYPE_NAME` const** (F4) — devtools/diagnostics quality-of-life. Defer to derive-macro design phase.
- **Source-location key synthesis** (F2 + React/Leptos pattern) — has fundamental limits (conditional rendering at same line, cross-parent reparenting). Inspiration only.
- **Drop View trait — widgets are functions returning Element** (F2 + Dioxus/Leptos analog) — too large a reframe to re-litigate; STRATEGY.md explicitly chose Flutter-shape ("реинвент … откатывается"). Confirms current direction.
- **Drop column!/row! macros — `[T; N]` + tuple literals** (F2) — viable but worse ergonomics for the heterogeneous-static case; `column!` macro stays.
- **Per-frame reconciliation vs event-driven dirty-children** (F3) — orthogonal optimization layer above reconciliation; out of scope for this contract, fits FLUI 0.2+.
- **Single Render variant with inner arity tag** (F2 + F6 maintenance flip) — round-2 already explicitly rejected this choice with rationale; survivor #2 (static-path skip) is the stronger reframe.
- **Flutter parity test corpus as standalone harness** (F4) — already scoped via SC-010; the *harness* infrastructure deserves a separate spec but not this contract.
- **3-phase PR template promoted to FOUNDATIONS pattern** (F4) — process-level enrichment; worth a follow-up doc but not in this spec.
- **Schema migration + MVCC analog** (F5) — validates current 3-phase shape; no action.
- **Database B-tree vs LSM analog** (F5) — validates current closed-enum choice; no action.
- **Bevy ECS archetype analog** (F5) — validates per-arity Render* variants; supports current FR-020 commitment.
- **Mechanism design — incentive-compatible authoring** (F5) — reframes SC-001 + bon enforcement; reinforces Deferred-Open-Questions on bon (currently "recommended" → mechanism-design says must be lint-enforced or strictly cheapest).

## Cross-cutting synthesis

Three threads run through multiple survivors:

1. **Key storage shape is under-decided** (S1 + S10): the contract commits to `Option<Box<dyn ViewKey>>` but multiple frames (memory cost, no_std hostility, closed-enum-discipline asymmetry, hot-reload survival, content-addressed identity option) converge on the conclusion that this storage choice is the **single most fragile commitment** in the spec. The Deferred-Open-Questions entry on memory cost understates the problem — three independent frames flagged it.

2. **Static vs dynamic ViewSeq paths are asymmetric** (S2 + S3): the contract treats them as parallel paths sharing the same algorithm, but multiple frames (compile-time-tree-diffing, Salsa analog, Merkle hashing, Rust-native reconciler) show the static-tuple path has no permutation invariant and can use a cheaper algorithm. The current FR-016 "both paths share the same algorithm" is a Flutter-loyalty constraint, not a Rust-necessary one.

3. **Type-erasure cost is dispersed but real** (S4 + S5 + S6 + S8): the contract sanctioned 3 dyn boundaries, but each one (AnimationListener closure capture, `BoxedView` per conditional return, recursive-widget boxing, derive-macro vs blanket-impl) creates a per-frame or per-widget tax. Individually small; aggregated across the catalog + 60fps + 1M-user scale, the sum is the actual production cost the spec does not measure.

## Recommended next actions (orchestrator best-judgment)

**Worth re-opening contract before Phase 1 commits:**
- **S1** — KeyId interning (resolve before FR-022 lands; otherwise breaking ripple later)
- **S2** — static-path-skips-keyed-reconciler (resolve before Phase 2 commits to symmetric algorithm)

**Worth FR enrichment (additions, not contract changes):**
- **S3** — `column!` macro friendly error for >16 children
- **S6** — recursive widget `#[view(recursive)]` derive hint + Edge Case
- **S7** — `ReconcileEvent` trace instrumentation (~30 LOC in reconciler)
- **S8** — port-check trigger #9 for sanctioned dyn boundary (~10 LOC)

**Worth Deferred-Open-Questions investigation:**
- **S4** — AnimationListener closure-capture benchmark + alternatives
- **S5** — `flui-macros` necessity vs blanket-impl-with-defaults
- **S9** — Phase 1 dead-code window cap (time-box vs document acceptable window)
- **S10** — ElementId stability via content-addressed identity (FLUI 0.2+ inspiration)

## Next step

This artifact is exploration, not a re-spec. Per `/ce-ideate` discipline: survivors route through `/ce-brainstorm` for the chosen ones (if any) before `/ce-plan` operationalizes them. Recommended sequence:

1. **Apply low-risk FR enrichments** (S3, S6, S7, S8) to spec — these are additive and improve the contract.
2. **Investigate via prototype + benchmark** (S1, S4) before locking corresponding FRs — both are P0-tier risks the spec deferred.
3. **Document as Deferred** (S5, S9, S10) — preserve as future-work signals without re-opening the contract now.
4. **Re-open contract** (S2) — only if static-vs-dynamic asymmetric reconciler is judged worth the divergence from Flutter behavior loyalty (likely no per STRATEGY.md, but worth a meeting).
