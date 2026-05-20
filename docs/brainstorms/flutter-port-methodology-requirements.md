---
date: 2026-05-19
topic: flutter-port-methodology
---

# Flutter → FLUI Port Methodology

## Summary

Playbook of porting Flutter (Dart) to FLUI (Rust), shaped for a solo-maintainer consumer. Form is a refactored exemplar inside `flui-rendering`, a per-crate `ARCHITECTURE.md` template, and a thin `docs/PORT.md` top-level index with refusal triggers. Lint enforcement grows reactively from review hits, not upfront.

---

## Problem Frame

The FLUI strategy mandates a port-not-redesign of Flutter's three-tree (View → Element → Render) architecture into Rust, with mobile-native delivery and DX day-1. ~65-75% of the surface is already ported (foundation, types, tree, painting, layer, platform clean; rendering and view skeletons present), but the port has drifted into patterns that violate the strategy's own constraints — most visibly `RwLock<Box<dyn RenderObject<P>>>` at `crates/flui-rendering/src/storage/entry.rs:46`, `Arc<Mutex<Vec<ElementId>>>` for dirty tracking at `crates/flui-rendering/src/storage/state.rs:1427`, two `unimplemented!()` blocking the render pipeline at `crates/flui-view/src/view/root.rs:477,486`, and ~46 `Box<dyn>` plus 62 `RwLock` sites across the workspace.

Without an explicit methodology, future ports (animation, reactivity re-enable; remaining widget catalog) and refactor passes on the friction zones will repeat the same shape, because the rules for Dart-inheritance → Rust-translation are tacit and have to be reconstructed each session. Three existing port-flavoured docs — `crates/flui-foundation/ARCHITECTURE.md`, `crates/flui-rendering/flutter-rendering-hierarchy.md`, `crates/flui-view/UNIFIED_ELEMENT.md` — show the appetite for in-crate documentation but lack a unifying template or an index that pins down stop-signals.

The cost shape is recurring: friction-pattern review-and-rewrite cycles, drift between crates, and ce-plan / implement-coordinator agents inventing port decisions that ought to be reusable.

---

## Actors

- A1. Solo maintainer (`vanyastaff`): runs ports and refactor passes by hand; primary reader and author of the playbook artifacts. Methodology is shaped around this actor; not optimised for parallel multi-author flow.
- A2. Implementation agent (Claude Code in `/aif-implement`, implement-coordinator): consumes per-crate `ARCHITECTURE.md` + `docs/PORT.md` as planning input when working a task in a port-touched crate. Not a primary author of the playbook, but a downstream reader whose handoff quality is a success criterion.

---

## Key Flows

- F1. Port a fresh Flutter file
  - **Trigger:** A new file from `../../../.flutter/flutter-master/packages/flutter/lib/src/<area>/` needs to land in the corresponding `crates/flui-<area>/`.
  - **Actors:** A1.
  - **Steps:** Read Dart source → consult `docs/PORT.md` for refusal triggers and the recipe class (mixin / nullable / Future / dynamic dispatch / etc.) → write Rust following the per-crate `ARCHITECTURE.md` conventions → record any new Mapping decision and update the Friction log in that crate's `ARCHITECTURE.md` → cross-check against Flutter test parity where applicable.
  - **Outcome:** Port lands without a refusal-trigger violation; the per-crate doc reflects the new decision; the next port in the same area can reuse the pattern.
  - **Covered by:** R2, R3, R7, R10, R13, R14, R15.

- F2. Refactor an existing friction zone
  - **Trigger:** A flagged friction site (e.g., `flui-rendering/src/storage/entry.rs:46`) is selected for cleanup.
  - **Actors:** A1.
  - **Steps:** Locate the friction in the crate's `ARCHITECTURE.md` Outstanding refactors list → pick the Rust-native shape that resolves the refusal-trigger violation (e.g., enum dispatch over `Box<dyn>`, typestate over runtime check) → refactor → move the entry from Outstanding refactors to Mapping decisions with the rationale → if the friction class is new, append a refusal trigger to `docs/PORT.md`.
  - **Outcome:** The friction site no longer matches any refusal trigger; the per-crate doc captures why this shape was chosen.
  - **Covered by:** R4, R5, R6, R10, R11, R13, R14.

