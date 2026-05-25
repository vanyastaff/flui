# Tasks — `core-0a-foundation-parity-to-flutter`

| Field | Value |
|---|---|
| Change ID | `core-0a-foundation-parity-to-flutter` |
| Phase | sdd-plan / tasks step (final sdd-plan step before sdd-apply) |
| Owner crates | `crates/flui-foundation`, `crates/flui-tree` |
| Source proposal | `openspec/changes/core-0a-foundation-parity-to-flutter/proposal.md` (re-scoped per §0) |
| Source design | `openspec/changes/core-0a-foundation-parity-to-flutter/design.md` (62.1 KB; §5 PR plan ratified) |
| Source specs | 8 domain spec files at `openspec/changes/core-0a-foundation-parity-to-flutter/specs/` (74 requirements) |
| Source audit | `docs/research/2026-05-22-flui-foundation-tree-audit.md` (47 findings; 34 closed by cycle-3, 13 deferred) |
| Strict TDD | **enabled** per `openspec/config.yaml rules.apply.strict_tdd: true` |
| Test runner | `cargo test --workspace` (apply phase); `just ci` (verify phase) |
| Review budget per task | 400 changed lines (`rules.tasks.protect_review_workload: true`) |
| Session review budget | 4,000 changed lines (preflight) |
| Mode this run | sdd-plan — **stop after this file**. Apply is a separate `/sdd-apply`. |

> **Read first.** This task list is the contract the sdd-apply phase executes. It chunks the work to satisfy the 400-line per-task review budget AND records strict-TDD evidence requirements for every contingent code unit. Per `design.md §5`, the default path is **3 doc PRs with ZERO code change**; strict TDD is therefore vacuously satisfied for the default path and binding only for contingent code PRs. Per `design.md §12` open questions, the supervisor MAY flip §4.6 verdicts at the delivery-decision gate at the end of this file — see `## Delivery Decision Required`.

