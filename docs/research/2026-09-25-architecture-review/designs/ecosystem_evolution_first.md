# FLUI target global architecture, ecosystem and evolution first (main @ cab06137d, 2026-09-25)

This takes the owner's 10-year plan (plan.md) at its word. FLUI will be a core plus official packages plus a community, and it must reach 1.0 and then H4 without a rewrite. The question asked is: which seams have to exist before the first crates.io publish so that H1–H4 add code, not break it? Everything below is grounded in the codebase maps, spot-checked against main (`crates/flui-interaction/src/text_input.rs:27`, `Cargo.toml:598 default = ["material"]`, `crates/flui-platform/Cargo.toml:300 default = ["desktop"]`, `src/lib.rs:126-157` whole-crate re-exports including `flui_app::android_activity`). Anything I did not verify is marked **hypothesis**.

---

## 1. Guiding principles

I keep the owner's 7 principles. Here is how the ecosystem angle reads them, and where I disagree.

| # | Owner principle | Architectural consequence | Stance |
|---|---|---|---|
| P1 | The mental model is sacred | View/Element/Render, keys, lifecycle and constraints-down are the **Stable tier**. Everything under them is free to change. | Agree. Add: the mental model must be reachable through **the facade only**. Today it also leaks arena internals (`ElementTree::get_mut`, `BuildOwner` in the prelude, `crates/flui-view/src/lib.rs:278-285`). |
| P2 | One language, one toolchain, one renderer | Cargo only; wgpu on every platform. | **Partly disagree.** The goal in P2 is "one *raster contract*". It should not mean "one rasterizer". H0 needs a CPU reference renderer (E7) and H2 needs a software fallback. `flui-engine/ARCHITECTURE.md:8-14` says "nothing here exists to make one pluggable", and `headless.rs:15-27` already renders three layer kinds differently from the windowed path. Rewording: *one scene contract with conformance tests; wgpu is the production rasterizer; a CPU rasterizer is a peer backend.* |
| P3 | No global state | Everything belongs to a realm or is passed explicitly. | Agree, with one clarification. **Immutable shared infrastructure** (font collection, GPU device, pipeline cache) may be app-scoped, but only if it is explicitly handed down. It must never be ambient. The ratchet file the roadmap relies on (`runtime-contract.toml`) no longer exists, and no gate replaces it. It has to come back as `cargo xtask globals`. |
| P4 | Everything machine-readable; DevTools is a client of the agent protocol | One wire-protocol crate; one catalog descriptor source. | Agree. Today the contract lives inside an unpublished binary (`tools/desktop-mcp/src/{error,params,server}.rs`) that no crate is allowed to depend on. |
| P5 | Proof, not claims | Conformance suites are the proof for extension points: render objects, backends, plugins. | Agree. Extend it to *extension points*. A community crate earns a badge by passing the same kit that first-party code passes. |
| P6 | Break explicitly | Every break goes through an ADR and a migration note; after 1.0, `flui migrate` with data shipped per crate. | Agree. |
| P7 | Market before decision | — | Agree. The market evidence changes three plan items: separate repos (see below), the Vello role, and Windows IME (TSF). |

The plan does not state three principles that the ecosystem angle needs. I propose adding them:

