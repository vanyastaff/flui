# ADR hygiene: current authority, scoped consolidation and obsolete records

**Status:** Approved by the maintainer on 2026-10-11. Implementation and available local validation completed on 2026-10-12; PR publication and CI acceptance remain pending. Conditional deletions and unresolved semantic policies remain excluded.

**Issue:** [#1553](https://github.com/vanyastaff/flui/issues/1553). **Date:** 2026-10-11. **Evidence baseline:** `6af2cf426809dbf8a33bbb415e904f0bfdc847c4`; all 160 existing ADR records reviewed.

## Problem Statement

Contributor должен понимать текущие границы FLUI без восстановления истории десятков изменений. Сейчас часть ADR сохраняет старые типы, implementation status, consumer claims и test references. Частичная supersession иногда соседствует с present-tense старым правилом; self-Absorbs metadata не имеет корректного предшественника. Отдельные полностью заменённые records всё ещё используются как текущая authority.

Простая массовая уборка опасна: Accepted не означает исполнение всех native checks, Proposed не становится Accepted после merge supporting code, а Superseded-by может относиться только к одному разделу. Неудачное объединение может потерять живые правила или скрыть незавершённые обязанности.

## Solution

Подготовить ограниченный documentation package с четырьмя действиями: исправить фактическое описание, сохранить/уточнить scoped authority, объединить доказанно общий контракт и удалить только полностью заменённый record после миграции всех ссылок и surviving clauses. Ни одно исправление не меняет production behavior скрыто.

Единица объединения — одно архитектурное решение с общими rationale и consequences. ADR-0125, ADR-0179 и ADR-0191 сохраняются раздельно: identity, controller-seat lifetime и topology могут меняться независимо. Их текущая связь описывается одной navigation map в существующей architecture documentation. Доказанное поглощение применяется к устаревшим font-stack records: живые clauses переходят в Accepted ADR-0092, после чего лишённые собственной authority records могут быть удалены.

## User Stories

1. As a contributor, I want an ADR to describe current ownership and types, so that I do not implement a removed API.
2. As a contributor, I want implementation status separated from owner approval and native evidence, so that a merged proposal is not mistaken for a released contract.
3. As a contributor, I want partial supersession to identify surviving clauses, so that I can follow the current authority without guessing.
4. As a contributor, I want completed blur/motion/clock migrations marked accurately, so that I do not repeat implemented work.
5. As a maintainer, I want unfinished accepted composition and delivery obligations retained, so that cleanup cannot close them accidentally.
6. As a contributor, I want canonical runtime/presentation/host vocabulary, so that names communicate actual responsibilities.
7. As a contributor, I want source/test references to resolve, so that contracts remain verifiable.
8. As a maintainer, I want actual predecessor history for self-Absorbs metadata, so that cleanup does not invent a decision lineage.
9. As an animation contributor, I want one navigation map linking registry identity, driven ownership and tree admission, so that I can find their interaction without confusing distinct decisions.
10. As an animation contributor, I want permanent exhaustion, stale tokens, retirement and parent transfer preserved during cleanup, so that shorter prose does not mean weaker rules.
11. As a maintainer, I want every old clause mapped before absorbing a superseded record, so that a summary cannot silently drop an exception or test obligation.
12. As a contributor, I want distinct native/portable/terminal recovery contracts kept distinct, so that one large ADR does not become a competing architecture manual.
13. As a contributor, I want obsolete font-stack records retired with current references migrated, so that current code no longer points at cosmic-text/global FontSystem.
14. As a researcher, I want historical evidence preserved through immutable links, so that retirement does not rewrite the experiment's conclusions.
15. As a maintainer, I want semantic contradictions handled explicitly, so that describing existing code does not ratify an accidental behavior change.
16. As a contributor, I want Proposed platform/capability/raster work still visible, so that documentation does not falsely claim implementation completeness.
17. As an application author, I want native evidence limits preserved, so that docs-only cleanup cannot imply new platform support.
18. As a maintainer, I want the remaining behavior backlog kept separate and actionable, so that ADR hygiene does not hide unfixed bugs.

## Implementation Decisions

### Factual reconciliation

- Correct already implemented blur commit (0030), TIME_DILATION/FONT_SYSTEM removal status (0097), motion projection integration (0174), typed animation error vocabulary (0186/0198) and removed Ticker/RunDelivery descriptions.
- Update obsolete precision, Arc/RwLock/thread-safety, raster configuration/production-consumer, runtime location, shortcut-resolution and named-route-refusal explanations. Existing approved replacement scope remains explicit.
- Replace absent current test/source references; keep historical experiments clearly dated. Supporting architecture docs and source comments join the same migration if they teach the stale rule. Comment-only Rust changes use their actual scope gates.
- Fix self-Absorbs in 0026/0027/0058. Verified history links retired 0022/0002/0029 respectively to the consolidating records; use explicit retired metadata and immutable history only where useful. Do not create missing live predecessors or renumber ADRs.
- Status has three distinct facts: acceptance scope, implementation, executed evidence. Preserve all 19 Proposed records and every qualified Accepted/Accepted-in-part declaration unless owner approval evidence supports a change.

### Decision boundaries and animation registry navigation

| Source | Independent decision and active rules |
|---|---|
| ADR-0125 surviving clauses | Backend/slot identity, weak removal authority, stale-token refusal, permanent exhaustion and existing exception-retirement limits |
| ADR-0179 | Driven owner alone owns controller registration lifetime; no restored public manual admission/removal path; weak wake/seat authority and consumer ownership |
| ADR-0191 | Single-parent acyclic registry topology, admission preflight, parent/root authority transfer, withdrawal, mute ancestry and healthy walk behavior |

- Keep all three records. Token identity/capacity allocation can change without changing controller ownership or topology; caller-managed controller admission can change without changing token/tree rules; shared-child topology can change without changing driven ownership. A common subsystem therefore does not make them one decision.
- Add a compact current-authority map, scoped backlinks and clause references through existing architecture documentation. Preserve every active clause, exception and cited test, including partial inherited replacements and independent first-failure/notification boundaries. Shorten repeated identity prose in0179 to a scoped0125 reference only when no unique rule is lost.
- No new omnibus registry ADR, registration algorithm, public type or deletion of0125/0179/0191. Alternatives considered: one combined record, three records with cross-links, or a duplicate architecture manual. Choose distinct decision records plus one navigation map.
- Any future consolidation must pass the same test: one decision, common alternatives/rationale/consequences, every live clause mapped and no independently reversible choice hidden inside it. Missing clause mapping prevents retirement; smaller file count is not acceptance evidence.

### Amendments and historical policy

- Use concise Context, Decision, Status and Consequences; record alternatives and negative consequences when they explain the choice. Acceptance, implementation and execution evidence remain separate facts.
- Factual type/path/status corrections identify the affected decision and dated amendment explicitly; do not replace its rationale with a present-tense description of code. New or reversed normative decisions get a new ADR with precise Supersedes/Superseded-by scope. A README or architecture map carries navigation, not a second competing source of normative rules.
- [ADR guidance](https://github.com/architecture-decision-record/architecture-decision-record#suggestions-for-writing-good-adrs) recommends one decision per record and explicit amendments/supersession; its [team guidance](https://github.com/architecture-decision-record/architecture-decision-record#teamwork-advice-for-adrs) also permits dated living updates. This informs the cleanup without importing another project's tooling or conventions.
- [Nygard's original guidance](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) retains reversed records marked Superseded. FLUI deliberately follows its existing repository policy instead: delete a record only when its decision no longer exists and no independent active clause remains. Preserve immutable history links, prior rationale and predecessor identity before deleting; never reuse retired numbers. This is a project-specific choice, not a universal ADR standard.

### Other families and explicit exclusions

| Family | Decision for this package |
|---|---|
| 0176 + 0195 motion timeline/policy | Improve shared architecture navigation; do not merge whole records because 0176 also owns sampling, velocity, retarget, typed values and protocol |
| 0192 loading motion | Keep independent visible-feedback/accessibility consumer decision |
| 0119 + 0127 retention | Candidate for a later general-retirement record after operation extent is resolved; do not erase that unresolved policy now |
| 0178 notification custody | Keep its distinct delivery/reentry boundary and link common retention rules |
| 0030 / 0069 / 0090 / 0135 / 0142 text/input | Keep portable vocabulary, native adapter, lock/commit/lifecycle boundaries and unfinished requirements distinct; improve current-authority navigation |
| ADR-0102 | Keep: 0103 supersedes only direct-primitive capability admission; expression/effect/damage/resource decisions still live |

### Obsolete record retirement

| Record | Required surviving authority | Proposed action |
|---|---|---|
| ADR-0016 | Accepted ADR-0092 font/shaping/registration ownership | Retire after clause/reference migration; removed global FontSystem is not current architecture |
| ADR-0059 | ADR-0092 family-resolution rule | Retire jointly with text-stack cleanup; cosmic-text choice is obsolete, family-resolution rationale is retained |
| ADR-0077 | ADR-0092 plus immutable spike evidence | Retire the superseded spike record; preserve measured historical evidence and remove only the dead exemption |
| ADR-0057 / ADR-0099 | 0102/0103 scoped surviving clauses | Conditional candidates only. Their successor acceptance ledger is unresolved; this package does not authorize deletion |

- Enumerate all incoming tracked references, including source comments, current specs, changelog, design and archives. Migrate normative references to current authority; retain historical meaning with immutable links or explicitly historical wording.
- Do not hand-edit Cargo.lock or CI workflows. A dead documentation-path exemption may be removed with the record that required it; no new exemptions or grep gate.
- External backlinks cannot be exhaustively enumerated. Git history remains available; archival evidence uses immutable URLs. Do not invent a second tombstone/archive policy against repository rules.

### Semantic discrepancies are separate explicit decisions

- ADR-0155 no-velocity wording conflicts with the approved derived-velocity physics contract. Record a narrow reconciliation/amendment grounded in that approval; do not silently change normative wording or physics.
- ADR-0135 BUG-only poisoning differs from current all-escaping-panic behavior. Preserve source until policy is explicitly confirmed and recorded; factual cleanup alone does not approve either interpretation.
- Literal one-event-per-key wording in0069 needs a scoped clarification of one text-edit authority versus keyboard/composition observations, without suppressing native commands or adopting DOM cardinality everywhere.
- Presentation sampling gets its own new partial supersession of0048. ADR0142's outstanding commit exits/anchor debt, AssetKey admission, directional action fallback, identity exhaustion, MIME, native input and CLI/tooling fixes remain behavior work with RED first.

## Testing Decisions

- Verify the draft and implementation against the exact starting commit, then reconcile only intervening relevant changes before editing. Acceptance is accurate authority, preserved clauses and resolvable references.
- For each factual edit record the old statement, current producer/consumer or approved replacement and resulting text. A code-shaped assertion in docs is checked against code; no test is added merely for a wording change.
- Before each absorption/deletion, produce complete clause mapping and incoming-reference inventory. Check source and architecture test-name references before changing any name. Every surviving reference to a retired ID is explicitly historical; active independent ADRs retain their current authority. No broken filesystem link remains.
- Review registry navigation and each retained record against identity/exhaustion, driven admission, parent transfer, reentry/retirement and public consumer contracts. The map introduces no competing normative rule; behavior and test implementation remain unchanged.
- Use existing docs-links/docs-paths/marker/workspace checks. If source comments or included docs change, use actual changed-scope validation. Do not classify included source as DOCS_ONLY.
- No new native, performance or W3C-conformance claim follows from passing doc checks. Semantic findings that require source changes return to their own specification and public regression.

## Out of Scope

- Mass renumbering, global filename renaming, automatic acceptance of Proposed records, a new ADR micro-framework or merging all recovery into one large document.
- Implementation of native TSF/key/IME, composition exits, identity, MIME, accessibility, CLI, persistence/capabilities or threaded raster decisions.
- Removing incomplete accepted tasks, changing historical measurements to current claims, or deleting a partially superseded record with surviving clauses.
- Treating merged code, a green gate or file-count reduction as approval/correctness proof.

## Further Notes

The maintainer approved factual scope, independent registry decisions with shared navigation and the bounded font-record absorption/retirement group on 2026-10-11. Conditional candidates and unresolved semantic policies remain visible for separate approval. This companion spec does not broaden presentation-sampling implementation scope.

Repository ADR policy requires explicit supersession and retirement only when the old decision has no remaining independent authority. Baseline clause/reference evidence is preserved privately; public changes contain durable rules, not audit/session notes or local paths.

Related specification: [Presentation sampling failure and recovery](https://github.com/vanyastaff/flui/issues/1552). The two implementation scopes remain separate.