- F3. Extend refusal triggers
  - **Trigger:** A review (self-review or PR review by A2) catches an anti-pattern not in the current refusal trigger list.
  - **Actors:** A1.
  - **Steps:** Add the pattern to `docs/PORT.md` refusal triggers with a one-line description → if the same pattern is caught a second time, promote it to a clippy lint (custom or via `clippy.toml`) → keep the doc entry as the human-readable surface.
  - **Outcome:** Future ports refuse the pattern at write time, not at review time.
  - **Covered by:** R10, R11, R12.

---

## Requirements

**Playbook scope and form**
- R1. The methodology is authored and consumed by A1 (solo maintainer). External-contributor onboarding and AI-agent-only consumption are explicitly out of scope; A2 reads the same artifacts as a side-effect, not as a different format.
- R2. The playbook is materialised as three coordinated artifacts: (a) one refactored exemplar file inside `flui-rendering` that demonstrates a clean Rust-native port shape, (b) a per-crate `ARCHITECTURE.md` template applied to active crates, (c) a top-level `docs/PORT.md` that indexes the per-crate docs and lists refusal triggers + general mapping rules.
- R3. `docs/PORT.md` does NOT duplicate per-crate detail. It contains only: link index to per-crate `ARCHITECTURE.md` files, the refusal-trigger list, and shared mapping conventions (e.g., ID offset, `Option<T>` for nullable, `Result<T, E>` for exceptions). Per-crate specifics live in the per-crate doc.

**Exemplar refactor**
- R4. The exemplar is a refactor of an existing friction zone in `flui-rendering`, not a fresh port. Because ~65-75% of the surface is already ported, the high-value demonstration is "how to clean up a violation," not "how to start from a blank file."
- R5. The exemplar is selected from `flui-rendering` friction sites identified by the Phase 1.1 investigation. Candidates: `storage/entry.rs:46` (`RwLock<Box<dyn RenderObject<P>>>`) or `view/root.rs:477,486` (`unimplemented!()` blocking render pipeline). Exact selection is an Outstanding Question for the maintainer.
- R6. Decisions taken during the exemplar refactor are recorded in `crates/flui-rendering/ARCHITECTURE.md` in the same commit as the code change, not as a follow-up.

