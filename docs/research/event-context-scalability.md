# Event context: scalability and evolution review

Date: 2026-09-27. Source baseline: `e330fc413` plus the working tree's scheduler
post-frame recovery extraction. This is a read-only architecture study, not an
accepted ADR, benchmark result, or claim that the remaining callback migration is
complete. No builds or performance experiments were run for this document.

## Recommendation

Keep the minimal, presentation-bound `EventCx` write capability. Do not freeze its
delivery semantics, animation listener topology, or SDK exposure yet. A richer
context is not required to fix today's build-time writes; conversely, attaching a
writer to a callback does not solve lifetime, fairness, routing, or backpressure.
Treat these as separate contracts and test them before publication.

Post-frame delivery is a correct phase-boundary repair for a notification discovered
during build. It is also a bridge around the current shared animation-listener
topology, not evidence that every UI notification belongs after paint. Keep
synchronous input notifications synchronous when safe; preserve query callbacks
that must answer immediately. Do not turn every event into an owner command.

## Evidence in this checkout

- [Writer implementation](../../crates/flui-view/src/reactive/writer.rs): `EventCx`
  borrows a `Writer`; `WriterSource` owns a cloned `Reactive`; `write` opens a
  context and calls the closure synchronously. It neither defers nor provides a
  transaction. `check_context` checks graph identity and the build guard, not
  target liveness. Sealed `EventOutcome` reports framework errors, not arbitrary
  application errors. Transitional `Reactive: WriteTarget` still exists.
- [Reactive graph](../../crates/flui-view/src/reactive/mod.rs): the graph is
  `Rc<RefCell<_>>`, signal slots are generational, and same-slot reentrant access
  is refused. A writer source can retain the graph; it is not a weak widget token.
- [Local post-frame lane](../../crates/flui-scheduler/src/post_frame.rs): scheduling
  boxes a closure and appends to an unbounded `Vec`; handles weakly address their
  scheduler/lane. Registration does not itself request a frame. It returns a
  closed-lane error but no cancellation token or capacity error.
- [Dispatch implementation](../../crates/flui-scheduler/src/scheduler/post_frame_dispatch.rs):
  one closed snapshot merges shared/local entries by ID, sorts, and drains
  synchronously. New registrations wait for a later completed frame. Panic
  consumes the offending entry and restores the untouched tail; it does not
  continue the poisoned frame. Snapshot finiteness is not a time or memory bound.
- [AnimatedSize](../../crates/flui-widgets/src/animated/animated_size.rs): a shared
  status listener increments an atomic completion count and requests a rebuild;
  build queues a local callback that iterates the captured count difference.
  Mounted status and the current callback are checked during delivery.
- [Dismissible](../../crates/flui-widgets/src/interaction/dismissible.rs): deferred
  entries capture an event payload and `Rc<DismissEvents>`, whose writer and current
  callbacks remain alive. Disposal suppresses invocation, not immediate reclamation
  of queued captures. The fully-slid input completion remains synchronous.
- [Animation contracts](../../crates/flui-animation/src/animation.rs) still impose
  `Send + Sync`; [Vsync](../../crates/flui-animation/src/vsync.rs) ticks controllers
  through a public `tick_all`. Removing one setter's `Send` bound does not resolve
  these storage and listener contracts.
- [Realm commands](../../crates/flui-runtime/src/ui_realm/commands.rs) already offer
  bounded, wake-bearing ingress, typed stale-target routing, and a pre-read-count
  drain. `SignalWrite` still invokes `FnOnce(&Reactive) + Send`. Its count bound
  cannot preempt a slow callback. [Runtime architecture](../../crates/flui-runtime/ARCHITECTURE.md)
  also records a 32-operation host continuation budget: this bounds outer operations,
  not the work hidden inside one frame's callback drain.
