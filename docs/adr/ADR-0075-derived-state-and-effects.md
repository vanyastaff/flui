# ADR-0075: Derived state and effects on the realm-scoped signal graph

Status: **Proposed** (2026-09-22). Split out of ADR-0074 by its adversarial review:
`Computed<T>` and `Effect` shipped in the first prototype (PR #1242, reverted from that PR)
and are designed here separately, because the prototype was wrong in four ways that a
"derived value" layer must get right before it is a contract. Nothing in this ADR is
implemented; the prototype's code is in the PR history (`git show 5c3922b4 --
crates/flui-view/src/reactive/mod.rs`) as a reference for what *not* to keep.

## Context

ADR-0074 (Accepted) gives the view layer realm-owned `Signal<T>` values and a reader
registry: a read in `build` subscribes the element, a write schedules exactly the readers.
The 3-screen app in ADR-0074 §1.2 also needs two things a plain signal does not give:

- **derived values** — "3 of 20 fields invalid", "total of the visible rows" — computed
  once per change and notifying their readers only when the *output* changes;
- **effects** — persist to disk, push to an `AnimationController`, log — run after a
  change, outside any build.

The prototype implemented both as extra node kinds in the same arena and measured well
(ADR-0074 §8.1: the "validity flips" form scenario rebuilt 4 elements instead of 44). The
review found the implementation unsound in ways the tests did not reach.

## What the prototype got wrong (requirements for this design)

1. **Effects never ran in the product.** `run_effects` was called only from
   `HeadlessBinding::pump_frame` (`crates/flui-testing`); `draw_frame_impl` in
   `crates/flui-view/src/binding.rs` and every runner in `flui-app` never invoked it. The
   green integration test proved the harness, not the product. Requirement: the effects
   phase is a named phase of the **binding's** frame (`build → effects → layout → paint`)
   with a test that drives `draw_frame`, not `pump_frame`, and the desktop/android/web
   runners are enumerated as call sites.
2. **A panic inside a computation or effect poisoned the graph.** The node index was
   pushed on `tracking` before the user closure ran and popped after; an unwinding closure
   left it there forever, so every later read in the realm registered a dead index, and
   the pending-effects list was lost with it. Requirement: tracking is an RAII guard (or
   `catch_unwind` with the pop on both paths), pending work survives a panicking effect,
   and the panic is contained the way `build_or_recover` contains a `build` panic.
3. **Diamond glitches.** `mark` recomputed dependents in registration order and
   `ensure_fresh` looked only at its own `stale` flag, so `C = f(A, B)` with `A` and `B`
   both derived from `S` could observe a new `A` with a stale `B` — concretely,
   `current = items[clamp(raw, items.len())]` indexed past the end after a `pop` inside
   `set`. The claimed Leptos stale/check/clean discipline was not implemented (it was
   two-phase eager). Requirement: topological (height-ordered) or mark-then-pull
   evaluation with the three-state algorithm actually implemented and a diamond test that
   fails on the prototype.
4. **Eager recomputation with zero readers, and no guard against a computation writing
   its own source.** Requirement: a computed value with no readers is lazy (recomputed on
   the next read only); a write to a source from inside a computation is a typed error
   (`SignalError::WrittenDuringCompute`), not recursion.

Two more from the correctness review:

5. A write whose only readers are effects must still **wake the frame** (under
   `ControlFlow::Wait` nothing else would), through a wake-only path on the external
   scheduler — or the ADR must say effects run only when something else wakes a frame.
6. A LayoutBuilder-scoped drain in the same frame can absorb an effect's write
   (`build_owner.rs` `service_child_requests_impl`), so "lands in the next frame" is
   only true for the main build phase; the guarantee must be stated precisely.

## Decision (proposed)

Design `Computed<T>` and `Effect` as ADR-0074's §5.1 sketched them — `Copy` handles into
the same realm arena, `PartialEq` on a computed output by construction, effects owned by
their creating element or the realm — with the six requirements above as acceptance
criteria, each backed by a test that fails against the reverted prototype:

| Requirement | Test |
|---|---|
| effects run in the product frame | a `flui-view` binding test drives `draw_frame` and observes the effect; grep-guard that every runner's frame closure calls the effects phase |
| panic containment | an effect that panics leaves the graph reading normally and the other pending effects run |
| glitch freedom | diamond `S → A, B → C` asserts `C` never sees mixed generations; the `items[clamp]` case from the review |
| lazy with no readers | a computed value with no readers is not recomputed on source writes (counter) |
| write-in-compute | typed error, no recursion |
| wake on effect-only write | scheduler counter shows one frame request |
| same-frame absorb | documented and asserted, whichever way it is decided |

The reader registry, the arena, `SignalSlot` identity and the run-time refusals are
inherited from ADR-0074 unchanged; this ADR adds node kinds, not a second graph.

## Alternatives

- **No derived layer**: compute in `build` from the signals read (a "Save" cell reads all
  20 fields). Correct today; costs a rebuild of that cell on every keystroke. The
  bench keeps this shape as the baseline the derived layer must beat.
- **`Memo<V>`/`can_update` only** (C1's view-level memoisation): stops propagation at an
  unchanged *view*, not an unchanged *value*; complementary, not a replacement.

## Consequences

Until this ADR is accepted, application code derives values in `build` and runs side
effects from callbacks, `did_update_view` or realm commands. The prototype's numbers in
ADR-0074 §8.1 for the "validity flips" scenario are kept there as a measurement of the
reverted design, not as a promise.

## References

ADR-0074 §5, §8.1; PR #1242 review threads (correctness, adversarial); `docs/research/state-model-2026.md`
§2 (Leptos three-state), §6 (Compose `derivedStateOf`), §7 (Solid memos).
