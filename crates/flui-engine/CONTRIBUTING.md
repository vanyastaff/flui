# Contributing to flui-engine

The GPU compositor: it turns a `flui_layer::Scene` into wgpu draw calls. This
page is what you need beyond the workspace-wide
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) — how to build, test, and debug
*this* crate.

## The three documents, and which one you want

| File | Answers |
|---|---|
| [`README.md`](README.md) | What are the entry points? Which one do I use? |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | How is it built inside — the module map, the record/replay split, ownership, mapping decisions, open items? |
| this file | How do I compile, test, and debug a change? |

## Build

```bash
cargo check -p flui-engine                    # fast
cargo check -p flui-engine --all-targets       # includes tests, benches, examples
cargo check -p flui-engine --target wasm32-unknown-unknown   # the web target
```

The crate is ~50% test code by line count, so `--all-targets` compiles
substantially more than the default.

## Test

**Default suite** — no GPU needed; runs anywhere:

```bash
cargo nextest run -p flui-engine
```

**GPU readback suite** — needs a real (or software) adapter, and is where the
pixel oracles live:

```bash
cargo nextest run -p flui-engine --features testing --lib
```

The `testing` feature gates an entire body of code — the readback
suite and the deterministic-replay tests — that the default workspace pass
never compiles. This is the single most common way to ship a broken change
here: you edit a GPU path, the default suite stays green because it never
compiled that file, and CI goes red. **Always run the second command for any
change touching render, layout, paint, or the command IR.**

Both clippy passes have the same shape and the same trap:

```bash
cargo clippy -p flui-engine --all-targets --locked -- -D warnings
cargo clippy -p flui-engine --all-targets --locked --features testing -- -D warnings
```

`cargo xtask lint` runs the workspace pass plus the `testing` one — that is
the one to run.

**A single test, with its output.** Without `--features testing` a filter only
reaches the default suite — for a GPU test, pass the feature:

```bash
# default suite (no GPU):
cargo test -p flui-engine <substring-of-test-name> -- --nocapture

# GPU suite, with stdout/stderr surfaced:
cargo test -p flui-engine --features testing <substring> -- --nocapture

# via nextest (faster, per-test process):
cargo nextest run -p flui-engine --features testing -E 'test(<substring>)'
```

## See what it renders

A green harness test is necessary but not sufficient — pixels can be wrong in
ways the test never looks at. When a change affects appearance, capture it:

```bash
cargo run -p flui --example screenshot -- material 900 760 /tmp/out.png
```

Demos: `material`, `cupertino`, `vertical-slice` (alias `vslice`), `gallery`,
`animated-box`, `colored-box`, `text`, `telemetry-overlay`, `sliver`,
`sliver-mid`, `sliver-collapsed`. Width and height default to 900×760 and the
output path to `<demo>.png`.

This mounts the tree through `HeadlessBinding`, extracts the `LayerTree`, and
rasterizes it offscreen on the same GPU path as an on-screen frame. It is the
fastest way to check shadows, blends, and clip edges. To cover a tree it does
not list yet, add a match arm in `examples/screenshot.rs`.

## Debugging a readback failure

The GPU readback oracles compare rendered pixels against a CPU model of the
fixed-function blender (`blend_oracle.rs`). When one fails:

1. **Read the assertion.** It names the oracle's expected value and the actual
   pixel, and usually names the defect class (`BUG 1`, `BUG 2`, …) with the
   reasoning. Those tags are explained in `ARCHITECTURE.md`.
2. **Dump the frame.** Set `FLUI_READBACK_DUMP_DIR=/tmp/dumps` and re-run; each
   readback helper writes the actual frame as a PNG there. CI uploads these as
   an artifact on failure, which is the only way to *see* a WARP-only mismatch.
3. **Check which path the test took.** Most readback tests self-skip when no
   adapter exists. A skip is not a pass — if you expect a GPU test to run and
   it returns early, check the adapter is actually present, or set
   `FLUI_REQUIRE_GPU=1` to turn an unavailable GPU into a loud failure.

## Where the render-object catalog lives

