# Proposal — `core-0a-foundation-parity-to-flutter`

| Field | Value |
|---|---|
| Change ID | `core-0a-foundation-parity-to-flutter` |
| Phase | Core.0 (sub-change 0a — foundation-of-foundations entry gate) |
| Owner crates | `crates/flui-foundation`, `crates/flui-tree` |
| Workflow | SDD (strict TDD per `openspec/config.yaml`) |
| Mode this run | sdd-plan (stop after `tasks.md`) |
| Upstream deps | none |
| Downstream gates | Core.0b (layer/semantics repair), Core.0c (C2 / C3 / C4+C6 contracts spec — `specs/004-view-element-core/`), Core.0d (D-block pipeline wiring) |
| Strict-TDD | **true** (RED → GREEN → REFACTOR mandatory for any code change) |
| Breaking changes allowed | yes (project lead, recorded in task brief; mostly unused in re-scoped change) |
| Review budget per task | 400 changed lines (per `openspec/config.yaml rules.tasks.protect_review_workload`) |

---

## 0. Scope-drift notice (read first)

The task brief that opened this run was written against the **pre-cycle-3** state of `flui-foundation` and `flui-tree` (12 foundation files / 5,424 LOC; 30 tree files / 18,024 LOC; "close all 47 findings I-1..I-22 + T-1..T-25"; "lift `TreeWrite::remove` cascade-by-default to trait contract"; "consolidate quadruple depth-constant drift"; "install `port-check.sh` triggers #8–#13"; "verify flui-log merged into flui-foundation").

