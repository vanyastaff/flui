[← Getting Started](getting-started.md) · [Back to README](../README.md) · [Crates →](crates.md)

# Architecture

FLUI combines two patterns: a **Layered Modular Workspace** (workspace structure) and a **tree pipeline** (runtime data flow: View → Element → RenderObject → Layer, with Semantics alongside). The first tells you *what may depend on what*; the second tells you *how a frame is built, laid out, and painted*.

The decision behind each part is an ADR in [`adr/`](adr/), and each crate's `ARCHITECTURE.md` holds its invariants. This page is the high-level orientation.

Input observation belongs to the resolved presentation. Before Keyboard or IME
dispatch, the runtime delivers that presentation's frozen measured pointer
prefix without advancing a frame or ending contacts; reentrant newer motion
stays pending and the first failure remains authoritative across the accepted
input round ([ADR-0163](adr/ADR-0163-presentation-owned-pointer-resampling.md)).
Keyboard keeps its admitted active presentation through callback focus changes;
layout and paint retain their ordinary frame transaction.

This page describes the architecture as it is. The decisions that set the target (ADR-0081 to ADR-0098) are in [`adr/`](adr/); some of those ADRs are accepted in part and the rest are Proposed, and each ADR's `Status` line says which.

## Layered Modular Workspace

The crates under `crates/` and `packages/` and the `flui` facade form a strict directed acyclic graph (DAG). Dependencies flow downward only; circular dependencies are forbidden. Each crate exposes its public API exclusively through `lib.rs` (and an optional `prelude` module). Internal modules default to `pub(crate)`.

Each crate's manifest declares a tier, its kind and its order (`[package.metadata.flui] tier`, `tier-kind`, `order`); the root `[workspace.metadata.flui] tiers` names the tiers bottom to top. The diagram below is read from those manifests, grouped by tier, with each crate's `order` in brackets:

```
tier pkg ── flui-material [1], flui-cupertino [2], flui-devtools [3],
        │    flui-hot-reload [4], flui-system [5]
        │    (official packages on the SDK: material, cupertino,
        │     devtools; flui-system is the capability crate)
        │
tier H  ── flui-native [1], flui-app [2], flui-cli [3], flui [4]
        │    (flui-native = the core host; only flui-app depends on it;
        │     flui = the facade, the root package)
        │
tier K  ── flui-view [1], flui-widgets [3], flui-runtime [4],
        │    flui-sdk [5], flui-testing [6]
        │    (flui-view = View/Element, BuildContext, reconciliation,
        │     signals; flui-testing = the test driver, dev edges only)
        │
tier R  ── flui-layer [1], flui-rendering [2], flui-objects [3],
        │    flui-engine [4]
        │    (objects → rendering, never the reverse;
        │     flui-engine = the one wgpu grant)
        │
tier S  ── flui-log [1], flui-scheduler [2], flui-painting [3],
        │    flui-interaction [4], flui-semantics [5],
        │    flui-animation [6], flui-assets [7]
        │    (flui-log = the subscriber backend; flui-app depends on
        │     it, and its allowed-dependents also admit flui-cli and
        │     the facade)
        │
tier C  ── flui-platform [1], flui-protocol [2]
        │    (flui-platform = platform contracts, no OS code;
        │     interaction → platform, platform → platform-api;
        │     flui-protocol = semantics and agent-protocol vocabulary,
        │     semantics → protocol)
        │
tier V  ── flui-macros [3], flui-foundation [4]
             (the foundation's `geometry` holds the plain-f64
              value types; no value-types crate, ADR-0098)
```

**This is not enforced by convention.** Each crate declares its tier, kind and order in its manifest, and `cargo xtask workspace` (part of `cargo xtask checks` and the CI `checks` job) validates every **normal** and build Cargo edge against them: a dependency points to a lower tier, or to a smaller `order` in the same tier, unless the dependent lists the edge in `edge-exceptions` with the ADR that removes it, and nothing with a tier depends on a `tier-kind = "tool"` package. Cargo rejects cycles itself. See [ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md). Dev-dependencies may point anywhere — a test fixture is not an architectural claim — except where the kind rule reads them ([ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) §3, [ADR-0088](adr/ADR-0088-official-packages-sdk-and-facade.md) §2): only applications name an official package (`tier-kind = "official"`: `flui-material`, `flui-cupertino`, `flui-devtools`, `flui-hot-reload`), in any dependency kind; any other crate that does lists the edge in its `edge-exceptions` (today only the facade), and one official package names another only through such an entry, so the two design systems name each other in no kind ([ADR-0028](adr/ADR-0028-design-system-decoupling-contract.md)).

