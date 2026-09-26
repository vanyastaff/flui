# FLUI target global architecture, DX-first design angle

Status: a proposal from the "developer experience first" angle, grounded in the codebase maps at main `cab06137d` and in the market surveys. Anything marked **(hypothesis)** has not been measured.

The design test for every decision here is how short the correct path is from `flui create` to a multi-screen app with forms, lists, navigation, tests and hot reload. The path should use one import, one state model, one navigation model, and errors that say what to do. Architecture matters here because it decides how short that path can be. Today the path is long for structural reasons more than for missing widgets:

- The counter needs two derive+impl pairs and a `bind` step (examples/counter.rs).
- The prelude leaks `BuildOwner` and `tracing::info!` (crates/flui-view/src/lib.rs:252,278-285).
- Clipboard is installed but unreachable (crates/flui-app/src/app/runtime.rs:1636).
- Some callbacks reject `Rc` captures because of stale `Send + Sync` bounds (crates/flui-widgets/src/scroll/page_view.rs:502).

---

## 1. Guiding principles

I accept the owner's seven principles. The DX angle adds five operational rules and differs from the plan in three places.

**Added rules**

1. **The facade is the product.** App authors see `flui` and nothing else. Internal crates may be published because crates.io needs them, but they are documented as framework-author surface with no semver promise. Today the facade re-exports seven whole crates (src/lib.rs:120-152), which amounts to about 6k pub items, so the product surface is accidental.
2. **One canonical way per concept.** That means one state model, one navigation model, one entry point, one test harness, one way to declare a view (a derive). Today there are five state mechanisms, about 30 push/pop variants, 8+ entry points, three test namespaces, and two view-authoring mechanisms (derives vs `impl_render_view!`).
3. **Errors are documentation.** Keep and extend `#[diagnostic::on_unimplemented]` (already on View/RenderBox/ParentData). No silent no-ops: an unbound `StateCell` mutation currently "schedules nothing" (crates/flui-view/src/state_cell.rs:48-60), which should be a type error or a debug panic with a fix hint.
4. **Generated, not hand-written.** The widget catalog, llms.txt, the AGENTS.md index for templates, the crate map and the ADR index are generated from code or manifests. The current hand-written ones are already stale: llms.txt teaches `flui_app::run_app`, and book/src/concepts/state.md:45 says signals do not exist.
5. **Every rule a gate.** A convention without a gate decays. The evidence: the ambient-reach ratchet file is gone, the import-direction check that justified keeping flui-widgets as one crate was never built, and there is no marker check.

**Where I disagree with or refine the plan**

- **Principle 2 ("one renderer").** Keep one *raster contract* and allow several rasterisers behind it. The engine's own stance, "nothing here exists to make one pluggable" (crates/flui-engine/ARCHITECTURE.md:8-14), blocks E7 goldens, the H2 software fallback and Linux/Android devices without Vulkan (crates/flui-engine/Cargo.toml:105-112 has no GLES). From the DX side, `flui test --golden` must run on any CI box without a GPU. That needs a CPU backend that implements exactly the same layer semantics, and the headless path already diverges on three layer kinds (headless.rs:15-27).
- **Official packages in separate repos.** Keep the monorepo until a package's release cadence actually differs. Flutter merged its engine back into one repo in 2024, and Slint keeps Material as a separate workspace in the same repo. What matters for users is that the *dependency* shape is right, meaning Material depends only on `flui`, not where the code lives. Move Material and Cupertino into a `packages/` directory with its own workspace in H1. Split repos only after H3.
- **"Home for Flutter developers".** The plan's own review already weakens this bet. The DX angle says to keep Flutter's *mental model* and drop Flutter's *names* from the Stable surface: `WidgetsBinding`, `RenderingFlutterBinding`, and `FlutterError` (flui-view lib.rs:201). Those names freeze a Dart-shaped API that Rust users do not recognise.

---

## 2. Workspace and crate topology

### 2.1 Target tiers

Six tiers replace the current eleven layers. Each tier has a *reach rule* (what it may link) as well as a direction rule.

| Tier | Crates | Reach rule (checked by `cargo xtask workspace`, as manifest data) |
|---|---|---|
| T0 values | `flui-geometry`, `flui-types`, `flui-macros` | no OS, no tokio, no wgpu, wasm-clean |
| T1 substrate contracts | `flui-foundation`, `flui-reactive` (new), `flui-platform-api` (new), `flui-protocol` (new) | same as T0 |
| T2 machine | `flui-scheduler`, `flui-painting`, `flui-interaction`, `flui-assets`, `flui-layer`, `flui-semantics`, `flui-animation`, `flui-rendering`, `flui-objects` | no winit/windows/objc2/android-activity/wgpu. tokio only behind a feature |
| T3 spine + runtime | `flui-view`, `flui-runtime` (new) | same as T2 |
| T4 catalog + rasterisers | `flui-widgets`, `flui-engine` (wgpu), `flui-engine-cpu` (new) | engine crates may link wgpu, widgets may not |
| T5 composition | `flui-platform` (backends), `flui-app` (runners), `flui` facade, `flui-test`, `flui-devtools`, `flui-cli`, `flui-log` | anything |
| Packages (above the facade) | `flui-material`, `flui-cupertino`, `flui-a2ui` (H1), plugins | may depend on `flui` only, plus `flui-test` as a dev dependency |

