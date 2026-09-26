# FLUI target global architecture: the minimalist view

*Design angle: the fewest concepts, crates and public types that still reach beta and 1.0. Anything off the critical path is merged, deleted or deferred. Where a mature crate exists, FLUI adopts it instead of maintaining its own code, because the maintenance load has to fit one author working with AI agents. Evidence is cited as `path:line` from the codebase maps at main `cab06137d`, or by market source. Claims I could not verify are marked **(hypothesis)**.*

---

## 0. Summary

FLUI's internals are healthier than its outline:
- a typestate pipeline;
- realm ownership;
- a B+-tree virtualizer;
- 0 production `unwrap()`, re-measured;
- a layer DAG checked by the compiler.

The outline has not caught up with the plan. There are 27 published crates, 11 layers, and 5 state mechanisms, all exported through the facade as whole crates. That adds up to about 6k public items. The minimalist target keeps the three-tree spine and the render protocol split, and cuts the rest.

| | Today | Target at H0 | Target at 1.0 |
|---|---|---|---|
| Crates on the publish train | 28 (27 + facade) | 17 | 15 core + separately versioned packages |
| Layers | 11 (`Cargo.toml:92-104`) | 6 named tiers | 6 |
| State mechanisms in the prelude | 5 (StateCell, StateHandle, ValueNotifier, RebuildHandle, Signal behind a flag) | 1 (Signal) + a documented `rebuild()` escape hatch | same |
| Hand-rolled subsystems replaced by ecosystem crates | — | text (Parley/fontique/ICU4X), hot reload (Subsecond), CPU raster (vello_cpu) | + a11y adapters for all 5 OSes (AccessKit) |
| Facade surface | whole-crate re-exports (`src/lib.rs:120-152`) | curated modules with a public-API snapshot gate | semver-checked facade only |

Five decisions carry most of the weight:
1. Split the platform **contract** from the platform **backends**, and put the backends inside `flui-app`.
2. Move the frame transaction down into `flui-view` so tests and production run the same pipeline.
3. Make signals the only state primitive and move the graph core low enough that render objects can subscribe to it.
4. Make the facade the only semver promise and move the design systems above it.
5. Adopt Parley, Subsecond and vello_cpu instead of maintaining or writing the equivalents.

---

## 1. Guiding principles

**The owner's seven principles, and where this view departs from them**

| # | Owner's principle | Minimalist position |
|---|---|---|
| 1 | The mental model is sacred | Agree. It is also the reason to **cut** the parallel models around it: five state APIs, three ID schemes, two Axis enums, Rect vs Bounds, Navigator 1.0 next to Router. One user concept should map to one type. |
| 2 | One language, one toolchain, one renderer | Agree, with one refinement. "One renderer" should mean one **raster contract** that has a GPU implementation (wgpu) and a CPU implementation (vello_cpu). Today there are already two hand-written walks that disagree: `crates/flui-engine/src/headless.rs:15-27` renders BackdropFilter, ShaderMask and Follower differently from `Renderer`. The Python and Swift device checks (`tools/device-checks`) and wasm-pack (`tools/web-server`) both break this principle and should be removed. |
| 3 | No global state | Agree, and it has no enforcement today. The ratchet file `runtime-contract.toml` cited in `roadmap.md:224,743` does not exist (git ls-files). A principle without a gate is an opinion, so the first minimalist task is to restore the check, not to add features. |
| 4 | Everything machine-readable | Agree. The minimal way to satisfy it is **one** vocabulary, AccessKit, and **one** wire-types crate. The current four representations (internal Flutter flags, AccessKit, the hand copy in `tools/desktop-mcp/src/a11y/role.rs:17`, and the unused `SemanticsSnapshot`) cost four times the maintenance of one. |
| 5 | Proof, not claims | Agree. For perf, prefer deterministic **counts** (elements built, layout roots, layers, damage area) gated on every PR over wall-clock trends. Counts are noise-free and cheap to maintain. |
| 6 | Break explicitly | Agree. The minimalist corollary is to **break now**. Every item in section 9 is cheaper before crates.io than after. |
| 7 | Market before decision | Agree. The minimalist reading: when a mature crate exists (Parley, AccessKit, Subsecond, vello_cpu, ui-events), adopting it is the default. Writing our own needs an ADR that shows the crate fails. |

**Three places where I disagree with the plan itself**