- [SDK](../../crates/flui-sdk/src/lib.rs) re-exports the whole view/widgets crates.
  A context signature change reaches package authors even when the internal crate
  remains behind the SDK. [ADR-0086](../adr/ADR-0086-signal-writes-through-event-context.md)
  explicitly leaves shared families and other migration work open;
  [ADR-0088](../adr/ADR-0088-official-packages-sdk-and-facade.md) defines the evolving
  package-author surface. Neither makes an untested future contract free to change.

## Failure sequences and their classification

### Large finite batches still monopolize the owner

Queue N post-frame callbacks before one close. Each takes C time; the owner must
spend approximately N*C plus merge/allocation costs before returning. Reentrant
registration cannot make that particular snapshot infinite, but a slow callback
cannot be interrupted. Two windows on the same owner can therefore interfere even
with the host's outer operation budget. This follows from the synchronous drain;
the magnitude on real hardware is unmeasured. It is preexisting scheduler behavior,
newly exposed to more widget callbacks by this migration.

AnimatedSize's single boxed callback can itself deliver many accumulated completion
notifications. Limiting callback *entries* would not bound this batch. Dismissible
uses a box per deferred payload instead. Producer bursts must be exercised through
real controller/tick/build paths; directly editing a private completion counter is
useful for a unit invariant but is not a representative throughput benchmark.

Do not silently coalesce completion, submit, or dismissal edges. Latest-value
coalescing may suit progress notifications only after consumer-observable semantics
are specified. A queue cap without an overflow policy simply converts latency into
lost events. A deadline cannot preempt user code; frame-path user work must remain
short and expensive pure work must leave the owner.

### Cancellation is not reclamation, and registration is not a wake

Enqueue a callback capturing a large application model; dispose the widget; leave
the lane alive without another completed frame. The mounted check prevents effects,
but the queued closure and its captures remain until drain or lane destruction.
A retained `WriterSource` can retain the graph too. This is a retention interval,
not proof of a permanent cycle. Weak lane handles do not make every queued payload
weak. Closing the lane drops callbacks; it does not invoke terminal notifications.

For current widgets, events are discovered during an already-running frame, so
non-waking registration has a plausible producer contract. Reusing this lane for
worker results or navigation without arranging a future frame would be incorrect.
Test no-frame/suspended owners before expanding the API. Do not add an implicit
wake to generic post-frame registration without deciding whether it should drive
otherwise idle/hidden presentations.

### Live callback replacement differs from event snapshot identity

Queue event E under configuration A; rebuild to B; deliver E to B's callback with
E's original payload. That is the recorded current behavior, not an accidental
stale closure. If B now represents another logical operation, the interpretation
may surprise consumers. Disposal suppresses E entirely. An accepted dismissal
therefore is not a durable business commit; applications needing durability must
record it at their domain boundary, not assume a disposed widget still notifies.

Keep replacement, handler removal, same-key reuse, disposal, and reentrant disposal
as distinct test cases. A future owner-target command needs an incarnation or
generation, not merely a reusable element index. Do not retrofit captured old
callbacks as a shortcut: that changes the latest-handler contract and retention.

### Authority is narrower than atomicity or liveness

Callback writes signal A, then a later operation returns `ForeignPresentation` or
panics. A is not rolled back. A stored `WriterSource` is not revoked automatically
when its acquiring widget dies. A borrowed `EventCx` cannot escape, but the source
can. These are existing capability semantics, not memory-safety defects.

Validate owner identity before non-signal mutations, release framework borrows
before invoking user code or dropping replaced user captures, and keep callback
outcomes explicit. Introducing transaction semantics would require a separate
design for arbitrary user side effects, not merely `&mut` in the signature.

### Panic recovery is a boundary contract, not UI fault isolation

The scheduler now preserves uninvoked tail entries when a caller catches a panic
and later drives another frame. It does not retry the panicker or guarantee that a
native host survives it. A widget batch inside one callback still loses the
unexecuted remainder of that callback if it panics. This is consistent with treating
one callback as the recovery unit; changing it requires a deliberate contract.
Do not sell scheduler headless recovery as production exception isolation.