The most valuable gate is a **reach fact**: `cargo tree -e normal -p flui-widgets` must not contain `winit|tokio|windows|objc2|wgpu`. Today it contains winit 0.30, tokio 1.53 and windows 0.62, all through one import (crates/flui-interaction/src/text_input.rs:27). Put these facts in each manifest as `[package.metadata.flui] forbid-reach = [...]`.

### 2.2 Fate of all 27 current crates

| Crate | Fate | Reason (evidence) |
|---|---|---|
| flui-geometry | **keep, trim** | Delete the unused vocabulary: Length/Rems, Transform2D, the Bezier types, text_path, the kurbo bridge, and the no-op `mint` feature (about 3.5k lines with no consumer). Keep one Rect, one Axis and Point+Offset. Fix Pixels so Eq and Hash agree (units.rs:91,575-595). Move the geometry tests out of flui-types and into this crate. |
| flui-types | **keep, shrink** | Delete `physics` (duplicated in animation) and its `BoxConstraints` (duplicated in rendering). Move `MaterialColors` to material, gesture details to interaction, and haptic/IME vocabulary to platform-api. Decide Color f32 + color space before publishing (color.rs:25). |
| flui-foundation | **keep, split internally** | The public part is IDs, keys, diagnostics names and observe. The runtime protocol (FrameStamp, PresentationAddress, ClaimSlot, OwnerAffinity) goes into `#[doc(hidden)] pub mod rt` and is not re-exported. It also absorbs Arity/Slot/Depth from flui-tree. Delete ListenerRegistry. |
| flui-macros | **keep, grow** | Add `#[derive(RenderView)]`, `#[derive(Store)]` and `#[derive(Catalog)]`, and make `#[flui::main]` an attribute. Add facade-only trybuild tests. |
| flui-tree | **merge** into foundation (arity, slot, depth). Drop the trait trio. | No generic consumer exists. ElementTree does not implement the traits. It pulls in `bon` for a single builder. |
| flui-log | **keep** | Clean composition-only design (allowed-dependents). |
| flui-platform | **split** into `flui-platform-api` (T1) and `flui-platform` backends (T5) | Separating contract from backends is the precondition for PlatformCapability plugins, headless purity and rebuild cost: 16 crates rebuild on every backend edit. |
| flui-scheduler | **keep, slim** | Split into a `!Send` owner core plus a `Send` waker. The async driver moves to flui-runtime. The global TIME_DILATION (config.rs:43) moves to the presentation clock. |
| flui-painting | **keep** | Owns DisplayList and a realm-injected text service (Parley). Remove FONT_SYSTEM (layout.rs:124) and the `fontdb::Family` re-export (lib.rs:87). |
| flui-interaction | **keep** | Depends on platform-api only. The owner-lane closure registry moves to flui-runtime. The gesture arena moves from `Arc<Mutex>` to `Rc<RefCell>`. |
| flui-assets | **keep, de-runtime** | Loaders run on an injected IO spawner. Delete `AssetRegistry::global()` and the bridge tokio runtime. Absorb the widgets decode cache (decode_cache.rs:96). |
| flui-layer | **keep, grow** | Gains a backend-neutral `lower` module (layer walk, `CommandRenderer`, LayerStateStack, effect decomposition, damage diff) moved out of the engine. Layer identity stays stable across frames. |
| flui-semantics | **keep** | AccessKit-native action set. Owner-lane action targets instead of `Send + Sync` handlers (action.rs:217). |
| flui-animation | **keep** | `!Send` controller, a single presentation clock, no wall-clock Ticker path. |
| flui-rendering | **keep** | Protocol plus pipeline. Gains a topology transaction API (drop `render_tree_mut`, accessors.rs:348), the build-during-layout cells and a lazy-child trait. Catalog parent data, delegates and ScrollPosition move out to objects. |
| flui-objects | **keep** | Concrete catalog. Its harness becomes a conformance kit exported via `flui::test::rendering`. |
| flui-engine | **keep, narrow API** | The wgpu rasteriser implementing the `flui-layer::lower` contract. `pub use ::wgpu`, `raster_owner` and `WgpuPainter` go behind `unstable-wgpu-interop` or become crate-private. |
| flui-view | **keep, split visibility** | Authoring surface: View, StatelessView/StatefulView, ViewState, InheritedView, RenderView, contexts, keys, Signal. Internals go into `#[doc(hidden)] pub mod __runtime`, which replaces the `runtime-internals` feature. LayoutBuilder, the sliver adaptor and the async builders move to widgets via a public element protocol. `WidgetsBinding` moves to runtime. |
| flui-widgets | **keep, one crate + module DAG gate** | Gains the Raw primitives (RawButton/FocusableActionDetector, toggles, Surface, ink), Router, Form, an Icons table. Drop `__private`. |
| flui-testing | **rename → `flui-test`, move to T5** | WidgetTester with finders over flui-protocol queries, goldens via engine-cpu. The widgets→testing optional normal edge is removed. |
| flui-hot-reload | **delete** | Replaced by a Subsecond hook in flui-runtime plus `flui run --hot`. Removes the dlopen unsafe code, 3 facade TREE_FACTS and 5 example members. |
| flui-material | **package** (`packages/material`, depends only on `flui`) | ADR-0028 substrate moves down into widgets. The `flui::sdk` module exposes what it needs (currently RenderPhysicalShape, LocalPostFrameHandle and the Canvas internals: material.rs:83, scaffold_messenger.rs:212). |
| flui-cupertino | **package** | Same shape. Gains focus and keyboard activation for free through the Raw primitives. |
| flui-localizations | **delete** | Its RTL table (281 lines) folds into `flui_widgets::localization`. Design systems own their own strings, as Flutter does since 3.47. ICU4X i18n becomes an H1 package. |
| flui-app | **keep, shrink to runners** | Realm, frame transaction, execution, semantics host and held input move to flui-runtime. What stays: `PlatformHost` adapters, window/surface/device lifecycle, policy wiring. Target under 15k lines. |
| flui-cli | **keep, independent version** | Add `mcp`, `devtools`, `test --golden`. Delete `format`, or make `analyze`/`test` emit per-item NDJSON with a schema. Absorb tools/web-server. |
| flui-devtools | **repurpose** as the in-process protocol server (T5, debug-only feature) | Currently has zero production consumers. Its profiler and timeline become protocol streams. |

