# AGENTS.md — flui-rendering

Render tree: `RenderObject` / `RenderBox` / `RenderSliver` with Protocol-based layout. The densest crate in the workspace.

## What lives here

- **RenderObject trait** — base for all renderables; split across `traits/render_object.rs` (trait surface), `storage/entry.rs` (owned storage), `storage/state.rs` (per-frame state), `storage/flags.rs` (atomic flags)
- **RenderBox** — 2D cartesian layout (most widgets); `protocol/box_protocol.rs`
- **RenderSliver** — scrollable content layout; `protocol/sliver_protocol.rs`
- **Protocol** — type-safe abstraction over layout protocols (Box vs Sliver)
- **PipelineOwner** — manages layout/paint/semantics phases with typestate-enforced ordering (`pipeline/owner/`)
- **Concrete render objects live in the sibling `flui-objects` crate** (Padding, Center, ColoredBox, Flex, Opacity, SizedBox, Transform, etc.) — a dev-only dependency here; this crate ships the protocol/pipeline machinery only
- **Parent data** — `parent_data/`: BoxParentData, SliverParentData, container mixin
- **Constraints** — `constraints/`: BoxConstraints, SliverConstraints, SliverGeometry, GrowthDirection

## Key constraints

- **Render-tree storage uses `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys** — ID offset pattern applies. No `Arc<Mutex<>>` on tree structures.
- **No `RwLock<Box<dyn RenderObject>>`** — enforced by port-check trigger #1. Boxed trait objects are owned by value in `RenderEntry<P>`.
- **Stack safety via `stacker::maybe_grow`** — recursive layout/paint/hit-test walks use `ensure_stack` (128KiB red zone / 4MiB segment). Not on wasm32.
- **`testing` feature** — opt-in test harness (`RenderTester`/`Probe` API). Forwards to `flui-layer/testing`. See `crates/flui-rendering/docs/TESTING.md` for the catalog rules.
- **`experimental-delegates` feature** — gates delegate trait modules with zero production impls: custom_clipper. (`sliver_grid`/`custom_painter`/`flow`/`single_child_layout`/`multi_child_layout` ship unconditionally now that `RenderSliverGrid`/`RenderCustomPaint`/`RenderFlow`/`RenderCustomSingleChildLayoutBox`/`RenderCustomMultiChildLayoutBox` landed — ADR-0007.)
- **Benchmarks** — 5 benches: `layout`, `paint`, `virtualizer`, `intrinsic_parent_data`, `semantics_assembly`. `autobenches = false` because the first three share the `benches/helpers.rs` module (declared via `mod helpers;`), which must not compile as its own bench target.
- **Integration tests** — 42 files under `tests/` (41 test modules + `main.rs`), all compiled into the single `rendering_it` binary via `tests/main.rs` (`autotests = false`); byte-identical scaffolding is shared through `tests/common/`. The render-object catalog CI guard (`render_object_harness.rs`) lives in `flui-objects`.

## Architecture doc

- `crates/flui-rendering/ARCHITECTURE.md` — Flutter source mapping, mapping decisions, thread-safety surface, friction log, outstanding refactors
- `crates/flui-rendering/docs/TESTING.md` — RenderTester API, Probe, catalog rules
- `crates/flui-rendering/flutter-rendering-hierarchy.md` — Flutter class hierarchy (1352 LOC search index)