## Minimal capability versus richer context and targeted commands

| Choice | Solves | Cost and limit | Recommendation |
|---|---|---|---|
| Current `EventCx` plus lifecycle capabilities | Explicit graph writes; familiar direct callbacks; keeps interaction below reactive layer | Caller-supplied writer source is not a live target, cancellation token, or scheduler | Keep; finish removal of transitional write escape paths under ADR-0086 |
| Add focus/navigation/spawn/tree access to every context | Potentially simpler composed handlers | Must define dispatching versus target presentation, unavailable capabilities, nesting, query phases; broadens SDK authority and future compatibility burden | Add only after a concrete package consumer and a cross-presentation test justify each capability |
| Owner-target event records resolved at delivery | Stale-generation rejection, compact payloads, central admission/diagnostics, potentially prompt cancellation | Registry ownership, type erasure or closed variants, extra dispatch hop, target lifetime and ordering contract | Prototype for remaining shared animation/semantics boundaries; not for every synchronous callback |
| All callbacks become generic commands | Uniform queue syntax | Loses immediate query answers and synchronous notification ordering; risks arbitrary closure transport and a new executor vocabulary | Reject as a default; existing runtime favors typed operations |

The likely long-term composition is a narrow borrowed capability for local effects
plus explicitly addressed messages at ownership boundaries. Those mechanisms are
complementary. The event context should not become a service locator merely because
another framework's lower-level event context has tree authority.

## Primary-source comparisons

