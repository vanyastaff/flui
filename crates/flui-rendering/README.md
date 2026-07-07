# flui-rendering

**The render-tree engine of FLUI** — the third tree in the five-tree
architecture (View → Element → **Render** → Layer → Semantics) and the densest
crate in the workspace: traits, layout/paint/hit-test protocols, the pipeline
owner, and the render-object test harness.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io). Concrete render objects live in
[`flui-objects`](../flui-objects); this crate owns the machinery they plug
into.

## What lives here

- **Traits & protocols** — `RenderObject`, `RenderBox` (2D cartesian),
  `RenderSliver` (scrollable), with the type-safe `Protocol` abstraction and
  Arity-parameterized children (`Leaf`/`Single`/`Optional`/`Variable` —
  child-count mismatches are compile errors).
- **Pipeline** — `PipelineOwner` drives dirty-tracked layout → paint →
  composite through the Flutter contract (sync, on-demand phases). The
  re-entrant layout walk's `unsafe` is confined to one SAFETY-audited arena
  module (`pipeline/owner/subtree_arena.rs`), machine-checked by a miri CI
  job.
- **Storage** — slab-backed `RenderTree` with 1-based `NonZeroUsize`
  `RenderId`s (the workspace-wide ID offset pattern).
- **Virtualization** — protocol-agnostic windowing math backing the lazy
  sliver family.
- **Testing harness** (`testing` feature) — `RenderTester`/`Probe` build real
  `PipelineOwner` trees through the production pipeline; every concrete
  render object in `flui-objects` is catalog-tested against it. See
  [`docs/TESTING.md`](docs/TESTING.md).

## Flutter parity

Layout, paint, hit-test, and dirty-propagation behavior is ported 1:1 from
Flutter's `rendering/` library (behavior, not structure). Changes here must be
cross-checked against the reference; the harness suite and `docs/PORT.md`'s
refusal triggers are the enforcement.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-rendering --open`. Deep architecture:
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## License

MIT OR Apache-2.0, per the workspace license.