---

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | **~970 net (default path, additions-only)** · worst-case ~2,970 net (default + all 5 contingent PRs fire) |
| Per-PR LOC: PR1 (tree/ARCHITECTURE.md) | ~450 added / 0 deleted / **net +450** |
| Per-PR LOC: PR2 (foundation/ARCHITECTURE.md amend + PORT.md flip) | ~120 added / ~20 deleted (Architecture Decision Summary rows for deleted ObserverList/FoundationError/WasmNotSend) / **net +100** |
| Per-PR LOC: PR3 (parity-verification research doc) | ~350 added / 0 deleted / **net +350** |
| Per-PR LOC: PR4 (contingent — I-7 `Key::try_new`) | ~80-150 net (1 test + 1 const-eval impl + doc-comment) |
| Per-PR LOC: PR5 (contingent — I-8 `is_global_key` abstract) | ~120-180 net (1 trait edit + 4 impl edits + 4 tests) |
| Per-PR LOC: PR6 (contingent — I-21 `KeyRef::new` deprecation) | ~40-60 net (1 attr + caller migrations) |
| Per-PR LOC: PR7 (contingent — parity-sweep divergence #1) | ≤400 net (TBD by sweep) |
| Per-PR LOC: PR8 (contingent — parity-sweep divergence #2) | ≤400 net (TBD by sweep) |
| 400-line budget risk | **Low** for each PR individually (default + contingent both per-PR ≤400). Total session budget worst-case 2,970 of 4,000 → within budget. |
| Chained PRs recommended | **Yes** — 3 default PRs as stacked-to-main chain; contingent PRs land per-feature as they fire. |
| Suggested split | Default: **PR1 → PR2 → PR3** (stacked, dependency-ordered per design §5). Contingent: each verdict-flip OR divergence fix is its own atomic PR. |
| Delivery strategy | **ask-on-risk** — design §12 has 6 open supervisor questions; delivery decision pause REQUIRED before sdd-apply runs (see `## Delivery Decision Required` at end of this file). |
| Chain strategy | **stacked-to-main** (default path; doc-only PRs; each PR rebases on main after merge). Contingent code PRs land on main after their feature flag triggers fire. |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low
```

**Forecast notes:**

- Default-path PRs are entirely **doc** changes (`crates/flui-tree/ARCHITECTURE.md` NEW; `crates/flui-foundation/ARCHITECTURE.md` AMEND; `docs/PORT.md` 1-line edit; `docs/research/<date>-foundation-parity-verification.md` NEW). No `crates/*/src/**` files are touched.
- Net delta vs Flutter foundation: cycle-3 already moved flui-foundation + flui-tree from 23.4k → ~17.9k LOC (~5.5k LOC reduced). This change adds ~970 LOC of documentation; **no code LOC change in default path**.
- Per-PR ≤400-line review budget holds for every PR in both paths. **PR1 has the largest delta (~450 added).** Per the design §5 line estimate, PR1 may exceed 400 lines by ~50; see `## PR1` mitigation note.
- The 4,000-line session review budget is generous: even the all-contingent-PRs-fire scenario consumes ≤2,970 lines (≤74% of budget).

**Per-PR-against-400-budget table:**

| PR | Estimated net | 400 budget | Notes |
|----|--------------:|-----------:|-------|
| PR1 | ~450 | 400 | **Slightly over.** Mitigation: §4 Friction log MAY be deferred to PR1.5 if pre-merge line-count check shows >400. Section is permitted to be one-line ("No active friction items at HEAD") per `tree-architecture-md/spec.md R5` if no friction surfaces. |
| PR2 | ~100 net | 400 | Well under. |
| PR3 | ~350 | 400 | Under, but close — author SHOULD keep parity-verification doc to 14 rows + header + appendix without bloat. |
| PR4 | ~150 | 400 | Contingent only. Strict TDD: RED→GREEN→TRIANGULATE→REFACTOR. |
| PR5 | ~180 | 400 | Contingent only. |
| PR6 | ~60 | 400 | Contingent only. |
| PR7 | ≤400 | 400 | Contingent only. |
| PR8 | ≤400 | 400 | Contingent only. |

---

## Per-PR Task Decomposition

### PR1 — Author `crates/flui-tree/ARCHITECTURE.md`

| Field | Value |
|---|---|
| Scope (one sentence) | Create the new `crates/flui-tree/ARCHITECTURE.md` per the PORT.md 5-section template, ratifying cycle-3 deletions and deferring T-17/T-19/T-24 with explicit triggers. |
| Audit findings closed | T-15 (partial, ratified delete of `MountableExt` + keep `TreeReadExt`/`TreeNavExt`), T-17, T-19, T-24 (recorded as `revisit-later-with-trigger` per design §4.6); ratifies T-3, T-4, T-5, T-6, T-7, T-8 deletions (already shipped cycle-3 PR #103/#105). |
| Spec requirements closed | `tree-architecture-md/spec.md` R1, R2, R3, R4, R5, R6, R7, R9 |
| Estimated net LOC | ~450 added / 0 deleted / net +450 |
| Strict-TDD requirement | **Vacuous** (doc-only commit; no production code change → no RED test required) |
| Downstream consumer impact | None at compile time (doc is read-time reference). Downstream crates (`flui-rendering`, `flui-layer`, `flui-semantics`) MAY cite this file in future ARCHITECTURE.md amendments. |

#### Units (in order)

**U1.1 — Skeleton with all 5 fixed sections**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (NEW)
- **Content:** Top-matter (date, owner, status), then 5 `##` headings in strict order: `## Flutter source mapping`, `## Mapping decisions`, `## Thread safety`, `## Friction log`, `## Outstanding refactors`. Each section initially holds a TODO marker.
- **RED test (vacuous — doc-only):** n/a. No production code.
- **GREEN minimal impl:** Single file write with the 5 headings present.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** `grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md` = **5**, in the listed order (`tree-architecture-md/spec.md` R2).

**U1.2 — Fill `## Flutter source mapping` section**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R3:**
  - Explicit "no direct Flutter counterpart" declaration.
  - Reference to `.flutter/packages/flutter/lib/src/widgets/framework.dart` (Element-tree mechanics live in widgets, not foundation).
  - Map each surviving `flui-tree` concept (TreeRead, TreeNav, TreeWrite, Identifier, ArityMarker, Depth) to the framework.dart line range that motivated it (e.g. `Element.visitChildren` ≈ TreeNav; `Element._parent` ≈ TreeNav parent walk; `Element._owner.removeChild` ≈ TreeWrite::remove cascade).
- **RED:** n/a.
- **GREEN:** Section populated.
- **TRIANGULATE:** n/a.
- **REFACTOR:** Verify each framework.dart line range exists by grepping `.flutter/`.
- **Acceptance:** spec R3 scenarios pass (`grep -E 'framework\.dart' crates/flui-tree/ARCHITECTURE.md` ≥ 1 match; explicit "FLUI-only construct" / "no direct Flutter counterpart" string appears).

**U1.3 — Fill `## Mapping decisions` section (cycle-3 deletions ratification table)**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R4:** "Accepted trade-off" format. One row per deleted surface: state.rs, visitor/, diff.rs, iter/{cursor,path,breadth_first,depth_first}, arity/{storage,arity_storage,accessors,runtime,aliases}, traits/node.rs, MountableExt. Each row: **deleted surface**, **rationale** = `no-quick-wins-vanyastaff` memory + audit Appendix A.2 zero-consumer evidence, **revival trigger** = "when a real in-workspace consumer materialises, port from git history at commit `<sha>`".
- Three preserved-with-rationale entries: TreeReadExt/TreeNavExt kept (real consumers), arity markers kept (used as type-level binding tags in flui-rendering), simplified `Arity` trait kept.
- Cross-reference design §4.3 disposition table (18 rows).
- **RED:** n/a.
- **GREEN:** Table populated.
- **TRIANGULATE:** Verify each "deleted surface" name appears in spec R4's enumerated list.
- **REFACTOR:** n/a.
- **Acceptance:** spec R4 scenarios pass (every named module appears ≥ 1 time; `no-quick-wins-vanyastaff` mentioned ≥ 1 time).

**U1.4 — Fill `## Thread safety` section**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R5:** Explicit declaration "flui-tree is a pure trait/abstraction crate; concrete trees own their locks. No locks are held in this crate." Then mention `Depth::AtomicDepth` as the **only** atomic in the crate, with explanation of its per-instance value-type nature.
- **RED:** n/a.
- **GREEN:** Section populated.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** spec R5 scenarios pass (substring "no locks" OR "no shared mutable state" matches; AtomicDepth context explains per-instance value-type nature).

**U1.5 — Fill `## Friction log` section (or one-line declaration if empty)**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R6:** Either (a) a list of current-on-`main` shape concerns that violate a PORT.md refusal trigger or strategy clause, OR (b) the explicit one-line declaration "No active friction items at HEAD post-cycle-3 closure."
- Expected post-cycle-3 state: minimal-to-empty (audit drained most of these); likely candidates only surface during authoring.
- If a friction item surfaces that should be coded NOW (per design RD4), it flips to a contingent PR — surface as decision back to supervisor before merging this U1.5.
- **RED:** n/a.
- **GREEN:** Section non-empty.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** spec R6 scenarios pass (section non-empty; empty `## Friction log` heading with zero body lines MUST NOT exist).

**U1.6 — Fill `## Outstanding refactors` section (T-17/T-19/T-24 mirror)**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R7:** One entry each for T-17 (`Slot::with_siblings` positional → `bon` builder), T-19 (`TreeNav::depth` slow-default doc + recommend override), T-24 (`Descendants::new` / `Ancestors::new` / `Siblings::new` `pub(crate)` visibility). Each entry: **file:line ref**, **scope detail**, **trigger condition** (per design §4.6 table).
- Cross-reference design §4.6 (verdict source).
- **RED:** n/a.
- **GREEN:** Section populated.
- **TRIANGULATE:** Verify the audit IDs appear nowhere else in `## Mapping decisions` (those are for accept-permanent items, not deferred).
- **REFACTOR:** n/a.
- **Acceptance:** spec R7 scenarios pass (`T-17`, `T-19`, `T-24` each appear ≥ 1 time AND each has an associated trigger condition).

**U1.7 — Add audit cross-reference + finalise**

- **Files touched:** `crates/flui-tree/ARCHITECTURE.md` (edit)
- **Content per spec R9:** Cite `docs/research/2026-05-22-flui-foundation-tree-audit.md` at least once (typically at top in a "Source audit" line).
- Final review pass: line count ≤ 600 (spec line-count guidance).
- **RED:** n/a.
- **GREEN:** Cross-reference added.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** spec R9 scenarios pass (`grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-tree/ARCHITECTURE.md` exits 0).

#### Verification commands (PR1)

```bash
test -f crates/flui-tree/ARCHITECTURE.md                          # SC1
git ls-files crates/flui-tree/ARCHITECTURE.md | grep -q ARCHITECTURE  # SC1
[ "$(grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md)" -eq 5 ]  # SC1
grep -E 'framework\.dart' crates/flui-tree/ARCHITECTURE.md         # spec R3
grep -E 'no-quick-wins-vanyastaff' crates/flui-tree/ARCHITECTURE.md  # spec R4
grep -E '(no locks|no shared mutable state)' crates/flui-tree/ARCHITECTURE.md  # spec R5
grep -E '(T-17|T-19|T-24)' crates/flui-tree/ARCHITECTURE.md         # spec R7
grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-tree/ARCHITECTURE.md  # SC11 + spec R9
bash scripts/port-check.sh -v                                       # SC7
just ci                                                             # SC6 (gate)
```

---

### PR2 — Amend `crates/flui-foundation/ARCHITECTURE.md` + flip `docs/PORT.md` Index

| Field | Value |
|---|---|
| Scope (one sentence) | Sync foundation/ARCHITECTURE.md with post-cycle-3 state (delete obsolete rows, add log/ row, refresh Notifier row, add foundation-side deferred verdicts to `## Outstanding refactors`) AND flip docs/PORT.md Index row for flui-tree to "Templated <date>". |
| Audit findings closed | I-7, I-9, I-10, I-12 (recorded as `revisit-later-with-trigger`); ratifies I-1, I-2, I-3, I-4, I-5, I-11, I-13, I-14, I-16, I-19, I-20, I-22 (cycle-3 closures); records I-6, I-8, I-15, I-17, I-18, I-21 as `accept-permanent`. |
| Spec requirements closed | `tree-architecture-md/spec.md` R8, R10, R11 (covers both files); `foundation-listenable-changenotifier/spec.md` R7 (ObserverList delete ratification); `foundation-binding/spec.md` R5 (BindingBase mapping decision); `foundation-id-system/spec.md` R8 (ValueNotifier acceptance); `foundation-key/spec.md` R9 (KeyRef accept-permanent); `tree-architecture-md/spec.md` R11 (line-count cap). |
| Estimated net LOC | ~120 added / ~20 deleted (3 obsolete rows in Architecture Decision Summary table) / net ~+100 |
| Strict-TDD requirement | **Vacuous** (doc-only commit). |
| Downstream consumer impact | None at compile time. |

#### Units (in order)

**U2.1 — Amend Architecture Decision Summary table**

- **Files touched:** `crates/flui-foundation/ARCHITECTURE.md` (edit only)
- **Content:**
  - **Delete** rows for: `ObserverList` (cycle-3 delete per I-1), `FoundationError`/`ErrorContext` (cycle-3 delete per I-2), `WasmNotSend` (cycle-3 delete per I-22).
  - **Add** row for: `log/` module (cycle-3 merge from former `flui-log` crate per design §2 / proposal §1.2 #2).
  - **Update** row for: `Notifier` / `ChangeNotifier` — clarify `SmallVec<[CB; 4]>` snapshot-then-fire shape (per I-4), `Arc<AtomicBool> disposed` (per I-15), `ValueNotifier::into_value` disposes (per I-20).
- **RED:** n/a.
- **GREEN:** Table rows updated.
- **TRIANGULATE:** Verify no orphan reference to `ObserverList` / `FoundationError` / `WasmNotSend` remains anywhere in the file.
- **REFACTOR:** n/a.
- **Acceptance:** spec R8 scenarios pass.

**U2.2 — Add `## Mapping decisions` entries for cycle-3 design choices**

- **Files touched:** `crates/flui-foundation/ARCHITECTURE.md` (edit)
- **Content:** Six new "Accepted trade-off" entries for `accept-permanent` verdicts from design §4.6:
  - **I-6** — `Key::from_str` collision-with-zero fallback is intentional (preserves `NonZeroU64` invariant; documented hash-collision rationale).
  - **I-8** — `ViewKey::is_global_key()` has default `false`, not abstract (safety-by-default catches "forgot to override").
  - **I-15** — `ChangeNotifier::has_listeners/is_empty/len` use `Mutex<HashMap>` length, NOT lock-free `AtomicUsize` (Mutex uncontended steady-state cost is bounded).
  - **I-17** — `ValueNotifier::take/replace/value_mut` are FLUI-native ergonomic additions (not Flutter divergence; useful for state-mutation patterns).
  - **I-18** — `Marker` trait does NOT carry `+ Debug` supertrait (drops a transitive requirement that surfaced in cycle-3 audit).
  - **I-21** — `KeyRef::new` is NOT deprecated (well-defined call sites; `From<Key>` is the additive Rust-native idiom, not a replacement).
- Cross-reference design §4.6 verdict table.
- **RED:** n/a.
- **GREEN:** Entries added.
- **TRIANGULATE:** Verify each of I-6, I-8, I-15, I-17, I-18, I-21 appears ≥ 1 time in the new `## Mapping decisions` content.
- **REFACTOR:** n/a.
- **Acceptance:** spec R8 scenarios pass.

**U2.3 — Add `## Outstanding refactors` entries for foundation-side deferred items**

- **Files touched:** `crates/flui-foundation/ARCHITECTURE.md` (edit)
- **Content:** Four new entries for `revisit-later-with-trigger` verdicts from design §4.6:
  - **I-7** — `Key::try_new` Result-returning ctor. **Trigger:** "A workspace consumer materialises that needs to recover from `Key::new()` counter overflow without panicking (counter is `AtomicU64`, 584-years-at-1-ns saturation; current shape panics on overflow)."
  - **I-9** — `Id<T>::from_raw` / `zip_unchecked` / `new_unchecked` visibility `pub(crate)`. **Trigger:** "A cycle 4+ workspace audit migrates `flui-scheduler::id::*` off the public `unsafe` constructors so the locked-down visibility no longer breaks the scheduler's public API."
  - **I-10** — `RawId` + `Index` visibility `pub(crate)`. **Trigger:** Same as I-9.
  - **I-12** — Sweep doc-comments to cite Flutter file:line uniformly. **Trigger:** "The §9 parity-verification sweep (PR3) discovers a divergence whose root cause is a missing file:line ref leading to silent drift."
- Cross-reference design §4.6 verdict table.
- **RED:** n/a.
- **GREEN:** Entries added.
- **TRIANGULATE:** Verify each of I-7, I-9, I-10, I-12 appears ≥ 1 time AND each entry has an explicit trigger condition.
- **REFACTOR:** n/a.
- **Acceptance:** spec R8 scenarios pass.

**U2.4 — Flip `docs/PORT.md` Index row for `flui-tree`**

- **Files touched:** `docs/PORT.md` (1-line edit at ~line 794)
- **Content:** Change the index row for `flui-tree` from "Not yet templated" to `Templated 2026-05-25` (substitute commit-day's ISO date if different).
- **RED:** n/a.
- **GREEN:** Edit applied.
- **TRIANGULATE:** Verify exactly one match for the new row pattern (case-sensitive).
- **REFACTOR:** n/a.
- **Acceptance:** spec R10 scenarios pass (`grep -cE '^\|\s*`flui-tree`\s*\|\s*Templated' docs/PORT.md` = 1).

**U2.5 — Reference cycle-3 audit doc from foundation/ARCHITECTURE.md**

- **Files touched:** `crates/flui-foundation/ARCHITECTURE.md` (edit)
- **Content:** Add or confirm citation of `docs/research/2026-05-22-flui-foundation-tree-audit.md` (per SC11; existing doc may already cite — verify and add if missing).
- **RED:** n/a.
- **GREEN:** Citation present.
- **TRIANGULATE:** `grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-foundation/ARCHITECTURE.md` returns the path.
- **REFACTOR:** n/a.
- **Acceptance:** SC11 satisfied.

#### Verification commands (PR2)

```bash
grep -cE '^\|.*ObserverList' crates/flui-foundation/ARCHITECTURE.md  # MUST be 0 (deleted row)
grep -cE '^\|.*FoundationError' crates/flui-foundation/ARCHITECTURE.md  # MUST be 0
grep -cE '^\|.*WasmNotSend' crates/flui-foundation/ARCHITECTURE.md  # MUST be 0
grep -cE '^\|.*log/' crates/flui-foundation/ARCHITECTURE.md          # ≥ 1 (added row)
grep -E '(I-6|I-8|I-15|I-17|I-18|I-21)' crates/flui-foundation/ARCHITECTURE.md  # each ≥ 1
grep -E '(I-7|I-9|I-10|I-12)' crates/flui-foundation/ARCHITECTURE.md  # each ≥ 1
grep -cE '^\|\s*`flui-tree`\s*\|\s*Templated' docs/PORT.md           # = 1 (SC2)
grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-foundation/ARCHITECTURE.md  # SC11
wc -l crates/flui-foundation/ARCHITECTURE.md                          # spec R11: ≤ 600 lines
bash scripts/port-check.sh -v                                          # SC7
just ci                                                                # SC6 (gate)
```

---

### PR3 — Author `docs/research/2026-05-25-foundation-parity-verification.md`

| Field | Value |
|---|---|
| Scope (one sentence) | Author the parity-verification research doc that, per Flutter foundation type, records FLUI counterpart, observable-behavior tests, and divergence verdict — fulfilling proposal §2.3 and design §9. |
| Audit findings closed | None directly closed; this PR is the *parity audit deliverable* that may surface NEW divergences (which then route to PR7/PR8 contingent). |
| Spec requirements closed | proposal §2.3 (parity-verification report); design §9 row template + 14-row cross-reference table. |
| Estimated net LOC | ~350 added / 0 deleted / net +350 |
| Strict-TDD requirement | **Vacuous** (doc-only commit; no production code change). The doc REFERENCES tests but does not author them. |
| Downstream consumer impact | None at compile time. Sets the parity-verification ledger for cycle 4+ audits. |

#### Units (in order)

**U3.1 — Author document skeleton + row template**

- **Files touched:** `docs/research/2026-05-25-foundation-parity-verification.md` (NEW; substitute commit-day's date at author time)
- **Content:** Header (title, date, owner, status), purpose paragraph cross-referencing proposal §2.3 + design §9, then the row template from design §9 reproduced verbatim, then the 14-row skeleton table (Flutter type, FLUI counterpart, FLUI home, audit cross-ref, expected verdict) from design §9.
- **RED:** n/a.
- **GREEN:** Doc skeleton present.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** SC4 file existence + row-count grep against 14-row type list.

**U3.2 — Per-type rows: Listenable + ChangeNotifier + ValueNotifier + ValueListenable + VoidCallback**

- **Files touched:** same file (edit)
- **Content:** For each of these 5 types, fill in: Flutter file:LINE-LINE range (verified by reading `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart`), FLUI source file:LINE-LINE (verified by reading `crates/flui-foundation/src/notifier.rs` + `callbacks.rs`), test names (verified by `grep test_ crates/flui-foundation/tests/`), divergence verdict + rationale per design §9 skeleton.
- **RED:** n/a.
- **GREEN:** 5 rows complete.
- **TRIANGULATE:** Per row, verify the listed test name exists and passes via `cargo test --workspace -p flui-foundation -- <test_name>`.
- **REFACTOR:** n/a.
- **Acceptance:** Each row has all template fields populated AND at least one test name per row exists in the test corpus.

**U3.3 — Per-type rows: Key family (Key + LocalKey + ValueKey + UniqueKey + ObjectKey + GlobalKey)**

- **Files touched:** same file (edit)
- **Content:** For each of these 6 types, fill in row per template. Note: `ObjectKey` and `GlobalKey` live in `flui-view/src/key/` (not `flui-foundation`) — cite there. Note `LocalKey` collapsed-into-ViewKey-trait divergence rationale.
- **RED:** n/a.
- **GREEN:** 6 rows complete.
- **TRIANGULATE:** Per row, verify test names.
- **REFACTOR:** n/a.
- **Acceptance:** Each row complete; divergence column flags deliberate-design for collapsed Local/Global hierarchy.

**U3.4 — Per-type rows: Diagnosticable + BindingBase + ObserverList (deleted)**

- **Files touched:** same file (edit)
- **Content:** Fill final 3 rows. `ObserverList` row explicitly cross-references design §4.5 (deliberate divergence: deleted).
- **RED:** n/a.
- **GREEN:** 3 rows complete.
- **TRIANGULATE:** Per row, verify test names. The ObserverList row has no FLUI test; instead carries `! test -e crates/flui-foundation/src/observer.rs` as the parity assertion.
- **REFACTOR:** n/a.
- **Acceptance:** All 14 rows complete.

**U3.5 — Discovered-divergences appendix (initially empty)**

- **Files touched:** same file (edit)
- **Content:** Add `## Discovered divergences` appendix section. Header table (Flutter type / Before-fix shape / After-fix shape / Resolution PR / Regression-test name). **Initially empty** (proposal RP2: this is the deliverable that surfaces divergences for contingent PR7/PR8).
- If U3.2, U3.3, or U3.4 surface a divergence during the per-row verification, add the row HERE and **pause** PR3 to surface a decision back to supervisor: this divergence becomes a contingent code PR (PR7 or PR8) per design §5's contingent path.
- **RED:** n/a (no test added by THIS PR).
- **GREEN:** Section present.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** SC4 + section present.

**U3.6 — Conformance log appendix (for Engram-unavailable fallback per design §8)**

- **Files touched:** same file (edit)
- **Content:** Add `## Conformance log` appendix section per design §8 Engram-unavailable fallback. Initially holds: SC1–SC12 status checkboxes (filled by T-final verify task at apply phase), per-PR strict-TDD evidence rows for any contingent PR (initially empty), date strings matching the change-merge commit.
- **RED:** n/a.
- **GREEN:** Section present with empty placeholders.
- **TRIANGULATE:** n/a.
- **REFACTOR:** n/a.
- **Acceptance:** Section structure present (gets filled by T-final).

#### Verification commands (PR3)

```bash
test -f docs/research/2026-05-25-foundation-parity-verification.md   # SC4
# Or use a glob if date may shift:
ls docs/research/*-foundation-parity-verification.md
# Row-count grep against 14-row type list:
for t in Listenable ChangeNotifier ValueListenable ValueNotifier VoidCallback \
         Key LocalKey ValueKey UniqueKey ObjectKey GlobalKey \
         Diagnosticable BindingBase ObserverList; do
  grep -q "^### $t\$" docs/research/*-foundation-parity-verification.md || \
    echo "MISSING TYPE ROW: $t"
done
grep -E 'Flutter source.*foundation/.*\.dart' docs/research/*-foundation-parity-verification.md  # ≥ 14
grep -E '## Discovered divergences' docs/research/*-foundation-parity-verification.md  # ≥ 1
grep -E '## Conformance log' docs/research/*-foundation-parity-verification.md  # ≥ 1
bash scripts/port-check.sh -v                                        # SC7
just ci                                                              # SC6 (gate)
```

---

### PR4 (CONTINGENT — only if §4.6 I-7 flipped to `revisit-now`) — Add `Key::try_new`

| Field | Value |
|---|---|
| Scope (one sentence) | Add additive `Key::try_new() -> Result<Self, KeyOverflow>` constructor for callers that need recoverable counter-overflow handling. |
| Audit findings closed | I-7 (if flipped from `revisit-later-with-trigger` → `revisit-now`). |
| Spec requirements closed | `foundation-key/spec.md` R3 (if flipped). |
| Estimated net LOC | ~80-150 net (1 trait edit + 1 const-eval impl + 2 tests + doc-comment + parity row update) |
| Strict-TDD requirement | **BINDING**. RED → GREEN → TRIANGULATE → REFACTOR commits required. |
| Downstream consumer impact | None at compile time (additive `pub fn`); existing `Key::new` retains signature. |

#### Units (in order — STRICT TDD)

**U4.1 — RED: failing parity test asserting `Key::try_new` exists and returns Result**

- **Files touched:** `crates/flui-foundation/tests/key_try_new.rs` (NEW)
- **RED test name:** `key_try_new_returns_ok_below_counter_overflow`
- **Expected failure:** `error[E0599]: no function or associated item named 'try_new' found for type 'Key' in the current scope`
- **CI status after commit:** `cargo test --workspace` exits 1 with the new test failing to compile.
- **Acceptance:** SC5 — RED commit precedes GREEN commit.

**U4.2 — GREEN: minimal `Key::try_new` impl**

- **Files touched:** `crates/flui-foundation/src/key.rs` (edit)
- **GREEN minimal impl:** Add `pub fn try_new() -> Result<Self, KeyOverflow>` that delegates to `AtomicU64::fetch_add` on the counter and returns `Err(KeyOverflow)` on saturation; the `KeyOverflow` zero-sized error type also added.
- **CI status after commit:** `cargo test --workspace` exits 0; new test passes.
- **Acceptance:** SC5 — GREEN commit makes RED test green with minimal change.

**U4.3 — TRIANGULATE: second test exercising overflow path**

- **Files touched:** `crates/flui-foundation/tests/key_try_new.rs` (edit, append)
- **TRIANGULATE test name:** `key_try_new_returns_err_at_counter_saturation`
- **Content:** Force counter to `u64::MAX - 1`, call `try_new` twice, expect first `Ok(_)` and second `Err(KeyOverflow)`.
- **CI status:** exits 0.
- **Acceptance:** Triangulating test exercises the adjacent invariant the GREEN commit could trivially break.

**U4.4 — REFACTOR (optional): factor common counter-bump path with `Key::new`**

- **Files touched:** `crates/flui-foundation/src/key.rs` (edit)
- **Content:** Extract the counter-bump logic into a private `fn next_counter_value() -> Option<NonZeroU64>` used by both `new` and `try_new`. Verify all tests still pass.
- **CI status:** exits 0.
- **Acceptance:** No behavior change; only duplication removed.

**U4.5 — Update parity-verification doc**

- **Files touched:** `docs/research/2026-05-25-foundation-parity-verification.md` (edit)
- **Content:** In the Discovered divergences appendix, add the row for I-7 with "Resolution PR: PR4" + regression-test names from U4.1/U4.3.
- **Acceptance:** SC4 + appendix updated.

#### Verification commands (PR4)

```bash
cargo test --workspace -p flui-foundation -- key_try_new          # both tests pass
cargo clippy --workspace -- -D warnings                            # no new lints
git log --oneline | head -10  # MUST show RED → GREEN ordering for SC5
bash scripts/port-check.sh -v
just ci
```

---

### PR5 (CONTINGENT — only if §4.6 I-8 flipped to `revisit-now`) — Make `ViewKey::is_global_key()` abstract

| Field | Value |
|---|---|
| Scope (one sentence) | Remove `is_global_key()` default-`false` impl from the `ViewKey` trait, forcing every implementor to explicitly state global-key status. |
| Audit findings closed | I-8 (if flipped). |
| Spec requirements closed | `foundation-key/spec.md` R6 (if flipped). |
| Estimated net LOC | ~120-180 net (1 trait edit + 4 impl edits + 4 tests + parity row update) |
| Strict-TDD requirement | **BINDING**. |
| Downstream consumer impact | `flui-foundation::key::{ValueKey, UniqueKey}` + `flui-view::key::{ObjectKey, GlobalKey}` (≥4 impls). Each gets `fn is_global_key(&self) -> bool { false }` (or `true` for `GlobalKey`). Mechanical fix ≤15 LOC across 4 files. |

#### Units (in order — STRICT TDD)

**U5.1 — RED: trait-level test asserting `is_global_key` is required (compile-fail test)**

- **Files touched:** `crates/flui-foundation/tests/view_key_abstract.rs` (NEW) + `crates/flui-foundation/tests/view_key_abstract.fail.rs` (NEW; uses `trybuild` or `compiletest_rs` for compile-fail assertion).
- **RED test name:** `dummy_view_key_without_is_global_key_does_not_compile`
- **Expected failure:** Currently passes (no compile error) because of the default impl. Test asserts compile-fail; current trait state means test fails because the dummy CAN compile without overriding.
- **Acceptance:** SC5.

**U5.2 — GREEN: remove default impl from trait**

- **Files touched:** `crates/flui-foundation/src/key.rs` (edit `ViewKey` trait)
- **GREEN minimal impl:** Remove `fn is_global_key(&self) -> bool { false }` body, leaving `fn is_global_key(&self) -> bool;` abstract signature.
- **Mechanical fix in same commit:** Add explicit `fn is_global_key(&self) -> bool { false }` to `impl ViewKey for ValueKey<T>`, `impl ViewKey for UniqueKey`, `impl ViewKey for ObjectKey<T>` (in `flui-view`). Add `fn is_global_key(&self) -> bool { true }` to `impl ViewKey for GlobalKey<T>` (in `flui-view`).
- **Acceptance:** SC5; compile-fail test now passes; `cargo test --workspace` exits 0.

**U5.3 — TRIANGULATE: per-impl behavior test**

- **Files touched:** `crates/flui-foundation/tests/view_key_abstract.rs` (edit, append) + `crates/flui-view/tests/global_key_global.rs` (edit if exists, else NEW)
- **TRIANGULATE test name:** `value_key_is_not_global_key` + `global_key_is_global_key`
- **Content:** Assert behavior of each new explicit impl.
- **Acceptance:** All tests pass.

**U5.4 — REFACTOR (optional): doc-comment on trait method**

- **Files touched:** `crates/flui-foundation/src/key.rs`
- **Content:** Add `/// Implementors MUST explicitly declare whether they are a GlobalKey...` doc-comment for clarity.
- **Acceptance:** No behavior change.

**U5.5 — Update parity-verification doc**

- **Files touched:** `docs/research/2026-05-25-foundation-parity-verification.md` (edit)
- **Content:** Discovered divergences appendix row for I-8 + LocalKey/GlobalKey divergence resolution.
- **Acceptance:** SC4.

#### Verification commands (PR5)

```bash
cargo test --workspace -- view_key_abstract global_key_global
cargo clippy --workspace -- -D warnings
git log --oneline | head -10  # RED → GREEN ordering
bash scripts/port-check.sh -v
just ci
```

---

### PR6 (CONTINGENT — only if §4.6 I-21 flipped to `revisit-now`) — Deprecate `KeyRef::new`

| Field | Value |
|---|---|
| Scope (one sentence) | Add `#[deprecated]` attribute on `KeyRef::new`, replace internal call sites with `KeyRef::from(k)`. |
| Audit findings closed | I-21 (if flipped). |
| Spec requirements closed | `foundation-key/spec.md` R9 (if flipped). |
| Estimated net LOC | ~40-60 net (1 attr + 2 caller migrations + 1 RED test asserting deprecation warning) |
| Strict-TDD requirement | **BINDING**. |
| Downstream consumer impact | ≥ 2 internal call sites (verified by `grep -rn 'KeyRef::new' crates/`). Zero external workspace consumers. |

#### Units (in order — STRICT TDD)

**U6.1 — RED: caller-warning test (compile-warning capture)**

- **Files touched:** `crates/flui-foundation/tests/key_ref_deprecated.rs` (NEW)
- **RED test name:** `caller_of_key_ref_new_emits_deprecation_warning`
- **Content:** Use `trybuild` to assert that calling `KeyRef::new(k)` from a test file emits the `#[deprecated]` warning (currently fails because attribute not yet added).
- **Acceptance:** SC5 — RED commit fails.

**U6.2 — GREEN: add `#[deprecated]` attribute**

- **Files touched:** `crates/flui-foundation/src/key.rs` (1-line attribute add)
- **GREEN minimal impl:** `#[deprecated(since = "0.x.0", note = "use `KeyRef::from(key)` instead")]` on `KeyRef::new`.
- **Mechanical fix in same commit:** Migrate internal callers (≥ 2 sites) from `KeyRef::new(k)` to `KeyRef::from(k)`.
- **Acceptance:** SC5; RED test now passes.

**U6.3 — TRIANGULATE: assert `KeyRef::from` produces equivalent value**

- **Files touched:** `crates/flui-foundation/tests/key_ref_deprecated.rs` (edit, append)
- **TRIANGULATE test name:** `key_ref_from_equals_key_ref_new_value`
- **Content:** Pre-deprecation parity: `KeyRef::new(k) == KeyRef::from(k)` for a known key.
- **Acceptance:** Test passes.

**U6.4 — REFACTOR (optional): doc-comment cross-referencing `From<Key>`**

- **Acceptance:** No behavior change.

**U6.5 — Update parity-verification doc**

- **Files touched:** `docs/research/2026-05-25-foundation-parity-verification.md` (edit)
- **Content:** Discovered divergences appendix row for I-21.
- **Acceptance:** SC4.

#### Verification commands (PR6)

```bash
cargo test --workspace -- key_ref_deprecated
cargo build --workspace 2>&1 | grep -E 'deprecated.*KeyRef::new' && echo "PASS" || echo "FAIL"  # warning visible
cargo clippy --workspace -- -D warnings  # WARN: deprecated warnings allowed at clippy level
git log --oneline | head -10  # RED → GREEN ordering
bash scripts/port-check.sh -v
just ci
```

---

### PR7 (CONTINGENT — only if §9 parity sweep discovers a divergence) — Divergence fix #1

| Field | Value |
|---|---|
| Scope (one sentence) | TBD by parity sweep in PR3. Each divergence row in the `## Discovered divergences` appendix becomes its own contingent PR. |
| Audit findings closed | TBD by sweep. |
| Spec requirements closed | TBD by sweep. |
| Estimated net LOC | ≤400 net (budget cap). |
| Strict-TDD requirement | **BINDING**. Same RED → GREEN → TRIANGULATE → REFACTOR pattern as PR4. |
| Downstream consumer impact | TBD by sweep. |

#### Units (template — instantiated per discovered divergence)

**U7.1 — RED:** Failing parity test asserting Flutter-canonical behavior. Test name: `parity_<flutter_type>_<observable_behavior>`. Expected failure: current FLUI behavior diverges.

**U7.2 — GREEN:** Minimal impl change in `crates/flui-foundation/src/<file>.rs` (or `crates/flui-tree/src/<file>.rs`) to flip the RED test green.

**U7.3 — TRIANGULATE:** Additional test exercising adjacent invariant.

**U7.4 — REFACTOR (optional):** Clean up duplication surfaced by GREEN+TRIANGULATE.

**U7.5 — Update parity-verification doc:** Update Discovered divergences appendix row with Resolution PR + Regression-test name.

#### Verification commands (PR7)

```bash
cargo test --workspace -- parity_<flutter_type>
cargo clippy --workspace -- -D warnings
git log --oneline | head -10  # RED → GREEN ordering
bash scripts/port-check.sh -v
just ci
```

---

### PR8 (CONTINGENT — only if §9 parity sweep discovers a SECOND divergence) — Divergence fix #2

Same template as PR7. **Hard cap: 5 contingent PRs total (PR4–PR8) per proposal §3.1 + design §5.**

---

### T-final — Verify task (REQUIRED after all merged PRs)

| Field | Value |
|---|---|
| Scope (one sentence) | Run all SC1–SC12 success criteria + implicit-coverage findings inventory; record outputs; assert gate-pass. |
| Audit findings closed | None directly; this is the change-merge gate. |
| Spec requirements closed | All SC1–SC12 (proposal §7) plus implicit-coverage findings per design §13. |
| Estimated net LOC | 0 (script run; outputs appended to parity-verification doc's `## Conformance log` appendix). |
| Strict-TDD requirement | **n/a** (verify task, not impl). |
| Downstream consumer impact | None (verify pass; no file edits except Conformance log appendix). |

#### Units (in order)

**T-final.1 — Run SC1–SC12 commands; capture outputs**

```bash
# SC1
test -f crates/flui-tree/ARCHITECTURE.md
[ "$(grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md)" -eq 5 ]
# SC2
grep -cE '^\|\s*`flui-tree`\s*\|\s*Templated' docs/PORT.md
# SC3 — 13-finding inventory grep against design.md §4.6 table
for f in I-6 I-7 I-8 I-9 I-10 I-12 I-15 I-17 I-18 I-21 T-17 T-19 T-24; do
  grep -q "$f" openspec/changes/core-0a-foundation-parity-to-flutter/design.md \
    || echo "MISSING VERDICT: $f"
done
# SC4
ls docs/research/*-foundation-parity-verification.md
# SC5 — only if any contingent PR landed
git log --oneline core-0a-foundation-parity-to-flutter 2>/dev/null \
  | grep -E '(^[a-f0-9]{7,} test|^[a-f0-9]{7,} (feat|fix))'  # RED→GREEN per code PR
# SC6
just ci
# SC7
bash scripts/port-check.sh -v
# SC8
! grep -rEn 'unimplemented!\(\)|todo!\(\)' crates/flui-foundation/src crates/flui-tree/src
# SC9 — cycle-3 deletion regression check
for p in crates/flui-foundation/src/observer.rs \
         crates/flui-foundation/src/error.rs \
         crates/flui-tree/src/state.rs \
         crates/flui-tree/src/visitor \
         crates/flui-tree/src/diff.rs \
         crates/flui-tree/src/iter/cursor.rs \
         crates/flui-tree/src/iter/path.rs \
         crates/flui-tree/src/iter/breadth_first.rs \
         crates/flui-tree/src/iter/depth_first.rs \
         crates/flui-tree/src/traits/node.rs \
         crates/flui-tree/src/arity/storage.rs \
         crates/flui-tree/src/arity/arity_storage.rs \
         crates/flui-tree/src/arity/accessors.rs; do
  ! test -e "$p" || echo "REGRESSION: $p reappeared"
done
# SC10 — covered by SC7
# SC11
grep -l '2026-05-22-flui-foundation-tree-audit' \
  crates/flui-foundation/ARCHITECTURE.md \
  crates/flui-tree/ARCHITECTURE.md
# SC12 — per-task review-budget check
for sha in $(git log --oneline core-0a-foundation-parity-to-flutter --format='%h'); do
  lines=$(git diff --shortstat "$sha~1..$sha" | grep -oE '[0-9]+ insertions' | awk '{print $1}')
  [ "${lines:-0}" -le 400 ] || echo "OVER-BUDGET: $sha ($lines lines)"
done
```

**T-final.2 — Implicit-coverage findings inventory (per design §13)**

```bash
! test -e crates/flui-foundation/src/observer.rs                  # I-1
! test -e crates/flui-foundation/src/error.rs                      # I-2
! grep -rn 'FoundationError' crates/flui-foundation/src crates/flui-foundation/examples
grep -E '#\[non_exhaustive\]' crates/flui-foundation/src/diagnostics.rs \
  | grep -E '(DiagnosticLevel|DiagnosticsTreeStyle)'               # I-11
grep 'Box<str>' crates/flui-foundation/src/diagnostics.rs          # I-19
grep 'Box<str>' crates/flui-tree/src/error.rs                      # T-16
grep 'remove_cascade_is_stack_safe_on_deep_chain' crates/flui-tree/src/traits/write.rs  # PR #103 Codex P2 regression
```

**T-final.3 — Update parity-verification doc Conformance log appendix**

- **Files touched:** `docs/research/2026-05-25-foundation-parity-verification.md` (edit Conformance log appendix)
- **Content:** Fill in SC1–SC12 status checkboxes with `[x]` or `[!]`, append per-PR strict-TDD evidence rows (RED SHA, GREEN SHA, TRIANGULATE SHA if any, REFACTOR SHA if any) for every contingent PR that landed.

**T-final.4 — Emit verify pass/fail**

```bash
# If all of the above exited 0:
echo "VERIFY: core-0a-foundation-parity-to-flutter PASS"
# Else:
echo "VERIFY: FAIL — see captured outputs in apply-progress.md and Conformance log appendix"
```

#### Verification commands (T-final)

```bash
# All commands from T-final.1, T-final.2 in sequence.
# Plus:
just ci         # SC6 final gate
bash scripts/port-check.sh -v  # SC7 final gate
```

---

## Downstream consumer impact summary

| PR | Downstream crates touched | Mechanical fix size |
|---|---|---|
| PR1 | none (doc-only) | n/a |
| PR2 | none (doc-only) | n/a |
| PR3 | none (doc-only) | n/a |
| PR4 (contingent) | none (additive `pub fn`) | n/a |
| PR5 (contingent) | `flui-foundation::key`, `flui-view::key::{ObjectKey, GlobalKey}` (≥4 impls) | ~12-15 LOC across 4 files |
| PR6 (contingent) | internal `flui-foundation` + any in-tree caller of `KeyRef::new` (≥2 sites) | ~4 LOC |
| PR7 (contingent) | TBD by sweep | ≤400 LOC budget cap |
| PR8 (contingent) | TBD by sweep | ≤400 LOC budget cap |
| T-final | none | 0 LOC (script-only; updates Conformance log appendix in PR3's research doc) |

**No downstream crate is touched in default path** (PR1+PR2+PR3 only). Reverse-dependency graph (proposal §1.2 #7) is not perturbed by default-path PRs.

---

## Strict TDD evidence requirements (sdd-apply phase)

Per `openspec/config.yaml rules.apply.strict_tdd: true` and the task brief's explicit requirement: the sdd-apply phase MUST record, **per unit** in `apply-progress.md`:

1. **RED — failing test output.** Capture stdout/stderr from `cargo test --workspace` showing the new test failing. Record commit SHA of the RED commit (test-only). For doc-only PRs (PR1/PR2/PR3), this is **vacuously satisfied** (no production code → no RED required) and apply-progress.md SHOULD note "n/a — doc-only PR".
2. **GREEN — minimal implementation diff.** Capture `git diff <red-sha>..<green-sha> -- 'crates/**/*.rs'` showing the smallest possible production-code change that flips the RED test green. Record commit SHA of the GREEN commit.
3. **TRIANGULATE — additional test(s).** Capture the new test name(s) and stdout proving they pass. Record commit SHA. **Triangulating tests should exercise an adjacent invariant that the GREEN commit could trivially break** (per `multi-agent` skill philosophy of evidence-driven testing).
4. **REFACTOR — refactor diff (if any).** Capture `git diff <triangulate-sha>..<refactor-sha>` showing the duplication or naming cleanup. Record commit SHA. **No behavior change SHALL accompany the refactor commit.** All tests SHALL still pass.

**For each contingent code PR (PR4–PR8), apply-progress.md MUST contain a section per unit with these 4 evidence rows.** Default-path PRs (PR1, PR2, PR3) note "n/a — doc-only".

**Acceptance for SC5:** `git log --oneline` per code PR shows the RED→GREEN ordering (test commit before fix commit). The verify task asserts this for every contingent PR.

**Evidence template (for each contingent unit):**

```markdown
### Unit U<N>.<M> — <name>

| Stage | Commit SHA | Files | Output snippet |
|---|---|---|---|
| RED | `<sha>` | `crates/<crate>/tests/<file>.rs` (new) | `error[E…]: …` (truncated to 5 lines) |
| GREEN | `<sha>` | `crates/<crate>/src/<file>.rs` (edit) | `test result: ok. N passed; …` |
| TRIANGULATE | `<sha>` | `crates/<crate>/tests/<file>.rs` (append) | `test result: ok. N+1 passed; …` |
| REFACTOR | `<sha>` (or `n/a`) | `crates/<crate>/src/<file>.rs` | `test result: ok. N+1 passed; …` (no behavior change) |
```

**Engram-available evidence path (per design §8):** save to topic key `sdd/core-0a-foundation-parity-to-flutter/apply-progress` after each PR merge. **Engram-unavailable fallback:** mirror to `chain-runs/949e3e92/apply-progress.md` rolling file + Conformance log appendix in PR3's research doc.

---

## Verification commands (all PRs combined — final gate)

```bash
just ci                                                                          # SC6
bash scripts/port-check.sh -v                                                    # SC7 + SC10
test -f crates/flui-tree/ARCHITECTURE.md                                          # SC1
grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md  # = 5
grep -cE '^\|\s*`flui-tree`\s*\|\s*Templated' docs/PORT.md                       # = 1 (SC2)
ls docs/research/*-foundation-parity-verification.md                             # SC4
grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-foundation/ARCHITECTURE.md crates/flui-tree/ARCHITECTURE.md  # SC11
! grep -rEn 'unimplemented!\(\)|todo!\(\)' crates/flui-foundation/src crates/flui-tree/src  # SC8
# SC9 deletion regression — 13 paths
for p in crates/flui-foundation/src/observer.rs crates/flui-foundation/src/error.rs \
         crates/flui-tree/src/{state.rs,visitor,diff.rs,traits/node.rs} \
         crates/flui-tree/src/iter/{cursor.rs,path.rs,breadth_first.rs,depth_first.rs} \
         crates/flui-tree/src/arity/{storage.rs,arity_storage.rs,accessors.rs}; do
  ! test -e "$p" || { echo "REGRESSION: $p"; exit 1; }
done
# SC12 per-task line check
git log --oneline core-0a-foundation-parity-to-flutter --format='%h' \
  | xargs -I{} sh -c 'git diff --shortstat {}~1..{} | awk "{print \$4+\$6}" | xargs -I X test X -le 400'
```

---

## Delivery Decision Required

> **STOP — DO NOT RUN sdd-apply UNTIL THE SUPERVISOR HAS RESOLVED THIS SECTION.**

### Total review burden

- **Default path:** 3 doc PRs, total net ~970 added lines / ~20 deleted = **~950 net LOC**. Well under the 4,000-line session review budget (~24% consumed). All per-PR estimates ≤ 400 lines (PR1 is largest at ~450; mitigation noted in `## PR1`).
- **Worst-case contingent path:** 8 PRs (3 doc + 5 contingent code), total net ≤ ~2,970 LOC. Still under 4,000-line session budget (~74% consumed). Each contingent PR is independently TDD-gated and ≤400 LOC.
- **Strict TDD evidence:** vacuous for default path (zero code change); binding for any contingent code PR. Evidence template in `## Strict TDD evidence requirements` above.

### Recommended PR strategy

**Default — stacked-to-main chain in this order:**

1. **PR1** — Author `crates/flui-tree/ARCHITECTURE.md` (5 sections; ~450 LOC).
2. **PR2** — Amend `crates/flui-foundation/ARCHITECTURE.md` + flip `docs/PORT.md` Index (~100 net LOC).
3. **PR3** — Author parity-verification research doc (~350 LOC).
4. **T-final** — Verify task (no LOC change; runs SC1–SC12 and updates Conformance log appendix).

Each merged before the next stacked PR opens. After PR3 lands, T-final runs; if T-final passes, the change is complete.

**Contingent — instantiate only as triggered:**

- **PR4** — only if supervisor flips §4.6 I-7 verdict to `revisit-now`.
- **PR5** — only if supervisor flips §4.6 I-8 verdict to `revisit-now`.
- **PR6** — only if supervisor flips §4.6 I-21 verdict to `revisit-now`.
- **PR7** — only if PR3's parity sweep discovers a behaviour-bug divergence.
- **PR8** — only if PR3's parity sweep discovers a SECOND behaviour-bug divergence.

### Supervisor questions to resolve before sdd-apply

These are the **6 open questions from design §12** that gate the apply phase. Each has a documented default; if the supervisor does not adjudicate, defaults stand and sdd-apply proceeds with the default PR1→PR2→PR3 chain.

| # | Question | Default | Impact if flipped |
|---|---|---|---|
| Q1 | §4.2 depth-constant shape — two independent constants OR derive `INLINE` from `MAX`? | **two independent** (current `main` state) | If flipped: requires spec revision (`tree-depth-canonical/spec.md` R1), NOT a code change. Spec-step revision delay only. |
| Q2 | §4.6 deferred-13 verdict table — accept 6/7/0 split OR flip any to `revisit-now`? | **6 accept-permanent / 7 revisit-later / 0 revisit-now** | If 1+ flipped: instantiate PR4/PR5/PR6 accordingly. ≤3 contingent PRs; each ≤400 LOC; strict TDD binding. |
| Q3 | §4.5 ObserverList — keep deleted OR re-introduce as `unstable-observer-list` feature? | **keep deleted** | If re-introduce: large contingent PR (~600 LOC port + feature-gate scaffolding) — exceeds 400 LOC review budget; chunking required. |
| Q4 | §3 peer-review — accept skip OR require post-hoc Codex/Gemini validation? | **accept skip** (decisions landed cycle-3) | If override: invoke `multi-agent` skill broadcast at sdd-apply on §4.1 TreeWrite contract + §4.5 ObserverList; post-hoc validation only (no code change). |
| Q5 | §5 PR ordering — 3 PRs OR collapse PR1+PR2? | **3 PRs** (cleanest review chunking) | If collapse: single ~580 LOC PR — still under 600; lose PR1's standalone-doc theme. |
| Q6 | §7 port-check.sh — skip FR-037 MAX_DEPTH regex OR add it? | **skip** (spec scenario covers) | If add: 1 contingent rule addition to `scripts/port-check.sh` (~20 LOC); strict-TDD applies (test-then-rule). |

### Other delivery considerations

- **Doc dates:** PR3 doc filename uses `2026-05-25` placeholder; substitute commit-day's actual ISO date at PR3 author time. T-final asserts the file exists with a date string matching the change-merge commit day or earlier.
- **Engram availability at apply:** unknown until sdd-apply runs. Both paths documented in design §8 and `## Strict TDD evidence requirements`.
- **Friction items surfaced during PR1 authoring (RD4):** PR1 `## Friction log` MAY surface a code-now item; that flips to a contingent PR and surfaces back to supervisor for delivery decision before PR1 merges.
- **Behaviour-bug divergence surfaced during PR3 authoring (RD3):** PR3 `## Discovered divergences` appendix flags it; that flips to PR7/PR8 and surfaces back to supervisor for delivery decision before PR3 merges.

### Confirmation required

**Please respond with one of:**

1. **"Proceed with defaults"** → sdd-apply executes PR1→PR2→PR3→T-final as written above. No contingent PRs unless RD3/RD4 fires during authoring.
2. **"Proceed with adjustments: <Q1=…, Q2=…, …>"** → sdd-apply executes the adjusted plan. Contingent PRs instantiated per adjusted verdicts.
3. **"Reject — re-scope at sdd-plan"** → tasks.md is re-issued; do NOT run sdd-apply.

Until one of these is received, **sdd-apply does NOT run**. This is the natural stop point for the sdd-plan chain.

---

## References

- `openspec/changes/core-0a-foundation-parity-to-flutter/proposal.md` — §0 scope-drift, §2.1–§2.4, §3.1–§3.2, §7 SC1–SC12.
- `openspec/changes/core-0a-foundation-parity-to-flutter/design.md` — §4.1–§4.6, §5 PR plan, §6 breaking changes, §7 port-check.sh, §8 Engram protocol, §9 parity row template + 14-row table, §10 SC mapping, §11 RD1–RD9, §12 open questions, §13 tasks notes.
- `openspec/changes/core-0a-foundation-parity-to-flutter/specs/{tree-architecture-md,tree-treewrite-contract,tree-depth-canonical,tree-surface-reduction,foundation-binding,foundation-id-system,foundation-key,foundation-listenable-changenotifier}/spec.md` — 8 domain specs with 74 RFC 2119 requirements.
- `openspec/config.yaml` — `rules.apply.strict_tdd: true`, `rules.apply.test_command: cargo test --workspace`, `rules.verify.test_command: just ci`, `rules.tasks.protect_review_workload: true`.
- `docs/research/2026-05-22-flui-foundation-tree-audit.md` — 47-finding cycle-3 audit (Status + Deferred tables at lines 2200–2256).
- `docs/PORT.md` — port methodology + per-crate ARCHITECTURE.md template (`:756+`) + Index (`:790+`).
- `scripts/port-check.sh` — 13-trigger refusal gate (#8–#13 installed PR #151 at lines 396–1005).
- Predecessor envelopes in chain run `949e3e92`: `init.md` → `proposal.md` → `spec.md` → `design.md` → (this) `tasks.md`.

---

*End of tasks.md. Chain pauses here for supervisor delivery decision. If sdd-apply is greenlit, it executes PR1→PR2→PR3→T-final with strict-TDD evidence captured per unit per the template above.*
