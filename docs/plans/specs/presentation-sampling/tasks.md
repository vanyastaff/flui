# Presentation sampling implementation tasks

**Status:** Approved by the maintainer on 2026-10-11; implementation in progress. All acceptance items remain open until their observable evidence is recorded.

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