- **Official packages in separate repos (plan.md, delivery layers).** Not yet. Flutter merged its engine back into one monorepo in December 2024 (flutter/flutter#160628). Slint keeps Material as a separate *workspace in the same repo*. A second repo only pays for itself once a package has its own release cadence, and with a bus factor of 1 each extra repo is a place CI and dependency drift can hide. Target: a `packages/` workspace in this repo whose crates depend **only on `flui`** with caret requirements. That makes the later move out of the repo a `git mv`.
- **"Custom render objects **and layers**" as an extension point.** Open render objects, yes. Open layers, no. The `Layer` enum should stay closed (`flui-layer ARCHITECTURE.md` decision 1). Add one `Layer::External { id, rect }` backed by an engine registry. That covers video, WebView and third-party wgpu content without freezing a trait-object layer API.
- **setState "stays as the low level" as a peer API.** It should be demoted to a single escape hatch, `ctx.rebuild()`, and not remain a second canonical model in the prelude. The data: 0 `Signal<` uses in examples, 59 notifier uses in the catalog, and `book/src/concepts/state.md:45` saying signals do not exist. As long as two models ship, users learn the wrong one.

**Two principles I would add**

- **P8 — The crate count is a cost.** A new crate needs an ADR that names the second consumer or the compile or semver seam it buys.
- **P9 — Unwired public surface is deleted at the next minor.** A `pub` item with no production caller either names its follow-up in the PR or gets removed. This is the repository's most common defect class (AGENTS.md review guidelines).

---

## 2. Workspace and crate topology

### 2.1 Six tiers instead of eleven layers

Several of today's layers are one-crate layers: L5 view, L8 localizations (281 lines), L10 facade. Meanwhile L2 mixes platform, interaction, scheduler, painting, assets, log and tree, and the same-layer edges inside it are invisible to the gate (`tools/xtask/src/workspace.rs:268-282`). Tiers should encode what matters, which is which crates are headless, OS-free and wasm-clean.

| Tier | Crates | Reach rule, checked as `forbid-reach` facts in the manifests |
|---|---|---|
| T0 values | `flui-types`, `flui-foundation`, `flui-macros` | no wgpu, winit, tokio, windows, objc2, accesskit adapters |
| T1 contracts | `flui-platform` (contract only), `flui-scheduler`, `flui-painting`, `flui-interaction`, `flui-layer` | same |
| T2 render machine | `flui-rendering`, `flui-objects`, `flui-engine` | engine is the only crate here that may reach wgpu |
| T3 spine and catalog | `flui-view`, `flui-widgets` | headless: no wgpu, winit, tokio |
| T4 composition | `flui-app` (runtime wiring + OS backends), `flui` (facade) | anything |
| T5 dev and packages | `flui-testing`, `flui-protocol`, `flui-cli`, `packages/{material,cupertino,…}`, `tools/*` | packages depend on `flui` only |

Enforcement is a TREE_FACT per headless crate: `cargo tree -p flui-widgets -e normal` must contain none of winit, tokio, windows, objc2, wgpu, android-activity. The mechanism already exists (`tools/xtask/src/tasks/facade.rs:49-90`); today it is used only for hot reload.

### 2.2 Fate of every current crate

| # | Crate (today's layer, size) | Fate | Reason |
|---|---|---|---|
| 1 | flui-geometry (L0, 19.3k) | **Merge into `flui-types::geometry`** and delete about 3.5k lines of unused GPUI vocabulary | Only widgets and the facade depend on it directly. Length/Rems/Transform2D/Bezier/text_path/kurbo bridge/ScaleFactor have zero consumers. Two Axis enums and Rect vs Bounds collapse into one each. |
| 2 | flui-types (L0, 22.5k) | **Keep, shrink**, and absorb geometry | Delete `physics/*` (1,797 lines, duplicated in `flui-animation/src/simulation.rs:31`), `layout::BoxConstraints` (duplicate of `flui-rendering/.../box_constraints.rs:41`), `MaterialColors` (570 lines, used nowhere) and about 15 orphan types. Move gesture details to interaction. Rule: an item lives here only if 2+ crates use it. |
| 3 | flui-foundation (L1, 11.5k) | **Keep**, absorb `flui-tree`'s Arity/Slot/Depth and the signal graph core | Move runtime-protocol IDs (FrameStamp, PresentationAddress, ClaimSlot, OwnerAffinity) into a `#[doc(hidden)] runtime` module, or move them back to their owners. Delete `ListenerRegistry` (523 lines, unused), `GenerationGate` and `ViewId`. |
| 4 | flui-macros (L1, 0.9k) | **Keep** | Proc macros must be a separate crate. It gains `View`/`RenderView`/`Store`/`Catalog` derives (section 5). |
| 5 | flui-tree (L2, 6.9k) | **Delete**. Arity/Slot/Depth go to foundation; the TreeRead/Nav/Write trio is removed | The traits have no generic consumer (`flui-layer/src/tree/tree_traits.rs:18` etc. only import them for method syntax). The crate pulls in `bon` for a single builder. |
| 6 | flui-log (L2, 3.7k) | **Merge into `flui-app::log`** | Its `allowed-dependents` are already app/cli/facade (`crates/flui-log/Cargo.toml:66`), and cli does not use it. |
| 7 | flui-platform (L2, 50.4k) | **Split**. The contract (traits, IME/a11y/input vocabulary, the capability registry, the owner capability) stays as `flui-platform` at about 3-5k lines (**hypothesis**). The backends move into `flui-app::shell::{win32,appkit,uikit,android,web,winit}` | The only reason flui-interaction depends on platform is one trait import (`crates/flui-interaction/src/text_input.rs:27`). Through it, winit, tokio and windows get into rendering, view and widgets, and a Win32 edit rebuilds 16 crates. |
| 8 | flui-scheduler (L2, 21.2k) | **Keep**, absorb flui-animation, and split into an owner-local core plus a `Send` waker | One clock per presentation removes the two-clock problem (`flui-animation/src/vsync.rs:1-20`). `TIME_DILATION` (`config.rs:43`) becomes realm config. Delete `async_driver.rs`'s `Send` bound. |
| 9 | flui-animation (L3, 16.7k) | **Merge into `flui-scheduler::animation`** | Its only non-trivial dependencies are scheduler and macros. The merge makes one time source a type-level fact. |
| 10 | flui-painting (L2, 7.1k) | **Keep**. Shaping moves to Parley and FONT_SYSTEM is deleted | Stop re-exporting `cosmic_text::fontdb::Family` (`lib.rs:87`). |
| 11 | flui-interaction (L2, 41.3k) | **Keep, shrink** | Drop the platform edge. Delete EventRouter, RawInputHandler, InputPredictor, OneEuroFilter and BasicVelocityTracker (no production consumers). Move InteractionLane's paint registry to rendering. |
| 12 | flui-assets (L2, 5.4k) | **Delete from core.** Byte and image loading becomes a `flui-widgets::image` module over an injected IO spawner. Network images become an official package later | It owns its own tokio runtime (`registry/bridge.rs:42-66`) and a public global (`registry/mod.rs:83`). Its only consumer is widgets. It is not wasm-capable. |
| 13 | flui-layer (L3, 5.0k) | **Keep** as the raster contract: Scene, LayerTree, DamageRegion, the backend-neutral walk and `CommandRenderer`, lifted out of engine | This gives GPU and CPU one lowering path. |
| 14 | flui-semantics (L3, 11.0k) | **Merge into `flui-rendering::semantics`** (it is already re-exported as `flui_rendering::semantics`, `lib.rs:80`) | SemanticsOwner lives on PipelineOwner. Platform speaks `accesskit::TreeUpdate` directly and never depends on it. |
| 15 | flui-rendering (L4, 57.8k) | **Keep** as the protocol crate | Catalog parent data, delegates and `ScrollPosition` move to objects/widgets. The build-during-layout cells move here from objects. |
| 16 | flui-objects (L4, 38.3k) | **Keep** as the first-party catalog | It is the proof of the third-party extension point. flui-view must stop depending on it (section 3). |
| 17 | flui-engine (L4, 74.0k, about 18.8k of real code) | **Keep**. Add a `cpu` feature backed by vello_cpu implementing the flui-layer contract | Make `WgpuPainter`/`raster_owner` crate-private, and put `pub use ::wgpu` behind `unstable-wgpu-interop`. |
| 18 | flui-view (L5, 54.4k) | **Keep**, absorb the realm/presentation/frame-transaction core (a module, not a crate) | This lets flui-testing drive the production transaction. Delete `ElementBuildContext`, the dead `ElementKind` variants, and the `A` arity parameter. Move lazy-sliver and LayoutBuilder elements up to widgets. |
| 19 | flui-widgets (L6, 82.7k, about 55-60k production, **hypothesis**) | **Keep** one crate with the module-direction gate the 2026-09-23 decision promised and never shipped | Absorbs localizations. Its `testing` module moves out to flui-testing. Delete `__private`. |
| 20 | flui-testing (L6, 3.9k) | **Keep, move above widgets** (T5). It absorbs `flui_widgets::testing` (2.1k) and adds semantic finders over flui-protocol queries | Today widgets has a normal edge into it (`crates/flui-widgets/Cargo.toml:89`), which is why WidgetTester has no home. |
| 21 | flui-hot-reload (L6, 2.9k) | **Delete**. Replace with a `hot` feature on flui-view/flui-app using `subsecond` | The plan chose Subsecond, which Iced and Dioxus also use. The dlopen design carries documented UB risk (`lib.rs` header) and five example workspace members. |
| 22 | flui-material (L7, 26.9k) | **Move to `packages/flui-material`**, depending on `flui` only (caret). Out of the facade default | Flutter did the same in 3.47. It currently reaches into objects, scheduler and rendering (`material.rs:83`, `scaffold_messenger.rs:212`). |
| 23 | flui-cupertino (L7, 4.3k) | **Move to `packages/`, `publish = false` until H3** | The plan puts Cupertino after beta. It has no focus or keyboard path (`button.rs:34-43`). |
| 24 | flui-localizations (L8, 0.3k) | **Delete**. The RTL table goes to `flui-widgets::localization`; design-system strings go into each package; ICU4X i18n becomes an H1 official package | It is a module disguised as a crate and has a layer to itself. |
| 25 | flui-app (L9, 44.5k) | **Keep** as the composition root. It gains the OS backends and log, and loses the realm core to view, semantics_host to rendering and held_input to interaction | Net size after the backends arrive is larger, but everything in it is platform wiring, which is what a composition root is for. |
| 26 | flui-cli (L9, 17.7k) | **Keep, versioned independently.** Add `mcp` and `devtools`. Delete the `test`/`analyze`/`format` wrappers until they add value (their `--json` mode drops the results, `runner.rs:166-168`) | It has zero workspace dependencies, so it should not ride the lockstep train. |
| 27 | flui-devtools (L9, 2.6k) | **Delete**. Its Timeline/profiler become protocol streams in the in-process backend | It has no production consumer (`crates/flui-testing/Cargo.toml:106` is the only reference). |
| new | `flui-protocol` (T5, serde + accesskit only) | **Create.** ADR-0080 wire types, error codes, outline serializer, handle model, `AccessibilityBackend` | The contract is currently locked inside a `publish=false` binary that crates cannot depend on (`tools/desktop-mcp`). |
| — | tools/web-server | **Delete** in favour of `flui run --device browser:` | It is a duplicate server built with a second toolchain (wasm-pack). |
| — | tools/device-checks (Python/Swift) | **Port** to the Rust driver library that desktop-mcp becomes, one check at a time as each is next touched | P2. There are currently four drivers in three languages. |

**Result.** 27 crates become 15 core crates: types, foundation, macros, platform, scheduler, painting, interaction, layer, rendering, objects, engine, view, widgets, app, and the `flui` facade. On top of that come 2 dev/tooling crates (testing, protocol), 1 independently versioned CLI, and packages. The minimalist move that **adds** a crate is `flui-protocol`, which is justified by three existing consumers (desktop-mcp, the CLI, the in-process backend). The platform split is a net zero, because the backends move into app.

*Alternative recorded:* keep the backends as a separate `flui-shell` crate if per-backend CI and unsafe budgets turn out to need a crate boundary. I prefer module-level `#![expect(unsafe_code)]` scoping, which already exists, plus an xtask unsafe ledger. That has the same effect without a crate.

### 2.3 Module boundaries inside the large crates

- **flui-widgets:** add the xtask module-direction gate *now*. Today there is exactly one cross-import among text/scroll/navigator/overlay/image (navigator → overlay), so locking it is free. The DAG is `base (layout, paint, flex, …) → interaction → {text, scroll, overlay} → navigator/router → app`.
- **flui-app:** `shell/<os>` (backends, `#![expect(unsafe_code)]` per module), `runner` (one runner over a `PlatformHost` trait instead of four divergent ones: `android.rs` 704, `ios.rs` 551, `web.rs` 438, `desktop.rs` 809 lines), `execution`, `log`, `inspect` (the in-process protocol server, `feature = "inspect"`, compiled out of release).
- **flui-view:** `authoring` (the Stable surface), `runtime` (`#[doc(hidden)]`: realm core, BuildOwner, ElementTree), `reactive`, `hot` (feature).
- **flui-engine:** `gpu` (wgpu), `cpu` (vello_cpu, feature), with lowering shared through flui-layer.

### 2.4 Feature-flag policy

1. **Additive only, and never a visibility switch.** Remove `flui-view/runtime-internals` (`Cargo.toml:118`). flui-app turns it on for every production graph (`crates/flui-app/Cargo.toml:90`). Replace it with `#[doc(hidden)] pub mod runtime`.
2. **One `unstable` feature on the facade** for the Experimental tier. There are no per-crate `testing` features on production crates (there are 9 today); test seams become hook registries the harness installs.
3. **Delete every empty feature:** app `desktop/android/ios/web/debug-overlay/performance-overlay`, platform `desktop` (it pulls in winit and gates no code, `crates/flui-platform/Cargo.toml:300-303`), `web`, `wayland`, `x11`, geometry `mint`, types `simd`.
4. **Facade features:** `default = []`, plus `images`, `network-images` (package), `hot`, `inspect`, `testing`, `unstable`. Signals and a11y adapters are no longer features: signals always on, AccessKit Win/macOS unconditional, Linux AT-SPI behind a default-on `a11y-linux`.

### 2.5 Facade and prelude

- Delete `pub use flui_view as view` and the other whole-crate aliases (`src/lib.rs:120-152`). Every facade module is an explicit list, which is how `src/rendering.rs` already works.
- `flui::prelude` is **enumerated**, with no globs of lower-layer preludes. Today the glob chain exports `BuildOwner`, `ElementTree` and the `tracing` macros into every app (`crates/flui-view/src/lib.rs:252,278-285`).
- The prelude is catalog-neutral: Raw primitives only. `flui_material::prelude` is globbed next to it by apps that want Material.
- `cargo xtask public-api` snapshots the facade (cargo-public-api) in `checks`, so any surface growth shows up as a reviewed diff.

### 2.6 Publish order and versioning

- The core train uses exact `=` pins internally, but versions are declared once in `[workspace.dependencies]`. Today there are 172 hardcoded `=0.2.0-dev` pins.
- `cargo publish --workspace` (stable since Rust 1.90) handles ordering. Add a `cargo xtask release-check` (package dry-run plus semver-checks on the facade) as a CI job.
- Order: types → foundation → macros → platform → scheduler → painting → interaction → layer → rendering → objects → engine → view → widgets → app → flui → testing → protocol.
- Packages and the CLI are versioned independently and depend on `flui = "0.x"` with caret requirements.

---

## 3. Runtime model

**Trees.** View → Element → RenderObject → Layer, with semantics assembled in rendering. No change to the mental model. Changes to the plumbing:

- **The spine must not know the catalog.** Move LayoutBuilder, the lazy sliver adaptors, persistent headers and Future/StreamBuilder from `flui-view/src/element/*` to widgets, behind a small public element protocol: `ElementBehavior` plus a narrow owner facade (schedule, register a layout callback, child-manager hooks). Today `ElementOwner` fields are `pub(crate)` (`owner/element_owner.rs:115-150`), which is what forces view → objects (`element/sliver_adaptor.rs:58`) and view → animation.
- **View configs are shared, not deep-cloned.** Children become `Rc<dyn View>`, so an unchanged subtree costs a pointer comparison. Today `dyn_clone` runs at every level (`view/into_view.rs:178-184`, `element/behavior.rs:1071`).
- **Render topology commits are local.** flui-view submits per-parent child lists through a `PipelineOwner` topology API, and `render_tree_mut` becomes private. That removes the global `synchronize_render_children` pass (`element_tree.rs:1428-1520`) and the O(D·N) slab scan (`storage/tree.rs:82-110`).

**Realms, stated honestly.** Realms are single-writer ownership domains. N realms share **one** owner thread per process (`runner/host.rs:25-47`). ADR-0027's verdict that "multiple realms may execute concurrently" should be amended to match the code. Per-realm owner threads are not free, because AppKit and UIKit pin the main thread. The minimal model: one owner thread, isolated realms, and parallelism only in the lanes (raster, IO, compute). Parallel layout inside a realm is recorded as *not planned*. The types already say so: `RenderObject` is not `Send` (`traits/render_object.rs:178`) and `PipelineCell` is `Rc<RefCell>`.

**Frame transaction.** One implementation lives in `flui_view::runtime`, parameterised by a `Clock` and a `FrameSink`. `UiRealm` is that core plus platform wiring. `HeadlessBinding` is the same core with a manual clock and a headless sink. Today flui-testing re-sequences the frame by hand (`flui-testing/src/lib.rs:955-1079`), so tests exercise an ordering production does not use.

**Scheduling.** Frame demand has one authority: `FrameClock::mark_demand(reason)` per presentation. There are five today (DemandMask, the loop-wide `needs_redraw` at `runtime.rs:727`, the scheduler hook, Vsync, AsyncDriver). The scheduler core is `!Send` (Cell/RefCell), and a `SchedulerWaker` (an atomic plus a callback) is the only `Send` part. Today it is about 20 Mutexes plus a DashMap (`scheduler.rs:743-855`). Effects run as a named phase: build → effects → layout → paint (following Solid 2.0 and the TC39 rationale). Consider adopting Subduction's `frameclock` demand classes (INPUT, CONTINUOUS_INPUT, ANIMATION, BACKGROUND) instead of designing our own **(hypothesis: its API fits)**.

**Lanes and async.**
- Raster: keep the `SceneSnapshot` mailbox protocol and `RasterMode::Inline` as a permanent mode. Accept ADR-0045 as mode-agnostic. Move web off `DirectSink` (`runner/web.rs:85`). Thread the lane on Win32/Linux only after a measurement shows overlap.
- Async: `LifecycleContext::spawn(fut)` for `!Send` UI futures on the owner lane (AsyncDriver without its `Send` bound, `async_driver.rs:102`), and `spawn_io`/`spawn_compute` for `Send` work that returns through `SignalSender`. Both are cancelled on unmount. One owner of execution: flui-app. tokio sits behind `flui-app/tokio` and is not in platform or assets. That leaves one runtime instead of up to four (`execution.rs:327,343`, `bridge.rs:66`, `platform/executor.rs:66`).

**State and reactivity.** The graph core moves into `flui-foundation::reactive`: realm-owned slab, push-pull Clean/Check/Dirty propagation, intrusive links, no per-edge HashMap. Subscriber kinds are Element (rebuild), RenderObject (`needs_layout`/`needs_paint`) and Effect. This is Compose's per-phase read tracking. It also lets `CustomPainter::repaint` and animation drop the `Arc<Mutex>` Listenable (`notifier_generic.rs:41-45`). The graph is **per realm**, not per `BuildOwner`. Today one graph per presentation breaks `SharedRealm` and `UiCommand::SignalWrite` targets the primary window (`app/ui_realm/commands.rs:450-455`). Reactive collections come from `#[derive(Store)]`: path-keyed triggers whose keys are the reconciler keys (Dioxus Stores, Leptos reactive_stores).

**Rendering, text, engine.**
- Stable layer identity: the `render_id` on boundary OffsetLayers already exists (`flui-layer/src/tree/layer_tree.rs:38`). Retained boundary subtrees become `Arc` subtrees so a graft is O(1). A layer diff produces `DamageRegion::Partial`. The damage is applied to a **persistent retained target plus a blit**, not by scissoring a rotating swapchain image. wgpu has no buffer age or present regions (wgpu#682), so the scissor path in `renderer.rs:2290-2330` risks stale pixels **(hypothesis)**.
- Text: Parley over one fontique `Collection { shared: true }`, a per-realm `FontContext`/`LayoutContext`, and a per-device glyph atlas (ADR-0067). The rasterizer comes through skrifa or glifo. The ADR-0077 spike must also answer the question of **glifo vs. our own glue**. ICU4X arrives with Parley, so delete the separate `unicode-segmentation` (`crates/flui-widgets/Cargo.toml:50`).
- GPU: one `GpuContext` per app (instance, device, pipelines, atlas) and a per-window `Presentation`. Today every window builds its own `wgpu::Instance` (`renderer.rs:1140-1168`).

**Platform.** A thin contract crate that holds:
- the owner capability (ADR-0039);
- a lifecycle state machine that every backend emits;
- a **pull-model IME** (text in a range, selection, rect for a range, index for a point), because TSF, NSTextInputClient, UITextInput and InputConnection all query synchronously, while today's trait is push-only (`traits/text_input.rs:24-45`, macOS returns nil at `text_input.rs:430-446`);
- `ui-events` input types;
- `accesskit::TreeUpdate` a11y.

Native windows are `!Send` and owner-owned. A `Send` `WindowHandle` has a closed verb set. That removes most of the 26 `unsafe impl Send/Sync`. One backend per OS: native Win32/AppKit/UIKit/Android/web, winit on Linux. Delete `LinuxPlatform` and the winit option on Windows/macOS. Windows IME is built on **TSF**, not IMM32; this is a B1 exit blocker, since today `grep WM_IME|Imm` in `platforms/windows` finds nothing.

**A11y.** AccessKit is on by default. Add the android and ios adapters (0.9 and 0.2.1 as of 2026-09-25). Use `tree_id` for multi-window. Semantic actions route through the owner lane at the Idle commit point with `Rc` handlers. Today `Send + Sync` handlers (`action.rs:217`) force an AtomicBool-mailbox pattern into every widget (`gesture_detector.rs:446-570`) and block onFocus (ADR-0079).

---

## 4. Extension points and plugin model (H1-H4)

The design rule: each extension point is **one trait or one registry**, lives in the lowest tier that can hold it, and is reachable through `flui` alone.

1. **PlatformCapability (H1).** One generic method on the sealed `LifecycleContext`. The trait set stays closed and the capability set becomes open:

```rust
// flui-platform (T1, no OS deps)
pub trait PlatformCapability: 'static {
    type Handle: Clone + 'static;
    fn attach(native: &NativeContext) -> Result<Self::Handle, Unsupported>;
}
// flui-view
pub trait LifecycleContext: BuildContext + Sealed {
    fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported>;
    // existing handles become sugar over this
}
// app
App::new(root).with_capability::<Clipboard>().with_capability::<my_camera::Camera>().run();
```

`NativeContext` exposes raw handles (HWND/NSWindow via raw-window-handle, JNI VM/Activity, UIViewController) plus an owner-thread `run_on_owner(Box<dyn OwnerTask>)`. The first three built-in capabilities go through exactly this seam: clipboard (installed today but `expect(dead_code)`, `runtime.rs:1636`), haptics (`presentation.rs:866-893`, dead) and file dialogs. Delete `PlatformCapabilities` (bool table, no reader) to avoid the name clash. Endorsed implementations come from `[target.'cfg(..)'.dependencies]`, following Flutter's federated plugins, with an override hook from day one (flutter#80374).

2. **Third-party render objects (H0).** `flui::rendering` becomes the Stable authoring tier, extended with the sliver set, `ViewportOffset` and `LayerLink`. Add a conformance kit, `flui::testing::rendering::conformance::check_box::<T>()` / `check_sliver`, which the first-party catalog also runs. It replaces the string registry (`render_object_harness.rs:151`) and its "mentioned in text" coverage check (`:14440-14450`). A fixture crate adds a custom `RenderSliver`; today it has zero (`tests/fixtures/facade_extensions.rs`).

3. **External GPU content (H1 spike, H2 insertion).** `Layer::External { id, rect }`, an `ExternalContent` capability (a texture registry through the raster mailbox, `GpuContext` device sharing behind `unstable-wgpu-interop`), and a `Texture` widget. Delete the no-op `PlatformViewLayer` render (`layer_render.rs:381-384`) until the presenter work lands. A system compositor (DirectComposition/CALayer/SurfaceControl) sits behind a `Presenter` trait. Borrow Subduction's shape, and do not build a generic one before a second presenter exists.

4. **Themes as data (H1).** A serde token map in `flui-widgets::theme` (color, type, shape, motion), resolved through InheritedView + FieldMask. Packages derive their ThemeData from it (`ThemeData::from_tokens`). The Color type decision (section 9) comes first.

5. **Agent protocol.** `flui-protocol` defines the types. `flui-app::inspect` is the in-process server: realm semantics TreeUpdate, actions, frame traces, bounded reads. `flui mcp` is a stdio adapter, and desktop-mcp becomes the OS-level verification backend. The transport is a pipe or Unix socket with a per-launch token, compiled out of release builds. The MCP SDK DNS-rebinding advisories and egui_inspection's unauthenticated port are the precedents to avoid. Consider adopting or co-designing the egui_inspection message set instead of a private one.

6. **A2UI (H1).** `packages/flui-a2ui` (Evolving tier, since A2UI renamed properties at v0.9). It consumes catalog metadata produced by `#[derive(Catalog)]` in flui-macros on stable Rust, not by rustdoc JSON, which is nightly-only. The same metadata emits the G6 JSON manifest, the A2UI JSON-Schema catalog under a URI id, the compressed index shipped in `flui create`'s AGENTS.md (Vercel evals: 100% vs 79%), and llms.txt. Data binding is a JSON-Pointer projection of realm signals.

---

## 5. Public API and DX (sketches)

The canonical app uses one state primitive, no wrapper root type and no clone noise:

```rust
use flui::prelude::*;          // catalog-neutral, enumerated
use flui_material::prelude::*; // optional skin

#[derive(View)]
struct Counter;

impl Counter {
    fn build(&self, cx: &mut Cx) -> impl IntoView {
        let count = cx.signal(0usize);          // Copy handle, element-owned
        Column::new((
            Text::new(format!("{}", count.get(cx))),
            Button::new("+").on_press(move |cx| count.update(cx, |n| *n += 1)),
        ))
    }
}

fn main() -> flui::Result<()> { flui::App::new(Counter).run() }   // any View kind
```

What changes against today:
- `run_app` currently requires `StatelessView + Clone` (`runner/mod.rs:214`), so every example defines a wrapper type.
- `column!` collides with `std::column!` (`macros/mod.rs:42-56`). Teach tuples.
- Signal writes take the realm through the callback context (`update(cx, …)`), not `&Reactive` plus `Result`.
- `View for Option<V>` and an `Either` combinator remove `.boxed()` on branches. There are 39 in examples and 110 in material.

Stateful views keep a struct but lose the bind step:

```rust
#[derive(View)]
struct Editor { doc: Signal<Doc> }

impl Stateful for Editor {
    type State = EditorState;
    fn init(&self, cx: &mut LifecycleCx) -> EditorState {
        EditorState { clip: cx.capability::<Clipboard>().ok(), field: TextFieldState::new() }
    }
    fn build(&self, s: &EditorState, cx: &mut Cx) -> impl IntoView { /* … */ }
}
```

`TextFieldState` is a signal-backed buffer with an `edit(|b| …)` transaction, input and output transforms and undo, which is Compose's state-based field lesson. It replaces `Arc<Mutex<ControllerInner>>` (`text/controller.rs:261-267`) and is the base for Form.

Custom render objects use one derive instead of the `impl_render_view!` macro_rules path (`view/render.rs:510-545`):

```rust
#[derive(RenderView)]
#[render(object = RenderBadge, protocol = Box)]
struct Badge { #[render(set = set_color)] color: Color, #[view(child)] child: BoxedView }
```

`CustomPainter: Any` drops the required `as_any` (trait upcasting is stable). Routing is `Router<Route>` with `#[derive(Route)]` on an enum and the URL as the source of truth. `Navigator` becomes its internal stack, and the named/keyed/generated push variants (`navigator.rs:1706-2300`) leave the prelude.

**One unit rule:** widget lengths take `impl Into<Px>`, and `From<f32>` is allowed only for `Px` at the widget boundary. Today 132 public functions take `f32` and 4 take `Pixels`.

**One threading rule:** UI callbacks, delegates, painters and `RenderView::RenderObject` are `!Send` (ADR-0027). Today `render.rs:451`, `custom_painter.rs:105`, `draggable.rs:299`, `page_view.rs:502` and others demand `Send + Sync`, so an `Rc` state handle fails to compile when captured in them.

---

## 6. Performance model

Cost should scale with **what changed**, not with what exists. Five structural items, in order:

1. **Damage producer plus retained target.** Every frame is `DamageRegion::Full` today (`raster_lane.rs:354,486`). ADR-0061's bench: 2,901 µs full vs 56 µs damaged at 64 layers.
2. **Arc-shared retained layer subtrees.** Today the graft clones every retained node and mints fresh ids (`paint.rs:1383-1425`).
3. **Local topology commits and direct disjoint borrows.** This removes the O(tree) steps (#1039, #1041).
4. **Shared view configs.** This removes clone × depth, where a 10k-row append currently rebuilds 20k elements in 34.6 ms.
5. **Lazy children built in layout, not in 6-10 fixpoint passes** (`layout_builder.rs:64,74`). Measure passes per frame first.

Principles:
- The frame path holds no locks. Enforce it with clippy `disallowed_types` for Mutex/RwLock in rendering, objects, view and widgets, with an allowlist.
- No `info!` in hot modules. `behavior.rs:1059,1090` log on every mount under the default filter.
- Startup: the host font scan, adapter creation and pipeline creation run concurrently. Pipelines come from a closed, prewarmable set, with `wgpu::PipelineCache` on Vulkan.

**Measurement.** One end-to-end harness on the shared frame transaction (10k/100k scroll, one-text change, route push, cold start to first present) reports counts that gate every PR, with wall time kept as a nightly trend. Fix `bench-collect` so it no longer skips feature-gated benches (`tools/xtask/src/bench.rs:36`); it currently never runs the damage baseline.

---

## 7. Safety model

- **Globals gate:** `cargo xtask globals` in `checks`, with an allowlist that states reasons. Pure ID counters are allowed; any `static` holding Mutex/Lazy/OnceLock and any `thread_local!` outside `flui-app` fails. Burn-down list: FONT_SYSTEM (`layout.rs:124`), decode CACHE (`decode_cache.rs:96`), ERROR_VIEW_BUILDER (`error.rs:41`), TIME_DILATION, `AssetRegistry::global`, navigator/lane/GlobalKey thread-locals. `APP_RUNTIME` stays as the one sanctioned host slot and is documented as such.
- **The !Send flip is finished before H3** in one breaking change: Listenable, Animation, TickerProvider, CustomPainter, the delegates, ScrollPhysics, HitTestTarget and ViewKey drop their `Send + Sync` supertraits. Removing a supertrait after the freeze would break every implementor.
- **Unsafe:** keep `unsafe_code = warn` plus a per-site expect. Add a per-backend unsafe ledger that ratchets down, and turn on `undocumented_unsafe_blocks` module by module. Miri covers `subtree_arena.rs` (29 blocks) already. A PR that touches a backend CI never executes must show a live-run artifact.
- **Panics:** a cheap syn lint for the `expect("BUG: …")` convention. There are about 255 non-BUG literals today (upper bound). Wrap every call into third-party render code in `guarded_call`, including hit-test, intrinsics and semantics, which are currently unguarded. Repeated paint poison substitutes an error box instead of stalling the frame.
- **Dependencies:** `multiple-versions = "warn"` with a reasoned skip list, ratcheted down (67 duplicate names today). Remove winit from the Windows/macOS default graph. Wrap `android_activity` instead of re-exporting it (`src/lib.rs:157`).

---

## 8. Delete, merge, or replace with ecosystem crates

| Replace | With | Removes |
|---|---|---|
| cosmic-text + global FONT_SYSTEM + unicode-segmentation | **Parley + fontique + ICU4X** (Bevy migrated too) | the global mutex, a second Unicode data source, the `fontdb::Family` leak |
| dlopen hot reload (flui-hot-reload, 3-crate template, `flui run --scene`, 5 example members) | **Subsecond** | about 3k lines of unsafe-laden code, a crate, a facade feature, three TREE_FACTS, about 2.5k lines of `run.rs` |
| A future hand-written CPU reference and software fallback | **vello_cpu** behind the flui-layer contract. Filter graphs are limited, so GPU-only effects get semantic goldens only | a third raster walk |
| hand-copied role enum, SemanticsSnapshot, part of the Flutter flag model | **accesskit::Role/Action** directly in the protocol | 3 of the 4 vocabularies |
| custom a11y on mobile | **accesskit_android / accesskit_ios** | an H1 build |
| frame-demand redesign (#1172) | evaluate **frameclock** (Subduction) | a bespoke state machine **(hypothesis)** |
| Color `{r,g,b,a: u8}` | a FLUI newtype over the **`color` crate** (f32 + color space, the peniko family), wrapped because it is pre-1.0 | the 8-bit banding and HDR dead end before the serde form freezes |
| kurbo | keep bridge-only, wire it only when Parley/vello use it | the unbuilt feature (`kurbo` never enabled) |
| Vello GPU | **deferred to H4**, measured against lyon behind the same contract | — |

**Delete outright** (all have no production consumer, per the maps):
- `flui_types::physics`, `layout::BoxConstraints`, MaterialColors, and about 15 orphan types;
- the GPUI vocabulary in geometry;
- the flui-tree traits;
- `ListenerRegistry`, `ElementBuildContext` and its builder, the `test_only_*` exports, the dead ElementKind variants, the `ElementArity` parameter;
- the `src/window.rs` Window family, `PlatformEmbedder`, `PlatformCapabilities`, `LinuxPlatform`, `BackgroundExecutor`/`Task`;
- EventRouter/RawInputHandler/InputPredictor/OneEuroFilter/BasicVelocityTracker;
- `SharedEngineServices` (empty) and the `embedder` stub (22 lines);
- `__private`, the predictive-back surface self-marked `REMOVE_BY: 2026-12-22`;
- StateCell/StateHandle (replaced by Signal), the named/keyed/generated Navigator variants;
- tools/web-server, `flui test/analyze/format` (restore once they emit structured results);
- about 140 process markers (Phase/wave/slice) in code and manifests.

Estimated reduction: about 15-20k production lines and about 1,500 public items **(hypothesis; a count is needed)**.

---

## 9. Breaking changes to make now, before 1.0

1. The platform contract/backends split. Interaction depends on the contract only, and TREE_FACTS enforce headless reach.
2. Curate the facade: no whole-crate aliases, an enumerated catalog-neutral prelude, `default = []`, and a public-API snapshot gate.
3. Material and Cupertino move to `packages/`, depending on `flui` only. Anything they need from objects, scheduler or rendering (PhysicalShape surface, post-frame handle) is added to the facade's curated modules as Raw primitives. InkWell/FocusableActionDetector/RawButton/RawToggle/Surface move down into widgets, as ADR-0028 already requires.
4. Signals become the only state model. The graph core moves to foundation and is per realm, and `signals` is no longer a feature. Rewrite FOUNDATIONS C1 and the `Cargo.toml:72-77` comment in the same change.
5. The `!Send` flip across all UI-side traits and callbacks.
6. One ID scheme: generational `GenId<M>` for Element, Layer and Semantics. Fix the AGENTS.md "ID offset" rule, which already contradicts `id.rs:1163`.
7. One vocabulary each: Rect (drop Bounds), one Axis, Point+Offset (drop Vec2), `impl Into<Px>` at widgets.
8. Color moves to f32 with a color space before any serde theme format exists.
9. `Pixels` Eq/Hash/Ord consistency. Today `0.0 == -0.0` hashes differently (`units.rs:91,575-596`), which is a cache-key hazard for H2.
10. Router ADR, and freeze Navigator features. `NavigatorCommand` in the runtime command channel (`ui_realm/commands.rs:8`) becomes a URL/route intent.
11. The pull-model IME contract (text-editing ADR, decided together with Parley).
12. Merges: geometry → types, tree → foundation, animation → scheduler, semantics → rendering, log → app, localizations → widgets. Deletes: devtools, hot-reload, assets from core.

Each item gets one ADR with `Supersedes:` and one CHANGELOG entry. Items 1-3 and 12 are mechanical. Items 4-5 are the expensive ones and should land before the catalog grows (Form, Slider, Dropdown).

---

## 10. Evolution H0 → H4 without rewrites

- **H0 (beta).** Topology cuts (section 9: 1-3, 12). Restore the globals gate and add the module-direction gate. Shared frame transaction. Signals canonical. `flui-protocol` + `flui mcp` + in-process inspect. vello_cpu goldens + semantic goldens (outline format = golden format). Parley + per-realm fonts. TSF IME. AccessKit default on the desktop OSes. Damage producer at boundary granularity. Publish 17 crates with `cargo publish --workspace`. The exit test stays the owner's: a clean consumer builds Notes from crates.io using `flui` + `flui-material` only.
- **H1.** Mobile adds `shell/android`/`shell/uikit` modules behind the existing `PlatformHost` runner, with no new crates. PlatformCapability is already the seam the built-in clipboard uses, so five plugins outside the repo is an adoption exercise, not a design one. `packages/flui-a2ui` reads derive metadata. Themes-as-data token map. `Layer::External` spike behind a Presenter trait. Android/iOS AccessKit adapters.
- **H2.** Everything H2 needs has a slot already: Arc-shared layers + raster cache, the threaded raster lane (mode switch on the existing protocol), IO lane via `spawn_io`, render-level signal subscribers for animation-rate values, store collections for 100k rows, and the counts gate for "not worse than Flutter". Per-realm fonts make multi-window cheap. Parallel layout stays out of scope.
- **H3.** Freeze tiers map one to one: Stable = the `flui` facade (prelude, view authoring, `flui::rendering`, Router, signals, Raw catalog). Evolving = packages and protocol. Experimental = `unstable`. The semver-checks surface is one crate, not 27. Packages may leave the repo if their cadence has diverged, which is a `git mv` because they already depend only on `flui`.
- **H4.** The community depends on the Stable facade and runs the conformance kit. An automated `flui` badge check (builds against current flui, passes conformance and semantics snapshots, has no undocumented unsafe) replaces a curation committee the project cannot staff. Embedded/kiosk hosts implement `PlatformHost` against the minting seam (#560) from the contract crate. Vello GPU is evaluated behind the raster contract.

---

## 11. Risks and deliberate non-goals

**Risks**
- *Backends inside flui-app make one very large crate* (about 90k+ lines). It is mitigated by per-OS cfg (one backend compiles per target) and module-level unsafe scoping. If per-backend CI or unsafe budgets need a crate boundary, split to `flui-shell` then (P8).
- *Moving the reactive core to foundation is a design change on an Accepted ADR* (ADR-0074 placement). It needs a superseding ADR and a benchmark showing per-phase subscribers do not regress the 600-reader fan-out (1.77 ms today).
- *Adopted crates are pre-1.0* (Parley, AccessKit 0.25, ui-events 0.3, vello_cpu, subsecond). This is the Bevy/glam semver trap. The rule: never re-export them from the Stable facade; wrap them. An H2 exit criterion is a public-signature leak audit.
- *vello_cpu cannot render every GPU effect* (filter graphs). GPU-only effects are excluded from pixel goldens and covered by semantic goldens. "Zero golden diff across OSes" additionally needs a pinned SIMD level **(hypothesis)**.
- *Subsecond resets thread-locals and cannot migrate struct layout.* The contract states it: build and logic edits keep state, State-type edits restart the realm. The globals ratchet is a precondition.
- *Deleting the `test`/`analyze` wrappers* may look like a DX regression. They come back once they emit per-test NDJSON.
- *Cutting the prelude and moving Material out of the default* changes every example and template. That is exactly why it has to happen before beta.

**Deliberate non-goals**
- Parallel layout within a realm; per-realm owner threads.
- Open `Layer` trait objects; custom layout protocols (Box and Sliver stay sealed).
- A second repo per official package before 1.0.
- An own markup/DSL (Makepad and Slint bet on this); MCP Apps (HTML iframe) as the generative-UI runtime.
- A Vello GPU rewrite before H4; a native Linux backend while winit serves.
- Maintaining our own shaper, hot-reload engine, CPU rasterizer or a11y adapters.
- MCP embedded inside framework crates: the protocol types live in the core, and the MCP transport stays in the CLI and tools.
