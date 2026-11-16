# ADR-0345: The lifecycle starts where its window is, and every change walks the ladder

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §4 (lifecycle
  facts per presentation, the ladder, the aggregate), and
  [ADR-0248](ADR-0248-leaving-detached-enters-from-paused.md) whole (the ladder out of `Detached`)
- **Related:** [ADR-0237](ADR-0237-window-state-and-host-notices.md) (Android's three signals),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) (terminal close)

## Context and problem statement

A presentation's lifecycle source held no state until the UI runtime's first reconcile pass, and
the rule then in force sent a first change from that empty state straight to its target. A test
host that mounts a window and never reconciles before hiding it delivered `[Hidden]` to the
window's subscribers, with no `Inactive`. The state was known all along: a presentation is built
from its window's snapshot, and a UI runtime starts with the host `Resumed`. Flutter starts the
same way, from `PlatformDispatcher.initialLifecycleState`.

The scheduler's lifecycle listeners (`UpdateScheduler::add_lifecycle_state_listener`, public in the
facade) had the same skip in another form: the aggregate jumped to its target, so a window shown
again after a pause went `Paused → Resumed` with nothing in between, and a runtime over a window
that opens hidden reported `Resumed → Hidden` at start-up though no window was ever `Resumed`.

Two records stated the rule: ADR-0035 §4 and ADR-0248, which amended its ladder out of `Detached`.
This record states the whole rule. Which state does a lifecycle start in, and what does a listener
see on the way to a new one?

## Decision drivers

- An observer that starts work on `Resumed` and stops it on `Inactive` sees each step once, in
  order, on every path: Flutter's `AppLifecycleListener` contract.
- No observer hears a state more active than both ends of a change: a paused or hidden host never
  enables frames or attaches resources for a moment.
- The frame path is synchronous and the owner thread holds the state; seeding runs no user code.

## Considered options

1. Seed each presentation, and the scheduler, from the state known at creation; every change walks
   the ladder.
2. Keep an empty start and let the first change jump, and make every host reconcile once at mount.
3. Keep an empty start, and treat it as `Detached`, so a first change climbs from the `Paused` side.

## Decision

Chosen option: 1. Option 2 leaves the defect for the next host that forgets the reconcile, and the
test host was such a host. Option 3 reports `Paused` and `Hidden` for a window that opens
`Resumed`, which no window was.

### §1 Lifecycle facts are per presentation; the UI runtime aggregates

Each presentation owns its native visibility, focus and execution snapshot and its last delivered
local state. A UI runtime derives the scheduler state from its live presentations: any visible and
focused → `Resumed`; otherwise any visible → `Inactive`; otherwise `Hidden`; an empty forest →
`Detached`. Keyboard routing history does not manufacture native focus. Android feeds the same
three signals as every other backend: `onStart`/`onStop` give visibility, `onResume`/`onPause` the
execution state and `onWindowFocusChanged` focus, and the state comes from
`derive_lifecycle_state`. Focus alone never walks to `Paused` (ADR-0237).

### §2 A lifecycle starts in the state its window gives

`UiRuntime::seed_presentation_lifecycle` commits a presentation's state from its snapshot and the
host's lifecycle when the presentation enters the forest: in `UiRuntime::construct` for the primary
one and in `install_presentation` for every later one, before a root mounts and anything can
subscribe. `UiRuntime::seed_runtime_lifecycle` then sets the scheduler to the aggregate of that
state. Neither runs user code. The reconcile pass therefore always has a previous state; an empty
source is a broken invariant (`expect("BUG: …")`), not a branch.

### §3 Every change walks the ladder

A presentation's steps follow `lifecycle_ladder(old, new)`, which walks the declaration order
`Detached, Resumed, Inactive, Hidden, Paused` (Flutter's
`ServicesBinding._generateStateTransitions`). The ladder is a ring that a presentation enters on
the side it leaves by: from `Detached` it climbs `Paused, Hidden, Inactive, Resumed` and stops at
the target, and into `Detached` it steps down through `Paused`. No step is more active than both
ends of its change.

Scheduler listeners see the aggregate, not a presentation's ladder, and the aggregate walks the
same ladder: each round of a pass takes one step from the scheduler's state toward the aggregate
as it stands then, so a pass that a callback starts inside this one is not undone by stale steps.
A pass runs as many rounds as its longest ladder, the aggregate's included.

### §4 Commit order, detachment and close

Local facts and the next ladder step commit before user callbacks; input cancellation precedes
lifecycle callbacks; binding observers see committed local and aggregate state. Restoring a
presentation re-dirties its own root and wakes a frame. Focus transfer cancels the former
presentation's active pointer sequences at once. Observed `Detached` is reversible and only
rejects input. Explicit stopping is a separate terminal intent. Closing a presentation removes it
from the aggregate, delivers `Detached` while its tree is alive, then disposes it; an observer
panic cannot skip removal.

### Consequences

- Good, because a window hidden from `Resumed` reports `Inactive, Hidden` on every host, the test
  host included, and the scheduler's listeners hear the same steps.
- Good, because a runtime over a window that opens hidden starts `Hidden` and reports nothing until
  the window changes.
- Bad, because `Paused → Resumed` is three notifications for a scheduler listener instead of one.
- Neutral, because the subscribers in the tree ignore the state `subscribe` returns, so a
  non-empty start changes nothing for them.

### Confirmation

- `lifecycle_derivation_matrix` pins every ladder, the `Detached` entries included.
- flui-testing's `tester::hiding_the_window_hides_the_application_and_showing_it_resumes` pins
  `[Inactive, Hidden]` for a subscriber of a mounted window.
- flui-runtime's `owner_input` table: `a_runtime_starts_in_its_window_lifecycle` pins the seeded
  scheduler, and `window_observations_update_only_the_target_runtime_lifecycle` pins the
  scheduler's climb from `Paused` through `Hidden` and `Inactive`.