**New crates:**

- `flui-reactive` (T1): the realm-owned graph core (push-pull, phase-typed subscribers, write journal).
- `flui-platform-api` (T1): capability traits, the IME document protocol, the event vocabulary, the `Unsupported` type.
- `flui-protocol` (T1): ADR-0080 wire types extracted from tools/desktop-mcp, with serde and accesskit only.
- `flui-runtime` (T3): realm, presentation, frame transaction, lanes, capability registry, Subsecond hook.
- `flui-engine-cpu` (T4): the CPU rasteriser.

Net result: 27 crates, minus tree, hot-reload, localizations, material and cupertino (the last two become packages), plus 5 new crates, gives **27 core publish units, 2 official packages and the CLI versioned independently.** The count stays about the same, but each boundary is now a real seam (contract vs implementation, headless vs OS, core vs package) and not an organisational one.

### 2.3 Module boundaries inside large crates

- **flui-widgets:** declare a DAG in its manifest metadata and have `cargo xtask checks` parse `use crate::…`/`super::` edges. The order is base (layout, paint, flex, stack) → interaction → {text, scroll, overlay, image} → navigation (Router, Navigator stack) → app. The gate is cheap to lock in now because cross-imports between feature modules are about 1 today (navigator→overlay).
- **flui-app** after the move: `runner/{desktop,android,ios,web}.rs` as thin `PlatformHost` impls, plus `window/`, `surface/`, `device/`. Also split realm_dispatch.rs (7,149 lines) by event class.
- **flui-engine:** `backend/` (wgpu passes) and `resources/` (atlas, textures, pipeline cache). Move the 22k lines of in-src readback suites under `src/tests/`. Fix the LOC metric in xtask: real production code is about 18.8k lines, not 74k.

### 2.4 Feature-flag policy

1. Features are additive. Every optional dependency is `dep:`. **Empty features are deleted.** Today these have zero cfg sites: flui-app desktop/android/ios/web/debug-overlay/performance-overlay; flui-platform `desktop` (it just pulls in winit, Cargo.toml:303), `web`, `wayland`, `x11`; geometry `mint`.
2. Exactly one convention for unstable surface: `unstable-<area>`, with a single `unstable` meta-feature on the facade. Internal seams are `#[doc(hidden)] pub mod __runtime`, never cargo features, because feature unification turned `runtime-internals` on in every app (crates/flui-app/Cargo.toml:90).
3. No `testing` features on production crates. Test hooks are `cfg(test)`, or a hook-registry trait that `flui-test` installs. Today 57 `cfg(feature="testing")` sites reshape `PipelineOwner` fields (owner/mod.rs:251-257), so tests run a pipeline that differs from the one that ships.
4. Platform backends are target-conditional dependencies of flui-app, not features. `a11y` is on by default for Windows and macOS. Linux AT-SPI is `a11y-linux`, on by default with an opt-out.
5. Facade features: `images`, `network-images`, `serde`, `devtools` (implied by debug builds through the CLI), `unstable`. **No design-system features.**

### 2.5 Facade and prelude

```rust
// flui (facade) — curated modules only, each tagged with its tier in rustdoc
pub mod prelude;     // Stable. Explicit list, no globs of lower preludes
pub mod view;        // Stable: authoring traits, contexts, keys, Signal/Computed/Effect/Store
pub mod widgets;     // Stable: base catalog + Raw primitives + Router + Form
pub mod rendering;   // Stable: RenderBox/RenderSliver authoring, conformance kit hook
pub mod painting;    // Stable: Canvas, Paint, Path, TextStyle
pub mod platform;    // Stable: PlatformCapability, Unsupported, built-in capabilities
pub mod animation;   // Stable
pub mod sdk;         // Evolving: what design systems / catalogs need (Surface internals, post-frame)
pub mod test;        // Stable (dev): WidgetTester, finders, goldens
#[cfg(feature = "unstable")] pub mod unstable; // Experimental
```