Note on value types: there is no value-types crate. Geometry (`Point`, `Offset`, `Size`, `Rect`, `Matrix4`, the device-grid types, `DevicePixelRatio`) lives in `flui_foundation::geometry`, a leaf crate with no internal-crate runtime deps; paint, styling and typography values in `flui-painting`; constraints in `flui-rendering`; gesture details in `flui-interaction`; and platform values in `flui-platform`. Logical lengths are plain `f64`, with no unit wrapper ([ADR-0098](adr/ADR-0098-owned-f64-geometry-values.md)).

See [`crates.md`](crates.md) for the full inventory and current status of each crate.

### Why this structure?

- **Tooling enforces the layout, not review.** Cargo rejects a dependency *cycle* at build time, but an upward edge that does not close a cycle (`flui-rendering → flui-devtools`, say) builds fine — which is how three placements in this diagram drifted from the code unnoticed. `cargo xtask workspace` closes that gap by comparing each manifest's declared tier and order against Cargo's normal and build edges.
- **Public API discipline scales.** A consumer cannot reach into another crate's internals because they are `pub(crate)`. Reviewers reject changes that expose internals "just to make it compile" — that is the signal an abstraction is wrong.
- **Backends slot in via traits.** `Platform`, `RasterBackend`, `RenderBox`, and similar are extension points. Implementations live in dedicated crates, not in widget code.

## Tree Pipeline

Every frame, data flows through the trees in a fixed order (the Semantics tree is built alongside, from the render tree):

```
View Tree        ──build──▶   Element Tree   ──layout──▶   Render Tree   ──paint──▶  Layer Tree  ──submit──▶  GPU
(immutable)                   (mutable state)              (RenderBox)                (composition)            (wgpu)
```

| Phase | Owner | Input | Output | Constraint |
|-------|-------|-------|--------|------------|
| Build | `BuildOwner` | dirty `View` nodes | reconciled `Element` tree | `View::build()` is pure — no I/O, no external mutation |
| Layout | `PipelineOwner<Layout>` | `Constraints` | `Size` per `RenderBox` | Single-pass O(n) where possible (constraints down, sizes up) |
| Paint | `PipelineOwner<PaintPhase>` | `RenderBox` tree | `DisplayList` → layers | Recording is in `flui-painting`; GPU submission in `flui-engine` |

The pipeline is **on-demand**. The platform event loop waits (`ControlFlow::Wait`, or `WaitUntil` for a scheduled deadline), and a frame runs only when something asks for one: a dirty tree (`mark_needs_layout`, `mark_needs_paint`), or a scheduled frame callback (a ticker, a transient callback, an async completion), which `UpdateScheduler::schedule_frame_callback` turns into a frame request even when no tree is dirty. A render loop that polls every frame is not an accepted design.

### Threading & ownership model

One owner thread hosts every UI runtime ([ADR-0266](adr/ADR-0266-one-owner-thread-ui-runtimes-isolate.md)). A `UiRuntime` is a unit of isolation, not of parallelism: an `!Send + !Sync` single-writer owner of its state, keys and scheduling, run in turn with the others. What crosses a thread is the bounded typed mailboxes committed at Idle and the owned `SceneSnapshot` handed to the raster executor, one per `GpuContext` ([ADR-0091](adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md), [ADR-0305](adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)); decode, shaping, IO and raster run in parallel outside the UI transaction. The ownership levels are [ADR-0027](adr/ADR-0027-owner-affine-ui-realms.md)'s, and the owner-local surface and its `Send` gates are [ADR-0136](adr/ADR-0136-owner-local-ui-surfaces.md)'s.

## Type-Safe Children: the Arity System

Every `RenderBox` declares its child count as an associated type, `type Arity`, drawn from the sealed `flui_foundation::Arity` markers; the layout context it receives, `BoxLayoutContext<'_, A, PD>`, is typed by that arity.

| Arity | Children | Used by (for example) |
|-------|----------|---------|
| `Leaf` | 0 | `RenderParagraph`, `RenderImage`, `RenderColoredBox` |
| `Optional` | 0 or 1 | — |
| `Single` (`Exact<1>`) | exactly 1 | `RenderPadding` |
| `Exact<N>`, `AtLeast<N>`, `Range<MIN, MAX>` | a fixed count, a minimum, a range | — |
| `Variable` | 0..n | `RenderFlex`, `RenderFlow`, `RenderListBody` |

