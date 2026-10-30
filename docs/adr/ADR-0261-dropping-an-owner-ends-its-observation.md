# ADR-0261: Dropping a build owner ends its tree observation

- **Status:** Accepted (2026-10-28). Not implemented on `main`: `BuildOwner`'s `Drop` reclaims its
  key claims only (`crates/flui-view/src/owner/build_owner.rs`; §1 is in review with the flui-view
  guard work), and `flui-testing` has no `DropLedger`; the tests that count drops today each keep a
  probe of their own.
- **Date:** 2026-10-28
- **Supersedes:** [ADR-0040](ADR-0040-tree-observation-seam.md) §3, the "Teardown
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
  in the framework would be a `static`), [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (a
  double two test files need lives in the kit)
- **Sources:** LeakCanary, *How LeakCanary works*
  (<https://square.github.io/leakcanary/fundamentals-how-leakcanary-works/>);
  `flutter/flutter@97cd226ec5ee4c4075b8516ea294152dba1fbfcc`
  `packages/flutter/lib/src/foundation/memory_allocations.dart`;
  `chromium/chromium@b57678612bff5bf5c88c18146bdf5878292e6384`
  `content/web_test/browser/leak_detector.h`

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

1. `BuildOwner`'s `Drop` calls `detached()` once on the observer still in its slot.
2. A teardown hook in the UI runtime that calls `clear_tree_observer` before dropping the owner.
3. Keep the embedder's obligation.

For §2: 2a, debug counters in the framework; 2b, ownership sentinels planted by the test's
application from one ledger in the testing kit; 2c, the `TreeObserver` itself.

## Decision

Chosen option for §1: 1, because the owner is the one value that always goes when its tree goes.

### §1. The owner's drop ends the stream

`BuildOwner`'s `Drop` leaves the observer in its slot and calls `detached()` once on a clone of
its `Arc`, through the contained call `clear_tree_observer` and `set_tree_observer` already use for
the observer they displace (`notify_detached`). Nothing of the owner is borrowed during the call.
A panic in `detached()` is contained and logged as for any observer (ADR-0040 §6), its payload and
that clone are retained, and the drop goes on.

```rust
// flui-view
impl Drop for BuildOwner {
    fn drop(&mut self) {
        // … reclaim the owner's key claims
        if !std::thread::panicking()
            && let Some(observer) = self.tree_observer.as_ref()
        {
            notify_detached(Arc::clone(observer)); // contained; the slot keeps its Arc
        }
    } // the fields drop after this, in declaration order, the slot among them
}
```

The observer stays in the slot because the slot is a field: taking it out would move its `Arc`'s
drop ahead of the other fields', and the owner's fields are dropped in an order the key-retirement
rules rely on. After `clear_tree_observer` or a replacement the slot holds no observer, or a new
one, and the old observer was told then, so every stream has exactly one end. `detached()` is the
last call the observer gets from that owner: the drop runs after the tree's unmounts, and nothing
of the owner emits after it.

On an exceptional path the observer is not called and not dropped:

- **During an unwind.** `BuildOwner::drop` runs as the unwind passes, so `thread::panicking()` is
  true there, and the drop does not call `detached()`; the slot's own `Drop` retains the `Arc`
  (`mem::forget`) instead of dropping it (ADR-0127 §2–3).
- **A close that holds a failure (ADR-0123).** Here `thread::panicking()` is false, since the
  failure was caught, so the drop could not tell; it is never asked. That path never drops the
  owner: `UiRuntime`'s `Drop` and presentation removal decide from the failure they hold
  (`first.is_some()`) or from the presentation's own retained mode (`preserving_close()`), close
  the presentation with `CloseMode::PreservingFailure`, and retain it whole with `mem::forget`
  (`crates/flui-runtime/src/ui_runtime/mod.rs`, `ui_runtime/presentations.rs`). The
  `BuildOwner` inside is never dropped, so its `Drop`, `detached()` included, never runs, and the
  observer's `Arc` is retained with it.

Such a stream ends without `detached()`, as it does today for an observer that panicked. A path
that holds a failure and still drops the owner would be a defect of that path, not of this rule;
`a_close_that_holds_a_failure_never_detaches` (`flui-runtime`) closes a presentation that holds a
caught failure and asserts that `detached()` was not called and the observer is still alive. A
`dispose` that panics is not such a close: it is contained where it runs (ADR-0040 §6), the close
stays healthy, the owner drops, and the stream ends with `detached()`.

### §2. A teardown test proves the release with sentinels from one ledger in the testing kit

Chosen option for §2: 2b. A closed UI runtime must release every element, render object,
animation controller, ticker, text and scroll controller it created, and its routes, overlay
entries and hero flights, and a test must say "created equals released". The test's application
plants a sentinel in each value it hands the framework, and the sentinel's `Drop` is what counts:

```rust
// flui-testing; flui::testing::DropLedger
#[derive(Clone, Default)]
pub struct DropLedger { /* Arc<{ planted: AtomicUsize, dropped: AtomicUsize }> */ }
impl DropLedger {
    pub fn plant(&self) -> DropSentinel; // planted += 1
    pub fn planted(&self) -> usize;
    pub fn live(&self) -> usize;         // planted − dropped
}
#[must_use]
pub struct DropSentinel { /* the ledger */ } // Send + Sync; Drop: dropped += 1, once
```

One ledger per kind of value (states, controllers' listeners, route pages, futures), so a failure
names the kind by the ledger that kept a live sentinel, not by a label. A sentinel goes where the
value's owner drops it: a field of a `ViewState`, a capture of a controller's listener or of a
route's page builder, a capture of a `FutureBuilder` future. A framework value that leaks keeps
what it captured, and with it the sentinel; a slab node outlives its runtime only through `forget`
or a leaked runtime, and both leave the sentinel alive. The typed refusals of stale handles after a
UI runtime is dropped (`OwnerGone`, `Closed`) stay the second proof that the UI runtime itself is
gone.

The ledger is one type in the kit, not a copy per test: the tests that count drops today keep six
hand-written probes (`DropProbe` in `flui-widgets` and `flui-animation`, `DropCounter`, `DropFlag`),
and a double two test files need moves into the kit (ADR-0243 D4). They move to it when they are
next touched: a test under `tests/` uses `flui-testing` as a dev-dependency even in a crate below
it (ADR-0243 D2). Only a unit test inside a crate's `src/` keeps a probe of its own, since it cannot
pass the crate's types to the kit, and the testing allowlist names it.

Why sentinels, and not counters or the observer:

- **A sentinel proves the destructor ran.** LeakCanary watches objects the framework has
  destroyed through weak references and reports the ones still reachable after a collection (*How
  LeakCanary works*); a sentinel is that watch where values are freed by ownership, not by a
  collector, so the check needs no wait and no collection.
- **Counters need a process-global registry.** Flutter's leak tracking rests on framework objects
  dispatching creation and disposal events to a shared instance (`memory_allocations.dart`), and
  Chromium's web-test leak detector compares DOM object counts between two pages
  (`leak_detector.h`). Both count what the framework announces from a process-wide place; in FLUI
  that is a `static` with an ADR-0097 grant, and a decrement counts a call, not a destructor. They
  earn their keep where objects are collected and opaque to the test, which a slab freed by its
  owner is not.
- **The observer sees unmounts,** and an unmount is not a destructor (ADR-0040); render objects,
  controllers and futures are not in its stream.

What the sentinels cannot see, a framework-internal cycle that holds no application value, is
bounded: every route, overlay entry, hero flight, controller and future the requirement names holds
an application value (a page builder, a listener, a view, a future), and a leaked tree or runtime
holds the states planted in it.

### Consequences

- Good, because a devtools panel sees every closed window's stream end, without any host code.
- Good, because the "teardown hook that guarantees `clear_tree_observer`" follow-up of ADR-0040
  needs no hook.
- Bad, because the drop of a `BuildOwner` now runs user code, contained, where it ran none.
- Neutral, because a stream that ends on an exceptional path still ends silently, by the retention
  rule every container follows.
- Good, because a release test proves destructors ran, with one kit type and no `static`.
- Bad, because a release test sees only what its application planted: a framework-internal cycle
  that holds no application value passes it.

### Confirmation

- `dropping_an_owner_with_an_observer_detaches_it_once` (`flui-view`): an owner dropped with an
  observer calls `detached()` once; after `clear_tree_observer` the drop calls nothing; no event
  follows `detached()`; an owner dropped during an unwind calls nothing and retains the observer;
  the owner's key-retirement rows keep passing, since the slot's field order is unchanged.
- `a_close_that_holds_a_failure_never_detaches` (`flui-runtime`, new): the presentation is
  retained whole, its owner never dropped, `detached()` never called.
- `stale_handles_refuse_after_the_runtime_drops` (new), one row per handle type.
- §2, three new tests: `acceptance_app_close_releases_every_owner_after_navigation` (the
  acceptance app through the facade: Home, Note, Settings and back, then close; every ledger's
  `live()` is 0, and was above 0 before the close),
  `last_window_close_releases_the_runtime_through_the_runner` (`flui-app`'s headless runner, a
  tree with sentinels closed by its last window), and `drop_ledger_counts_each_sentinel_once`
  (`flui-testing`: a sentinel dropped once counts once, a `mem::forget` leaves it live).

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

#### 2b. Ownership sentinels from one ledger in the testing kit (chosen)

The test's application plants a guard in each value it hands the framework (a capture, a
controller's listener, a route's page); a guard reports its own `Drop` to a ledger the test reads.
The ledger is `flui_testing::DropLedger`, so the acceptance app and the runner's tests share it.

- Good, because it proves the destructor ran. A slab node outlives its runtime only through
  `forget` or a leaked runtime, and both leave the sentinel alive.
- Good, because it needs no framework surface beyond the kit, and no `static`.
- Good, because it replaces the probes the tests already copy.
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
without a `static`, or if a leak is found that holds no application value.
