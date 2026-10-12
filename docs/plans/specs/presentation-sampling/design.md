# Presentation sampling design proposal

**Status:** Approved by the maintainer on 2026-10-11; implementation in progress.

**Requirements:** https://github.com/vanyastaff/flui/issues/1552.

## One owner operation

`flui-runtime` owns one presentation sampling operation. It encapsulates retained-premise reconciliation, tick selection, animation and input stages, origin accumulation, continuation demand and frame admission. Higher callers receive the resulting usable/held/rejected/no-attempt outcome; they do not manually choose catch order or first-failure disposal.

Animation and interaction producers stamp provenance at real foreign-call and commit boundaries. The operation carries one first-failure receipt and separately accumulates cohort integrity. A later blocking fault can reject publication without replacing the first origin/payload. Already committed healthy samples are retained.

No second panic framework or global sampling state. The existing custody accounts for payloads, nested callbacks, wakers and retirement. New cross-crate integration is wired to the runtime; standalone calls preserve propagation. Runtime reporting receives plain typed metadata plus owned first-failure custody, not a mutable view into producer state.

## Ordering

1. Select the pump's host time once.
2. For each live presentation reconcile retained premises and select fresh or frozen motion tick.
3. Attempt animation and independent input/deadline work with stage-level provenance and live authority revalidation.
4. After all sampling, decide fresh attempt admission once; retained work retains its identity. Compute report disposition from participation and complete cohort integrity.
5. Commit rejection/withdrawal and retry/report obligations, then deliver sampling diagnostics without owner guards.
6. Reconcile again after sampling/diagnostic side effects; process healthy admitted segments using their chosen tick, without double polling FrameClock.
7. Finish ordinary terminal outcomes and re-arm remaining obligations after mark-rendered. Refresh the owner scheduling snapshot before returning its lease; queued operation diagnostics wait for guard-free delivery.

Sampling reports have their own documented pre-build boundary. Existing lifecycle/segment reports keep current timing. A later segment error does not duplicate the earlier sampling cause; a segment is not entered after a rejected sampling attempt.

## Failure table

| First observed cause | Later evidence | Participation | Result |
|---|---|---|---|
| Delivery after valid commit | All required proposals proven usable | Admitted | Contained, normal segment permitted |
| Delivery after valid commit | Later computation or unknown fault | Admitted | FrameDropped once; first delivery cause preserved, blocker identified |
| Computation before valid commit | Any later delivery | Admitted | FrameDropped once; original cause preserved |
| Unknown open-extension state | No proof restoring commit integrity | Admitted/retained | Rejected attempt; previous presented output retained |
| Any operation fault | No fresh or retained admitted attempt | Hidden/paced/no-attempt | Contained operation report; debt retained, no invented streak increment |
| Reentry supersedes proposal | No actual fault | Any | Existing cancellation/supersession rules, no false failure |

Handled non-finite source policy is preserved. An arbitrary user source is not reclassified by inspecting its final value after a callback.

## Retained text

Delivery-only failure preserves a compatible numerical frontier and frozen tick. Invalidation withdraws it atomically: old receipt seal rejected, old geometry withdrawn, fresh layout runnable and readiness uncommitted until replacement. Old native work may physically finish, but settlement cannot authorize the new attempt. Retained callbacks become eligible only after valid replacement completion.

## Scheduling observation and recovery

Host getters use owner snapshots because their callers can hold registry/AppRuntime borrows. Foreign deadline queries, tracing, callbacks and user destruction occur during guard-free owner execution. Snapshot updates follow every owner activity capable of replacing or changing sources.

Runtime extensions publish deadline changes through narrow identity-bound update authority. This removes a hidden caller duty to trigger a second wake manually. Silent external state changes have no supported scheduling guarantee. Built-ins and real runtime consumers migrate with the integration; independent live-query use remains available.

Query or untrusted poll failure excludes only that source's stale contribution and retains its unresolved obligation. A pure query gets one automatic re-probe after 100 ms; a second fault parks debt until explicit source update. Unknown poll never automatically replays the same contact/deadline revision, because user side effects may already have occurred. New revision/rearm or retirement resolves its eligibility without claiming old completion. Healthy earlier deadlines execute; reads do not retry, report or move due time. Closed/stale/exhausted authority cannot operate on a successor. Suspended OS execution and terminal-clock limits remain explicit.

## Alternatives

An outer catch alone preserves no computation/delivery provenance and cannot handle mixed faults correctly. A catch that resumes after healthy sampling still escapes the owner loop and leaves publication unresolved. Blindly dropping all listener errors loses valid output. A new protocol for all gesture algorithms is larger than the required deadline-update and unknown-consistency boundary. The proposed typed owner operation preserves existing successful paths and makes the ambiguous case explicit.

Precise Rust item names and visibility are implementation choices within these contracts. Public diagnostic additions are non-exhaustive and must have production consumers; concrete DTO compatibility, SDK exports and internal exhaustive matches are checked before final integration.