```rust
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> RenderResult<Size> {
        // ...
    }
}
```

## ID Offset Pattern

Slab-based storage uses 0-based indices internally. Every public ID has a niche (a `NonZero*` payload), so `Option<Id>` costs no extra space, but only the plain slab-backed IDs are the slot plus one. There are two shapes in `flui_foundation::id`:

- **Plain IDs** wrap a `NonZeroUsize`. The slab-backed ones (`ViewId`, `LayerId`, `SemanticsId`) are the slot plus one: insert `slab_index + 1`, look up `id.get() - 1`. The rest (`ListenerId`, `FrameCallbackId`, `TaskId`) are opaque counters with no slot behind them, minted only by the `IdMint` family (`IdMint` owner-local, `AtomicIdMint` where two threads mint) — an id is never issued twice, and an exhausted mint refuses, typed, forever.
- **Generational IDs** (`ElementId`, `RenderId`, `UiRuntimeId`, …) pack the slab index with a 31-bit `Generation` into a `NonZeroU64` whose top bit is reserved for a domain tag (a synthetic semantics identity sets it; ADR-0320), so an id held across a slot's reuse fails the generation check instead of addressing the new occupant. They have no `get()`; the owning tree's accessors use `.index()` (0-based) and `.generation()`. A slot never returns to circulation under a generation an id has carried: `RenderTree` and the signal arena retire a slot whose generations are exhausted, and `ElementTree` refuses that removal with a panic before any teardown.

```rust
let slab_index = self.nodes.insert(node);
let id = ViewId::new(slab_index + 1);
self.nodes.get(id.get() - 1);
```

## Platform Abstraction

`flui-native`, the core host, exposes one `Platform` trait over its native and headless hosts:

```rust
// Sealed: `HostPlatform<H>` over a host's table is its one implementation.
pub trait Platform: sealed::Sealed + Send + Sync + 'static {
    // Lifecycle
    // The handler hears and answers what the platform has for the application:
    // its bootstrap, the quit, a reopen, a session end, a notice, the exit
    // policy and the wake deadline.
    fn run(self: Box<Self>, handler: Rc<dyn AppHandler>) -> Result<(), PlatformError>;
    fn quit(&self);

    // Windows + displays
    // The handler hears everything the window delivers, from its first event on.
    fn open_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError>;
    fn active_window(&self) -> Option<WindowId>;
    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    // Input
    fn clipboard(&self) -> Rc<dyn NativeClipboard>; // a read answers through a Resolver (ADR-0152)

    // Metadata
    fn name(&self) -> &'static str;
    // ... plus file pickers, app activation, exit re-evaluation, preferences and data dirs
}

// AppHandler, the core host's trait, implemented by flui-app and by code that drives a host directly: ready(OwnerPlatform),
// quit_requested, reopen, session_end, host_notice, exit_policy (an ExitAnswer), wake_deadline (ADR-0254 §4).
let platform = current_platform()?; // Result<Box<dyn Platform>, PlatformError>
```

