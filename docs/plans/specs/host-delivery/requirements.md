# App, runtime and scheduler delivery contracts

- **Status:** Approved specification; implementation awaiting integration review.
- **Date:** 2026-10-11
- **Tracker:** [#1546](https://github.com/vanyastaff/flui/issues/1546)
- **Design:** [design.md](design.md)
- **Tasks:** [tasks.md](tasks.md)

## Problem Statement

Application authors need predictable ownership and delivery when several windows
and UI runtimes share a host. A window must wake itself, accepted work must survive
reentry and recoverable failures, and an operation's result must describe what
actually completed. Contributors need these guarantees to be explicit before the
pre-1.0 API becomes a lasting public contract.

The reviewed baseline misdirected runtime wakes, replaced sibling deadline
sources, invoked service factories under an app-storage borrow, lost lifecycle
notifications after panic or reentry, collected unbounded timing data without a
subscriber, withheld typed callback failure reports, and returned secondary-window
admission as though installation had completed. The scope includes the ownership,
recovery and consumer paths around these defects, rather than only their local
diffs.

## Solution

Preserve runtime and presentation ownership throughout host delivery. Separate
admission from user execution and completed installation. Serialize notifications,
preserve the first failure and the healthy delivery tail, and collect bounded
telemetry only when somebody consumes it. Keep runtime and scheduler independent
of native platform and GPU ownership. Document and migrate the intentionally
changed pre-1.0 interfaces.

## User Stories

1. As an application author, I want a runtime wake to address its own window, so that opening another window cannot redirect updates.
2. As an application author, I want each runtime to retain its redraw obligation, so that rendering a sibling cannot consume accepted work.
3. As an application author, I want infrastructure notifications to reach their runtime consumers, so that shared font or reload changes still propagate.
4. As an application author, I want all live presentations to contribute deadlines, so that installation order cannot starve an earlier window.
5. As an application author, I want deadlines to remain visible during driver execution, so that temporarily leased state does not disable progress.
6. As an application author, I want closing one window to remove only its deadlines, so that siblings continue making progress.
7. As an application author, I want hidden windows to stop visual work without losing accepted work, so that resuming can finish it.
8. As a platform implementer, I want native text sizing to retain its independent progress contract, so that disabled visual frames do not erase its deadline.
9. As a service author, I want my factory to call window APIs, so that legitimate reentry does not become a bootstrap panic.
10. As a service author, I want executor admission and refused-future destruction to occur outside app borrows, so that custom executors and destructors can reenter safely.
11. As an application author, I want owner replacement during service construction to refuse obsolete work, so that a stale service never runs on a new owner.
12. As an application author, I want service state committed before executor invocation, so that synchronous execution sees an admitted service.
13. As a lifecycle observer, I want committed transitions delivered in order, so that reentry cannot leave me observing an obsolete final state.
14. As a lifecycle observer, I want another observer's panic to preserve my notification, so that one failure does not silently skip healthy listeners.
15. As an observer owner, I want removal before my delivery turn to prevent invocation, so that retired subscriptions do not receive callbacks.
16. As a scheduler user, I want the original observer failure preserved across nested execution, so that cleanup cannot replace it or run opaque destructors under failed custody.
17. As a scheduler user, I want owner replacement during notification to inherit active failure custody, so that replacement does not reopen unsafe retirement.
18. As a scheduler user, I want the next healthy operation to make progress, so that containing a failure does not permanently disable cancellation or notifications.
19. As an application author, I want timing collection disabled without subscribers, so that ordinary frame activity does not build an unused backlog.
20. As a telemetry consumer, I want bounded timing retention and automatic delivery, so that I do not have to maintain a private drain protocol.
21. As a telemetry consumer, I want reentrant and failing callbacks to preserve the healthy tail, so that telemetry follows the same recovery rules as lifecycle delivery.
22. As a runtime consumer, I want typed contained callback reports at the affected address, so that I can diagnose failures without parsing log strings.
23. As a runtime consumer, I want those reports to preserve frame-failure streaks and the original failure, so that diagnostics do not alter recovery policy.
24. As an application author, I want a secondary-window installation receipt, so that native creation is not mistaken for initialized content.
25. As an application author, I want renderer, mounting and installer failures returned through that receipt, so that deferred failure is observable.
26. As an application author, I want early closure or owner replacement to settle an admitted request as cancelled, so that admission refusal and lost ownership remain distinguishable.
27. As an application author, I want dropping a receipt observer to leave accepted installation intact, so that observation does not implicitly own cancellation.
28. As an application author, I want shutdown to settle every pending request even if a receiver waker panics, so that one receiver cannot strand another window.
29. As an example reader, I want opening state to follow completed installation, so that the example demonstrates the actual API contract.
30. As an application author, I want unsupported configuration refused explicitly, so that an accepted-looking option cannot silently do nothing.
31. As a contributor, I want canonical names and accurate owner-affinity documentation, so that the API communicates responsibilities without aliases or hidden thread-safety assumptions.
32. As a maintainer, I want regression tests that fail on the broken producer, so that a passing gate is evidence of behavior rather than a restatement of implementation.

## Implementation Decisions

- Runtime wake authority carries independent redraw debt and a weak reference to
  its exact native window. Loop-wide infrastructure wake remains a distinct
  responsibility. No native handle enters `UiRuntime` or the scheduler.
- Install one deadline hook per host owner, fenced by typed owner identity. Fold
  logical runtime deadlines, native text-sizing deadlines and each live desktop
  presentation's recovery, fallback and first-reveal deadlines. Keep presentation
  metadata available while the driver is leased. Visual deadlines obey runtime
  frame eligibility; native text sizing retains its separate progress policy.
- Service startup is prepare, construct, commit, then execute. Storage is borrowed
  only for preparation and commit. Commit checks the original owner generation,
  current admission and root cancellation, then registers completion state before
  calling the executor. Refused futures retire outside storage borrows.
- Lifecycle delivery is an owner-local FIFO. Commit lifecycle state and frame
  eligibility before calling observers. Nested transitions append rather than
  recursively overtaking current delivery. Removed-before-turn listeners are
  skipped; all remaining healthy listeners are attempted before propagating the
  first failure.
- Observer delivery participates in scheduler failure custody inside and outside
  frames. Nested owner execution inherits the current receipt and restores the
  enclosing binding. Replacement execution owners inherit active custody. Callback
  stores, task cancellation, post-frame work and wake delivery preserve that receipt;
  opaque captures are not destroyed while the first failure is authoritative.
  Existing containment limits for destructors that abort before a boundary remain.
- Timing collection requires a live subscriber. Retain at most the newest 128
  records. Automatically report at completed idle boundaries after 100 ms in debug
  or one second in release, or when a batch becomes full. Drain before invoking
  callbacks. Removing the final subscriber clears backlog before retiring its
  capture. Explicit reporting obeys the same failure and reentry contract.
- A contained owner callback panic emits `CallbackPanic` with `Contained`
  disposition at its presentation address before the runtime lease returns.
  Foreign runtime addresses are refused. Reporting does not increment frame
  failure streaks, replace the original failure, or invoke handlers under owner
  storage borrows. Initialization, runtime work and stopping runtimes use this path.
- `WindowRequest` observes main or secondary window installation. The immediate
  `open_window` result describes admission and native creation; awaiting the
  receipt describes publication and initialization. Success promises installed
  content, not compositor presentation or native focus. Dropping the observer does
  not cancel accepted installation. Immediate refusal is `AdmissionClosed`;
  post-admission owner loss is `Cancelled`.
- Shutdown separately contains native closing, receipt delivery and reservation
  settlement for every pending request. A receiver waker panic cannot erase the
  accepted tail or reinterpret completed installation as failure. Retire outgoing
  ownership outside app borrows under existing failure policy.
- The multi-window consumer uses a lifecycle-acquired async driver, retains its
  task token, awaits installation and sets opened state only after success.
- Initialize managed startup diagnostics before platform construction. Remove
  unwired fullscreen and debug-paint configuration. Refuse configured storage
  directories with a typed unsupported-configuration error until persistence host
  integration supplies their implementation.
- Use `phase` and `is_frame_scheduled` as the canonical scheduler vocabulary.
  Describe the scheduler as owner-affine and its frame waker as the cross-thread
  capability. Migrate callers, examples and active documentation together.
- Preserve the existing crate layers and record the cross-crate ownership contract
  in ADR-0185, consistent with ADR-0027, ADR-0039, ADR-0175 and ADR-0182. Deliberate
  pre-1.0 API changes receive a changelog entry and a consumer migration.

## Testing Decisions

- Assert external behavior at the highest existing usable seam. Prefer a family
  of table-driven scenarios over separate binaries. Never assert private field
  layouts, helper names or a duplicate implementation of the admission predicate.
- Three boundary families are sufficient: application host/window installation,
  public scheduler execution/observation, and public runtime owner delivery. A
  single public app harness cannot inject renderer failure or a leased native
  driver today. Those scenarios use existing private app seams; do not introduce
  test-only public APIs.
- Existing prior art is `host_delivery_contract`,
  `installed_frame_driver_contract`, `secondary_installation_contract`,
  `observer_delivery_contract` and `owner_delivery_contract`. Extend these tables
  and retain their recovery scenarios rather than duplicate their machinery.
- Host acceptance: install A then B; waking A reaches A; rendering B leaves A's
  debt pending. Deadlines include both presentations during a real driver lease,
  hide/resume and sibling retirement. A stale owner hook cannot affect its successor.
- Service acceptance: factory, rejecting executor and rejected-future destructor
  reenter through the window API. A factory actually replaces the owner; obsolete
  work is refused before executor admission, destruction can query the new owner,
  and a subsequent new-owner service is accepted.
- Observer acceptance: panic, competing failures, removal and nested transitions
  preserve order and the healthy tail. Cover lifecycle and timings in standalone,
  active and nested execution, task/callback cancellation, execution-owner
  replacement and the next healthy operation.
- Telemetry acceptance: 10,000 frames without a subscriber retain no backlog;
  subscribed activity automatically delivers bounded batches; unsubscription,
  explicit reporting and callback reentry preserve recovery behavior.
- Runtime acceptance: direct reports and real owner producers deliver exactly once
  at the correct address, including initialization and stop. Foreign addresses are
  rejected; other runtimes' accepted work remains deliverable after containment.
- Installation acceptance: success, renderer error, mount/initialization or
  installer panic, early close, owner replacement and observer drop. Shutdown two
  pending windows with the first receiver waker panicking; both close and both
  receipts settle as cancelled.
- For each behavior repair preserve the initial failing public-path or real
  producer reproduction, then verify the regression becomes red when the relevant
  production hunk is reverted in isolation. Restore original bytes before any
  subsequent build. Distinguish defect proof from refactor equivalence.
- Validate changed crates and migrated consumers, then run the repository's scoped
  final gate and structural checks once after the final source change. Cover
  optional features and target-gated compilation; report unavailable execution
  separately. A native event/presentation claim requires its own live smoke run.

## Out of Scope

- Reimplementing keyboard, touch, pen, mouse translation or gesture recognition;
  the preceding interaction work is separate from this host-delivery specification.
- Implementing persistence host integration, splitting all loop and window
  configuration, introducing new platform capabilities or redesigning the frame
  topology. Unsupported storage is explicit refusal, not completed persistence.
- Unconfirmed hypotheses about background CPU spin, starvation or input latency.
  They require backend-specific traces before becoming behavior fixes.
- Native OS event conversion, GPU presentation correctness and mobile background
  behavior cannot be certified by the headless acceptance tables. Their execution
  remains a distinct validation obligation where applicable.
- Changes to CI workflows, framework parity claims, or a general claim that every
  app/runtime/scheduler architecture concern is resolved.

## Further Notes

This specification formalizes the reviewed delivery-hardening task. Implementation
already exists locally in commit `e7ea484c3`; it must be reconciled against the
approved specification rather than represented as future work or recreated.
At specification approval the implementation was local and unmerged. Integration
and tracker completion are recorded by the implementation PR.

The recorded final gate against baseline
`2084cbc53589724c09bd37a7eb40c3ddcdb526fc` passed: 860 workspace tests, 46 xtask
tests and one platform compiler-guard test, with 29 workspace tests skipped.
Strict rustdoc, doctests, Windows feature clippy, WASM workspace clippy and the
WASM hot-reload facade check passed; structural checks passed separately.
These are results for the existing commit, not a new run for this draft.

No live OS-window/mobile smoke was run. macOS and Android source checks lacked
their toolchains; the native platform suite requires Linux/Xvfb.
`cargo-semver-checks` is unavailable locally. Deliberate API breaks have a
changelog and compiled consumer migrations, but an automated SemVer report is
not part of the completed evidence.

The maintainer confirmed the proposed acceptance seams on 2026-10-11. The
specification is published with `ready-for-agent`; requirements, design and a
dependency-ordered acceptance task graph record the approved scope. Reconcile
each task with the existing implementation and evidence. Only genuine gaps warrant
additional source changes or repeated tests. Tracker completion follows merge of
the implementation; local success alone does not close the specification.
