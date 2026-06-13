# AGENTS.md — flui-rendering

Render tree: `RenderObject` / `RenderBox` / `RenderSliver` with Protocol-based layout. The densest crate in the workspace.

## What lives here

- **RenderObject trait** — base for all renderables; split across `traits/render_object.rs` (trait surface), `storage/entry.rs` (owned storage), `storage/state.rs` (per-frame state), `storage/flags.rs` (atomic flags)
- **RenderBox** — 2D cartesian layout (most widgets); `protocol/box_protocol.rs`
- **RenderSliver** — scrollable content layout; `protocol/sliver_protocol.rs`
- **Protocol** — type-safe abstraction over layout protocols (Box vs Sliver)
- **PipelineOwner** — manages layout/paint/semantics phases with typestate-enforced ordering (`pipeline/owner.rs`)
- **Concrete render objects** — `objects/`: Padding, Center, ColoredBox, Flex, Opacity, SizedBox, Transform, etc.
- **Parent data** — `parent_data/`: BoxParentData, SliverParentData, container mixin
- **Constraints** — `constraints/`: BoxConstraints, SliverConstraints, scroll metrics (gated behind `scrolling` feature)

## Key constraints

- **Render-tree storage uses `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys** — ID offset pattern applies. No `Arc<Mutex<>>` on tree structures.
- **No `RwLock<Box<dyn RenderObject>>`** — enforced by port-check trigger #1. Boxed trait objects are owned by value in `RenderEntry<P>`.
- **Stack safety via `stacker::maybe_grow`** — recursive layout/paint/hit-test walks use `ensure_stack` (128KiB red zone / 4MiB segment). Not on wasm32.
- **`testing` feature** — opt-in test harness (`RenderTester`/`Probe` API). Forwards to `flui-layer/testing`. See `crates/flui-rendering/docs/TESTING.md` for the catalog rules.
- **`scrolling` feature** — gates `ScrollMetrics` trait + `FixedScrollMetrics` + `FixedExtentMetrics` (~452 LOC). Zero workspace consumers currently.
- **`experimental-delegates` feature** — gates delegate trait modules (~1800 LOC): custom_painter, flow, multi_child_layout, single_child_layout, sliver_grid, custom_clipper. Zero production impls.
- **Benchmarks** — `layout` and `paint` benches. `autobenches = false` (shared `benches/helpers.rs` module).
- **Integration tests** — 31 test files under `tests/`: render_object_harness.rs (catalog CI guard), pipeline_scenarios.rs, deep_tree_stack.rs, etc.

## Architecture doc

- `crates/flui-rendering/ARCHITECTURE.md` — Flutter source mapping, mapping decisions, thread-safety surface, friction log, outstanding refactors
- `crates/flui-rendering/docs/TESTING.md` — RenderTester API, Probe, catalog rules
- `crates/flui-rendering/flutter-rendering-hierarchy.md` — Flutter class hierarchy (1352 LOC search index)
