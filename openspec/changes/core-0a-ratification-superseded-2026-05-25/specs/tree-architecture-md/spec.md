# Tree — ARCHITECTURE.md Required Content Specification

## Purpose

Define the required structure and content of `crates/flui-tree/ARCHITECTURE.md`,
the per-crate decision ledger that PORT.md mandates for every Layer-1+
crate. As of cycle-3 closure, `flui-tree` is the lowest-numbered
"Not yet templated" crate per `docs/PORT.md` Index (line 794), and
Core.0b / Core.0c / Core.0d cannot proceed cleanly without it.

This spec pins the document's mandatory sections, content
requirements, and cross-references. The implementing task in
`tasks.md` creates the file; this spec is the acceptance contract.

Owner crate: `crates/flui-tree`.
Owner doc: `crates/flui-tree/ARCHITECTURE.md` (new file).

## Requirements

### Requirement: ARCHITECTURE.md file exists at the canonical path

`crates/flui-tree/ARCHITECTURE.md` MUST exist as a regular file (not
a symlink) and MUST be tracked in git.

**Audit ref:** Proposal RN2 (missing ARCHITECTURE.md is the
proximate gap that this change addresses). `docs/PORT.md:794`
("Per-crate ARCHITECTURE.md Index") lists `flui-tree` as
"Not yet templated".

**Flutter ref:** None — FLUI-native discipline per
`docs/PORT.md`.

#### Scenario: File exists at HEAD

- GIVEN the repository at HEAD after change merge
- WHEN `test -f crates/flui-tree/ARCHITECTURE.md` is run
- THEN it MUST exit 0

#### Scenario: File is git-tracked

- GIVEN the repository
- WHEN `git ls-files crates/flui-tree/ARCHITECTURE.md` is run
- THEN the output MUST contain the path (proves git tracking)

---

### Requirement: ARCHITECTURE.md has all five PORT.md template sections

The document MUST contain all five fixed-name `##` sections in
this exact order:
1. `## Flutter source mapping`
2. `## Mapping decisions`
3. `## Thread safety`
4. `## Friction log`
5. `## Outstanding refactors`

Each section MUST appear as a top-level (`##`) heading with the
exact title shown. Subsections may be added under any of them
using `###` headings.

