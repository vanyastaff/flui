# ADR-0405: An element's view-independent code is compiled once

- **Status:** Proposed (2026-11-27)
- **Date:** 2026-11-27
- **Related:** [ADR-0245](ADR-0245-a-view-handed-back-unchanged-is-not-built-again.md) (an element
  holds the `Rc<V>` its view came in), [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md)
  (the panic containment the moved halves keep), [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (where a rule is held)
- **Sources:** `flutter/flutter@3.50.1`, `packages/flutter/lib/src/widgets/framework.dart`:
  `abstract class Element` (line 3575), its `Widget get widget` (3671), `ComponentElement`
  (5793), `StatelessElement.build`, `(widget as StatelessWidget).build(this)` (5907),
  `StatefulElement.build`, `state.build(this)` (5949), `RenderObjectElement` (6635)

## Context and problem statement

Every element is one `Element<V, A, B>` (`crates/flui-view/src/element/unified.rs`), generic over
its view type. Rust compiles a generic function once for each type it is called with, so each view
type an application uses gets its own copy of every element method, and of every helper those
methods call that names `V`. What bears on it:

| Fact | Evidence |
|---|---|
| The generated counter links 33 element types. Their `ElementBase` methods take 325,983 bytes of machine code, 9,878 bytes per type: `build_into_views` 172,248, `update` 56,021, `unmount` 31,945, `mount` 29,605 | `nm -S` of the counter built with the template's release profile, unstripped, v0 symbols, at `641799f6a` |
| Most of that copy does not depend on the view: the dirty check and its trace, building the `BuildContext`, recording a caught panic and substituting the `ErrorView`, inserting and adopting the render object, the `BoxedView` unwrap of an update. Of those bodies only `view.build(ctx)`, `create_render_object`, `update_render_object`, the downcast to `V` and `should_skip_rebuild` name `V`; the rest needs `TypeId::of::<V>()` at most, as a value | `behavior.rs`, `behavior_commons.rs`, `render_custody.rs`, `dispatch.rs` |
| `CoreState` already took the cold lifecycle transitions (`mount`, `activate`, `set_pipeline_owner`) out of the generic core for this reason; the build, update and render-custody helpers still took `&ElementCore<V, A>` and were inlined into each copy | `crates/flui-view/src/element/generic.rs` |
| Per view type is where a FLUI application grows faster than a Flutter one: the Material counter links 135 element types and the notes example 166 | the size audit (counter, Material counter, notes), 2026-11-23 |
| Flutter's `Element` is one class tree that is not parameterized by the widget. A `StatelessElement` reads its widget through `Element.widget` and calls `(widget as StatelessWidget).build(this)`: the only per-widget code is the widget's own `build` | Sources |

How much of an element is compiled once per view type?

## Decision drivers

- An application's binary grows with the code its views write, not with a copy of the framework
  per view type.
- The authoring API does not change: `StatelessView`, `StatefulView`, `RenderView` and their
  hooks keep their signatures, and an author writes no new code.
- The frame path keeps its cost: no virtual call or downcast is added where a build or an update
  runs per frame.
- Panic containment (ADR-0287) and every lifecycle record keep their order and their attribution
  to the element's view type.

## Considered options

1. Keep the element generic and inlined (today).
2. Keep `Element<V, A, B>` generic and give every view-independent half its own function, compiled
   once, over `CoreState` and the view's `TypeId`.
3. One non-generic element, as Flutter has: the view held as `Rc<dyn View>`, the behavior as a
   trait object, each hook downcasting to its view type.

## Decision

Chosen option: 2, because it removes the framework's copy per view type, the cause, with no change
to how views are written and no new indirection on the frame path. Option 3 goes further only by
the part that names `V`, which is the view's own code in both shapes.

### §1. A helper that does not name `V` takes `CoreState`

The element's view-independent state is `CoreState` (`element/generic.rs`), reached as
`ElementCore::state`. A helper whose body needs nothing of `V` but its `TypeId` takes `&CoreState`
and the `TypeId` as a value, not `&ElementCore<V, A>`. A generic function keeps only the
expressions that name `V` (the call of the author's hook, the downcast, the comparison) and hands
the rest to such a helper.

### §2. A body that runs after user code is its own function

The half that runs after a caught hook (recording the panic, `ErrorView` substitution, the render
object's insertion and adoption, its removal) and the traces of the build and update paths are
functions marked `#[inline(never)]`. A function the release profile's optimizer would inline into
each caller is copied again into every view type's code, so the attribute is what keeps it once.
They are `finish_build`, `finish_child_views`, `should_build_traced`, `live_build_ctx`,
`finish_armed_hook`, `record_contained_hook`, `finish_provided`, `should_build_render`,
`finish_render_build`, `adopt`, `push_target`, `update_context`, `finish_push`,
`finish_unmount`, `remove_render_object_from_tree`, `unwrap_boxed` and the dispatch's traces.
`adopt` is generic over the render protocol alone, so it is compiled once per protocol.

### §3. What stays generic

The call of the author's hook, the `BoxedView` the build returns, the downcast of an update and
`should_skip_rebuild` stay in the generic function. That is the code Flutter also has per widget:
its `build`.

### Consequences

- Good, because the counter's element code falls from 325,983 to 196,598 bytes (9,878 to 5,958 per
  type, 40%), for 8,685 bytes of helpers compiled once, and its stripped binary from 21,347,096 to
  21,219,328 bytes. At the counter's 3,920 bytes per type, an application with 135 element
  types (the Material counter) would save about 0.5 MB; that is an estimate, not a build.
- Good, because no author-facing signature changes, and a build or an update makes the same calls
  in the same order.
- Bad, because a helper call that was inlined is now a call: a dirty check and a trace on each
  element built. They run once per element per build, against the author's `build`.
- Neutral, because what remains per type (5,958 bytes) is the view's own `build`, `update`'s
  downcast and comparison, and each behavior's hooks; option 3 could not remove the first two.

### Confirmation

- The CI `size` job's counter bytes (`perf/size.toml`), lowered from `main`'s measurement once
  this lands, hold the total.
- A reader of "Before a merge" checks that a new helper in `element/` that does not name `V`
  takes `CoreState`, and that a body after a caught hook is `#[inline(never)]`.
- `flui-view`'s suite holds the behavior: build recovery, lifecycle records and render custody
  are unchanged by it.

## Pros and cons of the options

### 1. Keep the element generic and inlined

- Good, because nothing changes.
- Bad, because each view type copies about 10 KB of framework code, so an application's binary
  grows with the number of its view types faster than with its own code.

### 2. View-independent halves compiled once

- Good, because it removes the copy without changing the authoring API or the element's shape.
- Good, because each step is local: a helper at a time, measured.
- Bad, because the split between a generic function and its once-compiled half is a convention
  the compiler does not check; a new helper can take `&ElementCore<V, A>` again.

### 3. One non-generic element

- Good, because it matches Flutter's shape, where the element is one class tree.
- Bad, because each hook downcasts its view and its state through `dyn Any` on every build and
  update, which the generic element does not.
- Bad, because the behaviors become trait objects held beside the element, one more allocation
  per element, against ADR-0245's shared view.
- Bad, because what it removes beyond option 2 is the view's own code, which it keeps per type
  behind a vtable.
