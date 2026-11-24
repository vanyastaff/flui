# ADR-0399: A frame phase borrows the pipeline owner it runs on

- **Status:** Implemented (2026-11-24)
- **Date:** 2026-11-24
- **Related:** [ADR-0048](ADR-0048-frame-transaction-boundary.md) (a frame that unwinds leaves the
  presentation's owner in its `PipelineCell`, which this decision holds by a borrow instead of a
  catch); [ADR-0017](ADR-0017-build-during-layout-callback-seam.md) (the layout pass the
  build-during-layout fixpoint drives); [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md)
  (the phases commit before they call out, unchanged);
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (the by-value
  frame that destroyed its owner under containment, which goes)
- **Sources:** `flutter/flutter@3.50.1` `packages/flutter/lib/src/rendering/binding.dart:691`
  (`drawFrame` calls `flushLayout`, `flushCompositingBits`, `flushPaint` and `flushSemantics` on
  the root pipeline owner in place), `packages/flutter/lib/src/rendering/object.dart:1138`,
  `:1240`, `:1294` and `:1464` (the four flushes), `:1128` and `:1285` (`_debugDoingLayout`,
  `_debugDoingPaint`: Flutter's ordering is a debug-mode flag)

## Context and problem statement

`flui-rendering` keeps the order of a frame's phases in types: `PipelineOwner<Phase>` carries a
zero-sized marker, each `run_*` method lives on its phase's impl block, and a phase is left by a
by-value move to the next marker (`rebind_phase`, the "Phase ORDERING lives in the type system"
entry of [`crates/flui-rendering/ARCHITECTURE.md`](../../crates/flui-rendering/ARCHITECTURE.md)).
The order is right; the moves are what it costs:

| Fact | Evidence |
|------|----------|
| A `PipelineOwner` is 4,240 bytes | `size_of::<PipelineOwner>()` on Linux x86-64 at the commit this record lands with |
| One frame moves it about a dozen times | `run_frame_in_place` builds a vacant owner for the slot (`PipelineOwner::vacant`), swaps it in, passes the owner by value to `run_frame_contained`, which rebinds it five times (`into_layout`, `into_compositing`, `into_paint`, `into_semantics`, `finish`), returns it in a tuple and swaps it back |
| A label change spends about 4% of its instructions copying it | callgrind over `perf_scaling_a_label_change_costs_the_same_on_any_tree_size` (`memcpy` called from `in_place`, `run_frame_contained`, `over`, `into_layout`, `into_idle` and `Semantics::finish`) |
| `rebind_phase` lists every field by hand | `crates/flui-rendering/src/pipeline/owner/mod.rs`: about fifty fields, a mirror each new field must join |
| The slot is kept on an unwind by catching it | `contain_phase` and the pending-dirty drain run under `catch_unwind` so the owner can be written back before the panic resumes: two `#[expect(clippy::disallowed_methods)]` sites that exist only because the owner leaves its slot |

Flutter runs the same four phases on the owner in place and checks their order with debug flags.
The question: how does FLUI keep the order a compile error without moving the owner?

## Decision drivers

- An out-of-order phase stays a compile error, not a runtime condition (ADR-0303: rules are types).
- A frame copies no owner: the cost of a frame does not grow with the fields the owner gains.
- The owner stays in its slot on every exit, an unwind included (ADR-0048), without a placeholder
  that a retry could run on.
- Relocation, actions and the other operations of an idle owner stay unreachable while a phase
  runs.
- The frame path is synchronous and owner-thread only: no lock, no `Rc` indirection on per-node
  state.

## Considered options

1. Keep the by-value typestate.
2. Box the owner's state, so a typestate move copies a pointer.
3. A runtime phase field with checks, as Flutter's debug flags.
4. A phase is a pass that borrows the owner and hands the borrow to the next phase.

## Decision

Chosen option: 4, because it is the only one that keeps the order a type error and moves nothing.

### §1. The owner has no phase parameter; a phase is a `FramePass` over `&mut PipelineOwner`

```rust
pub struct PipelineOwner { /* the render tree, queues and handles */ }

/// The owner lent to one phase of a frame.
pub struct FramePass<'o, P: PipelinePhase> {
    owner: &'o mut PipelineOwner,
    phase: PhantomData<P>,
}

impl PipelineOwner {
    pub fn run_frame(&mut self, writer: &mut SceneWriter<'_>)
        -> Result<bool, TreeError<RenderError>>;
    pub fn layout_pass(&mut self) -> FramePass<'_, Layout>;
}

impl<'o> FramePass<'o, Layout> {
    pub fn run_layout(&mut self) -> RenderResult<()>;
    pub fn into_compositing(self) -> FramePass<'o, Compositing>;
}
impl<'o> FramePass<'o, Compositing> {
    pub fn run_compositing(&mut self);
    pub fn into_paint(self) -> FramePass<'o, PaintPhase>;
}
impl<'o> FramePass<'o, PaintPhase> {
    pub fn run_paint(&mut self, writer: &mut SceneWriter<'_>) -> RenderResult<()>;
    pub fn into_semantics(self) -> FramePass<'o, Semantics>;
}
impl FramePass<'_, Semantics> {
    pub fn run_semantics(&mut self) -> RenderResult<()>;
}

impl<P: PipelinePhase> Deref for FramePass<'_, P> {
    type Target = PipelineOwner;
}
```

- The markers `Layout`, `Compositing`, `PaintPhase` and `Semantics` stay; `Idle` goes. An idle
  owner is an owner no pass borrows, so every operation that needs one (relocation, actions,
  `run_frame`, a root change) takes `&mut PipelineOwner`, which the borrow checker refuses while a
  pass is alive.
- A pass ends when it is dropped. There is no `into_idle` or `finish`; the phase's work is
  already committed when its `run_*` returns or unwinds, so a dropped pass owes nothing.
- A pass reads the owner through `Deref` (shared), so a test or a bench inspects geometry and
  queues mid-frame. It has no `DerefMut`: a mutation between phases ends the pass and takes a new
  one, the same order a frame follows.
- `run_frame` is the one sequencing authority, as before. The by-value `run_frame(self)` and
  `run_frame_in_place` are one method on `&mut self`.

### §2. A frame that unwinds keeps its owner because the owner never left

`run_frame` no longer catches: a panic unwinds through the passes with the owner in its slot, and
`PipelineCell::run_frame` keeps the cell's borrow rules. `PipelineOwner::vacant`, `in_place`,
`rebind_phase`, `contain_phase`'s `catch_unwind` and the drain's `catch_unwind` are deleted with
their `#[expect(clippy::disallowed_methods)]`. A phase error still gains its path in the tree
(`in_tree`) before it is returned.

The owner after a panic in the middle of a phase is the one the phase itself leaves, because a
pass holds no state of its own: it is a borrow and a marker, and it has no `Drop`. Each phase
settles before the unwind leaves its `run_*` (the rules of "A frame that unwinds keeps its owner"
in `crates/flui-rendering/ARCHITECTURE.md`):

- the phase is closed: layout, paint and semantics call `exit_phase` on the unwind as on a return,
  which clears the scheduler's debug flag and drains the marks routed to its side queue back into
  the dirty lists, and paint also ends its local paint window;
- the work the phase did not finish stays queued: layout puts the unfinished entries of its batch
  back and commits the paint the interrupted entry owes, compositing keeps its queue and writes a
  node's bits only after its hook answered, paint keeps its queue;
- the frame is not counted: `frames_produced` grows only after semantics returns, and nothing
  marks the owner as inside a frame, so there is no flag for an unwind to leave set.

The next `run_frame` therefore redoes what the failed one owed and nothing else. There is no
"frame interrupted" mark to set or clear, and correctness never depends on a pass's destructor
running: forgetting a pass is the same as dropping it, which `FramePass` holds with
`const _: () = assert!(!std::mem::needs_drop::<FramePass<'static, Layout>>())`. A pass ended early
on purpose (a test that stops after layout) leaves the owner as the last finished phase left it.

### §3. A phase's body is the owner's, reached only through its pass

The work of a phase reads and writes the whole owner, so its body is a `pub(super)` method of
`PipelineOwner` inside the owner module, and the pass's `run_*` is its only caller. The order is a
type error on the public surface and on every path outside `pipeline::owner`; inside that module it
is the one call each pass makes.

### Consequences

- Good, because a frame moves no owner, and the cost of a frame stops depending on the owner's size.
- Good, because the hand-written field list of `rebind_phase`, the vacant placeholder and two
  `catch_unwind` sites go, and with them the window in which a slot holds a stand-in.
- Good, because an idle-only operation during a phase is still a compile error (a second borrow),
  now held by the borrow checker rather than by which impl block a method sits in.
- Bad, because a pass carries a lifetime: a test harness that kept `PipelineOwner<Layout>` in a
  field keeps the owner and takes a pass when it runs a phase.
- Bad, because inside `pipeline::owner` a phase body is callable out of order; one caller per body
  keeps that visible.
- Neutral, because the public API changes shape before the first release: `PipelineOwner<Idle>`
  is `PipelineOwner`, `into_layout` is `layout_pass`, `into_idle` and `finish` are a drop.

### Confirmation

- The compiler: `crates/flui-rendering/tests/compile_fail/paint_from_layout.rs` (no `run_paint` on
  a layout pass) and `frame_from_layout.rs` (no `run_frame` while a layout pass borrows the owner).
- `PipelineOwner` has no type parameter and no method that takes `self` by value to change phase,
  so no frame path can move it: the shape in §1 is the check.
- `tests/frame_unwind.rs`: every row of `a_frame_that_unwinds_leaves_the_pipeline_its_owner` panics
  in the middle of a phase (a paint span, a layout or paint report, a wake, a compositing hook) and
  ends by checking that no phase is left open, that the unwound frame is not counted, and that the
  retry and the frame after it paint (§2);
  `a_pass_ended_early_leaves_the_next_frame_whole` drops a pass after layout and checks that the
  next frame paints what it owed and the one after owes nothing. `flui-runtime`'s
  `a_pipeline_frame_that_unwinds_keeps_its_tree_for_the_retry` holds ADR-0048.
- The `needs_drop` assertion in `crates/flui-rendering/src/pipeline/owner/pass.rs` fails to compile
  the day a pass gains a destructor, so forgetting a pass stays the same as dropping it.
- The instruction counts `perf-gates` keeps on `main` (ADR-0396 §3) fall by the copies the
  context table names.

## Pros and cons of the options

### 1. Keep the by-value typestate

- Good, because nothing changes.
- Bad, because every frame copies the owner about a dozen times, and the copy grows with each
  field.
- Bad, because the field list in `rebind_phase` is a mirror, and the slot needs a placeholder and
  two catches.

### 2. Box the owner's state

- Good, because a move copies a pointer and `rebind_phase` becomes one field.
- Bad, because the slot still needs a placeholder while the owner is out, which is one allocation
  per frame or an `Option` that every reader of the slot must handle; the catches stay.
- Bad, because every field access in a layout or paint walk goes through one more pointer.

### 3. A runtime phase field

- Good, because it is Flutter's shape and moves nothing.
- Bad, because an out-of-order phase becomes a runtime error that a test must reach, against
  ADR-0303, and the ordering entry of `ARCHITECTURE.md` is reversed for no gain the borrow does
  not also give.

### 4. A pass that borrows the owner

- Good, because the order stays in types, nothing moves, and the owner never leaves its slot.
- Bad, because a pass has a lifetime, and a phase body is a private method of the owner.

## More information

This record closes the `docs/gaps.md` entry "The pipeline owner is copied whole through its phases
every frame", and the ordering and unwind entries of `crates/flui-rendering/ARCHITECTURE.md` state
the rule as it is built here.
