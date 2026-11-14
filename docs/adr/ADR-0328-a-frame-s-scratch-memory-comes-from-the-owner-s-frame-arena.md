# ADR-0328: A frame's scratch memory comes from the owner's frame arena

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Related:** [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) (the arena is
  owner-thread state of one pipeline owner);
  [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  (it is neither `Send` nor `Sync` and no signature says it crosses);
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no `static` or `thread_local!` holds it);
  [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) (the
  retained scene, which stays outside the arena);
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (a phase that
  unwinds drops its scratch on the way out)

## Context and problem statement

`cargo xtask perf` counts what a frame asks of the heap. A run with a probe that tracks each
allocation's lifetime (Linux x86_64, dev profile, `main` of 2026-11-12) shows that most of a busy
frame's allocations do not outlive the frame:

| Scenario | Allocations | Freed inside the frame |
|---|---|---|
| `list_10k_scroll_one_screen` | 3145 | 1776 (56 %) |
| `full_reassemble` | 1473 | 806 (55 %) |

The freed ones are scratch memory of a phase walk, allocated once per node per frame and freed
before the frame ends. The largest sites were in layout (the per-child state a parent lays its
children out with), in the semantics assembly (each node's fragment list, its conflict marks, the
built nodes the pass publishes and drops) and in build (the desired-parent lists of a render-edge
sync in `flui-view`).

Each site knows its scratch dies with the walk. The only tool for that was a buffer kept on the
owner and reused, which works for one flat loop and not for a recursive walk: every level of the
recursion needs its own list at the same time. Rust 1.100 stabilises the allocator API
(`std::alloc::Allocator`, `Vec::new_in`, `Box::new_in`), so a collection can name where its memory
comes from without a crate of its own collections. Where should a frame's scratch memory come
from?

## Decision drivers