The prelude is **catalog-neutral** and snapshot-tested. `cargo xtask public-api` runs on the facade and fails on any diff that has not been reviewed. Material ships `flui_material::prelude`. A Material app therefore has two glob imports, and that is intentional: when Material leaves the release train, no app imports break.

### 2.6 Publish order

This is one train with `cargo publish --workspace` (stable since Rust 1.90). Internal versions move to `[workspace.dependencies]` so a bump is one line; there are currently 172 hard-coded `=0.2.0-dev` pins. Order:
geometry → types → macros → foundation → reactive → platform-api → protocol → log → scheduler → painting → interaction → assets → layer → semantics → animation → rendering → objects → view → runtime → widgets → engine → engine-cpu → platform → app → devtools → test → flui. After that, the packages and the CLI are published on their own versions.

CI gains `cargo xtask release-check` (a package dry-run in this order plus semver-checks on `flui`). There is currently no publish pipeline at all: release.yml:17 says "Publishing to crates.io is not done here".

---

## 3. Runtime model

**Trees.** Keep View → Element → RenderObject → Layer plus Semantics. Two changes:

- **The Layer tree gets stable identity.** Each repaint boundary gets a persistent `Arc` subtree keyed by `RenderId`, so grafting a boundary costs O(1) and damage becomes a diff. Today every frame mints fresh `LayerId`s (paint.rs:1383-1425) and sends `DamageRegion::Full` (raster_lane.rs:354).
- **View configurations are shared, not deep-cloned.** Children move into child elements, or they are `Rc`-shared. Today `BoxedView::clone` is a deep `dyn_clone` at every level (into_view.rs:178; behavior.rs:1071).

**Realms.** Be honest about what exists. Each owner thread hosts N `!Send` realms. On AppKit, UIKit and wasm the owner thread *is* the main thread. Amend ADR-0027's "realms execute concurrently" to "realms are isolated, not concurrent". Record intra-realm parallel layout as a non-goal, since the types already rule it out: RenderObject is not Send and PipelineCell is `Rc<RefCell>`. Move the realm registry out of the `APP_RUNTIME` thread_local (host.rs:46) into an `OwnerHost` object owned by the runner.

**`flui-runtime`** is one platform-free core with a single frame transaction:

```rust
pub struct Realm { presentations: Forest, reactive: flui_reactive::Graph, capabilities: CapabilityRegistry, global_keys: GlobalKeyScope, /* … */ }
impl Realm {
    pub fn pump(&mut self, clock: &mut dyn FrameClockSource, sink: &mut dyn FrameSink) -> FrameOutcome;
}
```

`flui-app` drives it with the platform clock and a raster-lane sink. `flui-test` drives it with a `ManualClock` and a headless or CPU sink. This removes the second pipeline in `HeadlessBinding::pump_frame` (flui-testing/src/lib.rs:955-1079), which today makes widget tests exercise an ordering that production does not use.

**Phases:** input → build → **effects** → layout ⇄ build (lazy children built *inside* sliver layout rather than between passes, removing the up-to-10-pass fixpoint in layout_builder.rs:64-74) → compositing → paint → semantics → submit. Frame demand has one authority, the presentation's `FrameClock` DemandMask. Today five mechanisms produce demand and a loop-wide `needs_redraw` flag is shared across realms.

**Lanes.**

- *Raster lane:* accept ADR-0045 as mode-agnostic. `Inline` is permanent on macOS and wasm, and a thread runs it on Win32 and Linux. Web moves onto the same lane, and `DirectSink` is deleted.
- *IO/compute lane:* reachable from views through a capability (below). There is one executor owner. tokio is the default implementation behind a feature, and hosts can inject their own. Delete the runtimes in flui-platform (executor.rs:66) and flui-assets (bridge.rs:66).

**Async (DX surface):**

```rust
impl ViewState<Search> for SearchState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let results = self.results;                // Signal<Vec<Hit>>, Copy
        self.task = cx.spawn_io(async move {       // cancelled on unmount; !Send result delivered next frame
            fetch_hits().await
        }).then(move |hits, rx| results.set(rx, hits));
    }
}
```

`AsyncDriver` gains `!Send` local futures. An owner-scoped task cannot read a released slot, which avoids the Leptos "already disposed" failure class.

**State.** Signals become canonical and on by default (`signals` is currently off, Cargo.toml:645). The graph core moves down to `flui-reactive` (T1) as a realm resource. Its subscribers are phase-typed: an element rebuild, a render object's `needs_layout`, or its `needs_paint`. This follows Compose's per-phase reads. It answers the plan's "element vs render granularity" question with "both", lets an animated colour repaint without a rebuild, and retires the `Arc<Mutex>` Listenable repaint path (notifier_generic.rs:41). Other parts:

