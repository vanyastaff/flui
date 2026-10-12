# ADR-0199: Presentation sampling preserves provenance and independent progress

- **Status:** Accepted
- **Date:** 2026-10-11
- **Supersedes:** [ADR-0048](ADR-0048-frame-transaction-boundary.md), in part:
  its exclusion of animation and gesture-deadline sampling before the segment;
  [ADR-0044](ADR-0044-driver-loop-hybrid.md), in part: its exclusion of due
  gesture deadlines while the runtime's frame gate selects background work.
- **Related:** [ADR-0123](ADR-0123-exceptional-presentation-close.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0177](ADR-0177-controller-ordered-delivery.md),
  [ADR-0179](ADR-0179-controller-registration-ownership.md),
  [ADR-0181](ADR-0181-fallible-text-and-layout-measurement.md).

## Context

Sampling runs before widget build. Letting one controller listener or input
deadline unwind through the whole walk prevents later presentations from doing
already accepted work. Catching only the walk loses a necessary distinction:
a source can fail before its proposal commits, while a listener can fail after
valid state and its delivery obligations have committed.

The first failure alone cannot decide publication. A listener may fail first,
then another controller's source may fail before commit. The first cause remains
the diagnostic cause, but the later failure makes that presentation's new frame
unsafe. An open gesture extension also cannot promise a valid postcondition
merely because its timer disappeared before a panic.

