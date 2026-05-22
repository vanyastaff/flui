# Specification Quality Checklist: Core Contracts (C2 + C3 + C4 + C6)

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-05-22
**Last revised**: 2026-05-22 (round-2, post doc-review revision)
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs) — *adapted: see Notes*
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders — *adapted: see Notes*
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain (4 open items in "Deferred / Open Questions" — appropriate for plan-phase resolution, not blocking decisions)
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details) — *adapted: see Notes*
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows (6 user stories across 3 audiences)
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification — *adapted: see Notes*

## Round-1 Doc-Review Findings — Resolution Status

Round 1 (2026-05-22) `/ce-docs-review` produced 27 findings across 5 reviewers (coherence / feasibility / product-lens / scope-guardian / adversarial). All P0 (4) and P1 (9) are resolved in this revision; P2 (11) and P3-FYI (6) are either resolved, folded into Deferred / Open Questions for plan-phase resolution, or explicitly accepted with rationale.

### P0 — all resolved

| Round-1 finding | Resolution |
|---|---|
| Reconciler is stub, not "tested starting point" (feasibility + scope + adversarial 3-way agree) | FR-024 honestly frames the scaffold state — start/end fast paths work, keyed middle is TODO stub, tests cover zero keyed cases. Assumption "Reconciler scaffold honestly framed" + "Algorithm source" rewritten. |
| FR-008 ⇒ FR-010 ordering missing (feasibility) | FR-021 explicit internal-ordering precondition stated ("FR-019 must land first; the two cannot land split"). FR-024 explicitly depends on FR-022 (`key` storage) landing first. |
| 6-variant enum vs current generics (`RenderBehavior<V>` + `AnimationBehavior` composition) (feasibility + adversarial) | FR-019 + FR-020 reflect real taxonomy: `ElementKind` enum, plan-phase decision between four `Render*` variants vs one `Render(RenderElementData)`-with-inner-arity-enum. `AnimationBehavior` folded into `Stateful` as optional `animation_listener` field (matching actual composition). |
| FR-002/003 misname trait (View has no `build()`) (feasibility + scope) | FR-003 explicit: View trait does NOT gain `build()`. FR-007 + FR-008 put `build()` on `StatelessView` and `ViewState` correctly. US4 acceptance scenarios show the correct trait + derive pattern. |

### P1 — all resolved

| Round-1 finding | Resolution |
|---|---|
| `ElementNode` name collision (scope) | FR-019: outer `ElementNode` struct keeps its name + tree-traversal metadata; inner `kind: ElementKind` enum is the new field. No collision. |
| SC-004 trigger #8 doesn't exist (scope + coherence) | SC-004 + FR-033: dedicated grep added to `port-check.sh`, attribution to trigger #8 removed. Assumption "No new refusal trigger #8 needed for SC-004" makes the change explicit. |
| Orphan `wrappers/render.rs` (scope) | FR-031 enumerates affected sites including `crates/flui-view/src/wrappers/render.rs` — must be re-included with corrected impl or deleted with justification. |
| GlobalKey collision conflict with existing warn-and-overwrite (adversarial) | Edge Case rewritten: `debug_assert!` in debug, `tracing::warn!` + first-wins fallback in release. Reconciles new rule with existing `element_tree.rs:522` behavior symmetrically. |
| GlobalKey reparenting bypasses reconciler (adversarial) | FR-030 explicit: reparenting flows through the new keyed reconciler; existing `global_key_registry` becomes an index, not a side-channel. SC-003 strengthened: test asserts reparenting flows through the reconciler, not via the side-channel. |
| US3 mis-prioritized as P2 (product-lens) | US5 (was US3) re-prioritized to P1 with widget-author-reliability framing. US6 keeps the framework-contributor extensibility claim at P2. |
| Sequencing C4+C6 before C2 wrong (3-way agreement) | **Spec scope unified to C2 + C3 + C4 + C6** — the finding is resolved fundamentally. ROADMAP Core.0 entry updated to reflect unification. |
| FR-010 "tested" claim wrong (feasibility) | FR-024 honestly states the test corpus must be written; the existing tests cover zero keyed cases. SC-002 strengthens the test corpus requirement. |
| `impl IntoView` breaks object-safety of sub-traits (feasibility) | FR-007 + FR-008 explicit acknowledgement: `StatelessView` / `ViewState` become non-object-safe via RPIT, acceptable because no `dyn StatelessView` use exists. |

