# ADR-0345: The lifecycle starts where its window is, and every change walks the ladder

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §4 (lifecycle
  facts per presentation, the ladder, the aggregate), and
  [ADR-0248](ADR-0248-leaving-detached-enters-from-paused.md) whole (the ladder out of `Detached`)
- **Related:** [ADR-0237](ADR-0237-window-state-and-host-notices.md) (Android's three signals),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) (terminal close)
- **Sources:** Flutter 3.50.0, `flutter/flutter@ab2912035e`
  [`packages/flutter/lib/src/services/binding.dart:296-342`](https://github.com/flutter/flutter/blob/ab2912035e/packages/flutter/lib/src/services/binding.dart#L296-L342)
  (the initial state and the generated transitions)

## Context and problem statement

A presentation's lifecycle source held no state until the UI runtime's first reconcile pass, and
the rule then in force sent a first change from that empty state straight to its target. A test
host that mounts a window and never reconciles before hiding it delivered `[Hidden]` to the
window's subscribers, with no `Inactive`. The state was known all along: a presentation is built
from its window's snapshot, and a UI runtime starts with the host `Resumed`. Flutter reads
`PlatformDispatcher.initialLifecycleState` at start-up (`binding.dart:296`) and from then on hands
`handleAppLifecycleStateChanged`, which its scheduler and observers share, each state of
`_generateStateTransitions` (`binding.dart:303-342`); only that first read jumps.

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

Chosen option: 1, because the state is known when the presentation is built, so no host has to
remember a step for the first change to ladder, and no listener hears a state no window was in.

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
subscribe; it runs no user code. `UiRuntime::seed_runtime_lifecycle` then sets the scheduler to
the aggregate of the primary at construction, before anything subscribes. Installing a later
presentation stays a pure publication that runs no user code and moves no focus. The host
delivers the window's `Snapshot` observation next, inside the owner's turn: it takes the focus
from the siblings (forgetting their held keys and cancelling their pointer sequences) and
delivers the change to the aggregate. Every host does so: flui-app's window installation, as the
first of the window's initial observations, and the headless host's `open_window`.

A failure belongs to the presentation whose code failed, as Flutter reports a listener's
exception through `FlutterError.reportError` instead of failing the platform message that
carried it. A window's observation (its snapshot, focus, visibility or execution state) changes
only that window's facts, so what it runs of a sibling's code is what taking the sibling's focus
runs: its key-cancel handlers, its pointer cancellation and its lifecycle observers. A panic
there is contained once the sibling's focus is written, the pass goes on with the other
presentations, and the panic is reported against the sibling's address; the observation, a
window's installation included, is not failed by it. Within the sibling, the panic ends the walk
it broke, as any binding notification does: its observers after the one that failed miss that
state. Every other step is the pass's own: the window's own presentation, the scheduler's
aggregate and its listeners, a restored frame, a close, and the gesture pause of a sibling
already hidden or paused, which is the interaction layer's code. So is every failure of a pass
no window started (the host's lifecycle, a stop) and of a pass that a callback starts while
another pass still has steps pending, the outer window's own among them: it fails the
observation. A pass ends before it reports its siblings' failures, so a report handler that
changes a window starts a pass of its own, which attributes as any other. flui-app
fails the window's installation with it, and `open_window` resumes it with the window open. The
reconcile pass
always has a previous state; an empty source is a broken invariant (`expect("BUG: …")`), not a
branch.

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

- flui-runtime's `lifecycle_derivation_matrix` pins the derivation truth table, checks for every
  pair of states that the ladder moves one state of activity at a time straight from its start to
  its target, and pins exact ladders up, down, into and out of `Detached`.
- flui-testing's `tester::hiding_the_window_hides_the_application_and_showing_it_resumes` pins
  `[Inactive, Hidden]` for a subscriber of a mounted window, and
  `tester::opening_a_second_window_makes_the_first_inactive` pins the headless host's snapshot of
  an installed window, and `headless_host::opening_a_window_cancels_the_keys_the_focused_one_held`
  pins the focus transfer that forgets the held keys of the window that loses the focus.
- flui-runtime's `owner_input` table: `a_runtime_starts_in_its_window_lifecycle` pins the seeded
  scheduler and the snapshots of a second window opened hidden or focused, and
  `window_observations_update_only_the_target_runtime_lifecycle` pins the scheduler's climb from
  `Paused` through `Hidden` and `Inactive`.
- flui-runtime's `owner_registry` table:
  `a_siblings_failure_in_a_new_windows_snapshot_is_the_siblings` pins a sibling observer's panic
  in a new window's initial snapshot as reported against the sibling, which keeps its committed
  `Inactive`, with the installation ready, and an application listener's panic there as the
  installation's failure; flui-testing's
  `headless_host::a_panicking_cancel_in_the_window_losing_focus_does_not_fail_opening_another`
  pins a panicking key-cancel and pointer-cancel handler of the window losing the focus, and
  flui-runtime's `a_windows_failure_in_a_pass_nested_in_its_own_stays_its_own` pins a window's
  own observer failure in a pass its key-cancel handler started as its own change's, and
  `a_report_handler_that_changes_a_window_starts_its_own_pass` pins a sibling's failure in a
  pass a report handler started as reported against that sibling.

## Pros and cons of the options

### 1. Seed from the state known at creation; every change walks the ladder

- Good, because every host, the test host included, gets the ladder without a step to remember.
- Good, because the scheduler starts in a state some window is in.
- Bad, because a scheduler listener hears three notifications for `Paused → Resumed` instead of
  one.

### 2. Keep an empty start, and make every host reconcile once at mount

- Good, because the reconcile pass keeps one rule for a first change.
- Bad, because a host that forgets the reconcile ships the skip, as the test host did.
- Bad, because the scheduler still starts at `Resumed` and reports a change no window made.

### 3. Keep an empty start and treat it as `Detached`

- Good, because a first change ladders with no new state to seed.
- Bad, because a window that opens `Resumed` reports `Paused`, `Hidden` and `Inactive` first,
  states it was never in, and an observer that starts work on `Resumed` and stops it on `Inactive`
  runs for nothing.

## More information

Flutter jumps only once, when its binding reads the platform's initial state, and its observers
are usually registered after that; FLUI's seed is that read, done before any subscriber exists.
Revisit when a host can report a window's state only after the presentation is built: the seed
would then have to wait for that report instead of the snapshot.