[HTML animation callback iteration](https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#run-the-animation-frame-callbacks)
invokes surviving callbacks with report behavior;
[Web IDL](https://webidl.spec.whatwg.org/#invoke-a-callback-function) reports an
exception and returns to the caller. This supports continuing independent work.
It does not prescribe native presentation recovery, FLUI's frame admission, or
the deadline retry interval below; those follow FLUI's ownership model.

## Decision

### One operation owns sampling and admission

`flui-runtime` owns a presentation sampling operation. `flui-animation` and
`flui-interaction` attach provenance at their actual proposal, commit and
delivery seams, using the existing `PanicRecovery` custody. No host, native
window or GPU ownership moves into the runtime.

Every presentation samples before any presentation builds. The pump selects
one raw host timestamp; each presentation applies its motion clock and policy.
An animation failure does not cancel independent input deadlines of the same
live presentation. Controllers, child registries, deadline members and sibling
presentations continue under their existing identity and admission fences.
Removed-before-turn work, reentrant supersession, mute and close keep their
ordinary domain meaning.

After sampling, the operation decides fresh admission once through the existing
`FrameClock` gates. An unfinished retained text segment is already an admitted
attempt. Recovery and publication eligibility are established before reporting.
Diagnostics cannot make the runtime poll the frame clock again or change the
disposition of an already reported operation.

### First cause and publication blocker are independent

| Provenance | Required evidence | Effect on an admitted attempt |
|---|---|---|
| `Computation` | A required proposal failed before valid commit | Preserve the producer's previous accepted value and timing; reject the new segment |
| `Delivery` | Valid framework state and delivery debt committed before the failing callout | Preserve committed state and attempt the live delivery tail; the segment may continue after premise validation |
| `Unknown` | The callout establishes no trustworthy postcondition | Reject the new segment without inventing computation or successful-delivery evidence |

Handled non-finite values and typed admission refusals retain their existing
policies. Panic text, `BUG:` prefixes, equal samples and cleared timers do not
prove origin. `BUG:` remains separate diagnostic metadata.

`FrameFailureKind::SamplingPanic` records:

- `first`: the first captured stage and origin, whose payload supplies the text;
- `blocking`: the first computation or unknown failure, even when it occurred
  later than `first`;
- `admitted_attempt`: whether the operation concerns an admitted fresh or
  retained frame attempt.

Stages distinguish `Preparation`, `Animation`, `GestureDeadline` and
`DeadlineDiscovery`. A report is `FrameDropped` only when an admitted attempt
has a blocker. Delivery-only and no-attempt failures are `Contained`.
The sampling cohort produces one aggregate report and counts one rejected
attempt. A contained report exposes the current failure streak without
incrementing or resetting it; a clean terminal attempt resets it.

After diagnostics, the runtime checks authority and retained premises again.
A newly failing preparation check is a separate operation with its own cause
and report. It neither repeats the earlier sampling payload nor replaces that
operation's sealed provenance. Ordinary callback-driven invalidation withdraws
obsolete work without manufacturing a panic report.

### Rejection preserves ownership and makes replacement runnable

The runtime submits no candidate scene or completion for a rejected attempt.
Already committed healthy controller samples remain; the last submitted scene
remains on screen. This is local containment, not rollback of application state.
Recovery demand survives the pump's idle clear and a healthy sibling's terminal
outcome. A later healthy operation can progress without new external input;
repeated frame failure retains the existing pacing and embedder escalation.

Compatible retained text keeps its frozen tick, frontier and receipts after a
delivery-only failure. Unknown or computation rejection withdraws the retained
attempt atomically: invalidate receipt authority, withdraw geometry, restore
runnable layout debt and keep the replacement barrier uncommitted. A late
native answer may physically finish, but its settlement returns `false` and
cannot authorize replacement geometry. Completion becomes eligible only after
a coherent replacement completes.

Restoring retained layout eligibility uses the rendering owner's existing
queue, after the presentation withdraws the old attempt. It admits no new input,
requests no wake by itself and proves no geometry or completion. The runtime
owns the replacement barrier and drives the coherent successor. Deterministic
authored text rejection retains ADR-0181's changed-input policy; this seam does
not turn unchanged invalid text into an automatic retry loop.

The first failure remains authoritative through recovery, secondary callouts
and opaque retirement. A local failure discovered while preserving an earlier
owner failure can still supply its addressed report without replacing the
earlier failure's propagation. Healthy destruction remains ordinary.
[Rust's unwind boundary](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
does not catch aborts or protect an aggregate whose destructors double-panic
before the boundary regains control; ADR-0127's retention limits still apply.

### Scheduling reads use committed observations

Host-facing wake and pending getters read owner snapshots. They execute no
extension query, callback, tracing or user destruction while registry or host
storage may be borrowed. Guard-free owner execution refreshes observations
after work capable of changing deadline sources, including input, build,
completion, asynchronous/background work and deferred text grants.

Metadata refresh shares the enclosing owner opportunity. A deadline source can
reenter an inbox operation, but that nested operation cannot restart discovery
of the source already being observed or acquire a separate polling budget.
Nested command admission still follows the ordinary owner rules; duplicated
metadata refresh waits for the enclosing operation to finish. Source revisions
and accepted wake debt remain authoritative across this deferral.

An outside-frame observation report is retained as framework-owned data until
the runtime lease is restored. An owned delivery batch invokes diagnostics and
the current handler without an owner-storage guard. Each report observes handler
replacement independently. Tracing and handler failures are contained
separately, cannot replace the original cause and do not retry the same report.
A direct runtime transaction delivers its queued reports after its own
transaction ends.

Background pumping advances due input deadlines even when frame production is
disabled. The private `WakeAction::PumpBackground` action names this complete
owner operation. It does not sample animation, build, paint or complete a
presentation segment. A failure without a retained attempt is contained; a blocker against
an existing retained attempt withdraws that attempt and reports its rejection.
These guarantees require an executing owner turn, not background execution
while the operating system suspends the application.

### Deadline faults preserve bounded, identity-fenced debt

A failed query or untrusted poll removes only that source's stale scheduling
contribution. Independent healthy deadlines retain their timing. Unresolved
work remains fenced by source and revision identity:

- A failed pure query receives one automatic re-probe after 100 ms. A second
  failure parks it until an explicit update or new source revision. Reads do
  not query again, move that due time, report again or create another probe.
  If the clock cannot represent the probe's due time, the obligation parks
  rather than wrapping or keeping an immediate wake hot.
- A failing poll with unknown consistency is never automatically replayed for
  the same revision, because user side effects may already have occurred.
  A new admitted revision or retirement changes eligibility without claiming
  the old poll completed.
- A committed built-in deadline transition is delivery. Its callback failure
  cannot reactivate that transition on the next pump.

Runtime-hosted extensions retain one linear `GestureDeadlineRegistration` for
their member-wide deadline source and publish timing through its weak
`GestureDeadlineUpdate`. Publication or cancellation commits a new
revision, including the same numerical due time, then wakes outside borrows.
`refresh` requests a guard-free query revision. Silent external mutation has
no scheduling guarantee. Closed, retired and exhausted authority refuses;
withdrawal cannot resurrect an old poll through implicit discovery.

The member's query and poll are aggregate hooks, so two active explicit
registrations for that same member are refused as `AlreadyRegistered`.
Legacy arena slots can share a member, but failure coverage remains member-wide
and a captured slot cannot bypass quarantine. Built-in recognizers use this
publication path. Standalone live-query and animation/deadline drive entry
points retain their documented first-panic propagation and healthy tails.

## Alternatives and limits

A blanket outer catch cannot preserve provenance or distinguish mixed failures.
Dropping every listener failure discards valid state. Inferring commit from the
remaining state is unsafe under reentry. Replacing the complete gesture API
with a universal proposal protocol is larger than the scheduling contract;
explicit unknown consistency gives open extensions an honest bounded fallback.

This decision preserves ADR-0044's frame-clock admission, native wake folding
and platform pacing. Only its background-deadline exclusion is superseded;
background pumping does not become a frame produce trigger.

It preserves ADR-0048's segment and local build recovery, submit
semantics and detail policy, and ADR-0123's terminal close policy. It adds no
parallel execution, performance isolation or simultaneous native multi-surface
submission. Logical Windows tests do not establish native pen/touch, browser
IME, mobile lifecycle or compositor execution.

## Verification contracts

- `presentation_delivery_failure_keeps_healthy_sampling_build_and_completion`
  and `presentation_computation_failure_rejects_its_frame_and_preserves_healthy_work`
  distinguish committed delivery, rejected proposals, mixed precedence and
  healthy sibling build/completion through the public runtime pump.
- `one_pump_shares_its_selected_time_across_parent_and_child` distinguishes the
  selected timestamp from later host readings.
- `registered_text_preparation_preserves_the_actual_presentation_attempt`
  covers frozen ticks, rejected old receipts, fresh geometry and report-driven
  premise invalidation through actual sizing work and completion.
- `owner_sampling_contract` covers inert observations, restored-lease report
  delivery, competing diagnostics, publisher updates, completion refresh and
  `deadline_query_reentry_does_not_rediscover_its_active_source`.
- `sampling_preserves_failure_origin_and_publication_integrity`,
  `owner_deadline_recovery_matrix` and
  `recovery_preserves_provenance_and_owned_failure` pin the producer,
  scheduling-authority and custody contracts below the runtime.