### P2 — folded into FRs or Deferred / Open Questions

| Round-1 finding | Resolution |
|---|---|
| Key model taxonomy wrong (5 keys + `ViewKey` trait, not 3 + `Key`) | FR-022 + Key Entities + Assumption "`Key` taxonomy" use the correct `ViewKey` trait + 5-impl set. `Option<Box<dyn ViewKey>>` is the storage type. |
| Migration scope wider | FR-031 enumerates: `flui-view`, `flui-app/runner.rs:628`, `wrappers/render.rs` orphan, `flui-cli/templates/`, `flui-hot-reload` docs. SC-012 adds CI verification of `flui-cli` template output. |
| SC-007 wrong axis (Flutter O(N) over list, not O(shift-distance)) | SC-006 rewritten: "linear in N regardless of permutation pattern (full-reverse, single-rotate, swap-first-and-last all stay within a constant factor of N)." Matches Flutter's actual algorithm bound. |
| SC-001 8-LOC uncalibrated (Flutter ~6 LOC) | SC-001 tightened to ≤ 6 lines + names Flutter parity directly. The criterion now serves the "better than Flutter or at parity" claim, not a permissive aspirational target. |
| impl-Trait ergonomic cliffs (conditional / recursive / named return) | Edge Cases explicitly address conditional return + recursive widgets. SC-009 measures conditional-return overhead at ≤ 2 tokens (`.boxed()` per branch). Recursive widgets documented as `Box<TreeNode>` at the recursion edge. |
| FR-014 misses platform-backend `dyn` point | FR-029 lists **three** sanctioned `dyn` points: element storage, dynamic-children fallback, **platform backend** (per FOUNDATIONS C9). Pre-existing `View::key() -> Option<&dyn ViewKey>` and `&dyn BuildContext` also acknowledged. |
| FR-014 pre-decides C2 `Vec<BoxedView>` | Not an issue under unified scope — FR-015 explicitly specifies `Vec<BoxedView>` as part of this contract. |
| SC-008 cross-cutting parity infra inflates scope | SC-010 now names the specific test files in scope (`key_test.dart` + keyed-reconciliation tests in `widgets/key_test.dart`). Bounded scope. |
| SC-009 deferred validation untestable at merge | SC-009 rewritten to be testable at merge — measures conditional-return overhead, not future-Core.1-compatibility. The Core.1-compatibility check is no longer an SC; it is an architectural intent enforced by the contract structure itself. |
| FR-007 depends on FR-006 internally | FR-021 (eliminate `downcast_ref`) explicit precondition on FR-019 (`ElementKind` enum) landing first. |
| SC-004 trigger attribution unclear | Resolved by SC-004 rewrite + FR-033 dedicated grep + Assumption. |

### P3 — FYI; acknowledged or noted in Deferred / Open Questions

| Round-1 finding | Resolution |
|---|---|
| AnimationBehavior overlap with Stateful | Resolved by FR-020 — `Stateful` carries an optional `animation_listener` field; no peer `Animation` variant. |
| Hash-collision silent merge | Resolved by FR-024 + Edge Case: keyed lookup uses `ViewKey::key_eq` (semantic equality on hash hit), not hash-only. Hash collisions between distinct keys do NOT silently merge. |
| FR-006 closes before C2 design (50) | Resolved by unification. |
| Audience missing end-app developers (50) | Audience section now names 3 user types: widget authors, end-app developers, framework contributors. |
| FR-002 no-lifetime not justified (50) | FR-002 has full rationale: lifetime parameter would force `impl<'a> View for MyWidget<'a>`, block `'static` arena slots, destroy `impl Trait` inference. |
| `can_update` typed form may break object-safety (50) | Assumption "`can_update` form" explicit: object-safe form is permanent; typed `Memo<V>` is a separate extension trait, not a View method. |