This crate has no `RenderBox`/`RenderSliver` — it renders a layer tree, not a
render tree. The catalog guard for those lives in `flui-objects`
(`cargo test -p flui-objects --test render_object_harness`), which verifies
every exported render object appears in `RENDER_OBJECT_TYPES` with a matching
`harness_*` test. If your change adds a *layer* to `flui-layer`, the compiler
is the guard instead: `layer_render.rs`'s exhaustive match over `Layer` will
not compile until the new variant is handled. See
[`crates/flui-rendering/docs/TESTING.md`](../flui-rendering/docs/TESTING.md)
for the harness API.

## The gates a change must pass

```bash
cargo xtask check-changed   # before a PR: this crate and its dependents
cargo xtask ci              # the full local gate
```

For a change confined to this crate, the parts that matter most:

| Gate | Command | Why it catches *this* crate |
|---|---|---|
| clippy, both feature sets | `cargo xtask lint` | The GPU-gated code is invisible to the workspace pass |
| engine tests | `cargo nextest run -p flui-engine --features testing` | The readback oracles |
| docs | `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-engine --no-deps` | Broken intra-doc links; the crate renamed a lot of public surface recently |
| doc examples | `cargo test -p flui-engine --doc` | Every `///` example in this crate is compile-checked |

## Invariants that are easy to break by accident

- **The ID offset pattern.** Slab indices are 0-based; public IDs
  (`RenderId`, `LayerId`, …) are 1-based `NonZeroUsize`. Insert is
  `slab_index + 1`; lookup is `id.get() - 1`. Getting this wrong is an
  off-by-one that only shows up at runtime.
- **`Matrix4` → `glam` at one boundary.** `Matrix4`-to-`glam` conversion
  happens in `layer_dispatcher.rs` and nowhere else. The record path
  (`batches/`), the pipeline set, and `replay/` are glam-only.
- **`lyon` lives in `tessellator.rs`.** The tessellator is the
  one adapter over that crate, so a lyon type never leaks into the Command
  IR or a pipeline layout — the same reason `etagere` stays behind `glyph_atlas.rs`.
- **No `async fn` on the hot path.** Async is for the acquisition edges
  (`Renderer::new`, `recover`, `HeadlessRenderer::new`) only. The layer walk,
  the dispatch, and the replay path are sync.
- **`DrawSegment` stays `Clone`, and holds no `PooledTexture`.** Textures
  are acquired at replay, never stored at record; the derive is what bars a
  `PooledTexture` field (it is `!Clone`). It does not bar a raw wgpu handle —
  those are `Clone` — so read `command_ir`'s field types, not the derive, when
  you need the "no GPU handle" property. See `ARCHITECTURE.md`'s
  record/replay section.
- **A public method that accepts a blend mode must honour it.** Two defects in
  this crate's history were exactly this: a mode accepted, carried on the
  wire, and dropped at the last step. `ARCHITECTURE.md` mapping decisions 5
  and 10 record both. If your change routes a mode anywhere, add a test that
  fails when the mode is discarded.

## Where the code lives

The crate is ~40k non-test lines. The densest files, and what each owns:

| Area | Files | Owns |
|---|---|---|
| Top-level entry | `renderer.rs`, `headless.rs` | The windowed and headless renderers; device/surface lifecycle, recovery |
| Frame walk | `layer_walk.rs`, `layer_render.rs`, `layer_dispatcher.rs` | Iterative layer traversal; per-layer rendering; `DrawCommand` → painter routing |
| Recording | `batches/`, `command_ir.rs` | Batched command IR — the record half |
| Replay | `replay/` | Command IR → wgpu encoding — the replay half |
| State | `state_stack.rs`, `layer_compositor.rs` | Transform/clip/opacity stacks; layer save-state |
| GPU resources | `{texture_pool,texture_cache,path_cache,buffer_pool}.rs` | Pooling and caching |
| Filters | `{blur,mode,gamma,color_matrix,morphology}/` | The offscreen filter passes, each with its own pipeline |
| Raster boundary | `raster_owner.rs`, `raster.rs` | The mailbox protocol between the app and the GPU |

## Getting help

- A design question: open a discussion or an
  [architecture change issue](https://github.com/vanyastaff/flui/issues/new/choose).
- A bug: the issue template asks for the reproduction, the expected vs. actual
  behavior, and `RUST_LOG=debug` output — the crate logs through `tracing`,
  and `flui.gpu` target events carry the per-frame GPU detail.
- A behavior question about what Flutter would do: `.flutter/` is the
  reference, pinned at tag `3.44.0` (`git -C .flutter describe --tags`). Check
  it rather than reasoning from memory.