- The graph moves from BuildOwner to the realm, so one signal is shared across a realm's windows. Today it is per presentation (build_owner.rs:444) and `SignalWrite` always targets the primary window (commands.rs:450).
- Propagation is push-pull, three-state, glitch-free (ADR-0075), and effects run in a named phase.
- `#[derive(Store)]` provides path-keyed collections whose keys line up with reconciler keys (A8).
- A write journal records slot, writer and frame. It feeds devtools, `flui mcp` and record/replay, which brings back iced-style time travel.
- `StateCell` and `StateHandle` go to `flui::view::state` as the setState-level escape hatch. `ValueNotifier` controllers become signal-backed stores. `TextEditingController` becomes a `TextFieldState` with an `edit{}` transaction, transforms and undo, replacing `Arc<Mutex<ControllerInner>>` (controller.rs:263).

**Rendering and text.** The raster contract lives in `flui-layer::lower`. The wgpu and CPU rasterisers are peers, and a conformance suite runs every scene through both. Custom content enters through a single open variant, `Layer::External { id: ExternalContentId }`, backed by typed registries (textures, shader programs). Text uses Parley as one shaper everywhere, with a per-realm `FontContext` over a fontique `Collection { shared: true }`. The glyph atlas is owned per GPU device, and rasterisation happens outside any shaping lock. The paragraph crossing in the DisplayList becomes a neutral shaped-run type (font blob id, glyph id, size, variation coordinates) rather than `Arc<cosmic TextLayout>` (command.rs:167). The host font scan becomes asynchronous: bundled faces are available at the first frame, and system fonts arrive later as a realm event that relayouts text. That event also fixes the missing invalidation when a font is registered.

**Platform.** `flui-platform-api` holds the contract:

- Owner-affine `!Send` windows, plus a `Send` `WindowHandle` proxy with a closed set of verbs (redraw, close, …). This removes most of the 26 `unsafe impl Send/Sync`.
- One lifecycle stream for all backends.
- A **pull-based IME document protocol**: text in range, selection, rect for range, index for point. The current push-only 2-method trait (traits/text_input.rs) cannot serve TSF, InputConnection or UITextInput.

Windows ships TSF from the start. The Win32 backend currently has no IME code at all, and IME is a B1 exit criterion. Backends are one per OS: winit is *the* Linux backend, native backends are used elsewhere, and `LinuxPlatform` (which panics with `unimplemented!`) is deleted.

**Accessibility.** On by default. There is one vocabulary, AccessKit, used natively in `flui-protocol`. Semantics can be enabled without an assistive technology running, through a refcounted `SemanticsHandle` capability, which is required for the in-process agent backend and for semantic goldens. Android and iOS adapters are H1 items. Every catalog widget declares semantics or explicitly declares none, checked by a generated test. Today EditableText publishes no text-field node.

---

## 4. Extension points and the plugin model

### 4.1 PlatformCapability (H1, design the seam in H0)

`LifecycleContext` stays sealed, which keeps the ADR-0078 guarantee that `build` cannot acquire anything. It gains **one generic method**, so the set of capabilities is open while the trait stays closed:

```rust
// flui-platform-api
pub trait PlatformCapability: 'static {
    type Handle: Clone;
    fn attach(native: &NativeContext<'_>) -> Result<Self::Handle, Unsupported>;
}
pub enum NativeContext<'a> { Win32 { hwnd: HWND /*raw*/ }, AppKit { .. }, Android { vm: .., activity: .. }, Ios { .. }, Web { .. }, Headless }

// flui-view
fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported>; // on LifecycleContext

// app side
App::new(root).capability::<Camera>().run();

// plugin crate (outside repo)
pub struct Camera;
impl PlatformCapability for Camera { type Handle = CameraHandle; fn attach(n: &NativeContext) -> Result<_, Unsupported> { … #[cfg(target_os)] … } }
```

This is modelled on Flutter's federated plugins (interface crate, endorsed implementation per target via `[target.cfg()]` dependencies, override hook from the start) without Flutter's global `Platform.instance` registration. Owner-thread work reaches a plugin as a typed `OwnerTask` trait object, not a closure channel, which respects ADR-0039. The first three "plugins" are built-in: clipboard (wiring the dead accessor at runtime.rs:1636 and giving EditableText copy/paste), haptics (presentation.rs:866, currently no caller) and file dialogs (currently Win32-only with no caller). Delete `PlatformCapabilities`, the boolean table with no consumer, to avoid the name clash.

### 4.2 Third-party render objects

The authoring surface is `flui::rendering`. It grows to include slivers, `ViewportOffset`, `LayerLink` and the lazy-child trait, so that a third-party lazy list compiles against `flui` alone. Today the facade fixture contains no RenderSliver. `#[derive(RenderView)]` replaces `impl_render_view!`:

```rust
#[derive(Clone, RenderView)]
#[render(object = RenderBadge, update(set_color = color))]
struct Badge { color: Color, #[view(child)] child: BoxedView }
```

The **conformance kit** is `flui::test::rendering::check_box::<T>(factory)` and `check_sliver`. It checks that dry layout equals layout, that intrinsics are finite and monotone, that the baseline is inside the size, that hit-testing stays within bounds, that relayout is idempotent, and that semantics are stable over two frames. The first-party catalog runs it for all 84 objects, replacing the string registry at render_object_harness.rs:151. `CustomPainter` drops `as_any` (trait upcasting is stable) and drops `Send + Sync`.