### Total finding resolution

- **27 findings → 27 resolved** (all P0/P1 addressed substantively in FRs/SCs; all P2/P3 either folded into FRs/SCs/Assumptions/Edge Cases or explicitly noted in Deferred / Open Questions).
- **4 items in Deferred / Open Questions** — appropriately deferred to plan-phase (Render-variant shape choice, `column!`/`row!` macro location, `view_match!` helper macro, `bon` enforcement) plus 1 ergonomics measurement note (`Option<Box<dyn ViewKey>>` per-node memory cost).

## Notes

### Framework-contract adaptation of "no implementation details" / "non-technical stakeholders" / "technology-agnostic"

This specification documents a **framework-internal contract** — the C2 + C3 + C4 + C6 clauses from `docs/FOUNDATIONS.md` Part III that every future widget in `flui-widgets`, `flui-material`, and `flui-cupertino` will commit to at its first line. Standard Speckit guidance ("no implementation details," "written for non-technical stakeholders," "technology-agnostic success criteria") is calibrated for user-facing product features where the user does not see the internals.

For a framework-contract spec that calibration does not apply cleanly:

- The **stakeholders are technical** — widget authors, end-app developers (who indirectly observe the contracts), framework contributors. Hiding the trait shape from them defeats the spec's purpose.
- The **implementation details *are* the user-facing surface** for this audience: a widget author who cannot see `View::build() -> impl IntoView` or `ViewSeq` cannot evaluate whether the contract is acceptable to them.
- The **technology references in success criteria** (`port-check.sh`, `cargo build`, `crates/flui-view/...` paths, `criterion`, `cargo-asm`) are the only objective measurements available — replacing them with technology-agnostic prose would make the criteria unverifiable.

Two compensating rules followed:

1. The spec frames every requirement in terms of *observable behavior* (what compiles, what tests pass, what state is preserved). Internal data structures or algorithms beyond what the contract obligates do not appear — those belong in the `/speckit.plan` step.
2. Every functional requirement and success criterion is independently testable by a `cargo` command exit code or an integration test, satisfying the underlying intent of "testable and unambiguous" even when the surface phrasing references framework APIs.

### Validation summary

- **Hard-pass items**: 11 of 14 (every requirement-completeness and feature-readiness item beyond the four adapted items).
- **Adapted items**: 4 — the three "no implementation details" / "non-technical stakeholders" / "technology-agnostic" content-quality items + the "no implementation details leak" feature-readiness item. All four pass under the framework-contract adaptation.
- **Round-1 doc-review findings resolved**: 27 of 27 (4 P0 + 9 P1 substantively addressed; 11 P2 + 6 P3 folded into FRs/SCs/Assumptions/Deferred).
- **`[NEEDS CLARIFICATION]` markers in spec**: 0.

### Readiness for next step

The spec is ready for `/speckit.plan` — the implementation-planning step that will produce:

- Concrete trait signatures (`View`, `StatelessView`, `StatefulView`, `ViewState`, `IntoView`, `ViewSeq`)
- The `ElementKind` enum definition (resolves Render-variant shape choice from Deferred / Open Questions)
- The `#[derive(StatelessView)]` / `#[derive(StatefulView)]` proc-macro contracts
- The `column!` / `row!` macro definitions and home (`flui-view` vs `flui-widgets`)
- The keyed-reconciliation entry point + call graph + tuple-static / Vec-dynamic dispatch
- Migration steps from the current code (the FR-031 enumeration, expanded with per-site diff)
- The test suite layout for SC-001 through SC-012
- The `criterion` benchmark layout for SC-006
- The `flui-cli` template-output CI test for SC-012

The spec is NOT yet ready for `/speckit.tasks` — that step requires the plan.

### Items marked incomplete

None.