**Per-crate `ARCHITECTURE.md` template**
- R7. The template has fixed sections: `## Flutter source mapping` (file → file), `## Mapping decisions` (where the Rust shape diverges from Dart and why), `## Friction log` (known sites that violate refusal triggers but haven't been refactored), `## Outstanding refactors` (planned cleanups with line references). Optional sections (e.g., `## Test parity notes`) may be added per crate.
- R8. Crates adopt the template incrementally as a port or refactor touches them — not as a workspace-wide big-bang sweep. A crate without recent port activity stays on its current doc state.
- R9. The three existing port-flavoured docs (`crates/flui-foundation/ARCHITECTURE.md`, `crates/flui-rendering/flutter-rendering-hierarchy.md`, `crates/flui-view/UNIFIED_ELEMENT.md`) are integrated into the template in-place rather than rewritten from scratch. The `flutter-rendering-hierarchy.md` reference may remain as a sibling appendix linked from the templated doc.

**Refusal triggers**
- R10. The initial refusal-trigger list, sourced from the Phase 1.1 investigation, contains: `RwLock` field on a type used inside `perform_layout` or `paint`; `Box<dyn RenderObject<_>>` stored in render-tree storage hot path; `async fn` declared on `View::build`, `RenderObject::layout`, or `RenderObject::paint`; `Mutex` on dirty-list state mutated during the build/layout/paint cycle; `Arc::clone` performed inside the per-frame paint loop on a per-render-object basis; recursive `Box<dyn View>` storage in element child collections. All six are seeded from day one — they map directly to friction sites the Phase 1.1 investigator counted in the existing code, not to theoretical concerns.
- R11. Refusal triggers grow reactively. A new trigger is added when an anti-pattern is caught in review, not preemptively from theoretical concerns.
- R12. A refusal trigger is promoted from doc-only to a clippy lint (custom via `dylint`, or workspace `clippy.toml` deny entry) only after the same pattern has been caught at least twice in review. Upfront lint infrastructure is rejected — it costs more than it returns at solo-maintainer scale.

**Conflict resolution rules**
- R13. When a Flutter semantics constraint conflicts with a Rust-idiomatic alternative, Flutter semantics wins. Strategy clause: "behavior loyal, structure Rust-native." Concretely: the Element lifecycle FSM, Flutter-style mixin → trait + ambassador delegation, and `RenderObject::parent_data` indirection stay even when a "cleaner" Rust shape (e.g., typestate-only) exists.
- R14. Where a runtime check and a compile-time check both express the same constraint, the compile-time form is required. Strategy clause: "compile-time over runtime." Concretely: arity (`Leaf` / `Single` / `Optional` / `Variable`), typestate builders (e.g., `BuilderContextBuilder<P, Pr>`), sealed traits (`PlatformBuilder`).
- R15. `async fn` is forbidden in the render hot path (`View::build`, `RenderObject::layout`, `RenderObject::paint`, and their helpers). Async is permitted at IO, scheduler, and build-pipeline boundaries only. Strategy clause: "sync hot path, async на краях."

---

## Acceptance Examples

- AE1. **Covers R4, R5, R6.** Given the exemplar target is `crates/flui-rendering/src/storage/entry.rs:46` (currently `RwLock<Box<dyn RenderObject<P>>>`), when the refactor lands, then the file holds no `RwLock` field on the hot-path storage type, no new occurrence of `RwLock<Box<dyn RenderObject<_>>>` appears elsewhere in `flui-rendering`, and `crates/flui-rendering/ARCHITECTURE.md` contains a Mapping decisions entry naming the replacement shape (e.g., "storage entry uses enum dispatch over arity-keyed variants") with the rationale linked to R10 and R13.

- AE2. **Covers R10, R11, R12.** Given a self-review catches `Arc::clone` inside the per-frame paint loop of a new render object, when the maintainer extends the methodology, then `docs/PORT.md` gains a refusal-trigger bullet for "`Arc::clone` in the per-frame paint loop" with a one-line description. If the same pattern is caught a second time on a different file, then a clippy lint (custom via `dylint` or a workspace `clippy.toml` deny entry) is added for the same pattern; the doc entry stays.

- AE3. **Covers R13.** Given a port of Flutter's `Element` lifecycle FSM (states: initial / active / inactive / defunct + mount/unmount transitions), when a Rust-idiomatic alternative (e.g., a typestate-only sealed enum without runtime state field) is proposed, then the Flutter FSM shape is kept, the per-crate `ARCHITECTURE.md` Mapping decisions section records "FSM preserved per R13 (behavior loyal)," and the typestate-only alternative is not adopted.

- AE4. **Covers R15.** Given a contributor (A1 or A2) attempts to add `async fn build(...)` to a `View` impl, when the refusal triggers are consulted, then the change is rejected at review time with a reference to R15 and the strategy clause "sync hot path." The corresponding clippy lint may or may not exist at that moment, per R12.

---

## Success Criteria

- A1 closes the chosen exemplar friction site (e.g., `flui-rendering/src/storage/entry.rs:46`) without introducing a new `RwLock<Box<dyn RenderObject<_>>>` anywhere else in `flui-rendering` or `flui-view`. Verified by grep at PR time.
- At least three active crates (one of them `flui-rendering`) hold an `ARCHITECTURE.md` that matches the template and was updated within the same commit-window as their last port or refactor. The doc reads as current, not stale.
- A2, given only `docs/PORT.md` plus the relevant crate's `ARCHITECTURE.md`, can pick up an entry from that crate's Outstanding refactors and produce a refactor PR without a fresh brainstorm or out-of-band clarification from A1. Verified by one end-to-end implement-coordinator dispatch.
- Refusal-trigger violations newly introduced after the playbook lands are caught at review time (self-review or PR), not at next-quarter cleanup time. Tracked anecdotally per friction-zone PR.

---

## Scope Boundaries

- Codemod, AST-driven migration, or `syn`-based source rewriters — rejected for solo-maintainer scale; carrying cost outweighs the gain.
- External-contributor onboarding doc, `CONTRIBUTING.md` expansion, dedicated "first PR" walkthrough — different consumer; the strategy values external PRs as a metric but they are not the methodology's primary reader. A separate brainstorm covers that audience if needed.
- Roadmap and re-enable ordering of currently disabled crates (`flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`, `flui-rendering`, `flui-view`) — that is a sequencing brainstorm, not a methodology brainstorm.
- Upfront custom clippy / `dylint` infrastructure — explicitly reactive per R12.
- Full widget catalog specification (Container / Row / Column / Stack / Text / Padding / Center / SizedBox API surface) — a separate brainstorm.
- Any methodology that legitimises `async fn` on the render hot path — strategy clause excludes async in the render hot path entirely.
- Heavy methodology-supporting dependencies (e.g., `tree-sitter`, large `syn` analyzers, bespoke documentation generators) — strategy clause "heavy dep tree" rejects them; doc-only and `dylint` fit inside existing tooling.

---

## Key Decisions

- Rooted-in-code (exemplar + per-crate docs) over a single standalone playbook file: drift is bounded by the surrounding code, and the existing `ARCHITECTURE.md` traction is leveraged rather than abandoned.
- Exemplar-first over write-first: writing the playbook before refactoring a real friction zone risks producing a philosophy page; the exemplar is the playbook's first proof.
- Reactive lint extraction, not upfront tooling: at solo-maintainer scale, the carrying cost of custom lint infrastructure is unjustified until a rule has fired more than once in review.
- Flutter semantics primacy on Dart ↔ Rust conflicts: the strategy's "behavior loyal, structure Rust-native" rule means non-idiomatic-but-Flutter-faithful shapes (Element FSM, mixin → trait + ambassador, parent_data indirection) stay even when a "cleaner" Rust shape exists.
- Compile-time over runtime where both express the same constraint: arity, typestate, sealed traits are required, not optional.
- Exemplar file (R5) is picked by the maintainer at the first refactor session — not pinned in the doc. Per the brainstorm dialogue, "any file from `flui-rendering`" is acceptable; candidates listed in R5 are starting points, not mandates.

---

## Dependencies / Assumptions

- `../../../.flutter/flutter-master/packages/flutter/lib/src/` is available locally as the canonical Flutter source reference; verified during Phase 1.1.
- `crates/flui-foundation/ARCHITECTURE.md`, `crates/flui-rendering/flutter-rendering-hierarchy.md`, and `crates/flui-view/UNIFIED_ELEMENT.md` remain valid raw input for the template integration. If any is materially stale, it is updated in place under the template, not deleted.
- Solo-maintainer ownership remains true through the v1 of this playbook. If a second author joins, R1 is revisited.
- STRATEGY.md (current revision dated 2026-05-19) and `.specify/memory/constitution.md` (v2.2.0) are the source of truth for refusal triggers and conflict resolution rules. If either changes, refusal triggers are re-examined against the new clauses.
- The investigator-reported counts (~46 `Box<dyn>`, 62 `RwLock`, 100+ TODO) are point-in-time approximations from the Phase 1.1 sweep, not audited. Reliance on the precise numbers is avoided; the rule applies to category, not headcount.

---

## Outstanding Questions

### Deferred to Planning

- [Affects R7][Technical] Exact markdown shape of the `ARCHITECTURE.md` template: frontmatter yes/no, table-of-contents requirement, naming convention for section headers, whether `## Friction log` and `## Outstanding refactors` are merged or kept separate. Resolve while writing the first template instance.
- [Affects R10][Needs research] Of the 62 `RwLock` sites surfaced by the investigator, how many actually sit on a layout/paint hot path versus an off-hot-path utility (e.g., `flui-platform` window state, `flui-engine` resource cache)? Whitelist categories should fall out of this count.
- [Affects R12][Technical] Mechanism for the first clippy lint when it is needed: `clippy.toml` workspace deny + comment markers, custom `dylint` plugin in a new `crates/flui-lints` crate, or `cargo-deny` for dependency-shape rules only. Resolve at first lint-promotion event.