Each host is a table (`flui_native::host::Host`: one type per position of the closed list, `Absent<F>` where a platform has none), and `HostPlatform<H>` is the one implementation of `Platform` over it (ADR-0151 §10). The crate is laid out by feature: `hosts/` (the tables), `owner/` (the owner thread and each platform's loop), `version/` (the OS version floors and the gates above them, ADR-0244), `storage/`, and one directory per feature of the closed list (`window`, `input`, `text`, `display`, `clipboard`, `drag`, `accessibility`, `haptics`, `preferences`, `lifecycle`), each its model beside one file per platform; `cargo xtask module-dag` checks which of them may name which. Hosts: `WindowsPlatform` (Win32), `MacOSPlatform` (AppKit), `WebPlatform`, the Android host (started over the Activity by `android_platform(app)`), `IOSPlatform` (UIKit), `HeadlessPlatform` (CI / tests), `WaylandPlatform` and `X11Platform` (the `wayland` and `x11` features, both in `desktop`), between which `current_platform()` picks on Linux by the session (ADR-0151 section 11). The two Linux hosts share the Linux files of their features: the displays (`crates/flui-native/src/display/linux/`), the login-manager and memory-pressure notices (`lifecycle/linux.rs`), the Settings portal (`preferences/linux.rs`) and AT-SPI (`accessibility/linux.rs`). The platform backends' types (`windows::*`, `objc2::*`/`objc2-app-kit::*`/`objc2-ui-kit::*`, `wayland_client::*`, `x11rb::*`) stay inside this crate: no public path names one (ADR-0151 §8, `cargo xtask api-closure` on every host target), a raw handle leaves through `raw-window-handle`, and an OS's extras are extensions of `HostWindow`. Two narrow Windows FFI sites live elsewhere: `flui-hot-reload`'s library loading (`LoadLibraryW`/`GetProcAddress`, `src/dynlib.rs`) and `flui-cli`'s handle-inheritance call (`SetHandleInformation`, `src/proc.rs`). The current user's SID that `flui-app`'s driver agent puts in its pipe's security descriptor comes from `flui_native::current_user_sid` (`src/process.rs`). The Apple backends both use the `objc2` binding family — macOS/AppKit and iOS/UIKit alike; the older `cocoa`/`objc` crates this backend used are gone (ADR-0071).

Text shaping is **not** a `Platform` method — a platform text-system binding (`PlatformTextSystem`) is deliberately absent. `flui-painting` shapes text with Parley over a per-UI runtime `TextContext`: `TextPainter` measures a paragraph, paints the runs of the same layout and answers carets and selection from it ([ADR-0092](adr/ADR-0092-per-realm-text-over-parley.md)); `flui-engine` rasterizes the runs' glyphs through `flui-painting`'s `SwashRasterizer` into its own glyph atlas.

## Confinement of `unsafe`

The workspace sets `unsafe_code = "warn"`; `flui-painting` and `flui-platform` forbid it outright, and `flui-native` allows it for its OS bindings. Outside `flui-native`, compiled production code uses `unsafe` at a few narrow sites: `flui-rendering`'s subtree arena, `flui-foundation`'s unchecked id constructor, `flui-log`'s subscriber backends, `flui-hot-reload`'s dynamic-library loading and `flui-cli`'s Windows std-handle inheritance call (`src/proc.rs`). `flui-engine` denies `unsafe` in its hand-written production code; its `*/generated.rs` bridges allow it for the included `wgsl_bindgen` output (its `unsafe impl` of `Pod` and `Zeroable`), and its test code has a fake window target and an allocation-counting `GlobalAlloc`; the `transmute` in `flui-cli`'s hot-reload template is source text emitted into a generated project, not code the CLI runs. Each `unsafe` block carries a `// SAFETY:` comment naming the invariant it relies on. The widget catalog, the design systems and application code are `unsafe`-free.

## Logging and Errors

- **Logging:** `tracing` only — never `println!`, `eprintln!`, or `dbg!`. Use `#[tracing::instrument]` on hot paths and lifecycle methods.
- **Errors:** library crates use `thiserror` and expose typed enums. Application / CLI / build glue may use `anyhow::Error`. `anyhow` MUST NOT cross a library crate boundary.

## Reference Sources

FLUI is designed against two external codebases for read-only architectural reference:

- Flutter framework source (the declarative widget composition FLUI takes its inspiration from).
- GPUI Rust UI library (platform abstraction, callback registries, type erasure patterns).

The Flutter and GPUI sources are read on GitHub (`gh`); a maintainer may keep shallow clones in the gitignored `.flutter/` and `.gpui/`, but nothing requires them. Both are studied, never copied; patterns are designed as FLUI idioms (Arity, no nullability, strict layered DAG).

## Hot Reload (Dev-Time)

Hot-reload is split into two layers so build tooling and runtime hosts stay decoupled:

1. **Build orchestration** — `flui run` watches the sources with the CLI's own debounced watcher (`SourceWatcher`, crate-private in `crates/flui-cli/src/watch.rs`) and triggers `cargo build`; the app being reloaded never watches files.
2. **Artifact reload** — `HotReloadDriver` polls the plugin `.so`/`.dll` mtime and reloads via `dlopen` without restarting the host.

See [Hot Reload](hot-reload.md) for workflows, `ReloadStrategy`, and integration examples.

## See Also

- [Hot Reload](hot-reload.md) — two-layer dev model, plugin workflows
- [ADRs](adr/) — the cross-crate decisions the code follows
- [`AGENTS.md`](../AGENTS.md) — the current cross-tool rules
- [Crates Map](crates.md) — per-tier crate inventory
- [Contributing](../CONTRIBUTING.md) — workflow and conventions
