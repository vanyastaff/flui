# ADR-0201: Animation observation through tracing

- **Status:** Accepted
- **Date:** 2026-10-12
- **Related:** ADR-0178, ADR-0199, ADR-0088

## Context

Animation behavior has deterministic controller and mounted-frame contracts,
but the frame profiler cannot attribute registry work, source sampling or
listener delivery. Substrate test frames used to sample their registries before
the scheduler frame span. Successful build/layout/paint timings therefore did
not establish that animation computation was measured.

Adding an observer callback API would give every controller another delivery
protocol. Making the framework call devtools would invert its dependency graph.
Unconditional verbose events would also put observation on ordinary frame paths.

## Decision

`flui-animation/diagnostics`, off by default, enables structured transition
receipts and CPU spans through the existing `tracing` dependency. SDK and facade
forward it as `animation-diagnostics`. `flui-devtools/profiling` enables the SDK
feature and observes the spans; no framework crate depends on devtools.
Warnings and errors remain available without the feature.

A controller owns an `animation_controller` span at target
`flui_animation::diagnostics`. Its subscriber-issued identity and monotonically
increasing run generation identify observation within that controller lifetime.
No global controller counter, display-name lookup or memory address is invented.
Without an interested subscriber, that span is disabled and no external identity
is promised. Reused subscriber IDs are interpreted with their span lifetimes.

Committed transition snapshots join the controller's existing delivery FIFO.
Their baseline is the actual constructed state, so the first playback or clock
change is observable before a run starts. Seeding the baseline emits no transition;
binding the controller to its owner is a clock transition.
They contain owned scalar state, never borrowed user values or user formatting.
The controller borrow is released before emitting a receipt. A reentrant
subscriber may query committed state or issue a normal controller command; the
snapshot describes the earlier committed transition even if callbacks have
already admitted a replacement. Logging does not coalesce accepted transitions.
The outcome and its generation describe the resolved run, separately from the
possibly newer active run in the same receipt. Ordinary unchanged samples emit
no transition event.

The optional span contract is:

| Target | Span | Meaning | Profiler phase |
|---|---|---|---|
| `flui_animation` | `animation_tick` | The complete public registry walk, including delivery and retirement | `Custom("Animation tick")` |
| `flui_animation` | `animation_sampling` | Time, simulation or repeat position computation before publication | `Custom("Animation sampling")` |
| `flui_animation` | `animation_listeners` | Value/status listeners, run continuations/wakers and their retirement | `Custom("Animation listeners")` |

Durations are inclusive extents, not disjoint portions of a CPU total. Registry
time includes its sampling and listener work. They must not be summed to infer
frame cost. Spans outside the profiler's owning scheduler frame are not assigned
to a later or unrelated frame. The raw `HeadlessBinding` samples inside its
pipeline closure before build/layout/paint, matching runtime sampling placement.
Standalone controller tests and direct registry benchmarks still work without
inventing a frame.

The profiler recognizes scheduler frames by their producer target and span
identity, rather than an arbitrary application's `frame` label. It finishes
animation phases and scheduler frames at their outermost entered exit, accounting
for same-span reentry. Retained current-span clones also retain parent identities;
they cannot extend either timing into another frame. Rendering phases preserve
their existing close-based extent.

Each subscriber callout is independently contained through foundation panic
custody. Observation failures retire exact text and retain opaque payloads under
the existing policy; they cannot replace an animation source/listener failure or
discard accepted delivery. Span exit and span retirement are separate callouts,
so a failing exit does not automatically unwind through another failing close.
Closing a span keeps a separate dispatcher alive until the close has returned
or been contained. Last-subscriber retirement is a separate foundation recovery
operation. An observation failure or an already caught animation failure
retains that opaque dispatcher rather than invoking its arbitrary destructor.
The controller passes caught failure custody explicitly; `thread::panicking()`
alone cannot describe an error already caught by a listener round.
The animation operation itself never runs inside a suppression boundary.
Sampling revalidates its run and sample identity after span creation/entry,
before invoking a user curve or simulation, and again before publication after
span closure. A subscriber that cancels or replaces the captured run cannot
leave its old source callable by the displaced sample.
Value notification likewise revalidates notifier liveness after opening its
listener span. A subscriber may dispose the owning controller at creation or
entry; value listeners then stay silent while cancellation delivery still drains.
Subscriber-internal partial registration or an aggregate destructor that
double-panics remains outside this guarantee; failed observation may lose data.

## Verification

`a_real_animation_reaches_the_profiler_with_its_work` drives an owned registered
controller through real scheduler frames. Its listener produces a painted frame,
the value reaches the expected midpoint, and the profile contains actual
tick/sampling/listener phases. After completion, an idle frame invents neither
sampling nor listener work. The test initially fails with a real progressing
controller and a profile containing only build/layout/paint.

The optional observation contract also covers lifecycle correlation, filtered
and feature-disabled capture, subscriber reentry, isolated/competing subscriber
failures and progress after containment through public controller handles.
`animation_observation_contract` also cancels real curve and simulation runs
from sampling-span creation: the displaced sources stay silent, their original
futures are canceled, healthy peers advance and a fresh run completes.
`diagnostic_subscriber_retirement_contract` reproduces competing listener,
span-exit, span-close and last-subscriber destructor failures in a child process.
`developer_history_respects_capacity_and_clear` also covers a runtime-driven
`FadeTransition` and real animation in a nested foreign scheduler frame.
Its application-frame row also proves a same-named application span cannot hide
real animation phases, and its retained-sampling row measures an actual curved
run despite current-span retention/reentry and release in the next idle frame.
`animation_observation_contract` disposes an owning controller from both listener
span creation and entry, then checks cancellation, silent listeners, healthy peers
and a fresh owner's completion.