- A steady frame allocates nothing from the global allocator (the allocation program's goal);
  the perf gates show it per scenario.
- Nothing an application or a render-object author writes changes: no new parameter, type or
  lifetime in `RenderBox`, `RenderSliver`, a view or a widget.
- The compiler proves that no scratch value outlives the reset that frees its memory; no
  run-time check on the frame path.
- The frame path is synchronous and owner-thread; no lock, no atomic.
- Memory a spike took is given back: one heavy frame does not pin its high-water mark forever.
- An idle frame allocates nothing, the arena's own bookkeeping included.
- A phase that panics and is caught leaves the owner able to run its next frame.
- Data that lives across frames (display lists, the semantics tree, layer trees) is not scratch;
  this record does not move it.

## Considered options

1. An owner-local frame arena behind the stable `Allocator` trait.
2. Buffers kept on the owner and reused, one per site.
3. A bump-arena crate with its own collection types (`bumpalo`).
4. A thread-local arena every site reaches.

## Decision

Option 1. `flui_foundation::arena` holds `FrameArena`; each `PipelineOwner` owns its arenas, a
phase walk lends them to its scratch collections, and the walk resets them as it begins.

### §1 The arena

```rust
// flui_foundation::arena
pub type FrameVec<'f, T> = Vec<T, &'f FrameArena>;

pub struct FrameArena { /* chunks, a bump cursor, the peaks of recent frames */ }

impl FrameArena {
    pub const fn new() -> Self;
    /// Frees every block at once. `&mut self` proves no collection still borrows the arena.
    pub fn reset(&mut self);
}

// SAFETY: a block is handed out at most once between two resets (§2).
unsafe impl Allocator for &FrameArena { /* bump; free and grow the last block in place */ }
```

- A bump allocator over chunks taken from the global allocator. An allocation that does not fit
  opens a chunk of at least twice the current one, so a frame takes `O(log n)` chunks.
- `deallocate` moves the cursor back when the block is the last one handed out, and `grow` and
  `shrink` resize the last block in place; any other block waits for `reset`. A walk that frees in
  reverse order of allocation reuses its memory within the frame.
- The arena records the most bytes it had handed out at once since the last reset (its peak).
  `reset` keeps the newest chunk when it holds that peak and is at most four times the largest
  peak of the last 120 resets that followed an allocation, and releases every other chunk. A
  reset that follows no allocation changes nothing, so passes with no scratch to list neither age
  the recent peaks nor release the chunk they sized. `reset` never allocates: when it keeps no
  chunk, the next allocation opens one sized for the largest recent peak. So a steady frame bumps
  through one kept chunk, an idle frame touches the global allocator not at all, and a chunk a
  spike grew is released once 120 busy resets have passed without needing it.
- `FrameArena` is `!Send` and `!Sync` (it holds `Cell`s and pointers); no `unsafe impl` changes
  that. It holds no `static` and no thread-local.

### §2 What the compiler proves

A scratch collection is `FrameVec<'f, T>`. It borrows the arena for `'f`, and `reset` takes
`&mut FrameArena`, so a collection alive at `reset` is a borrow error. The `unsafe impl` has one
invariant: a block is handed out at most once between two resets, which the bump cursor holds by
construction, since it moves back only over the last block when that block is freed or shrunk.
A block stays valid while the `&'f FrameArena` lives, because only `reset` and the arena's drop
free a chunk. Miri runs the arena's unit tests (the `flui-foundation` miri scope).

### §3 Who holds it and how a walk reaches it

The `PipelineOwner` owns its arenas beside its other per-frame state, so two UI runtimes on one
thread never share one (ADR-0266). A phase walk resets them as it begins (`&mut self` on the
owner), then lends `&FrameArena` to the walk's own context; the semantics assembly carries them in
its `AssemblyRegions`.

A walk keeps two kinds of memory apart, one arena each. Scratch (`frame_arena`) is freed before
the level that listed it returns: a fragment list, conflict marks, a reordering. What a level
builds for its parent (`built_arena`) outlives the level until the walk hands it on: the semantics
nodes the pass builds and their child lists, which it moves into the semantics owner. In one arena
a built list would sit above every scratch list freed after it, so none of those would be reused
within the walk and the frame's peak would grow to all of them. Apart, the scratch arena reuses
each level's memory for the next. A level takes over the first non-empty list a child returns
and reserves room for its remaining children while it is still the last block; each later
child's list is emptied into it and freed as the last block; and the level drops its scratch in
the reverse order it took it. A block freed while another sits above it (a list that must grow
past its size, the lists inside fragments a barrier discards) waits for the reset. A level that absorbs a child's node
as its first fragment takes over that node's child list: a chain of absorbed nodes with no
siblings hands one list up without copying it, and a level with fragments after it moves the list
once, as it would copy it into a new one. The built arena's peak is the built tree, plus a failed graft's partial nodes when
the pass falls back to a rebuild. Resetting at the start of a walk needs no frame-end hook, and a walk that
unwound has dropped its collections before the next one resets. `flui-view`'s build phase gets
an arena of its own on the `BuildOwner` under the same rules when its first site moves.

A render object's `perform_layout` and `paint` never see the arena. What they hand the pipeline
(a size, painted commands) is the pipeline's to store, and it stores retained data outside the
arena.

### §4 Panics

A phase that unwinds drops its scratch collections on the way out; their `deallocate` is a cursor
move or nothing, and cannot panic. The arena itself runs no destructor of a value: each collection
drops its own elements, so a user callback held in scratch (a semantics action handler) is
dropped by its collection, as it would be from a global `Vec`, and never by `reset`.

### §5 What moves to it

A site moves when the probe shows its allocation freed inside the frame. An allocation the probe
shows that can be removed is removed instead of moved: the layout walk's per-frame copy of each
child's parent data and the proxy layout context's own child-size list went that way. Each moving
step is its own pull request with the perf gates' before and after counts in its description and
blessed baselines. The sites still to move are listed in
[`docs/gaps.md`](../gaps.md#accepted-adr-sections-not-built-yet).

### Flutter counterparts

Flutter has none: Dart's generational collector makes short-lived allocations cheap, and its
framework never chooses where a list's memory comes from. FLUI has no collector, so a frame's
scratch costs a `malloc` and a `free` per site per node; this record removes that cost. It is a
pure Rust performance gain and changes nothing an author writes.

### Consequences

- Good: a frame's scratch costs a cursor move; steady frames move toward zero global
  allocations, and the perf gates hold each step.
- Good: recursion gets scratch per level for free, which a reused buffer cannot give.
- Bad: scratch collections carry an allocator parameter inside the pipeline, which their
  signatures show, and a scratch type that holds one carries the arena's lifetime.
- Bad: `unsafe impl Allocator` is new unsafe code in `flui-foundation`; miri covers it.
- Neutral: memory held between frames is one chunk per arena, sized by the peaks of recent frames.

### Confirmation

- `flui-foundation`'s `arena::tests`, under miri: a frame of the same shape reuses the one kept
  chunk, a frame that frees in order keeps its chunk, a reset never allocates, idle resets keep
  the chunk busy frames need, a spike's chunk is released after the window, the last block
  frees, grows and shrinks in place, and zero-size and over-aligned layouts are honoured.
- A `compile_fail` doctest on `FrameArena`: a `FrameVec` alive across `reset` is error E0502.
- `cargo xtask perf`: each moving pull request lowers `allocations` in the scenarios it touches,
  no scenario's count rises, and `idle_frame_retained_scene` stays at zero.

## Pros and cons of the options

### 1. An owner-local frame arena behind the stable `Allocator` trait

- Good: std collections, std API; no second collection library.
- Good: the borrow proves the reset safe at compile time.
- Good: owner-local, so runtimes stay isolated.
- Bad: a new unsafe trait implementation to maintain.

### 2. Buffers kept on the owner and reused

- Good: no unsafe code.
- Bad: one buffer per site, and a recursive walk needs a stack of them; the semantics assembly
  already threads several reused buffers by hand.
- Bad: a buffer that grew for one frame keeps its capacity forever.

### 3. A bump-arena crate with its own collections

- Good: mature implementation.
- Bad: its own `Vec`, `String` and `Box` types; code moving to the arena changes collection
  types, not only where they allocate.
- Bad: a new dependency on the frame path for what the standard library now offers.

### 4. A thread-local arena

- Good: no parameter to pass.
- Bad: process state, not runtime state (ADR-0097, ADR-0266); two runtimes on one thread would
  reset each other's scratch.
- Bad: nothing proves a value dead at reset; it would need a run-time check.

## More information

- The allocation profile behind the table: `cargo xtask perf` with a lifetime probe on the
  counting allocator of `crates/flui-widgets/tests/perf.rs`.
- Rust 1.100.0 release notes: the allocator API section.
