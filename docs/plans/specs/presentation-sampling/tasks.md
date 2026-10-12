# Presentation sampling implementation tasks

**Status:** Approved by the maintainer on 2026-10-11. Implementation and local acceptance completed on 2026-10-12. Publication, CI and merge status are tracked in [#1552](https://github.com/vanyastaff/flui/issues/1552).

**Requirements:** https://github.com/vanyastaff/flui/issues/1552.

## Task graph

The graph has independent producer work after baseline acceptance: animation provenance (2) and interaction/deadline authority (3) can progress separately. Safe scheduling (4) depends on 3; runtime integration (5–7) depends on 2–4. Consumer/decision migration (8) depends on the integrated contracts; counterfactual checks (9) and review/PR (10) depend on all implementation and migration work. No task is completed by this document alone.

## Ordered delivery

| Order | Work | Required acceptance and dependency |
|---|---|---|
| 1 | Reconcile approved spec against fresh main and establish public acceptance family | Keep existing sibling-sampling RED. Add decisive cases for new publication/provenance/report/retry promises; baseline preservation rows are passing controls. No production fix before its public missing-behavior proof |
| 2 | Add origin and cohort-integrity transfer at real animation source/commit/delivery seams | Computation, postcommit delivery, both mixed orders and healthy commit before failing source. Preserve first payload, current non-finite policy and standalone propagation |
| 3 | Add interaction provenance and deadline source authority | Built-in committed callbacks, open unknown poll/query, pointer-ID reuse, reentry, closing/removal and live delivery tail. Narrow deadline updates have actual built-in/runtime-extension consumers |
| 4 | Make scheduling observation guard-free and bounded | Snapshots after relevant owner frontiers; earlier deadline publication/cancel/stale update refusal. Real owner/native fold reads no foreign queries/tracing/Drop under guards. Pure query has one 100 ms re-probe then explicit-update debt; unknown poll never replays same revision; healthy earlier deadline progresses |
| 5 | Integrate one presentation sampling operation and one admission decision | All sampling before any build, selected/frozen tick, same-presentation independent deadlines, healthy sibling build/completion, no double FrameClock poll, hidden/no-attempt reports without dropped streak |
| 6 | Integrate atomic retained withdrawal and retry | Old receipt settle false, withdrawn geometry, fresh layout runnable, readiness uncommitted, replacement settle/completion. One-shot failure recovers without new external input; mark-rendered cannot clear debt |
| 7 | Add addressed sampling/operation reports and safe delivery timing | First origin plus blocking evidence, one aggregate sampling report, one dropped-attempt increment, exact redaction, reports before second reconciliation/build. Queue outside-frame reports until guards release; diagnostics competition preserves cause and progress |
| 8 | Migrate actual consumers and lasting documentation | Runtime, owner/host scheduling folds, built-ins/extensions, public reporting matches, SDK pins/examples/doctests. New ADR partially supersedes0048; source contract and architecture agree. No private local audit paths enter repository |
| 9 | Counterfactual proof and scoped final checks | Revert relevant producer hunk in isolation for each new behavior; record intended RED. Targeted tables first, one final scoped gate after final edits; optional/wasm/native compiler paths and execution limits stated |
| 10 | Review the full path and publish one implementation PR | Review source/consumer/ownership/recovery, not only diff. Verify every task, remove obsolete integration paths, add changelog for visible behavior/API. No merge authorization is implied |

## Definition of Done

Every new behavior has producer-level red/green evidence; migration reaches production callers; no callback/guard/debt is unaccounted at a failure boundary; compatible retained work and standalone semantics remain. A subsequent valid operation progresses after single and competing faults. Native execution/support is neither inferred from headless cases nor silently removed from the verification inventory.

ADR hygiene has its own approval/implementation package. Native protocol, composition, identity, MIME, accessibility and CLI/tooling backlog remains separate; it is not declared closed by sampling merge.

## Completion record

Implementation and consumer migration (orders 1–8) are complete. The lasting
decision is [ADR-0199](../../../adr/ADR-0199-presentation-sampling-recovery.md),
with producer/runtime contracts in the corresponding module and architecture
documentation. Public acceptance covers first cause versus publication blocker,
healthy sampling/build/completion, no-attempt reports, bounded deadline recovery,
retained authority withdrawal/replacement, numeric-answer release and fresh
operation progress after single and competing failures.

Counterfactual checks (order 9) reproduced the intended failures when producer
classification/receipt chronology, accepted font-tail delivery, retained report
revalidation or closed numeric-cohort retirement were reverted. Original source
bytes were restored before verification. Ordinary withdrawal and fresh-attempt
controls remained valid; compile-only fixture corrections are not counted as
behavioral RED evidence.

The public owner-dispatch row
`query_retry_participates_in_the_pending_gate_and_then_settles` also reproduced
an overdue recovered deadline receiving zero polls instead of one in its due
owner opportunity. Armed query retries now contribute to both cached wake timing
and pending admission through the same observation mapping. The 99 ms control
does not query early; a second query failure parks without repeated delivery,
reporting or wake debt. Restoring the old pending predicate reproduced the same
zero-poll failure, and exact source bytes were restored afterwards. This is
synthetic owner-dispatch acceptance, not a live native event-loop run.

Final local verification used source commit
`8e446cb612e5b5c5608f9bd1bd24550697d96883` and completed
`cargo xtask check-changed --base e419a04d36bb821be796d763150da500d72c33f5`:

- scoped and engine/testing strict clippy passed;
- 853 nextest tests passed, with 19 configured skips;
- strict rustdoc and doctests passed, including the executable custom timer
  cancellation/retired-authority example;
- Windows native-feature clippy, wasm32 clippy and the facade without default
  features passed;
- both 78-case feature clippy passes completed, covering production and
  tests/benches/examples;
- the platform compiler-guard family passed.

MacOS/iOS and Android source checks require SDK/toolchains unavailable on this
host; the Linux native platform suite requires `xvfb-run`. CI owns those paths.
Physical pen/touch, browser IME and native compositor/mobile execution were not
performed. Existing ignored doctests and configured skips remain explicit
verification limits.

Independent Standards and Spec reviews (order 10) have zero unresolved findings,
including full source/consumer paths, reentry, retirement and recovery. PR
publication, exact-head CI and merge are tracked through the linked issue; this
record does not authorize a merge or close other architecture scopes.