- **P8. The facade is the only promise.** Only `flui` (plus two small contract crates, §2) carries semver. Every other published crate is published but *internal* (the Slint `i-slint-*` model; Bevy tiers #16172). Official packages are built against the facade's author SDK, never against internal crates. Compose puts the same rule this way: "each layer builds on public APIs of the lower layers".
- **P9. Seams are proven by a second implementation.** An extension point exists only once a second implementation passes its conformance kit. For the scene contract that is wgpu + CPU; for PlatformCapability it is clipboard + haptics + file dialogs; for render objects it is `flui-objects` + an out-of-tree fixture; for the agent protocol it is the desktop backend + the in-process backend.
- **P10. No pre-1.0 upstream type in a Stable signature.** accesskit 0.25, ui-events 0.3, cosmic-text/parley, wgpu 30, and android-activity (currently re-exported at `src/lib.rs:157`) are wrapped. Bevy's glam lesson (discussion #9789) is that you cannot stabilise over them.

**On "official packages in separate repos, one release train" (plan.md delivery layers):** I disagree with *separate repos now*. Flutter merged its engine back into a monorepo in 2024. Slint keeps its Material library as a separate workspace in the same repo. GPUI inside the Zed repo shows the opposite risk, a framework that ends up forked. For a bus-factor-1 project the rule should be: **separate release unit and separate API boundary first; separate repo only when the package has its own cadence** (Flutter ships material_ui weekly and the core quarterly). Also, "one release train" contradicts "separate repos with independent cadence". Packages should depend on the facade with caret requirements, and only the core train uses `=` pins.

---

## 2. Workspace and crate topology

### 2.1 Tiers of delivery

| Tier | Semver promise | Where | Examples |
|---|---|---|---|
| **Core-Stable** | semver, LTS after 1.0 | monorepo `crates/` | `flui` facade, `flui-platform-api`, `flui-protocol` |
| **Core-internal** | published, exact-pinned, "internal" banner, no semver-checks | monorepo `crates/` | everything else in core |
| **Official** | own version, caret on `flui`, Evolving tier | monorepo `packages/` (own workspace or members with `tier="official"`), repo split when cadence diverges | material, cupertino, devtools, mcp, hot-reload, a2ui, i18n, net-image |
| **Tools** | none, `publish=false` | `tools/` | xtask, live-smoke fixtures, decoy-face |
| **Product binary** | independent version | `crates/flui-cli` | `flui` CLI (already has zero framework deps) |
| **Community** | none | crates.io | verified through `flui verify` badges (conformance kits) |

The tier is declared as `[package.metadata.flui] tier = "stable" | "internal" | "official" | "tool"` next to `layer` and is checked by `cargo xtask workspace`. Rules: `official` may depend only on `flui` (and on `flui-protocol` / `flui-platform-api`); `stable` may not re-export pre-1.0 upstream types (checked by a `cargo-public-api` snapshot); `tool` is never a dependency.

### 2.2 Target layers (6 named tiers instead of 11 numbers)

The current 11 numeric layers (root `Cargo.toml:87-104`) are too fine to guide decisions and too weak to control reach. L2 contains both the platform contract and every OS backend, which is how `flui-interaction → flui-platform` became legal. The layers below are named, and each one carries a **reach fact**: a set of crates that must not appear in its normal dependency graph.

| Tier | Crates | Must NOT reach (TREE_FACT, generalised from `tools/xtask/src/tasks/facade.rs:49-90`) |
|---|---|---|
| **V – values** | geometry, types, foundation, macros | tokio, winit, windows, objc2, wgpu, accesskit adapters |
| **C – contracts** | platform-api, protocol | same + no OS crate at all; serde optional |
| **S – substrate** | scheduler, painting, interaction, semantics, animation, assets, log | winit, windows, objc2, wgpu, tokio rt-multi-thread |
| **R – render machine** | layer, rendering, objects, engine, engine-cpu | winit, windows, objc2 (engine alone may reach wgpu) |
| **K – spine & catalog** | view, runtime, widgets, testing | winit, windows, objc2, wgpu (headless-clean, wasm-clean) |
| **H – hosts & facade** | platform (backends), app (runners), `flui` | — |

Intra-tier order is still checked (for example rendering → objects → engine must not invert; today all three are "layer 4" and unchecked).

### 2.3 Fate table for all 27 current crates

| Current crate | Fate | Target tier | Reason / evidence |
|---|---|---|---|
| flui-geometry | **keep, trim** | V | Delete the unused GPUI vocabulary (length.rs 1,160 lines, Transform2D, bezier, text_path, kurbo bridge, no-op `mint`). One Rect (drop Bounds), one Axis. Fix `Pixels` Eq/Hash inconsistency (`units.rs:91,575-596`). |
| flui-types | **keep, shrink** | V | Delete physics (duplicated in flui-animation, 1,797 lines), `BoxConstraints` (duplicated in rendering), `MaterialColors`, and orphan types. Rule: an item needs ≥2 consuming crates. Decide the Color model (f32 + color space) before publishing. |
| flui-foundation | **keep, split internally** | V | Framework vocabulary stays. The runtime protocol (FrameStamp, SurfaceGeneration, PresentationAddress, ClaimSlot, OwnerAffinity) moves to `foundation::runtime` (doc-hidden, internal) or to flui-runtime. Gains the reactive **graph core** (§3). ChangeNotifier becomes owner-local (`Rc`), not `Arc<Mutex>` (`notifier_generic.rs:41-45`). |
| flui-macros | **keep, grow** | V | Add `#[derive(RenderView)]` (replaces `impl_render_view!`), `#[derive(Catalog)]` (descriptor + JSON Schema), `#[derive(Store)]` (A8), `#[derive(Capability)]`. Document the proc-macro semantic edge in ADR-0041. |
| flui-tree | **merge** → rendering (Arity/Slot/Depth) | — | The trait trio has no generic consumer, ElementTree does not implement it, and `bon` is used for one builder. Saves one publish unit. |
| flui-platform | **split**: `flui-platform-api` (new, C) + `flui-platform` (backends, H) | C / H | One trait import pulls every backend, tokio and winit into rendering/view/widgets (`cargo tree -p flui-rendering -i winit` resolves through interaction). Delete the `desktop` default, `LinuxPlatform`, the second `Window` trait, `PlatformEmbedder`, `PlatformCapabilities`, and `background_executor`. |
| flui-scheduler | **keep, slim** | S | Keeps phase ordering, FrameClock, tickers. Loses `AsyncDriver` (goes to runtime) and `TIME_DILATION` (`config.rs:43`, goes to the realm clock). Becomes an owner-affine core plus a `Send` waker instead of ~20 Mutexes (`scheduler.rs:743-855`). |
| flui-painting | **keep** | S | Keeps the DisplayList recorder and the text service. FONT_SYSTEM (`text_layout/layout.rs:124`) becomes a realm `FontContext` over a shared fontique collection. `pub use cosmic_text::fontdb::Family` (`lib.rs:87`) gets wrapped. `DrawOp::Paragraph` carries a backend-neutral shaped-run contract. |
| flui-interaction | **keep** | S | Depends on platform-api only. `InteractionLane` TLS (`interaction_lane.rs:738`) moves into runtime as an explicit owner-lane registry passed in paint and hit-test contexts. Arena and recognizers move to `Rc`. |
| flui-semantics | **keep** | S | Gains a serde projection into `flui-protocol`. Decide whether the internal model becomes AccessKit-native (roles/actions stored directly) or stays a Flutter cascade. Action handlers become owner-local targets, not `Arc<dyn Fn+Send+Sync>` (`action.rs:217`). |
| flui-animation | **keep** | S | `AnimationController` becomes `!Send` (the self-declared exception at `controller.rs:177-180`). One clock per presentation; the wall-clock Ticker path is retired. |
| flui-assets | **keep, runtime-agnostic** | S | Delete `AssetRegistry::global()` and the owned tokio runtime (`registry/bridge.rs:42-66`); take an injected IO spawner. Absorbs the widgets decode cache (`decode_cache.rs:96`). Network loading moves to the official `flui-net-image`. |
| flui-log | **keep** | S (composition-only) | Clean as it is. |
| flui-layer | **keep, grow** | R | Becomes the **scene contract**: LayerTree with stable boundary identity, a damage differ (ADR-0061 producer, keyed on `LayerNode::render_id`, `layer_tree.rs:38`), and the backend-neutral lowering (layer_walk, LayerRender, CommandRenderer, LayerStateStack, all of which have zero wgpu references today and are locked `pub(crate)` in engine) plus a **conformance kit**. Adds `Layer::External{id}`. |
| flui-rendering | **keep** | R | Protocol plus pipeline. Absorbs Arity and the build-during-layout cells (currently in objects, `layout_constraints_cell.rs:31-35`). Catalog parent data and `ScrollPosition` leave for objects/widgets. Gains a topology transaction API; `render_tree_mut` (`accessors.rs:348`) is removed. Ships `conformance::check_box/check_sliver`. |
| flui-objects | **keep** | R | The first-party catalog. It is the first *client* of the conformance kit, and the spine no longer depends on it. |
| flui-engine | **keep, shrink surface** | R | The wgpu backend only. `pub use ::wgpu` goes behind `unstable-wgpu-interop`. `raster_owner` moves to runtime. `WgpuPainter` becomes crate-private. Introduces `GpuContext` (shared device, pipeline cache, glyph atlas) and a per-window `Presentation`. |
| — | **new: flui-engine-cpu** | R | Peer backend (vello_cpu or tiny-skia; decide in an ADR) implementing the flui-layer contract. Covers E7 goldens, the H2 software fallback, and CI without a GPU. |
| flui-view | **keep, re-cut** | K | Authoring surface (Stable) plus `#[doc(hidden)] __runtime`. Drops the objects/animation dependencies by moving the sliver adaptor, LayoutBuilder and async builders to widgets behind a public element protocol. Hosts the root-scope "binding views" (FocusRoot, VsyncScope, MediaQuery data) that flui-app currently imports from widgets. Deletes `ElementBuildContext` and the `runtime-internals` Cargo feature. |
| — | **new: flui-runtime** | K | UiRealm, presentation, frame transaction, lanes, execution services, capability registry, inspection hook, `Host` trait. Supersedes ADR-0041's "wait for two entry points" gate, because HeadlessBinding (`flui-testing/src/lib.rs:955-1079`) is already a second entry point, just a reimplemented one. |
| flui-widgets | **keep one crate** (the 2026-09-23 decision holds) | K | Adds the missing module-direction gate. Raw primitives (RawButton/FocusableActionDetector, RawToggle, Surface, ink) are pulled down from Material. Router. Form. Catalog registry. `__private` deleted. |
| flui-testing | **move above widgets** | K (dev) | WidgetTester, finders over protocol queries, golden via engine-cpu, replay. `flui_widgets::testing` (1.6k lines) folds into it. The 8–9 `testing` features on production crates shrink to doc-hidden hooks. |
| flui-material | **official package** | Official | Builds on `flui` only. Today it uses 9 internal crates (`material.rs:83`, `scaffold_messenger.rs:212`). Absorbs its localizations. |
| flui-cupertino | **official package** | Official | Same rule; it gains focus and activation for free once Raw primitives exist (`button.rs:34-43`). |
| flui-localizations | **delete** | — | 281 lines in its own layer. The RTL table goes into widgets, the strings into each design package (Flutter 3.47 did the same), and ICU4X into the official `flui-i18n` (H1). |
| flui-app | **shrink to hosts** | H | Thin `PlatformHost` adapters (desktop/android/ios/web), the `App` builder and `run_app`. Target < 15k lines. `bindings`, `runner`, `direct`, `embedder` stub and the dead features all go. |
| flui-cli | **product binary, own version** | Product | Gains `mcp`, `devtools`, `test --golden`, and later `migrate`/`publish`. Loses the dlopen `--scene` loop once Subsecond lands. |
| flui-devtools | **official package, redefined** | Official | The in-realm inspection server over `flui-protocol`, installed through a runtime hook, compiled out of release builds. Today it has zero production consumers. |
| flui-hot-reload | **official, rewrite on Subsecond**; `publish=false` until then | Official | The dlopen design is the one the plan rejected. Five workspace members and three facade features go with it. |
| — | **new: flui-platform-api** | C / Stable | Capability traits, the text-store IME contract, the a11y publish seam, input vocabulary, lifecycle state machine, `Unsupported`, a backend minting seam (#560), and a conformance test kit for backends. |
| — | **new: flui-protocol** | C / Stable (Evolving until H3) | Wire types lifted out of tools/desktop-mcp: handles, error codes, AccessKit-role vocabulary, outline serializer, catalog descriptor schema, event envelope, record/replay log. serde + accesskit only. |
| — | **new official: flui-mcp** | Official | tools/desktop-mcp becomes a lib (OS driver: UIA/AX/AT-SPI) plus a bin. xtask device and live-smoke consume the lib, which ends four drivers in three languages. |
| — | **new official (H1): flui-a2ui, flui-i18n, flui-net-image, flui-plugin-*** | Official | See §4. |

The published core ends up with about the same number of crates (~24). That is intentional: the count is not the problem. What changes is that **only 3 crates carry a promise**. Everything else can be refactored freely under an exact-pin train, which is the Slint model.

### 2.4 Module boundaries inside big crates

- **flui-widgets**: add `[package.metadata.flui.modules]` with an allowed-edge DAG. Base = layout, paint, flex, stack, animated. Then interaction(focus, actions), text, scroll, overlay, navigation (Router over Navigator), form, then app. Enforce it with `cargo xtask module-direction` (syn over `use crate::`), folded into `checks`. Lock it now: current cross-imports are almost zero (navigator→overlay only).
- **flui-runtime**: `realm`, `presentation`, `frame`, `lanes`, `exec`, `capability`, `inspect`. Only `frame` sees rendering and view internals.
- **flui-engine**: `context` (GpuContext), `present`, `lower` (implements the flui-layer contract), `effects`, `text_atlas`. In-src readback suites (22k lines) move to `src/tests/`.
- **flui-platform**: one module per backend. The shared owner/loop state machine moves into platform-api so it is not re-implemented 4–6 times (`winit/control.rs`, `macos/owner_lane.rs`, …).

### 2.5 Feature-flag policy

1. Features are additive. **Empty features are deleted**, and a check fails any feature with zero `cfg` sites (today: app desktop/android/ios/web/overlays, platform `desktop`/`web`/`wayland`/`x11`, geometry `mint`, types `simd`).
2. Backends are selected **by target, not by feature**. The winit backend is Linux-only plus a test/dev feature elsewhere.
3. The only visibility feature is **`unstable`** (facade) / `unstable-*` (per crate). Internal seams (`runtime-internals`, `test-utils`, `experimental-delegates`, the 8 `testing` features) become `#[doc(hidden)]` modules or move to flui-testing. A Cargo feature is not a visibility boundary, because unification exposes it to every consumer (`crates/flui-app/Cargo.toml:90`).
4. **a11y adapters are on by default** on Windows and macOS. Linux AT-SPI stays behind a default-on, opt-out feature because of its ~66-crate D-Bus stack.
5. **tokio is a feature of the default executor** in flui-runtime and nowhere else in core.
6. The facade default is **catalog-neutral**: `default = ["a11y", "images", "signals"]`, with no `material`.

### 2.6 Facade and prelude

- `flui::prelude` becomes an explicitly enumerated list. It contains no glob of layer preludes, no `tracing` macros, and no `BuildOwner`/`ElementTree`. It is snapshotted by a public-API test.
- Modules are curated in the same style as `src/rendering.rs` already is: `flui::{view, widgets, rendering, painting, interaction, animation, geometry, types, platform (capabilities), testing, protocol}`. There are no `pub use flui_x as x` re-exports.
- **Author SDK** (`flui::sdk`, Evolving): what a design system or third-party catalog needs. Surface/PhysicalShape, RenderView derive, post-frame handles, WidgetState, InheritedTheme, token types. Material compiling against this is the H0 proof of the official-package seam.
- Material ships `flui_material::prelude`; apps glob both preludes.

### 2.7 Publish order and train

The order is generated from tiers and layers: V → C → S → R → K → H, then the facade. Use `cargo publish --workspace` (stable since Rust 1.90). All internal edges come from `[workspace.dependencies]` with one version. Today there are 172 hand-written `=0.2.0-dev` pins. Upward dev-dependencies become version-less path deps so they do not constrain the order. `cargo xtask release-check` runs `cargo package` dry-runs, then semver-checks on the 3 Stable crates. Official packages publish on their own cadence against `flui = "0.x"`.

---

## 3. Runtime model

**Trees.** View → Element → Render → Layer, with Semantics alongside, unchanged (P1). Changes to the runtime around the trees:

- **Element storage.** Keep the slab + generational `ElementId`. Collapse the dead `ElementKind` variants and the `A` arity parameter (`element/kind.rs:342-350`). View configs stop being deep-cloned per level (`dispatch.rs:145`, `behavior.rs:1071`): children move by value, or are shared via `Rc` for pointer-compare skipping. There is a **public element protocol** (narrow `ElementOwner` facade: schedule, layout callback, child-manager hooks), so lazy lists and Router transition hosts can live outside the spine.
- **Render topology** is owned by rendering: `PipelineOwner<Idle>::set_children(parent,&[RenderId])` enforces arity and depth and evicts captures. The global `synchronize_render_children` pass (`element_tree.rs:1428`) becomes a debug-only verifier. The whole-slab `collect_disjoint_mut` scan (`storage/tree.rs:82`) is replaced by direct disjoint indexing.
- **Layer identity.** Repaint boundaries are `Arc` subtrees keyed by `RenderId`, so a graft is O(1). The flui-layer differ emits `DamageRegion::Partial`, and the engine caches Command IR per `Arc<DisplayList>`. The paint→layer contract is settled **before** H3, because it changes what `paint` produces.

**Realms and threads.** Record the decision that is effectively already made: **one owner thread per process on AppKit/UIKit/wasm; realms are ownership domains, not concurrency domains** until an H2 spike proves otherwise. ADR-0027's "may execute concurrently" gets amended accordingly (`runner/host.rs:25-47` shows N realms serialized in a TLS). The TLS `APP_RUNTIME` is replaced by a `Host`-owned registry, so two realms on one thread are expressible. Intra-realm parallel layout is declared a non-goal until the 100k-row data asks for it (render objects are deliberately `!Send`).

**The concurrency rule** (one ADR superseding the ADR-0027 exceptions): everything realm-owned is `!Send` and lock-free. That includes views, contexts, callbacks, render objects, delegates, painters, controllers, recognizers and notifiers. `Send` is reserved for `Scene`, lane mailboxes, `SignalSender`, `WakeHandle` and IO results. This deletes `Send+Sync` from about 15 protocol traits (`Listenable notifier.rs:78`, `Animation animation.rs:68`, `CustomPainter custom_painter.rs:105`, `ScrollPhysics`, `ViewKey`, …) and removes locks from the layout path. It has to happen **before H3**, because removing a supertrait later breaks every implementor.

**Scheduling.** One demand authority per presentation: `FrameClock::mark_demand(reason)`, using the frameclock demand classes INPUT/CONTINUOUS_INPUT/ANIMATION/BACKGROUND. The scheduler, AsyncDriver, Vsync and input only mark demand. Today there are five demand carriers plus a loop-wide `needs_redraw` shared across realms (`runtime.rs:727`). Effects run as a **named frame phase**: build → effects → layout → paint → semantics. One clock per presentation (virtual in tests), and time dilation is realm-scoped.

**Lanes.** Frame (owner), raster (inline mode is permanent for macOS/wasm, threaded on Win32/Linux), IO, compute. Execution is **one owner** (flui-runtime), vended as a capability with cancel-on-unmount. `flui-platform`'s tokio `BackgroundExecutor` (`executor.rs:66`) and the flui-assets bridge runtime are deleted. `AsyncDriver` gets a `!Send` local-future variant.

**State and reactivity.** The ADR-0074 graph core moves **down** into `flui-foundation::reactive` (realm-owned arena, Clean/Check/Dirty push-pull, intrusive slab links, write journal). Subscribers have a **kind**: Element-rebuild (view), needs_layout / needs_paint (rendering), which is Compose's per-phase read. The graph is **per realm**, not per BuildOwner. Today it is per presentation (`build_owner.rs:444,722`), and `SignalWrite` always targets the primary window (`commands.rs:450-455`). `signals` becomes default-on. StateCell/ValueNotifier remain as the low level. One `Reactive<T>` input type (`impl Into<Reactive<T>>`, following Leptos `MaybeProp`) is defined in core before the catalog grows.

**Rendering, text and engine.** `GpuContext` is app-scoped: one instance/device/queue, a pipeline cache, a glyph atlas. Each window has a `Presentation`. Today each window builds its own GPU stack (`renderer.rs:1140-1168`). Text: Parley, with per-realm `LayoutContext` over a shared fontique `Collection { shared: true }`. The glyph key carries font-blob identity, and the rasterizer runs outside the shaping lock. B1 (per-realm fonts) and B7 (Parley) are **one** ADR. The spike should evaluate `glifo` against hand-rolled Skrifa→atlas glue. ICU4X from Parley becomes the single Unicode source for editing, replacing a separate `unicode-segmentation`.

**Presentation (H1/H2 seam).** A `Presenter` trait (the Subduction shape) sits between the scene and the OS. The fallback is the single-swapchain wgpu presenter with a retained target + blit, because wgpu has no present-with-damage (#682). **Hypothesis:** the current direct-to-swapchain damage scissor (`renderer.rs:2290-2330`) would show stale pixels once damage exists; verify before E1. Native presenters (DirectComposition, CoreAnimation, SurfaceControl, Wayland subsurfaces) come later and carry `Layer::External` surfaces. `PlatformViewLayer` (a no-op today, `layer_render.rs:381`) is deleted until then.

**Platform.** `flui-platform-api` defines a **pull-based text-store IME contract** (text in range, selection, composing range, rect-for-range, index-for-point), which TSF, NSTextInputClient, UITextInput and InputConnection all need. The current trait has two push methods (`traits/text_input.rs`), and macOS answers `attributedSubstringForProposedRange` with nil (`macos/text_input.rs:430-446`). On Windows it is TSF, not IMM32, and today Win32 has *no* IME code at all. Native window objects are owner-owned `!Send`, with a `Send` `WindowHandle` proxy carrying a closed verb set. That removes most of the 26 `unsafe impl Send/Sync`. One lifecycle state machine replaces the four per-OS runners.

**a11y.** AccessKit is on by default on desktop. `accesskit_android` and `accesskit_ios` come in H1. `tree_id` is used for multi-window and embedded foreign trees. Semantics enablement becomes a realm capability (`SemanticsHandle`), so agents, devtools and goldens can turn the tree on without a screen reader. Today the handle is `expect(dead_code)` (`semantics_host.rs:30-50`).

---

## 4. Extension points and plugin model

Each extension point gets a crate home, a conformance kit, a first-party second implementation (P9) and a tier.

### 4.1 PlatformCapability (H1; seam built in H0)

```rust
// flui-platform-api (Stable)
pub trait PlatformCapability: 'static {
    type Handle: Clone + 'static;            // owner-local, !Send allowed
    const NAME: &'static str;                // stable, machine-readable (P4)
    fn attach(ctx: &mut NativeContext<'_>) -> Result<Self::Handle, Unsupported>;
}
pub struct NativeContext<'a> { /* per-OS: HWND/NSWindow/Activity+JNI/UIViewController, owner-lane executor */ }

// flui-runtime: realm-owned registry; flui-view: ONE generic method keeps LifecycleContext sealed
pub trait LifecycleContext: BuildContext {
    fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported>;
    // existing handles become sugar over this
}

// app
App::new(root).capability::<flui_plugin_camera::Camera>().run();
```

- The endorsed default comes via `[target.'cfg(..)'.dependencies]` in the plugin's app-facing crate. **The override hook exists from day one**, because Flutter still lacks one (#80374).
- Clipboard, haptics and file dialogs are migrated first. Clipboard is installed today but dead (`runtime.rs:1636`), so doing this also wires copy and paste.
- `NativeContext` exposes an owner-lane `OwnerTask` trait object rather than closures over a proxy (ADR-0039's no-closure rule).
- Conformance kit: headless fake plus typed `Unsupported` on unsupported targets.
- The unused `PlatformCapabilities` table is renamed or deleted to avoid the name clash.

### 4.2 Third-party render objects (H0)

- The Stable surface is `flui::rendering` plus the sliver authoring set, `ViewportOffset` (without `Send+Sync`) and `LayerLink`.
- `#[derive(RenderView)]` replaces `impl_render_view!`.
- `flui::testing::rendering::conformance::check_box::<T>(factory)` / `check_sliver` check dry==wet layout, finite monotone intrinsics, hit-test in bounds, idempotent relayout and stable semantics. `flui-objects` runs it for every object, which replaces the string registry at `render_object_harness.rs:151` and the 15.9k-line test file.
- The protocol is fixed to Box + Sliver (sealed `Protocol`), recorded as a deliberate 1.0 decision.
- Guarded calls: hit-test, intrinsics and semantics hooks get the same `catch_unwind` poisoning as layout and paint.

### 4.3 External GPU content (spike H1, insertion H2)

```rust
// capability, acquired in init_state
let tex: TextureRegistry = ctx.capability::<TextureRegistry>()?;
let id = tex.register(|dev: &GpuDevice| my_renderer.create_target(dev))?; // same device via GpuContext
Texture::new(id)            // RenderTexture → Layer::External { id, rect }
```

The frame-sync contract (who updates when, fences) is an ADR-0045 addendum written *before* the raster lane is threaded. Device sharing lives behind `unstable-wgpu-interop` (P10). Native presenters later place the same `External` surface as an OS compositor layer (zero-copy video).

### 4.4 Themes as data (H1)

The design-neutral token substrate lives in widgets (`flui::sdk::tokens`): serde value types for color, typography, shape and motion, resolved through InheritedView + FieldMask. `ThemeData::from_tokens`. `WidgetStateProperty` has a data form (per-state map) with the closure form as an escape hatch. ADR-0042's "no universal ThemeData" holds, because tokens are vocabulary, not a theme. The Color model is decided first.

### 4.5 Catalog as data → G6 and A2UI (H0 schema, H1 renderer)

```rust
#[derive(StatelessView, Catalog)]
#[catalog(id = "flui.basic/Button", role = Button, since = "0.3")]
pub struct RawButton { label: Reactive<String>, #[catalog(action)] on_press: Action, disabled: Reactive<bool> }
```

One derive emits, from the same source: a JSON Schema per component, a registry entry, examples compiled as tests, the G6 manifest, the A2UI catalog under a URI `catalogId`, prompt text, and the compressed index `flui create` writes into AGENTS.md (Vercel's evals: passive context beat on-demand skills 100% vs 79%). Interactive props are `Action` ids plus an optional closure. Nothing is generated from rustdoc JSON, which is nightly-only.

`flui-a2ui` is **official, Evolving**: a surface controller in the realm, a JSON-Pointer projection of signals, named catalog functions and an `A2uiTransport` adapter, with no LLM client. That is the shape Flutter GenUI arrived at in its May 2026 redesign. The A2UI v0.9 renames show why it must stay out of core.

### 4.6 Agent / devtools protocol

- `flui-protocol` (C, Evolving until H3): request/reply types, handles, error codes, outline format (which doubles as the semantic-golden file format), bounded reads (scope/depth/max_nodes), a trace export and the record/replay log.
- Backends: an in-realm server (`flui-devtools`, installed via `runtime::InspectHook`, `cfg(debug_assertions)` + runtime opt-in, pipe or loopback with a per-launch token) and the OS driver (`flui-mcp` lib).
- Clients: `flui mcp` (stdio), the DevTools UI, `flui test` finders.
- MCP stays **outside core** (the egui/Dart DTD shape, not Slint's in-app MCP). Build only on the 2026-07-28 MCP core (no sessions, Sampling or Logging).
- **Hypothesis to evaluate:** co-design with egui's `egui_inspection` wire format so AccessKit-generic inspectors (kittest) work on FLUI.

### 4.7 Renderer backends and embedding (H2/H4)

The `flui-layer` scene contract plus its conformance kit is the swap point: wgpu, CPU, and later vello_gpu if lyon loses. Backend choice is made at run time at the presenter, not by feature flag. The `Host` trait in flui-runtime (bring-your-own event loop and surface) is the H4 embedded/kiosk and VST seam. It needs the #560 minting seam in platform-api.

---

## 5. Public API and DX

Target hello-world and two screens, facade only:

```rust
use flui::prelude::*;
use flui_material::prelude::*;          // explicit, not default

#[derive(Clone, StatelessView)]
struct Counter;
impl StatelessView for Counter {
    fn build(&self, cx: &dyn BuildContext) -> impl IntoView {
        let count = cx.signal(0usize);                    // realm-scoped, Copy
        Column::new((
            Text::new(move |cx| format!("{}", count.get(cx))),
            ElevatedButton::new("+").on_pressed(move |r| count.update(r, |n| *n += 1)),
        ))
    }
}

#[derive(Routes, Clone, PartialEq)]                     // typed routes, URL = truth
enum Route { #[route("/")] Home, #[route("/note/:id")] Note { id: u64 } }

fn main() -> Result<(), AppError> {
    App::new(Router::<Route>::new(|r| match r { Route::Home => Counter.boxed(), Route::Note{id} => NoteView(id).boxed() }))
        .capability::<Clipboard>()
        .run()                                             // one builder, all targets
}
```

Rules this implies:

- `run_app(impl View)` accepts any view kind. Today it is `StatelessView` only (`runner/mod.rs:214`).
- One entry builder replaces 8 entry points.
- `View for Option<V>` plus `Either`, so conditional UI does not need `boxed()`.
- Widget lengths take `impl Into<Pixels>` (today 132 `f32` fns against 4 `Pixels` fns).
- Configuration objects are `#[non_exhaustive]` builders; struct literals only for pure data.
- Navigator's ~30 push/pop variants leave the prelude for `widgets::navigator`, and the Router ADR comes before any more Navigator work.
- Form and FormField are built on signals.
- `CustomPainter: Any` (trait upcasting) replaces the required `as_any`.
- `column!` is dropped in favour of tuples, or renamed `children![]` so it no longer collides with std.

---

## 6. Performance model

Cost should scale with **what changed**, not with what exists. The gaps that are structural:

- The per-frame fresh LayerTree plus `DamageRegion::Full` (`raster_lane.rs:354,486`) is fixed by §3 identity + differ + IR cache.
- O(tree) passes inside frames (topology sync, slab scan, uncoalesced mark_needs_layout #1042, deep view clones) are fixed by local commits.
- UI and raster serialized, and coupled through one font mutex: fixed by per-realm text plus the threaded lane.
- Startup is serial and unmeasured (font scan at `runtime.rs:140-147`, eager pipelines, no `wgpu::PipelineCache`).
- `info!` logs in per-element mount (`behavior.rs:1059,1090`) must be demoted, and a hot-path log-level rule enforced.

Measurement:

- An end-to-end harness on flui-testing's virtual clock. Scenarios: 10k/100k scroll, one-text change, route push, cold start to first present.
- **Deterministic counts gate every PR**: elements built, layout roots, layers, bytes uploaded, damage area, allocations.
- Wall time is a per-OS nightly trend.
- `bench-collect` must stop skipping `required-features` benches (`tools/xtask/src/bench.rs:36`), which today hides the ADR-0061 damage baseline.
- "Not worse than Flutter" (the H2 exit) needs a mirrored scenario suite, which should be designed in H1.

---

## 7. Safety model

- **Panics.** The unwrap work is done (0 in production). Make the `BUG:` convention machine-checked with a syn lint and a ratchet.
- **Unsafe.** Confined to named islands:
  - platform backends, about 309 sites. Kept per backend with an xtask ledger, `undocumented_unsafe_blocks` enabled per backend module, and a live-run proof for every PR that touches a backend CI does not execute.
  - `subtree_arena.rs`, which stays Miri-covered.
  - hot reload, which goes away with Subsecond.

  Delete the speculative ones: the SSE colour lerp and `pub const unsafe fn` in id.rs. Making realm types `!Send` removes most `unsafe impl Send/Sync`.
- **Globals.** `cargo xtask globals` with an allowlist (item, reason, horizon). Burn-down order: FONT_SYSTEM → decode cache → `ERROR_VIEW_BUILDER` → TIME_DILATION → `AssetRegistry::global` → navigator/lane/GlobalKey TLS. Process-wide ID counters become realm-scoped where they reach protocol or replay payloads. **Hypothesis** that some do; verify.
- **Fault isolation.** One `guarded_call(node, phase, f)` for every third-party trait call. On repeated paint poison, substitute an error box instead of freezing frames. Document that wasm panic=abort makes all of this inert.
- **Supply chain.** `multiple-versions = "warn"` with a reasoned skip list (67 duplicated crates today), and no platform types re-exported from the facade.
- **Devtools.** Off in release by type (cfg), not by env var.

---

## 8. Delete / merge / replace with ecosystem crates

| Delete or merge | Replace with |
|---|---|
| flui-tree (merge), flui-localizations (delete), flui-platform winit-on-Windows/macOS path, `LinuxPlatform`, second `Window` trait, `PlatformEmbedder`, `BasicVelocityTracker`, `EventRouter`, `InputPredictor`, `OneEuroFilter`, `SemanticsSnapshot`, `ListenerRegistry`, `ElementBuildContext`, `__private`, `embedder` stub, dead features, types physics/BoxConstraints/MaterialColors, geometry GPUI vocabulary, tools/web-server, Python/Swift device checks (port to the flui-mcp driver lib), dlopen hot reload | — |
| cosmic-text + FONT_SYSTEM | Parley + fontique (+ glifo if the spike wins) |
| hand glyph rasterization via swash | skrifa (+ glifo) behind the ADR-0067 door |
| unicode-segmentation (separate) | ICU4X via Parley |
| none (CPU goldens) | vello_cpu or tiny-skia in flui-engine-cpu |
| own frame-demand heuristics | frameclock-style demand/plan (evaluate the `frameclock` crate) |
| dlopen ABI hot reload | Subsecond |
| per-framework agent wire format | flui-protocol aligned with MCP 2026-07-28 + AccessKit; evaluate egui_inspection co-design |
| per-crate publish scripts | `cargo publish --workspace`, release-plz, cargo-semver-checks, cargo-public-api |
| Keep: ui-events (input vocabulary), accesskit, wgpu, lyon (until measured), slab | — |

---

## 9. Breaking changes to make now (pre-publish, ordered)

1. **Split platform-api from platform backends.** Remove the `desktop` default and add the reach facts. Unblocks plugins, compile times and wasm purity.
2. **Curated facade.** Catalog-neutral default, enumerated prelude, no whole-crate re-exports, `unstable` feature, no `android_activity` re-export. Add the public-API snapshot gate.
3. **Tier metadata**, plus the rule that official packages depend only on `flui`. Material and Cupertino are ported onto the author SDK. Raw primitives move down.
4. **Finish the !Send flip.** Protocol traits, controllers, notifiers, semantics handlers and render-object bounds.
5. **PlatformCapability seam and text-store IME contract** (TSF on Windows). Clipboard, haptics and dialogs as the first clients.
6. **Signals default, per-realm graph** moved down to foundation, `Reactive<T>` prop type, effects phase.
7. **Router ADR.** Freeze Navigator features; replace `NavigatorCommand` in the runtime channel with a navigation intent.
8. **Extract flui-runtime.** HeadlessBinding becomes a client of it. Delete `runtime-internals`.
9. **Scene contract in flui-layer** (identity, differ, lowering, conformance) plus the flui-engine-cpu skeleton. The engine's "nothing pluggable" stance is superseded by an ADR.
10. **flui-protocol crate.** desktop-mcp becomes a library; `flui mcp`.
11. **Value cleanup**: Color model, one Rect/Axis, the `Pixels` Eq fix, `GenId` for Layer/Semantics ids, trim dead surface.
12. **Parley + per-realm fonts** as one ADR.
13. **Subsecond**; delete the dlopen crate, examples and features.
14. **`cargo xtask globals`, module-direction and release-check gates**, ADR front-matter index, and removal of process markers.

Items 1–3 and 10 decide the ecosystem shape. They are the cheapest now and the most expensive after the first crates.io publish.

---

## 10. Evolution H0 → H4 without rewrites

| Horizon | What is added (no rewrite, because the seam already exists) | Seams that must already exist at the start of the horizon |
|---|---|---|
| **H0** | Router, Form, multiline editor; CPU goldens via engine-cpu; semantic goldens (outline format); `flui mcp` over flui-protocol; damage differ; Parley; Subsecond; crates.io publish of core + material package | platform-api, protocol, facade tiers, scene contract, runtime crate, catalog derive (schema only) |
| **H1** | Plugins (camera, files, geo, notifications) as official or community crates; accesskit_android/ios; mobile hosts as `PlatformHost` adapters; flui-a2ui over the catalog registry; tokens-as-data; flui-i18n (ICU4X); compositor spike via `Presenter` + `Layer::External`; WebGL2 through the same wgpu path | PlatformCapability + `NativeContext` + override hook; `Reactive<T>` + JSON-Pointer projection; `Presenter` trait; token types |
| **H2** | Threaded raster lane (Win/Linux), IO lane consumers (assets), layer/raster cache, native presenters with OS partial present, software fallback = engine-cpu in production, GpuContext multi-window, perf gates vs Flutter mirror suite | `!Send` realm state; scene identity; one execution owner; demand authority; pipeline-cache contract |
| **H3** | semver-checks on 3 Stable crates for 3 minors; Material 3 Expressive and Cupertino evolve on their own trains; `flui migrate` from per-crate migration data; docs site generated from the catalog; repo split for packages whose cadence diverged | Tier metadata + public-API snapshots from H0 (so the freeze is a label, not a refactor) |
| **H4** | Community catalogs verified by conformance kits (`flui verify` badges); embedded/kiosk via `Host` + engine-cpu; video and 3D via External content; Vello GPU as another scene backend if lyon loses; MCP extensions in flui-protocol's reserved namespace | Conformance kits for render objects, backends, capabilities and protocol |

The invariant that makes this work: **every horizon adds implementations behind contracts that were proven by a second implementation in an earlier horizon (P9)**. H3 then freezes only three crates' surfaces, which were already small and snapshotted.

---

## 11. Risks and deliberate non-goals

**Risks**

- **Seam-first delays the H0 user loop.** Mitigation: every seam in §9 is justified by an H0 exit item (plugins→clipboard for the form; protocol→`flui mcp`; scene→goldens; facade→crates.io). Anything that is not goes to H1. The WIP limit still applies.
- **The author SDK grows into a second full API.** Mitigation: it contains only what Material/Cupertino actually import, measured by their manifests compiling against `flui` only.
- **Upstream churn** (Parley/glifo pre-1.0, winit 0.31, A2UI v0.x, MCP): wrapped per P10, pinned as cohorts, protocol versioned separately from AccessKit.
- **CPU backend divergence** (SIMD dispatch, missing filters in vello_cpu): pin SIMD level in test mode; GPU-only effects are covered by semantic goldens only. **Hypothesis** that bit-identical goldens across OSes are reachable; the H0 exit may need to say "one golden OS + CPU reference" instead.
- **Moving the reactive core to foundation widens its blast radius.** Mitigation: the graph core is data-structure-only (no realm type), measured with `cargo build --timings`.
- **Bus factor.** Tiers reduce what must be reviewed, and conformance kits let AI and external contributors prove extension work without the author.

**Deliberate non-goals (until data says otherwise)**

- No intra-realm parallel layout. No per-realm owner threads on AppKit/UIKit/wasm.
- No separate repos for official packages before their cadence diverges. No crate split of flui-widgets (the 2026-09-23 decision stands, now with a real gate).
- No custom DSL or markup for AI generation (Makepad/Slint); typed catalog + A2UI instead, **measured** by agent-task success.
- No MCP inside core; no MCP Apps (HTML iframe) as the generative-UI runtime.
- No user-extensible `Layer` enum or `DrawOp` variants; one registered `External` / custom-program variant only.
- No third layout protocol besides Box and Sliver before 1.0.
- No stable guarantee on any internal crate, ever. The promise is the facade + platform-api + protocol.