**Cycle 3 (PRs #102–#106 + Polish, completed across 2026-05-22 → 2026-05-23) already closed 34 of the 47 audit findings and shipped every task-cited keystone.** Current source on `main` reflects this:

| Task-brief premise | Current on-`main` state |
|---|---|
| 12 foundation files / 5,424 LOC | 11 files / ~5.4k LOC (`observer.rs` deleted; `log/` folded in) |
| 30 tree files / 18,024 LOC | **16 files / 12,491 LOC** (~5.5k LOC of audit-targeted zombie surface deleted) |
| `TreeWrite::remove` cascade-by-default at trait level | **landed** — `crates/flui-tree/src/traits/write.rs:90+` (iterative cascade, `remove_shallow` opt-out, 2k-deep regression test) |
| Consolidate `MAX_TREE_DEPTH` / `INLINE_TREE_DEPTH` to single source | **landed** — `crates/flui-tree/src/depth.rs` is the single source |
| Install `port-check.sh` triggers #8–#13 | **landed** — PR #151 (`scripts/port-check.sh` lines 396–668, plus FR-033 + FR-036) |
| Merge `flui-log` → `flui-foundation` | **landed** — `crates/flui-foundation/src/log/` (commented in `lib.rs:18` as "merged from flui-log in D-block PR-C-1") |
| Close all 47 findings | **34/47 closed** across PRs #102–#106 + Polish; **13 explicitly design-deferred** per audit's "Findings deferred — judgment-call / design-needed" table |

The audit document is authoritative on its own outcome: see `docs/research/2026-05-22-flui-foundation-tree-audit.md`, section "Status (closed — all waves landed)" at lines 2200–2240, which itemises every closed finding by PR.

This proposal therefore **re-scopes** the change to the genuinely-open work. It does **not** re-litigate the 34 closed findings. The supervisor can reject the re-scope at this proposal-review gate; the chain's init envelope (`init.md`) already surfaced the scope-drift as a "high" risk, and this proposal records it formally as Problem-Frame evidence rather than building on a contradicted premise.

---

## 1. Problem Frame

Per `openspec/config.yaml rules.proposal.require_problem_statement: true`.

### 1.1 Why now

- **Core.0 is FLUI's entry gate.** Per `docs/ROADMAP.md ## Core.0 — Spine to target spec`, Core.0 brings the render spine from current state to target spec and locks the architecture contracts. Its sub-changes — **Core.0b** (layer/semantics repair, queued plan at `docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`), **Core.0c** (C2 / C3 / C4+C6 contracts spec, `specs/004-view-element-core/`), and **Core.0d** (D-block pipeline wiring) — all transitively read `TreeRead` / `TreeNav` / `TreeWrite`, `ChangeNotifier`, `Listenable`, `Key`, `Id<T: Marker>`, `BindingBase`, and `Diagnosticable`. Foundation defects compound multiplicatively across every higher layer.
- **The foundation has been hardened, but not finalised.** Cycle 3 deleted ~11,600 LOC of zombie surface, landed the cascade-by-default trait contract, and consolidated depth constants — but the **per-crate decision ledger that `PORT.md` makes the contract substrate** is incomplete: `crates/flui-tree/ARCHITECTURE.md` does not exist (`PORT.md` Index, line 794, lists it as "Not yet templated"), and the 13 deferred audit findings have explicit rationales but no recorded final disposition.
- **Foundation-of-foundations checkpoint.** Before Core.0b/0c/0d touch the layer / element / render trees, the foundation crate-pair must have (a) documented architecture per the template every other Layer-1+ crate has, (b) ratified verdicts on every cycle-3 deferral so the next audit does not re-litigate them, and (c) a verified parity-faithful behavior record against `.flutter/packages/flutter/lib/src/foundation/`. The project lead's mandate — *"должно быть как в .flutter по контракту но архитектурно лучше и эргономичнее, breaking разрешен"* (contract-faithful to `.flutter` but architecturally better/more ergonomic, breaking allowed) — is satisfied for behavior by cycle 3 but lacks the verification artifact pinning each Flutter type to its FLUI counterpart.

### 1.2 Evidence

1. **The 2026-05-22 Mythos audit** — `docs/research/2026-05-22-flui-foundation-tree-audit.md`, 2,290 lines, 47 findings (I-1..I-22 on `flui-foundation`; T-1..T-25 on `flui-tree`). The audit's own "Status (closed)" table (lines 2200–2240) and "Findings deferred — judgment-call / design-needed" table (lines 2243–2256) are the authoritative inventory.
2. **Closed (34/47)** per cycle-3 outcome table:
   - Foundation: I-1 (`ObserverList` deleted), I-2 (`FoundationError` / `ErrorContext` deleted), I-3 (`BindingBase::INITIALIZED` re-init-after-panic fixed + regression test), I-4 (`ChangeNotifier::notify_listeners` per-frame alloc → `SmallVec` inline-4), I-5 (`Default for Key` + `UniqueKey` surprising semantics deleted), I-11 (`#[non_exhaustive]` on `DiagnosticLevel` + `DiagnosticsTreeStyle`), I-13 (`consts.rs::approx_equal*` deleted), I-14 (`assert.rs::report_error!`/`report_warning!` deleted), I-16 (`ListenerCallback` explicit `+ 'static`), I-19 (`ParseDiagnostic*Error` → `Box<str>`), I-20 (`ValueNotifier::into_value` calls `dispose()` before drop), I-22 (`WasmNotSend` deleted).
   - Tree: T-1 (cascade-by-default trait contract), T-2 (parallel mutation APIs consolidated into trait impl), T-3 (`state.rs` typestate, 616 LOC, deleted), T-4 (visitor surface, ~2,560 LOC, deleted), T-5 (`diff.rs`, 1,234 LOC, deleted), T-6 (`iter/{cursor,path,breadth_first,depth_first}`, ~3,800 LOC, deleted), T-7 (`arity/{storage,arity_storage,accessors,runtime,aliases}`, ~3,000 LOC, deleted; markers + simplified `Arity` trait kept), T-8 (`traits/node.rs::Node` trait, 305 LOC, deleted), T-9 (`TreeNav::slot` streaming pass, no `Vec` alloc), T-10 (depth-constant single source), T-11 (`Descendants::next` loop, no recursion), T-12 (`Ancestors::next` bounded by tree size), T-13 (`TreeError::ArityViolation` `#[from] ArityError`), T-14 (`Identifier::From<Index>` always available), T-15 (partial: `MountableExt` deleted; `TreeReadExt` / `TreeNavExt` kept), T-16 (`TreeError::Internal(Box<str>)`), T-18 (`lowest_common_ancestor` `SmallVec<[I; INLINE_TREE_DEPTH]>`), T-20 (`Siblings::new` `SmallVec`), T-21, T-22, T-23, T-25 (obsolete by Wave 4+5 rewrite).
   - Plus PR #103 Codex P2 review item (TreeWrite::remove unbounded recursion → iterative cascade + 2k-deep regression test).
3. **Deferred (13/47)** per cycle-3 design rationale table: **I-6, I-7, I-8, I-9, I-10, I-12, I-15, I-17, I-18, I-21, T-17, T-19, T-24**. Each carries an explicit rationale in the audit (e.g. I-9/I-10: "`flui-scheduler::id::*` actively re-exports these; locking down would break the scheduler's public API contract"; I-15: "Risk of drift > benefit"). These are *judgment calls*, not aesthetic gaps.
4. **PORT.md Index** (`docs/PORT.md:790`–`812`, "Per-crate ARCHITECTURE.md template" section) lists `flui-tree` as **"Not yet templated"**. By contrast, `flui-foundation` is "Templated (grafted 2026-05-19)". The missing ledger is the proximate gap.
5. **FOUNDATIONS.md Part IV** (`docs/FOUNDATIONS.md:132`–`220`, "The target crate decomposition") explicitly cites cycle 3's deletions as closed:
   > "One earlier concern is already closed: `flui-tree`'s speculative `visitor`/`diff`/`cursor` surface (~10k LOC of zero-consumer scaffolding) was deleted in Cycle 3 — the crate is now lean, and its surviving unified `TreeRead`/`TreeNav`/`TreeWrite` trait trio is consumed by every production tree."
   FOUNDATIONS.md's Part IV "structural do-nows" list (`flui-log` → `flui-foundation`; split `flui-geometry`; constitution amendment) is partially complete: the `flui-log` merge landed, the `flui-geometry` split landed (PR #138), but the constitution layer-table amendment is still pending (out of scope for this change — it belongs to a separate doc change).
6. **Bloat-vs-Flutter metric.** Task brief: "Flutter's foundation package is 11.4k LOC; our flui-foundation + flui-tree is 23.4k LOC. We are ~2x bloated. The proposal's expected net delta should be REDUCTION." Current state: flui-foundation + flui-tree = **~17.9k LOC** (≈ −5.5k LOC vs the brief's number, courtesy of cycle 3). Still ~1.6× Flutter, but the residual is largely `flui-tree`, which has **no Flutter counterpart** — Flutter's tree primitives live in `widgets/framework.dart`. The "reduction is the expected net delta" framing is now stale: the largest reduction already shipped in cycle 3, and the remaining residual is structural (the unified-trio invariant `flui-rendering`/`flui-layer`/`flui-semantics` depend on).
7. **Reverse-dependency map** (audit Appendix A.10):
   - Foundation consumers: `flui-scheduler`, `flui-interaction`, `flui-rendering`, `flui-layer`, `flui-semantics`, `flui-view`, `flui-animation` (disabled), `flui-app`, `flui-platform`.
   - Tree consumers: `flui-rendering` (`TreeRead`/`TreeNav`/`TreeWrite` + arity markers), `flui-layer` (`TreeRead`/`TreeNav`), `flui-semantics` (`TreeRead`/`TreeNav`), `flui-view` (`IndexedSlot` only).
   - **No crate outside this list depends on foundation or tree.** The blast radius of any change in this proposal is bounded by this list.

### 1.3 Stakeholder impact

- **Every active downstream crate** listed in §1.2 #7. Their build, test, clippy, and `port-check.sh` results depend on this foundation pair.
- **Every Core.0b/0c/0d sub-change.** Core.0b needs `TreeWrite::remove` cascade semantics (now contract law) and `BindingBase` lifecycle. Core.0c needs `Key` family, `ChangeNotifier`, `Diagnosticable`. Core.0d needs `TreeNav` / `TreeRead` / `TreeWrite` cleanly. If the contract ledger is undocumented, each sub-change re-derives it.
- **Every future widget author.** Every catalog widget inherits Listenable / ChangeNotifier / Diagnosticable / Key semantics from this foundation.
- **Every Mythos cycle 4+ that audits a downstream crate.** Cycles 4+ look up the foundation contract in `crates/flui-foundation/ARCHITECTURE.md` and `crates/flui-tree/ARCHITECTURE.md`. Without the latter, audit cycle quality degrades.
- **Future devtools work** (`flui-devtools` re-enable, post-App.1). The cycle-3 mass-delete (`StatefulVisitor`, `TypedVisitor`, `Mountable` typestate, `ObserverList`, `Node` trait, `ChildrenStorage` modules) used the `no-quick-wins-vanyastaff` decision rule: deleted code is recoverable from git history when a real consumer materialises. The ratification step in this change records that decision as permanent (with the git-history-revival trigger condition spelled out), so the next devtools workstream does not re-litigate it.

### 1.4 Risk if not addressed

| # | Risk | Severity |
|---|---|---|
| RN1 | The cycle-3 deferred-13 rationales sit in a research doc, not in the per-crate `ARCHITECTURE.md` `## Mapping decisions` or `## Outstanding refactors` ledger. The next audit cycle (cycle 4, on a different crate-pair) cannot tell the difference between "permanently decided" and "still pending" — it re-litigates the 13. | High |
| RN2 | `crates/flui-tree/ARCHITECTURE.md` does not exist. Downstream crates that need to cite "see `crates/flui-tree/ARCHITECTURE.md ## Mapping decisions`" (the established discipline for cycle-2-and-later audits) have nowhere to cite. Core.0b/0c/0d's design docs must either invent the contract on the fly (drift risk) or block on this. | High |
| RN3 | Cycle 3's implicit goal was Rust-internal hardening, not Flutter-parity verification. The project lead's mandate — *"должно быть как в .flutter по контракту"* — has not been formally verified for the cycle-3-closed surface (Listenable, ChangeNotifier, Key family, BindingBase, Diagnosticable). Behavior is correct in practice (the audit closures verified each by code review against the relevant Dart file), but no single artifact maps each Flutter type → FLUI counterpart → parity-proof-test → divergence-rationale. Downstream layers inherit unverified parity. | Medium |
| RN4 | The 13 deferred findings include I-9 / I-10 (`Id<T>::from_raw` / `zip_unchecked` / `new_unchecked` / `RawId` / `Index` visibility), tied to `flui-scheduler`'s public API. Cycle 4 might touch `flui-scheduler` and re-open the question without seeing the audit's deferral rationale. | Medium |
| RN5 | Foundation-layer ARCHITECTURE.md template gaps proliferate. Five Layer-1+ crates have templated ARCHITECTURE.md (`flui-foundation`, `flui-rendering`, `flui-painting`, `flui-layer`, `flui-engine`). The remaining "Not yet templated" crates (per `PORT.md` Index) are `flui-types`, **`flui-tree`**, `flui-platform`, `flui-semantics`, `flui-scheduler`, `flui-log`, `flui-hot-reload`, `flui-app`. `flui-tree` is the lowest-numbered Layer (Layer 2 substrate, per FOUNDATIONS.md Part IV) and the one Core.0b/0c/0d need first; templating it now unblocks the cleanest sequencing. | Medium |
| RN6 | The "breaking changes explicitly allowed" headroom from the task brief was sized for the pre-cycle-3 scope (lift cascade contract, etc.). Post-cycle-3, the remaining potential breakers in the deferred-13 are small (I-7 adds `Key::try_new`, additive; I-8 forces `ViewKey::is_global_key` explicit override; I-21 deprecates `KeyRef::new`). The headroom is mostly unused — if a downstream-blocking divergence is discovered by the parity sweep, this change has design-phase headroom to ship it cleanly under strict TDD. | Low (provides headroom rather than threatens) |

---

## 2. Intent

Close the foundation-layer entry gate for Core.0 by completing the **three** items that cycle 3's polish PR did not, but that Core.0b/0c/0d cannot proceed cleanly without:

### 2.1 Author `crates/flui-tree/ARCHITECTURE.md`

NEW document, written from scratch per the PORT.md template (`docs/PORT.md:756`, "Per-crate ARCHITECTURE.md template", all five fixed sections required):

1. **`## Flutter source mapping`** — `flui-tree` has no direct Flutter counterpart; tree mechanics live in `.flutter/packages/flutter/lib/src/widgets/framework.dart` (`Element::visitChildren`, `Element::_parent`, `Element::_owner`, `Element::renderObject`, etc.). The mapping section records each surviving `flui-tree` concept and points to the framework.dart line range that motivated it. Per the "Mapping decisions" graft instructions for `flui-foundation`, this is acceptable: "a hierarchy reference (linking out to an appendix… or a narrative walk)".
2. **`## Mapping decisions`** — Ratify cycle 3's mass-deletions as permanent (each entry: deleted surface, rationale = `no-quick-wins-vanyastaff` memory + audit Appendix A.2 zero-consumer evidence, revival trigger = "when a real in-workspace consumer materialises, port from git history at commit `<sha>`"). Each entry uses the "Accepted trade-offs" format from `docs/plans/2026-03-31-custom-render-callback-design.md`.
3. **`## Thread safety`** — `flui-tree` is a pure trait/abstraction crate; concrete trees own their locks. The table is short or empty with an explicit "no locks held in this crate" declaration (acceptable per `PORT.md:763`, "An empty table is acceptable for crates with no shared mutable state").
4. **`## Friction log`** — Any current-on-`main` shape concern that violates a refusal trigger or strategy clause. Expected entries are minimal post-cycle-3 (the audit drained most of these); likely candidates surface during authoring.
5. **`## Outstanding refactors`** — The deferred-13 items whose verdict is *revisit-later-with-trigger* are mirrored here (specifically T-17 `Slot::with_siblings` `bon` builder, T-19 `TreeNav::depth` slow-default doc, T-24 iter-constructor visibility). Each entry includes file:line, scope detail, and the trigger condition that would force the refactor.

Update `PORT.md` Index (line 794) from "Not yet templated" → "Templated <date>".

### 2.2 Ratify the 13 deferred audit findings

For each of **I-6, I-7, I-8, I-9, I-10, I-12, I-15, I-17, I-18, I-21, T-17, T-19, T-24**, produce a final verdict in this change's `design.md` decision table:

- **accept-permanent** — audit's deferral rationale stands; closed without code change; rationale mirrored into the relevant ARCHITECTURE.md's `## Mapping decisions` (if it represents a *design choice*) or `## Outstanding refactors` (if it represents a *deliberately-deferred refactor with a trigger*).
- **revisit-now** — open a follow-on code task in this change. Each such task lands under strict TDD (RED test against the cycle-3-closed expectation → GREEN fix → REFACTOR) per `openspec/config.yaml rules.apply.test_command = cargo test --workspace`.
- **revisit-later-with-trigger** — record the trigger condition (e.g. "when `flui-reactivity` is re-enabled and lock-free counter benchmarks beat the current `Mutex::lock` uncontended steady-state") and mirror to `## Outstanding refactors`.

Default expectation per audit rationale: **most → accept-permanent**, possibly 1–3 → revisit-now (specifically I-7 `Key::try_new`, I-8 `ViewKey::is_global_key()` explicit, I-21 `KeyRef::new` deprecation — all additive or `#[deprecated]` annotations, well within strict-TDD review-budget). The design phase makes the final call; the proposal does not pre-commit to any specific verdict.

### 2.3 Parity-verification sweep against `.flutter/packages/flutter/lib/src/foundation/`

NEW document at `docs/research/2026-XX-XX-foundation-parity-verification.md` (date set at design phase). Lists, for each named Flutter foundation type from the project lead's mandate (`ChangeNotifier`, `Listenable`, `Key` / `LocalKey` / `ValueKey` / `UniqueKey` / `ObjectKey` / `GlobalKey`, `Diagnosticable`, `BindingBase`, `ObserverList`):

| Column | Content |
|---|---|
| Flutter type | Dart class name + `.flutter/packages/flutter/lib/src/foundation/<file>.dart` file:line |
| FLUI counterpart | Rust type + `crates/flui-foundation/src/<file>.rs` (or `crates/flui-view/src/key/<file>.rs` for the keys that live there) file:line |
| Observable-behavior tests | List of test names + paths that prove parity (each test must be passing) |
| Divergence | If any: the deliberate FLUI-side divergence, with cross-reference to the relevant ARCHITECTURE.md `## Mapping decisions` entry |
| Audit cross-ref | The cycle-3 audit finding(s) that touched this type |

If the sweep discovers a *real* parity divergence not previously recorded (i.e. a behavior bug, not a deliberate design divergence), it becomes a code task in this change's design phase under strict TDD. The proposal explicitly allows this and budgets review headroom for ≤2 such code tasks.

### 2.4 What this change does NOT do

- Does **not** re-attempt any of the 34 cycle-3-closed findings. Their PRs (#103, #104, #105, #106, + Polish PR) are the authoritative resolution; reopening is out of scope.
- Does **not** re-litigate the task brief's "delete vs feature-gate vs canonicalize" choice for `StatefulVisitor` / `TypedVisitor` / `Mountable` typestate / `ObserverList` / `Node` trait / `ChildrenStorage` modules. Cycle 3 chose **delete** per the `no-quick-wins-vanyastaff` memory rule ("feature-gated dead code is still maintenance burden + CI-compile overhead"; future devtools revives from git history). This change records the verdict as permanent in the new `crates/flui-tree/ARCHITECTURE.md` `## Mapping decisions`.
- Does **not** lift `TreeWrite::remove` cascade to trait contract — landed in cycle 3 Wave 1+2 (PR #103). Verified at `crates/flui-tree/src/traits/write.rs:90+`.
- Does **not** consolidate depth constants — landed in cycle 3 Wave 3 (PR #104). Verified single source at `crates/flui-tree/src/depth.rs`.
- Does **not** install `port-check.sh` triggers #8–#13 — landed in PR #151 (`scripts/port-check.sh:396–668`, plus FR-033 + FR-036).
- Does **not** merge `flui-log` → `flui-foundation` — landed earlier. Verified at `crates/flui-foundation/src/log/` with the comment in `lib.rs:17–18`: "Logging - cross-platform tracing backend (merged from flui-log in D-block PR-C-1 per docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md U1)".
- Does **not** touch `flui-geometry` — Option C math-stack decision has its own research doc (`docs/research/2026-05-24-flui-geometry-polish-pass-research.md`) and its own future `/sdd-new`.
- Does **not** touch `flui-types::physics` — separate Core.0 deliverable per ROADMAP.
- Does **not** touch `flui-platform`, `flui-macros` (no audits yet).
- Does **not** touch layer/semantics (Core.0b), view/element (Core.0c), D-block pipeline (Core.0d).
- Does **not** re-enable `flui-reactivity` or `flui-devtools`. Those re-enables will retire some of the *revisit-later-with-trigger* verdicts authored here; until then, the verdicts stand.

---

## 3. Scope

### 3.1 In scope

| Area | What changes | Modality |
|---|---|---|
| `crates/flui-tree/ARCHITECTURE.md` | **NEW.** All five PORT.md-template sections. ~300–500 LOC. | Doc add |
| `crates/flui-foundation/ARCHITECTURE.md` | **AMEND.** `## Outstanding refactors` updated with verdicts for the foundation-side deferred findings (I-6, I-7, I-8, I-9, I-10, I-12, I-15, I-17, I-18, I-21). `## Mapping decisions` extended for any reclassified item. Architecture Decision Summary table refreshed to reflect post-cycle-3 state (e.g. `ObserverList` row deleted; `Notifier` row clarified; `log` row added). | Doc amend |
| `docs/PORT.md` Index (line 794) | Flip `flui-tree` row from "Not yet templated" → "Templated <date>". | Doc edit (1 line) |
| `docs/research/2026-XX-XX-foundation-parity-verification.md` | **NEW.** Parity-verification report per §2.3. ~200–400 LOC. | Doc add |
| Up to 3 surgical code tasks | Only if any deferred-finding verdict is *revisit-now*. Each ≤400 review-line budget, strict TDD. Plausible candidates: I-7 `Key::try_new`, I-8 `ViewKey::is_global_key()`, I-21 `KeyRef::new` `#[deprecated]`. | Code change (RED → GREEN → REFACTOR) |
| Up to 2 surgical code tasks | Only if §2.3 parity sweep discovers a real divergence not previously recorded. | Code change (RED → GREEN → REFACTOR) |

### 3.2 Out of scope (each gets its own future `/sdd-new`)

- `flui-geometry` math-stack decision (Option C: `euclid` + `glam` + `kurbo` + `mint`; research doc `2026-05-24-flui-geometry-polish-pass-research.md`).
- `flui-types::physics` parity audit (Core.0 deliverable, separate change per ROADMAP).
- `flui-platform`, `flui-macros` (no audits yet).
- Layer/semantics repair (Core.0b — queued plan at `docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`).
- View/element/core contracts C2 / C3 / C4+C6 (Core.0c — `specs/004-view-element-core/`).
- D-block pipeline wiring (Core.0d).
- Constitution v2.3.0 amendment (layer table + edition/Rust-version line; per FOUNDATIONS.md `:273`). Doc-only change; out of scope for this foundation-code-focused change.
- Re-enabling `flui-reactivity` (deferred until the foundation's Notifier-vs-signals decision has a real call site).
- Re-enabling `flui-devtools` (would re-introduce some deleted tree-visitor surface; tracked here as the trigger condition for *revisit-later-with-trigger* verdicts on I-6 / T-15 partial / T-24).

---

## 4. Affected areas

| Path | Touch | Notes |
|---|---|---|
| `crates/flui-tree/ARCHITECTURE.md` | NEW file | Per PORT.md template (5 fixed sections). |
| `crates/flui-foundation/ARCHITECTURE.md` | AMEND | `## Outstanding refactors`, `## Mapping decisions`, Architecture Decision Summary table. |
| `docs/PORT.md` | 1-line edit | Index row for `flui-tree`. |
| `docs/research/2026-XX-XX-foundation-parity-verification.md` | NEW file | Parity-verification report. |
| `openspec/changes/core-0a-foundation-parity-to-flutter/spec.md` | NEW file | Spec phase, next sdd-plan step. |
| `openspec/changes/core-0a-foundation-parity-to-flutter/design.md` | NEW file | Design phase, includes 13-deferred verdict table. |
| `openspec/changes/core-0a-foundation-parity-to-flutter/tasks.md` | NEW file | Tasks phase, chunked by 400-line review budget. |
| `crates/flui-foundation/src/**` (optional) | EDIT | Only if a deferred-finding verdict is *revisit-now*. Strict TDD. |
| `crates/flui-tree/src/**` (optional) | EDIT | Only for T-17 / T-19 / T-24 if any becomes *revisit-now*. Strict TDD. |
| `crates/flui-foundation/tests/**` (optional) | NEW/EDIT | RED tests for any *revisit-now* item + parity-sweep proof tests. |
| `crates/flui-tree/tests/**` (optional) | NEW/EDIT | Same. |

**No downstream crate is touched** in the default path. The change is doc-and-decision-ledger plus optional surgical code; the reverse-dependency graph in §1.2 #7 is not perturbed.

---

## 5. Risks

| # | Risk | Likelihood | Mitigation |
|---|---|---|---|
| RP1 | Scope-drift between task brief (pre-cycle-3) and `main` reality (post-cycle-3) means the supervisor may want a *different* scope than this proposal honours. | High | Surfaced as §0 scope-drift notice and §1.2 evidence. Supervisor can reject and re-scope at the proposal-review gate. The init envelope already flagged this as "high". |
| RP2 | §2.3 parity sweep discovers a real divergence in cycle-3-closed surface (e.g. `ChangeNotifier` listener-ordering, `BindingBase` init-after-panic edge, `GlobalKey` ID-counter overflow vs `Key.from_str` collision-with-zero handling). | Medium | The parity report's deliverable structure (Flutter type → FLUI counterpart → proof test → divergence) surfaces any divergence. Real divergences become code tasks in design phase under strict TDD. Review budget headroom: ≤2 such tasks pre-allocated. |
| RP3 | Authoring `crates/flui-tree/ARCHITECTURE.md` exposes friction-log items that should be coded now rather than deferred. | Medium | Same as RP2 — items flip from `## Friction log` to a tasks.md task when remediation is in-scope; otherwise they stay in friction-log with a trigger condition. The 400-line review budget per task bounds maximum surgical surface. |
| RP4 | A "revisit-now" verdict on I-9 / I-10 (`Id<T>` / `RawId` / `Index` visibility) would break `flui-scheduler`'s public API. | Low | The audit's own deferral rationale is "Locking them down would break the scheduler's public API contract". Default verdict in design phase: **accept-permanent**, unless a stronger argument emerges. |
| RP5 | Breaking-change headroom granted by project lead is mostly unused (cycle 3 already shipped the big breaker — the `TreeWrite` cascade contract). | Low | Acknowledged in §1.4 RN6 as headroom rather than threat. Any RP2-driven breaking fix lands cleanly. |
| RP6 | `flui-tree` has no Flutter counterpart, so the parity-verification sweep cannot apply uniformly across the whole crate-pair. | High (already known) | The `crates/flui-tree/ARCHITECTURE.md` `## Flutter source mapping` section explicitly records `flui-tree` as "FLUI-only construct, justified by the unified-trio invariant required by `flui-rendering` / `flui-layer` / `flui-semantics`". Parity for `flui-tree` is *behavior-of-each-trait-method matches the framework.dart equivalent operation*, not file-to-file mapping. |
| RP7 | The `flui-log` merge (already on `main`) was a "structural do-now" per ROADMAP; if it is found to be incomplete (e.g. residual circular dep, missing platform sink), this change has no chartered headroom to fix it. | Low | The cited `lib.rs:18` comment + presence of `crates/flui-foundation/src/log/` substantiates merge completion. Any residual will surface in the §2.1 `## Friction log` and route either to *revisit-now* (in scope) or to a separate `/sdd-new`. |
| RP8 | The parity-verification document carries date `2026-XX-XX` — risk of stale-doc syndrome if not committed promptly. | Low | Date is fixed at design-phase commit; the tasks.md verify step asserts the file exists with a date string matching the change-merge commit's date or earlier. |

---

## 6. Rollback

- **Documentation-only changes** (the new `crates/flui-tree/ARCHITECTURE.md`, the `crates/flui-foundation/ARCHITECTURE.md` amendment, the `docs/PORT.md` Index flip, the new parity-verification report) revert via a single `git revert <merge-commit>`. No downstream crate's build, test, clippy, or `port-check.sh` result depends on these documents — they are read-time references, not compile-time inputs.
- **Optional surgical code changes** (RP2 / I-7 / I-8 / I-21 / parity-divergence fixes) are scoped to one task each per the 400-line review budget, each shipped under strict TDD with a passing regression test. Rollback is a focused per-task `git revert`. Each task can be rolled back independently because tasks are independently TDD-gated.
- **No data-format change.** No persisted file format, no serialization wire format, no on-disk migration is involved.
- **No API-shape removal.** Any API-shape change in the contingency space (I-7 `Key::try_new` additive constructor; I-8 trait-method addition with default-`false` implementation; I-21 `#[deprecated]` attribute) is additive or non-removing. Rollback is a single-commit revert with no migration burden on downstream callers.
- **Constitution / FOUNDATIONS.md / ROADMAP.md / PORT.md** are not amended by this change beyond the single-line `PORT.md` Index flip. The constitution layer-table amendment (FOUNDATIONS.md `:273`) is explicitly out of scope and unaffected.

---

## 7. Success criteria

Every criterion below is a command that exits 0/1 or a test that passes/fails — no prose-only criteria, per `docs/ROADMAP.md` exit-criteria discipline. Verified by the final task's verify step (tasks.md will pin the exact commands).

| # | Criterion | Verification command (illustrative; final form lives in tasks.md) |
|---|---|---|
| SC1 | `crates/flui-tree/ARCHITECTURE.md` exists and contains all five PORT.md-template fixed sections (`## Flutter source mapping`, `## Mapping decisions`, `## Thread safety`, `## Friction log`, `## Outstanding refactors`). | `test -f crates/flui-tree/ARCHITECTURE.md && [ "$(grep -cE '^## (Flutter source mapping\|Mapping decisions\|Thread safety\|Friction log\|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md)" -eq 5 ]` |
| SC2 | `docs/PORT.md` Index reports `flui-tree` as "Templated". | `grep -E '^\| .flui-tree. \| Templated' docs/PORT.md` exits 0. |
| SC3 | Each of the 13 deferred audit findings (I-6, I-7, I-8, I-9, I-10, I-12, I-15, I-17, I-18, I-21, T-17, T-19, T-24) has a recorded verdict ∈ {accept-permanent, revisit-now, revisit-later-with-trigger} in `openspec/changes/core-0a-foundation-parity-to-flutter/design.md`. | Inventory grep in design.md decision table; tasks.md verify step asserts all 13 IDs appear with a valid verdict. |
| SC4 | `docs/research/2026-XX-XX-foundation-parity-verification.md` exists and lists, for each of `ChangeNotifier`, `Listenable`, `ValueNotifier`, `Key`, `LocalKey`, `ValueKey`, `UniqueKey`, `GlobalKey`, `ObjectKey`, `Diagnosticable`, `BindingBase`: FLUI counterpart, parity-proof test name, divergence (if any). | `test -f docs/research/*-foundation-parity-verification.md`; row-count grep against the type list. |
| SC5 | Strict TDD: any code change committed in this PR has a preceding failing-test commit. | `git log --oneline core-0a-foundation-parity-to-flutter` shows RED → GREEN pair for every code task; manual review at the tasks.md verify step. |
| SC6 | `just ci` exits 0 (fmt-check + `cargo clippy --workspace -- -D warnings` + `cargo test --workspace`). | `just ci` |
| SC7 | `bash scripts/port-check.sh -v` exits 0 with all 13 refusal triggers green (including #8–#13 already installed). | `bash scripts/port-check.sh -v` |
| SC8 | No `unimplemented!()` / `todo!()` introduced into `crates/flui-foundation/src/` or `crates/flui-tree/src/`. | `! grep -rEn 'unimplemented!\\(\\)\|todo!\\(\\)' crates/flui-foundation/src crates/flui-tree/src` |
| SC9 | No re-introduction of cycle-3-deleted modules (`observer.rs`, `state.rs`, `visitor/`, `diff.rs`, `iter/cursor.rs`, `iter/path.rs`, `iter/breadth_first.rs`, `iter/depth_first.rs`, `traits/node.rs`, `arity/{storage,arity_storage,accessors,runtime,aliases}.rs`). | Per-path `! test -e` checks in tasks.md verify step. |
| SC10 | PORT.md refusal-trigger discipline on touched files (no `From<scalar>` escape hatches on wrappers per #8 SP-1; no `tracing::warn!` for protocol-violation paths per refusal triggers #11–#13; no speculative scaffolding additions per #11 SP-4). | Enforced by `bash scripts/port-check.sh` (SC7). |
| SC11 | `docs/research/2026-05-22-flui-foundation-tree-audit.md` is referenced from both ARCHITECTURE.md files. | `grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-{foundation,tree}/ARCHITECTURE.md` returns both. |
| SC12 | Review-budget protection: no task in tasks.md exceeds 400 changed lines (per `openspec/config.yaml rules.tasks.protect_review_workload`). | Per-task `git diff --shortstat` check in tasks.md verify step. |

A change-merge is gated on **all 12** criteria. If any fail, the change cannot land.

---

## 8. Notes for the spec / design / tasks phases

This proposal's premises shape the downstream artefacts:

- **`spec.md` acceptance criteria** must hard-bind to SC1–SC12. The spec's behavioural requirements are largely *documentation invariants* + *test-passing assertions* + *PORT.md-compliance assertions*; per `openspec/config.yaml rules.spec.require_acceptance_criteria`, each acceptance criterion must be testable.

- **`design.md` owns**:
  - The 13-deferred-finding verdict table (one row per finding, verdict ∈ 3 enums, rationale, mirror destination).
  - The parity-verification report's row template + the full per-Flutter-type cross-reference table.
  - The decision on whether any deferred item becomes a *revisit-now* code task in this change.
  - Per `openspec/config.yaml rules.design.require_tradeoffs`: each decision must list trade-offs considered (the audit's own deferral rationale is the starting point for most rows).

- **`tasks.md` chunks** within the 400-line review budget per task. Likely shape (numbering tentative; design.md may adjust):
  - **T1** — Author `crates/flui-tree/ARCHITECTURE.md` (single doc commit; ~300–500 lines new). 5 fixed sections + sibling parity-verification appendix.
  - **T2** — Amend `crates/flui-foundation/ARCHITECTURE.md` `## Outstanding refactors` + `## Mapping decisions` + Architecture Decision Summary; flip `docs/PORT.md` Index row for `flui-tree`. Single commit.
  - **T3** — Author `docs/research/2026-XX-XX-foundation-parity-verification.md`. Single doc commit.
  - **T4..T6** (contingent) — Up to 3 surgical code tasks for any deferred-finding *revisit-now* verdict (I-7 / I-8 / I-21 most plausible). Each: RED test commit → GREEN fix commit → optional REFACTOR commit. Each task ≤400 review lines.
  - **T7..T8** (contingent) — Up to 2 surgical code tasks for any parity-sweep-discovered divergence. Same TDD shape.
  - **T-final (verify)** — Run all command-form success criteria (SC1–SC12); record outputs in tasks.md verify section; capture `just ci` + `bash scripts/port-check.sh -v` transcripts; assert no regression of cycle-3-deleted modules.

- **Strict-TDD discipline applies to every code-touching task**: per `openspec/config.yaml rules.apply.strict_tdd: true` and `rules.apply.test_command: cargo test --workspace`, every task that adds production code must have a failing test commit first.

- **Verify gate** is `just ci` (per `openspec/config.yaml rules.verify.test_command`). The verify task does **not** skip this; the change does not merge unless `just ci` exits 0.

---

## 9. References

- `docs/research/2026-05-22-flui-foundation-tree-audit.md` — cycle-3 Mythos audit (47 findings; authoritative "Status (closed)" + "Findings deferred" tables at lines 2200–2256).
- `docs/FOUNDATIONS.md` Part IV — target crate decomposition (cites cycle 3 deletions as closed; locks the L1/L2 layering for `flui-foundation` / `flui-tree`).
- `docs/ROADMAP.md` ## Core.0 — phase definition, structural do-nows, exit criteria.
- `docs/PORT.md` — port methodology; refusal triggers #8–#13; per-crate ARCHITECTURE.md template (`:756`+); Index (`:790`+).
- `.flutter/flutter-master/packages/flutter/lib/src/foundation/` — Flutter source-of-truth (READ-ONLY parity reference).
- `crates/flui-foundation/ARCHITECTURE.md` — existing templated doc (grafted 2026-05-19).
- `crates/flui-foundation/src/lib.rs`, `crates/flui-tree/src/lib.rs` — current module surface.
- `scripts/port-check.sh` — refusal-trigger gates (#8–#13 installed PR #151).
- `openspec/config.yaml` — SDD configuration (strict_tdd=true; `cargo test --workspace`; `just ci`; `cargo clippy --workspace -- -D warnings`).
- Predecessor in chain: `init.md` (run 949e3e92) — surfaced the scope-drift this proposal addresses.
- Project lead mandate (binding, recorded in task brief): *"должно быть как в .flutter по контракту но архитектурно лучше и эргономичнее используя функционал раста и его паттерны и нам breaking разрешен."* Translation: contract-faithful to `.flutter`, but architecturally better/more ergonomic using Rust's patterns; breaking changes allowed.

---

*End of proposal. Chain proceeds to `spec.md` (next sdd-plan step). If the supervisor wants to re-scope away from the §0 re-scoping decision, they should reject this proposal at the review gate before `spec.md` is drafted.*