**Audit ref:** `docs/PORT.md:756` ("Per-crate ARCHITECTURE.md
template"). Discipline is workspace-wide; `flui-foundation`,
`flui-rendering`, `flui-painting`, `flui-layer`, `flui-engine`
all conform.

#### Scenario: All five section titles present

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN `grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$'`
  is run against it
- THEN the count MUST equal 5

#### Scenario: Sections appear in canonical order

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN the line numbers of each section heading are inspected
- THEN they MUST be in strictly ascending order matching the
  template order (mapping → decisions → thread safety →
  friction → outstanding)

---

### Requirement: ## Flutter source mapping section explains the "no direct counterpart" reality

The `## Flutter source mapping` section MUST explicitly note that
`flui-tree` has **no direct Flutter counterpart**. Flutter's tree
mechanics live in `.flutter/packages/flutter/lib/src/widgets/framework.dart`
(`Element::visitChildren`, `Element::_parent`, `Element::_owner`,
`Element::renderObject`, etc.). The section MUST map each
surviving `flui-tree` concept to the framework.dart operation that
motivated it:

| FLUI concept | Flutter inspiration (file:line range) |
|---|---|
| `TreeRead<I>` | `framework.dart::Element` per-class read accessors (`renderObject`, `widget`, `_owner`) |
| `TreeNav<I>` | `framework.dart::Element::visitChildren` / `_parent` / `Element.depth` |
| `TreeWrite<I>` cascade-by-default | `widgets/framework.dart::Element::deactivateChild` (Element-tree subtree teardown) + `rendering/layer.dart::LayerHandle._unref` (Layer cascade) |
| `Depth` + `AtomicDepth` | `framework.dart::Element._depth` (int field) — typed wrapper improves on raw int |
| `IndexedSlot<I>` | `framework.dart::Element.slot` (Object slot) — typed niche-optimised slot |
| Arity markers (`Leaf`, `Single`, `Optional`, `Variable`) | `RenderObjectWithChildMixin<ChildType>` + `ContainerRenderObjectMixin` — type-level binding tags replace mixin syntax |

The section MUST end with an explicit "no direct Flutter
counterpart for the crate as a whole; parity is per-method
behaviour against the framework.dart operation cited above"
declaration.

**Audit ref:** Proposal §2.1 #1 (`flui-tree`'s `## Flutter source
mapping` shape). `docs/PORT.md:760` accepts hierarchy references
for crates without a direct Flutter counterpart.

#### Scenario: Section explicitly notes the no-direct-counterpart reality

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN the body of `## Flutter source mapping` is inspected
- THEN it MUST contain a substring matching the regex
  `no direct Flutter counterpart` (case-insensitive)

#### Scenario: framework.dart is cited as the inspiration anchor

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN searched for `framework.dart` inside the
  `## Flutter source mapping` section
- THEN at least one match MUST appear

---

### Requirement: ## Mapping decisions ratifies cycle-3 mass-deletions as permanent

The `## Mapping decisions` section MUST contain an entry for each
of the seven cycle-3 deletion groups (covered in detail by
`tree-surface-reduction/spec.md`):
1. `state.rs` (Mountable/Unmountable typestate; 616 LOC)
2. `visitor/` (StatefulVisitor / TypedVisitor / built-in visitors
   / composition / fallible; ~2,560 LOC)
3. `diff.rs` (TreeDiff family; 1,234 LOC)
4. Four iterator files: `iter/cursor.rs`, `iter/path.rs`,
   `iter/breadth_first.rs`, `iter/depth_first.rs` (~3,800 LOC)
5. Three arity-storage files: `arity/storage.rs`,
   `arity/arity_storage.rs`, `arity/accessors.rs` (~3,000 LOC)
6. `traits/node.rs` (Node + NodeExt + NodeTypeInfo; 305 LOC)
7. `MountableExt` (deleted alongside `state.rs`)

Each entry MUST use the "Accepted trade-off" format used in
`docs/plans/2026-03-31-custom-render-callback-design.md` (or
similar workspace convention) and MUST include:
- **Deleted surface**: file name + LOC summary.
- **Rationale**: `no-quick-wins-vanyastaff` memory rule citation
  + audit Appendix A.2 zero-consumer evidence reference.
- **Revival trigger**: the per-deletion condition under which
  the surface should be ported back from git history (per
  `tree-surface-reduction/spec.md` requirements).
- **Git anchor**: the commit SHA range or PR number containing
  the deletion (e.g. "PR #105 Wave 4+5").

In addition, the section MUST record three preserved-with-
caveats decisions:
- `TreeReadExt` + `TreeNavExt` extension traits **kept** per
  T-15 partial-close rationale ("have real-world ergonomic
  value").
- Arity **markers** kept; arity-storage machinery deleted
  (T-7 split).
- `TreeWrite::remove` cascade-by-default trait contract
  (T-1+T-2 lift) recorded as the **architectural keystone**
  of cycle 3.

**Audit ref:** Multiple (T-3..T-8, T-15 partial, T-1, T-2);
proposal §2.1 #2 ("Ratify cycle 3's mass-deletions as
permanent").

#### Scenario: Each of the seven deletion groups has a Mapping decisions entry

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN the `## Mapping decisions` section is searched for the
  twelve names: `state.rs`, `visitor/`, `diff.rs`,
  `iter/cursor.rs`, `iter/path.rs`, `iter/breadth_first.rs`,
  `iter/depth_first.rs`, `arity/storage.rs`,
  `arity/arity_storage.rs`, `arity/accessors.rs`,
  `traits/node.rs`, `MountableExt`
- THEN every name MUST appear at least once

#### Scenario: TreeWrite cascade-contract is recorded as the architectural keystone

- GIVEN the `## Mapping decisions` section
- WHEN searched for the substring "cascade-by-default" AND
  "architectural keystone" (case-insensitive)
- THEN at least one entry MUST mention both phrases (or
  equivalent — "keystone" / "core contract" / "central
  invariant" are acceptable synonyms)

---

### Requirement: ## Thread safety section explicitly declares the "no locks held in this crate" invariant

The `## Thread safety` section MUST explicitly declare that
`flui-tree` is a pure trait/abstraction crate that does NOT
hold any shared mutable state. Concrete trees (`RenderTree`,
`LayerTree`, `SemanticsTree`) own their own locks /
`parking_lot::RwLock` / atomics; `flui-tree` itself ships only
trait definitions, trait blanket impls, value-type wrappers
(`Depth`, `AtomicDepth`, `Slot`, `IndexedSlot`), pure iterators,
and arity markers.

The exception is `AtomicDepth` (which contains an
`AtomicUsize`) — this MUST be explicitly noted as the **only**
shared-mutable-state element in the crate, with an explanation
that the atomic is the value type itself (per-instance, not a
shared registry).

Per `docs/PORT.md:763`, "An empty table is acceptable for crates
with no shared mutable state". This requirement permits an
explicit "no locks" declaration in place of a lock inventory
table.

**Audit ref:** Proposal §2.1 #3 (Thread safety expected to be
short or empty with explicit "no locks held in this crate").

#### Scenario: Section declares no-locks invariant

- GIVEN the `## Thread safety` section of
  `crates/flui-tree/ARCHITECTURE.md`
- WHEN searched for any of: `no locks held`, `pure trait`,
  `pure abstraction`, `no shared mutable state`,
  `no internal synchronization` (case-insensitive)
- THEN at least one match MUST appear

#### Scenario: AtomicDepth exception is acknowledged

- GIVEN the `## Thread safety` section
- WHEN searched for `AtomicDepth`
- THEN at least one match MUST appear AND the surrounding
  context MUST explain its per-instance value-type nature

---

### Requirement: ## Friction log captures any current-on-main shape concerns

The `## Friction log` section MUST list any **current-on-main**
shape concern that:
- violates a PORT.md refusal trigger (#1-#13), OR
- violates a `STRATEGY.md` rule, OR
- the audit's cycle 3 follow-up surfaced but cycle 3 did not
  fix.

If no such concerns exist (the expected post-cycle-3 state),
the section MUST contain an explicit declaration "No active
friction; cycle-3 closure drained known concerns; revisit at
next audit cycle".

New friction items discovered during the authoring of
ARCHITECTURE.md (per proposal RP3) MUST be added to this
section with one of two dispositions:
- **In-scope code task**: routes to a `tasks.md` task in this
  change (subject to the 400-line review budget).
- **Out-of-scope; trigger condition recorded**: stays in the
  friction log with a documented trigger.

**Audit ref:** Proposal §2.1 #4 (Friction log expected entries
minimal post-cycle-3).

#### Scenario: Section is non-empty (either lists items or declares none)

- GIVEN the `## Friction log` section
- WHEN inspected
- THEN it MUST be non-empty (either a list of friction items OR
  the explicit "No active friction" declaration). An empty
  section (zero lines of body) MUST NOT exist.

---

### Requirement: ## Outstanding refactors mirrors the 13 deferred-audit-finding verdicts

The `## Outstanding refactors` section MUST mirror the
**revisit-later-with-trigger** verdicts from this change's
`design.md` decision table. Specifically, every deferred-audit
finding whose verdict in design is `revisit-later-with-trigger`
MUST appear as an entry here with:
- **Audit finding ID** (e.g. `T-17`, `I-9`, `T-24`).
- **Scope detail**: file:line of the affected code on `main`.
- **Trigger condition**: the specific event that would force
  the refactor to land.
- **Estimated review-line budget** (per `openspec/config.yaml
  rules.tasks.protect_review_workload`).

The tree-side deferred findings that this section MUST track:
- T-17 (`Slot::with_siblings` `bon` builder; verdict =
  revisit-later-with-trigger per `tree-surface-reduction/spec.md`).
- T-19 (`TreeNav::depth` slow-default doc; verdict =
  revisit-later-with-trigger per `tree-depth-canonical/spec.md`).
- T-24 (iter::* constructors `pub(crate)`; verdict =
  revisit-later-with-trigger per `tree-surface-reduction/spec.md`).

Foundation-side deferred findings (I-6, I-7, I-8, I-9, I-10,
I-12, I-15, I-17, I-18, I-21) are mirrored in
`crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors`
per the `foundation-*` specs.

**Audit ref:** Proposal §2.1 #5 (Outstanding refactors mirrors
deferred-13 verdicts).

#### Scenario: T-17, T-19, T-24 each appear with a trigger condition

- GIVEN the `## Outstanding refactors` section of
  `crates/flui-tree/ARCHITECTURE.md`
- WHEN searched for the literals `T-17`, `T-19`, `T-24`
- THEN each MUST appear at least once AND each entry MUST
  contain a "Trigger" or "Revival trigger" sub-line

#### Scenario: Accept-permanent verdicts do NOT appear in Outstanding refactors

- GIVEN the `## Outstanding refactors` section
- WHEN searched for `T-1`, `T-2`, `T-3`, `T-4`, `T-5`, `T-6`,
  `T-7`, `T-8`, `T-10`, `T-11`, `T-12`, `T-13`, `T-15`
  (closed-by-deletion or closed-with-permanent verdict
  findings)
- THEN zero matches MUST appear (those are in `## Mapping
  decisions`, not here — Outstanding is reserved for live
  refactors awaiting triggers)

---

### Requirement: ARCHITECTURE.md cross-references the 2026-05-22 audit document

The document MUST cite
`docs/research/2026-05-22-flui-foundation-tree-audit.md` at
least once (in the body of any section), so a reader following
ARCHITECTURE.md can trace back to the original 47-finding audit
that drove cycle 3.

**Audit ref:** Proposal §2.1 (cross-reference discipline).

#### Scenario: Audit doc is cited

- GIVEN the file `crates/flui-tree/ARCHITECTURE.md`
- WHEN searched for the substring
  `2026-05-22-flui-foundation-tree-audit`
- THEN at least one match MUST appear

---

### Requirement: docs/PORT.md Index is updated to reflect templating

The single-line entry in `docs/PORT.md` for `flui-tree` (line ~794)
MUST be updated from "Not yet templated" to
`Templated <ISO-date>` (e.g. `Templated 2026-05-25`).

**Audit ref:** Proposal §2.1 final-paragraph requirement.

#### Scenario: PORT.md Index reports flui-tree as Templated

- GIVEN the file `docs/PORT.md`
- WHEN searched for a line matching the regex
  `^\|\s*`flui-tree`\s*\|\s*Templated\s+\d{4}-\d{2}-\d{2}`
- THEN exactly one match MUST appear (case-sensitive,
  pipe-delimited Markdown table row format)

---

### Requirement: flui-foundation/ARCHITECTURE.md is amended to reflect post-cycle-3 state

`crates/flui-foundation/ARCHITECTURE.md` MUST be amended to:
- Update its Architecture Decision Summary table: delete the
  `ObserverList` row, the `FoundationError` row, the
  `WasmNotSend` row (cycle-3 deletions); add a `log/` row
  (cycle-3 flui-log merge per `lib.rs:18` comment); update
  the `Notifier` row to mention the snapshot-then-fire
  `SmallVec<[CB; 4]>` shape and `Arc<AtomicBool>`-shared
  disposed state.
- Add a `## Mapping decisions` entry for the cycle-3 deletions
  (I-1, I-2, I-13, I-14, I-22).
- Update `## Outstanding refactors` to list each
  **revisit-later-with-trigger** verdict for the
  foundation-side deferred-13 findings (I-7, I-9, I-10, I-12,
  I-21 — per the per-domain spec verdicts).
- Cross-reference
  `docs/research/2026-05-22-flui-foundation-tree-audit.md`.

**Audit ref:** Proposal §3.1 "AMEND" row + RN1 (deferred-13
rationale moved into the per-crate ARCHITECTURE.md ledger).

#### Scenario: ObserverList row removed from foundation decision summary

- GIVEN `crates/flui-foundation/ARCHITECTURE.md` after the
  amendment task
- WHEN searched for `ObserverList` in any Architecture
  Decision Summary table
- THEN zero matches MUST appear (cycle-3 deletion ratified)

#### Scenario: log/ row added

- GIVEN `crates/flui-foundation/ARCHITECTURE.md` after the
  amendment task
- WHEN searched for `log/` or `flui-log` in any Architecture
  Decision Summary table OR `## Mapping decisions` section
- THEN at least one match MUST appear (cycle-3 merge ratified)

#### Scenario: Foundation Outstanding refactors lists revisit-later-with-trigger items

- GIVEN `crates/flui-foundation/ARCHITECTURE.md ## Outstanding
  refactors` section after the amendment task
- WHEN searched for `I-7`, `I-9`, `I-10`, `I-12`, `I-21`
- THEN each of the five MUST appear at least once (one entry
  per deferred-with-trigger verdict)

---

### Requirement: ARCHITECTURE.md is ≤ 600 lines (review-budget conformance)

The document MUST be ≤ 600 lines total. Per
`openspec/config.yaml rules.tasks.protect_review_workload` the
review budget per task is 400 changed lines; this requirement
allows the document a modest oversize allowance because (a)
adding the file is one task, and (b) headings + boilerplate
inflate line counts modestly above the substantive line count.

**Audit ref:** `openspec/config.yaml rules.tasks.protect_review_workload`
+ proposal §3.1 "~300–500 LOC" estimate (this requirement
extends the budget by 100 lines for headings + table rendering
without permitting unbounded growth).

#### Scenario: File is ≤ 600 lines

- GIVEN the file at HEAD
- WHEN `wc -l crates/flui-tree/ARCHITECTURE.md` is run
- THEN the line count MUST be ≤ 600