Masonry 0.4.0's widget-level context has target identity, focus/handled operations,
actions, and deferred widget mutation. FLUI's application callback capability is
at a different layer; copying that entire surface would import routing semantics
that FLUI currently leaves in interaction/lifecycle owners. The useful lesson is
to name deferred mutation and its target explicitly, not to maximize context size.
[Masonry EventCtx](https://docs.rs/masonry/0.4.0/masonry/core/struct.EventCtx.html).

Xilem Core 0.4.0 distinguishes an action, no change, a rebuild request, and a stale
message target. That is a useful experimental model for *delivery outcomes*;
FLUI's `EventOutcome` instead reports callback errors and must not silently become
a routing result under the same name.
[Xilem MessageResult](https://docs.rs/xilem_core/0.4.0/xilem_core/enum.MessageResult.html).

Winit 0.30.13's event-loop proxy explicitly reports a closed event loop when sending
an event. It demonstrates closure/liveness as a transport concern, not proof that
the payload remains meaningful when delivered. The comparison is version-specific,
not a recommendation to add Winit to the runtime layer.
[Winit EventLoopProxy](https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoopProxy.html).

## Experiments required before freezing the contract

The following are proposed acceptance criteria, not measured results. Record
toolchain, release profile, hardware, refresh rate, queue payload size, and baseline
commit. Virtual-clock correctness and native latency tests answer different questions.

| Experiment and workload | Required observations and acceptance |
|---|---|
| Signal-only dispatch: 1, 100, 10,000 writes; compare direct approved writer path and widget EventCx path | Identical values/errors and subscriber invalidations; count allocations per dispatch. No per-write callback box introduced by the context. Agree on timing budget after a reproducible baseline, not from an assumed nanosecond target |
| 1, 100, 1,000 animated widgets; 60/120/240 Hz clocks; bursts of 1/10/100 ticks between builds | Completion multiplicity and order match the explicit policy; capture queue high-water mark, retained bytes, callback time, and input-to-observable-update latency. A proposed budget is p99 framework notification overhead below 10% of the frame interval, excluding intentional expensive user work; failing it triggers topology work, not silent event loss |
| Post-frame queue of 1/100/10,000 entries with mixed local/shared and reentrant producers | FIFO for admitted entries; new entries never extend current snapshot; zero duplicate terminal events; bounded snapshots are reported separately from drain wall time |
| Mount 10,000 capturing widgets, enqueue, replace/remove/dispose, then suspend or destroy owner | No effects after disposal; only current callback receives retained payload; drop counters show all captures released when the lane dies. If an owner remains alive and unpumped, quantify retention; decide whether immediate cancellation is required before promising it |
| Two presentations in one realm, two realms on one owner, then separate owners where supported | Zero cross-graph mutation or delivery after target incarnation closes. Flood A while B receives input/close: log B's p50/p95/p99 latency and owner-turn counts. With no-op callbacks, B must progress at the documented continuation opportunity; deliberate long user callbacks are a diagnosed non-preemptible case |
| Saturated bounded command ingress plus an idle/hidden owner | Explicit full/closed outcome, no unbounded fallback queue, admitted lossless edges delivered once if owner survives, stale requests counted, remaining work receives a wake. Latest-value coalescing only for a named operation with pinned semantics |
| Callback rebuilds/replaces/disposes itself, navigates, writes another presentation, or panics | No framework borrow held across callback or capture destruction; identity rejection before non-signal mutation; panic unit and surviving tail exactly specified; no claim of rollback |
| External SDK fixture: custom widget, generic helper, borrowed payload, form+signal error, query callback, async completion | Measure higher-ranked callback parameter inference separately from return-error disambiguation. Documented helpers should cover the former; an explicit `-> Result<(), EventError>` for mixed `?` operations is legitimate, not an inference failure. Compile-fail demonstrates non-escaping context and non-Send owner capabilities; domain errors cannot disappear through generic outcome logging; capture actual source churn and diagnostics |

## Small architectural prototypes with go/no-go decisions

1. **Owner-local animation notifications.** Preserve shared immutable/controller
   interfaces where actually needed, but route an identified owner-local consumer
   without an atomic-build-post-frame bridge. Use AnimatedSize and PageView as
   consumers. Proceed only if it removes the bridge without putting graph state
   below `flui-view`, introducing per-node locks, or changing layout-triggered
   notification timing accidentally. Cost: listener storage/lifetime migration,
   animation API break, and a comparison against existing ordering tests.
2. **Weak closure delivery before a target registry.** First keep the existing
   closure lane and compare strong captures with `Weak` event-state/owner captures,
   a per-attachment generation check, and callbacks cleared on disposal. Exercise
   replacement, detach/reattach, and unpumped-owner retention through Dismissible.
   This needs no target registry; verify that clearing callbacks drops captures
   outside framework borrows. Only then prototype a generation-addressed record
   with a small typed payload if a registry has a measurable benefit the simpler
   design cannot provide, such as centralized admission or prompt queue removal.
   Require identical delivery semantics and allocation/retention evidence before
   proceeding. Cost: even the simpler option needs an attachment-lifetime contract;
   a registry adds lookup, ownership and reclamation protocols on top.
3. **Queue telemetry before queue policy.** Observe queue age/high-water mark,
   callback duration, presentation, event family, stale/drop reason and saturation
   at dispatch boundaries. Never record user text/payload contents by default.
   First prove traces are disabled cheaply and work without global counters. Then
   choose budgets/coalescing from workloads. Existing warning-only reporting lacks
   enough event provenance to diagnose a thousand identical refused writes.
4. **SDK compatibility fixture before acceptance.** Compile the above package
   examples against both the current minimal context and one narrowly enriched
   alternative. Separate source changes due to EventCx, signal migration, and
   listener ownership. Keep `EventOutcome` sealed and refuse an unexplained blanket
   application-error implementation. Cost: fixture maintenance; benefit: catches
   the external API burden that same-workspace edits hide.

None of these experiments requires weakening synchronous build/layout/paint or
adding ambient state. The decision to publish should require their contract evidence,
not a claim that present tests establish decade-long scalability.