### 4.3 External GPU content

`cx.capability::<TextureRegistry>()` returns a `Send` handle that registers and updates textures through the raster mailbox. A `Texture` widget backed by `RenderTexture` emits `Layer::External`. Device sharing is behind `unstable-wgpu-interop`. H2 adds the presenter split (a Subduction-style `Presenter`): the layer tree lowers to N surfaces plus a native compositor tree (DirectComposition, CoreAnimation, SurfaceControl), with a single swapchain as the fallback. That split is also the only route to real OS partial present, because wgpu has no present-with-damage (wgpu#682). Until then, delete the no-op `PlatformViewLayer` handler (layer_render.rs:381).

### 4.4 Themes as data

Add a design-neutral token substrate in flui-widgets: color, typography, shape and motion token maps with serde, resolved through InheritedView with FieldMask. The design systems provide `ThemeData::from_tokens`. `WidgetStateProperty` gets a serializable per-state map form and keeps the closure form as an escape hatch. ADR-0042's "no universal ThemeData" still holds, because tokens are not ThemeData.

### 4.5 Agent protocol

`flui-protocol` holds the ADR-0080 types: handles, AccessKit roles taken from `accesskit::Role` rather than the hand-copied enum (desktop-mcp role.rs:17), error codes, the outline renderer, bounded reads, and the event envelope. The clients are:

- (a) the in-process server (`flui-devtools`, debug builds, a named pipe or unix socket with a per-launch token, never TCP opened by an environment variable alone);
- (b) `flui mcp` (stdio, a thin adapter);
- (c) the desktop UIA/AX/AT-SPI backend, which becomes a library also used by `xtask device` and live-smoke;
- (d) `flui-test` finders.

Following the MCP 2026-07-28 revision, handles are backend-owned and there are no sessions. Every action returns the post-action outline. That outline is also the semantic-golden file format, one node per line and deterministic, so tests and agents read the same artifact.

### 4.6 A2UI

`#[derive(Catalog)]` on catalog views emits a JSON Schema for props, the semantics role, example IDs (which compile as tests) and a registry entry. From that one source the build generates the G6 manifest, the A2UI catalog (URI `catalogId`, versioned), the prompt text, the compressed index in the `flui create` AGENTS.md, and llms.txt. Interactive props are modelled as named actions: an action ID plus an optional closure. Data binds through a JSON-Pointer projection of realm signals. `flui-a2ui` is an Evolving package with a transport-adapter trait and no LLM client, following GenUI's May 2026 redesign. The protocol is pre-1.0 (v0.9 renamed properties), so it stays out of core.

---

## 5. Public API and DX

**Hello world.** No wrapper type is needed. Today `run_app` requires `StatelessView + Clone`, which forces one (runner/mod.rs:214).

```rust
use flui::prelude::*;

#[flui::main]
fn main() -> App { App::new(Counter) }

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState { count: Signal<u32> }

impl ViewState<Counter> for CounterState {
    fn create(cx: &dyn LifecycleContext) -> Self { Self { count: cx.signal(0) } }
    fn build(&self, _: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;                      // Copy: no clones
        Column::new((
            Text::new(format!("{}", count.get(cx))),
            RawButton::new(Text::new("+")).on_press(move |rx| count.update(rx, |n| *n += 1)),
        ))
    }
}
```

`#[flui::main]` expands to the correct entry point on each target, which replaces `run_app_android` and `run_app_ios`. `rx: &Reactive` is supplied to every callback by the framework, so a write is always attributable, which the journal and replay need, and needs no capture. Views implement `View for Option<V>` and `Either`, so a conditional does not have to box. The `column!`/`row!` aliases (macros/mod.rs:42), which collide with std, are dropped; `Column::new((a, b))` is taught instead.

**Form (C1).**

```rust
let email = cx.store(FormState::<Signup>::default());
Form::new(email, (
    TextField::bound(email.field(|s| &mut s.email)).validator(validators::email()),
    RawButton::new(Text::new("Sign up")).on_press(move |rx| if email.validate(rx) { submit(email.get_untracked(rx)) }),
))
```

**Router (D1).** The navigation state is a value, and `push` is a facade that edits it. There are no pageless routes.

```rust
#[derive(Route, Clone, PartialEq)]
enum AppRoute { #[route("/")] Home, #[route("/note/:id")] Note { id: NoteId } }

Router::new(|route: &AppRoute, cx| match route { AppRoute::Home => Home.boxed(), AppRoute::Note { id } => NoteView(*id).boxed() })
// anywhere: Router::of(cx).push(AppRoute::Note { id }); URL, back button, deep links, restoration all follow
```

This needs a freeze on Navigator features now. The named, keyed and generated push variants (navigator.rs:1706-2300) move out of the prelude, and the runtime command channel carries a route or URL intent instead of `NavigatorCommand` (commands.rs:8).

**Tests.** They share their query model with agents:

```rust
#[flui::test]
fn signup_rejects_bad_email(t: &mut WidgetTester) {
    t.mount(Signup);
    t.find(role::TextInput).label("Email").type_text("nope");
    t.find(role::Button).label("Sign up").activate();
    t.expect(role::Text).label_contains("invalid email");
    t.golden_semantics("signup_error");           // outline file
    t.golden_pixels("signup_error");              // engine-cpu, bundled font
}
```

**Conventions, as one page plus a lint.**

- Widget lengths take `impl Into<Pixels>`, and `From<f32>` is allowed at the widget boundary only. Today 132 widget functions take `f32` and 4 take `Pixels`.
- UI callbacks are owner-local and never `Send`. A gate compiles an `Rc` capture against every `pub fn on_*`.
- Builders for widgets. Public-field structs only for pure data, and those are `#[non_exhaustive]`.
- `Theme::of` falls back to a default theme instead of panicking.

**CLI.** `create` (templates contain the generated AGENTS.md catalog index), `run --hot` (Subsecond), `test [--golden|--accept]`, `devtools`, `mcp`, `doctor`, `devices`, `build`. NDJSON events are typed, schema-versioned and published with `flui --json-schema`. There is no command that merely wraps cargo.

---

## 6. Performance model

The budget is "cost is proportional to what changed", enforced by *counts*, which are deterministic and gated on every PR, with wall time kept as a nightly trend. The perf harness runs on the `flui-runtime` core with a ManualClock and reports elements built, layout roots, layout passes, layers grafted, damage area, bytes uploaded and allocations. Scenarios: a 10k and a 100k lazy-list fling, a single text change, a route push, and cold start to first present.

Structural fixes with their evidence:

1. A damage producer as a layer diff on stable boundary identity. The ADR-0061 bench shows 2901 µs for a full repaint versus 56 µs with damage.
2. Local topology commits in place of `synchronize_render_children`'s global pass (element_tree.rs:1428).
3. Disjoint slab indexing in place of the whole-slab scan per dirty root (storage/tree.rs:82).
4. Shared view configurations instead of deep clones.
5. Lazy children built inside layout.
6. Coalesced `mark_needs_layout` (#1042).
7. `tracing::info!` removed from per-element paths (behavior.rs:1059,1090), with a hot-path log-level gate.
8. Pipeline prewarm plus a `wgpu::PipelineCache` (none today).
9. One GPU context per process, with a surface per window. Today each window creates its own Instance, Device, pipelines and atlas (renderer.rs:1140).
10. Measured phases of cold start. The host font scan runs on the UI thread at install (runtime.rs:140) and is assumed to be the largest single item **(hypothesis)**.
11. For compile time, which is also DX: a dev-only `dylib` facade feature in the style of Bevy's `dynamic_linking`, plus removing winit and tokio from headless graphs. Every Win32 backend edit currently rebuilds 16 crates.

---

## 7. Safety model

- **Global state:** `cargo xtask globals`, backed by a checked-in allowlist, fails on any new `static` holding `Mutex/RefCell/OnceLock/LazyLock` and on any `thread_local!` outside flui-platform and the app runners. The ratchet file the roadmap cites no longer exists. The list to burn down: FONT_SYSTEM, the decode CACHE, ERROR_VIEW_BUILDER, TIME_DILATION, AssetRegistry::global, APP_RUNTIME, NAVIGATOR_COMMAND_TARGETS, the interaction lanes and the GlobalKey REGISTRY_STACK. This also makes Subsecond sound, because Subsecond resets thread-locals in the patched crate.
- **Thread affinity:** finish the ADR-0027 `!Send` flip before H3 in one breaking change covering Listenable, Animation, TickerProvider, CustomPainter, the delegates, ScrollPhysics, HitTestTarget, ViewKey, ViewportOffset and the cells. `Send` stays only on Scene, mailboxes, `WindowHandle`, `SignalSender` and task results. Pin this with `assert_not_impl_any!`. Remove lock types from public signatures (`ElementBuildContext::tree()`, element_build_context.rs:128).
- **Unsafe:** a ledger per backend module that can only go down. Turn on `undocumented_unsafe_blocks` per module. A live-run proof is required for any PR that touches unsafe in a backend CI does not execute. Keep Miri on subtree_arena. Deleting hot-reload removes about 90 unsafe lines.
- **Panics:** unwrap is already at 0. Add a syn-based check for the `BUG:` prefix. Add `catch_unwind` around hit-test, intrinsics and semantics hooks for third-party objects. Replace a repeatedly poisoned node's paint with an error box instead of stalling the frame.
- **Devtools:** compiled out of release builds, and an opt-in plus a per-launch token at run time.

---

## 8. Delete, merge, or replace with ecosystem crates

| Delete or merge | Replace with |
|---|---|
| flui-hot-reload (dlopen), the 3-crate template, `--scene` | Subsecond, with the contract stated: logic edits keep state, and a change to a State type restarts the realm |
| cosmic-text + global FONT_SYSTEM | Parley/fontique/HarfRust, per-realm contexts; evaluate glifo for the atlas |
| unicode-segmentation (planned unicode-bidi) | ICU4X via Parley, one Unicode source for both layout and editor |
| hand-rolled UIA/SendInput in xtask; Python/Swift device checks | the desktop driver library (uiautomation/objc2/AT-SPI) behind `flui-protocol` |
| tools/web-server (axum + wasm-pack) | `flui run --device browser` |
| flui-types physics + BoxConstraints; geometry unused vocabulary; ListenerRegistry; ElementBuildContext; PlatformEmbedder; second `Window` trait; BasicVelocityTracker; `__private` | nothing (dead) |
| per-platform text layout (not adopted) | — the market (Zed#13951) argues against it |
| future CPU renderer written from scratch | vello_cpu or tiny-skia behind `flui-layer::lower` |
| `multiple-versions = "allow"` (deny.toml:103, 67 duplicates) | `warn` plus a skip list with reasons |

Keep and converge on the Linebender vocabulary crates (ui-events, accesskit, kurbo at the edges). Wrap them rather than re-export them from the Stable tier, because they are pre-1.0 (the Bevy glam trap).

---

## 9. Breaking changes to make now, before any crates.io publish

1. A curated facade, a catalog-neutral prelude, no Material in `default` (Cargo.toml:599).
2. Signals on by default and canonical. The graph moves to the realm. `Signal` gets callback-supplied `rx`.
3. The `!Send` flip on the protocol traits and callbacks.
4. Split flui-platform into api and backends. The IME document protocol.
5. `LifecycleContext::capability::<C>()`.
6. The Router ADR. Navigator frozen to push/pop/replace.
7. Delete flui-tree, flui-localizations and flui-hot-reload. Rename flui-testing to flui-test and move it up a tier.
8. The Color model decision, and a single Rect/Axis.
9. Generational `LayerId`/`SemanticsId`, and fix AGENTS.md's stale ID-offset row. `ElementId` is already a generational u64 (flui-foundation/src/id.rs:1163).
10. `#[derive(RenderView)]` and `#[flui::main]`, retiring `impl_render_view!` and the per-OS `run_app_*` functions.
11. Drop the Flutter-branded Stable names.
12. Move internal pins to workspace dependencies. Version the CLI independently.

Each change gets an ADR with `Supersedes` and a `flui migrate` rule, as data shipped by the crate that makes the change.

---

## 10. Evolution H0→H4 without rewrites

- **H0:** changes 1–12 above, plus flui-runtime extraction, flui-protocol, flui-devtools as the server, `flui mcp`, engine-cpu goldens, the damage producer, the module DAG gate, the globals gate, the reach facts and release-check.
  - *Exit proof:* a clean consumer builds Notes with signals, Router and Form, using `flui` and `flui-material` from crates.io. An agent runs the scenario through `flui mcp`, and the same finders run in `flui test`.
- **H1:** the capability seam already exists, so plugins are additive: camera, geo and notification crates outside the repo. Mobile runners are `PlatformHost` impls over the same runtime, with no new frame path. The Android and iOS a11y adapters plug into the same semantics host. A2UI reads the catalog that `#[derive(Catalog)]` already generates. Material and Cupertino move to `packages/` with their own cadence; nothing in core changes. The compositor spike is a `Presenter` behind the existing `Layer::External`.
- **H2:** the threaded raster lane is a mode switch, because the protocol already carries every frame. The IO lane already exists behind `spawn_io`. The layer cache sits on stable identity. Software fallback is engine-cpu, already shipped for goldens. Multi-window uses the shared GPU context, and signals are already realm-shared. Nothing in the paint→layer contract changes, which is why identity and damage belong in H0 and not H2.
- **H3:** the freeze covers what `cargo xtask public-api` has been snapshotting since H0. Tiers are expressed through facade modules (Stable), `sdk`/packages (Evolving) and `unstable` (Experimental). Semver-checks have been running since H0, as advisory.
- **H4:** community crates depend on `flui` plus the conformance kit and get automated badges (xtask score: builds against the current train, harness passes, semantics declared). Separate repos become possible once cadences diverge.

---

## 11. Risks and deliberate non-goals

**Risks**

- **Breaking a lot in H0 stalls features.** Masonry lost most of 2024 to internals work. Mitigation: the WIP limit, and order the list by what freezes first. Facade, prelude, state and `!Send` go first because every example and widget written afterwards depends on them.
- **Parley rasterisation cost is unproven** (ADR-0077 spike). The shaped-run contract isolates it, so the fallback of cosmic-text behind the same contract keeps the DX surface.
- **Subsecond on Windows and Android is unverified.** Hot restart stays as the documented fallback.
- **Phase-typed signal subscribers add graph complexity.** Ship element subscribers first and add render subscribers behind a measured need (the plan's open question) **(hypothesis: needed for animation-rate values)**.
- **CPU and GPU pixel parity is not guaranteed.** GPU-only effects get semantic goldens only. Pin the SIMD level in test mode.
- **Bus factor:** the generated docs and gates are what let a contributor or agent finish work without the author.

**Non-goals:** a custom DSL or markup (Makepad and Slint bet on this; FLUI measures agent success on the typed catalog plus A2UI instead); webview hybrids; MCP Apps as the generative-UI runtime; intra-realm parallel layout; a CSS engine; per-platform native text layout in production; opening the `DrawOp`/`Layer` enums beyond the one `External` variant; separate repos before cadences differ; semver promises on any crate other than `flui` and the packages.
