# ADR-0261: Dropping a build owner ends its tree observation

- **Status:** Proposed. §1 is not implemented: `BuildOwner`'s `Drop` reclaims its key claims only
  (`crates/flui-view/src/owner/build_owner.rs`). §2, how teardown tests prove that a closed UI
  runtime released what it owned, is pending the owner's decision; its options are laid out and
  none is chosen.
- **Date:** 2026-10-28
- **Supersedes (on acceptance):** [ADR-0040](ADR-0040-tree-observation-seam.md) §3, the "Teardown
  honesty" item's sentences "A `BuildOwner` dropped without `clear_tree_observer` emits no
  `detached()` — embedders that install must clear (install/clear symmetry)", and the matching
  negative consequence and follow-up ("A UI runtime-teardown hook that guarantees
  `clear_tree_observer`"). The rest of §3, that nothing synthesizes `element_unmounted` for elements
  alive at teardown, stands.
- **Related:** [ADR-0127](ADR-0127-exceptional-path-retention.md) (a thread-shared `Arc` is
  retained on an exceptional path), [ADR-0123](ADR-0123-exceptional-presentation-close.md) (a close
  that holds a failure retains ownership),
  [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §5 (a destructor does not
  resume while user values live), [ADR-0097](ADR-0097-no-process-global-state-gate.md) (a counter
  in the framework would be a `static`)

## Context and problem statement

ADR-0040 made `TreeObserver::detached()` the end-of-life signal of the observation stream, and left
the one case where it never comes to the embedder: a `BuildOwner` dropped with an observer still
installed emits nothing. Every UI runtime that closes is such a case unless its host remembers to
clear the observer first, and none does: a devtools panel attached to a window that closes sees its
stream stop without an end, and cannot tell a closed window from a stalled one.

The same teardown work needs to prove, in tests, that a closed UI runtime released everything it
owned: elements and render objects, animation controllers and tickers, text and scroll controllers,
futures, routes and overlay entries. `detached()` says the stream ended; it does not say that
anything was released, and an unmount is not a destructor (ADR-0040).

When does the stream end, and what proves the release?

## Decision drivers

- the end of the stream is the owner's, not the embedder's: a rule an embedder must remember is a
  rule that is broken;
- an observer is user code: it is called outside every borrow, contained, and not called during an
  unwind (ADR-0127);
- no element event follows the end of the stream;
- a release test must fail when a value outlives its UI runtime, not when a callback was missed.

## Considered options

For §1:

1. `BuildOwner`'s `Drop` takes the observer out of its slot and calls `detached()` once.
2. A teardown hook in the UI runtime that calls `clear_tree_observer` before dropping the owner.
3. Keep the embedder's obligation.

For §2 (pending): debug counters in the framework, ownership sentinels in the acceptance app, or the
`TreeObserver` itself; see "Pros and cons of the options for §2" below.

## Decision

Chosen option for §1: 1, because the owner is the one value that always goes when its tree goes.

### §1. The owner's drop ends the stream

`BuildOwner`'s `Drop` takes the observer out of its slot (`Option::take`) and, with no borrow of the
owner held, calls `detached()` once through the same contained emission as every other event
(`emit_observation`). A panic in `detached()` is contained and logged as for any observer
(ADR-0040 §6), and the drop goes on. After `clear_tree_observer` the slot is empty and the
drop calls nothing, so the stream has exactly one end however it ends.

`detached()` is the last call the observer gets from that owner: the drop runs after the tree's
unmounts, and the slot is empty before the call.

On an exceptional path the observer is not called: while the owner is dropped inside an unwind, or
by a close that holds a failure (ADR-0123), the observer's `Arc` is retained, not dropped and not
called (ADR-0127 §2–3). Such a stream ends without `detached()`, as it does today for an observer
that panicked.

### §2. How a teardown test proves the release — pending owner decision

**Proposed — pending owner decision.** A closed UI runtime must release every element, render
object, animation controller, ticker, text and scroll controller it created, and its routes, overlay
entries and hero flights, and a test must say "created equals released". Which mechanism counts is
open: framework debug counters, ownership sentinels the acceptance app plants, or the observer
stream. Whichever is chosen, the typed refusals of stale handles after a UI runtime is dropped
(`OwnerGone`, `Closed`) are kept as the second proof that the UI runtime itself is gone.

### Consequences

- Good, because a devtools panel sees every closed window's stream end, without any host code.
- Good, because the "teardown hook that guarantees `clear_tree_observer`" follow-up of ADR-0040
  needs no hook.
- Bad, because the drop of a `BuildOwner` now runs user code, contained, where it ran none.
- Neutral, because a stream that ends on an exceptional path still ends silently, by the retention
  rule every container follows.

### Confirmation

- `dropping_an_owner_with_an_observer_detaches_it_once` (`flui-view`): an owner dropped with an
  observer calls `detached()` once; after `clear_tree_observer` the drop calls nothing; no event
  follows `detached()`.
- `stale_handles_refuse_after_realm_drop`, one row per handle type.
- §2's release rows (`acceptance_app_close_releases_every_owner_after_navigation`,
  `last_window_close_releases_the_realm_through_the_runner`) are named once the mechanism is chosen.

## Pros and cons of the options

### 1. The owner's `Drop` calls `detached()`

- Good, because it holds for every owner, in a test harness as much as in an app.
- Bad, because a destructor calls user code, which ADR-0127 then has to keep off the unwind path.

### 2. A teardown hook in the UI runtime

- Good, because no destructor calls user code.
- Bad, because an owner dropped elsewhere (a test, a hot reload that rebuilds the tree) still ends
  silently.

### 3. The embedder's obligation

- Good, because nothing changes.
- Bad, because no host meets it today.

### Pros and cons of the options for §2

#### 2a. Debug counters in the framework

Live-node counts per kind (elements, render objects, controllers, tickers), visible through
`flui-testing`.

- Good, because they see what the application cannot: internal cycles, framework-owned nodes.
- Bad, because a counter counts increments and decrements, not destructors: a node leaked through a
  path that still decrements passes.
- Bad, because a process-wide count is a `static` that needs its ADR-0097 entry and a new testing
  surface; a per-runtime count needs a runtime to hang on, which a dropped runtime no longer is.

#### 2b. Ownership sentinels in the acceptance app

The app plants a guard in each value it hands the framework (a capture, a controller's listener, a
route's page); a guard reports its own `Drop` to a ledger the test reads.

- Good, because it proves the destructor ran. A slab node outlives its runtime only through
  `forget` or a leaked runtime, and both leave the sentinel alive.
- Good, because it needs no framework surface.
- Bad, because it sees only what the app can plant into: a framework-internal cycle that holds no
  application value is invisible.

#### 2c. The `TreeObserver`

Count mounts and unmounts through the observer, and require them equal at `detached()`.

- Good, because the seam exists and §1 gives it an end.
- Bad, because an unmount is not a destructor (ADR-0040): elements alive at teardown are dropped
  with the slab and never unmounted, and the render objects, controllers and futures are not in the
  stream at all.

## More information

Revisit §2's choice if the framework gains a per-runtime arena that can answer its own live count
without a `static`.
