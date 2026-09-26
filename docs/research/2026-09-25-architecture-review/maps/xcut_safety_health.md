# Codebase map: xcut_safety_health — cross-cutting safety and code health (unsafe, panics, process-global state, locks/Send+Sync, file size, duplication, dependency weight)

_Raw output of the `map:xcut_safety_health` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

Re-measured on main cab06137d (2026-09-25). Several numbers in context.md and the roadmap are stale, and in the good direction. Production `.unwrap()` is 0, down from 1143. It was measured with a script that removes `#[cfg(test)]` items, comment lines and test-named files (C:\Users\vanya\AppData\Local\Temp\claude\D--flui\bbb28042-f972-4a10-8e94-731819e26161\scratchpad\measure.py). No crate-level `allow(clippy::unwrap_used)` is left outside benches/examples (rg). So track A7's unwrap half is already done. The remaining panic debt is the BUG: convention on `expect`/`panic!`, which no lint checks: about 255 `expect("BUG…")` against at most about 255 other literal messages, plus 51 non-literal ones. That second count is an upper bound because some test-support modules survive the filter.

Production unsafe in crates is about 403 syntax sites: 305 blocks, 57 `unsafe fn`, 41 `unsafe impl`. tools/ adds about 87 more, from desktop-mcp and xtask. flui-platform has about 309 of them (macos 148, windows 137, android 23, ios 17, web 12). They cannot run on Linux CI, and Miri cannot reach them. The second concentration is not in the platform crate. It is the layout core: flui-rendering/src/pipeline/owner/subtree_arena.rs has 29 blocks and 7 `unsafe fn` that alias `&mut RenderNode` through raw pointers so layout can re-enter. That one is Miri-covered (`cargo xtask miri` plan, tools/xtask/src/tasks.rs:474-480). Gates: `unsafe_code = "warn"` under -D warnings (Cargo.toml:337) means every site needs an `#[expect]`. That is good. But `undocumented_unsafe_blocks = "allow"` (Cargo.toml:398), and docs/safety-review.md covers only the hot-reload plugin macros, not platform or layout.

Process-global state is the biggest structural gap against principle 3 ("no global state"). The ambient-reach ratchet file (`runtime-contract.toml`) that roadmap.md:224/743 relies on does not exist in the tree (git ls-files), and no xtask checks statics. What remains:
- Stateful process statics: FONT_SYSTEM (flui-painting/src/text_layout/layout.rs:124), the image decode CACHE (flui-widgets/src/image/decode_cache.rs:96), ERROR_VIEW_BUILDER (flui-view/src/view/error.rs:41), TIME_DILATION (flui-scheduler/src/config.rs:43), AssetRegistry::global and the INTERNER (flui-assets).
- Thread-local registries used as realm stand-ins: APP_RUNTIME, referenced about 246 times across flui-app/runner; NAVIGATOR_COMMAND_TARGETS; the interaction LOCAL_LANES; the GlobalKey REGISTRY_STACK; the pending secondary windows.
- About 20 static ID counters.

ADR-0027 declares every UI-side type and callback `!Send + !Sync`. The core traits comply: View, RenderObject and ParentData carry no Send bound. The auxiliary protocol traits were never converted: Listenable, Animation, TickerProvider, CustomPainter, every layout delegate, ScrollPhysics, HitTestTarget, ViewKey, BuildDuringLayoutCell. Some of them sit on the layout path. As a result, ListenerCallback is `Arc<dyn Fn + Send + Sync>`, AnimationController and TextEditingController are `Arc<Mutex<…>>`, the sliver ChildManager registry takes a per-node Mutex, and unsafe `impl Send` workarounds appear. AnimationController's own doc calls this "a recorded, scoped exception until the engine-wide !Send flip lands" (flui-animation/src/controller.rs:177-180).

File size: 49 tracked .rs files are over 2000 raw lines, but none has over 2000 production code lines (m3.py). The size comes from inline test modules and heavy narrative comments: about 175k of 599k raw lines are comments.

Dependencies: 763 packages in Cargo.lock, 67 crate names in more than one version, and deny.toml:103 sets `multiple-versions = "allow"`. One `use flui_platform::traits::PlatformTextInput` in flui-interaction (text_input.rs:27) pulls all of flui-platform (tokio, windows, accesskit, winit) into flui-rendering and everything above it. flui-platform's default `desktop` feature compiles winit on Windows and macOS although no code checks `feature = "desktop"`.

## Responsibilities and boundaries

This area is not one crate. It is the set of invariants every crate should share: the panic policy (docs/PANIC-POLICY.md), unsafe confinement (the platform backends plus a few named islands), realm ownership of all mutable state (ADR-0027, principle 3), no locks in per-node or frame-path state or in public signatures (AGENTS.md "Frame path is synchronous"), and a lean, deduplicated dependency graph (cargo xtask deps). Where it works today: the lints (unwrap_used, unsafe_code warn plus per-site expect, significant_drop_in_scrutinee, unsafe_op_in_unsafe_fn deny) are real merge gates. Where the boundary leaks:
(1) Process-global and thread-local state has no gate, so realm ownership is enforced by review only, and several globals sit in low layers (painting, scheduler, view, assets) that cannot see a realm at all.
(2) Thread-affinity policy is split: the ADR-0027 !Send flip covered the core trees but not the protocol traits in foundation, animation, rendering delegates, interaction and widgets, so Send+Sync bounds keep pushing Arc<Mutex> into widget state and the layout path.
(3) Platform types leak outside flui-platform through the facade (`pub use flui_app::android_activity`, src/lib.rs:157; flui-app/src/lib.rs:116) and through flui-hot-reload's direct `windows` dependency (crates/flui-hot-reload/Cargo.toml:47). That breaks AGENTS.md's rule that every platform type stays inside flui-platform.
(4) The capability *traits* (PlatformTextInput and others) live in the same crate as the backends, so any crate that names a capability links every backend.
What belongs elsewhere: capability traits belong in a thin, backend-free platform-API module or crate. Font, image and asset caches belong in SharedEngineServices / realm resources (ADR-0027 already lists "ImageCache · font service" there, docs/adr/ADR-0027-owner-affine-ui-realms.md:76). Time dilation and the error-view builder belong to realm/binding configuration.

## Key types and contracts

- docs/PANIC-POLICY.md: unwrap never in production; expect("BUG: <invariant>") for invariants; try_ twin for handle-misuse panics. Enforced: clippy::unwrap_used (Cargo.toml:370) under -D warnings; expect_used deliberately off, so the BUG: prefix is review-only
- Cargo.toml [workspace.lints.rust] unsafe_code = "warn" (line 337): every unsafe site needs #[expect(unsafe_code)]; flui-platform opts out per backend module (22 module-level allows), flui-hot-reload 26 sites
- Cargo.toml:398 undocumented_unsafe_blocks = "allow" (reverted after measuring 16+ undocumented sites plus 12 codegen'd unsafe impls in flui-engine OUT_DIR files)
- flui-rendering/src/pipeline/owner/subtree_arena.rs: NodePtr raw alias of &mut RenderNode + LayoutCycleGuard + PhantomData<&'tree mut ()> - the only unsafe island inside the render machine; covered by `cargo xtask miri` (tools/xtask/src/tasks.rs:474-480: rendering pipeline::owner, view owner::global_key, engine surface_lease, cancelling_renderer_new)
- ADR-0027 section 2 (docs/adr/ADR-0027-owner-affine-ui-realms.md:102-116): UiRealm, trees, PipelineCell, views, contexts and UI callbacks are !Send + !Sync; Scene/LayerTree Send by value; UiCommandSender Clone+Send+Sync
- flui_foundation::notifier::ListenerCallback = Arc<dyn Fn() + Send + Sync> (crates/flui-foundation/src/notifier.rs:46); Notifier<Arg> { listeners: Arc<Mutex<HashMap<…>>> } (notifier_generic.rs:42); trait Listenable: Send + Sync (notifier.rs:78)
- flui-view ChildManagerRegistry = Arc<Mutex<HashMap<RenderId, Arc<Mutex<dyn ChildManager>>>>> (crates/flui-view/src/element/child_manager.rs:56)
- pub ElementBuildContext { tree: Arc<RwLock<ElementTree>>, owner: Arc<RwLock<BuildOwner>> } with pub fn tree()/build_owner() returning &Arc<RwLock<…>> (crates/flui-view/src/context/element_build_context.rs:50-53,128-135), exported at crates/flui-view/src/lib.rs:181; production uses BuildCtx<'_> (element_build_context.rs:786)
- static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>> (crates/flui-painting/src/text_layout/layout.rs:124)
- thread_local APP_RUNTIME: RefCell<AppRuntime> (crates/flui-app/src/app/runner/host.rs:46), documented as a seam 'until retired (ADR-0027 follow-up 5)'
- deny.toml:103 multiple-versions = "allow"; cargo xtask deps = cargo-deny + cargo-shear

## Dependencies

Inbound: every crate is subject to these invariants. Outbound gates: cargo xtask lint (clippy), cargo xtask deps (cargo-deny, cargo-shear), cargo xtask miri (4 filters), cargo xtask workspace (layering only, no global-state or unsafe accounting).

Dependency weight, from `cargo tree -p <crate> -e normal --prefix none | sort -u | wc -l` on the Windows host with default features:
- geometry 11, types 12, foundation 20, tree 32 (bon builder macros)
- rendering 112, view 125, widgets 126, engine 137, app 189, facade 191
- flui-platform 56 for the host target, 198 over `--target all`

The jump from tree (32) to rendering (112) is mostly flui-platform, reached through flui-interaction. `cargo tree -p flui-rendering -i flui-platform` shows flui-platform -> flui-interaction -> flui-rendering, and flui-interaction's only use is `use flui_platform::traits::PlatformTextInput` (crates/flui-interaction/src/text_input.rs:27; `rg flui_platform crates/flui-interaction/src` gives 1 hit).

Cargo.lock has 763 packages and 67 names at more than one version (awk over Cargo.lock). The notable ones and their sources:
- windows-sys ×4
- windows 0.61 (enigo, desktop-mcp only) and 0.62 (accesskit_windows, flui-platform, flui-hot-reload)
- objc2 0.5 (accesskit_macos 0.27) and 0.6 (flui-platform's own AppKit binding)
- skrifa 0.40 and 0.44, read-fonts 0.37 and 0.41: both inside cosmic-text 0.19 (cosmic-text directly and via swash)
- syn 2 and 3; thiserror 1 (winit/calloop/ndk) and 2
- x11rb 0.13 and 0.14; hashbrown ×3; getrandom ×3; rand 0.9 and 0.10; bitflags 1 and 2

tokio is a normal dependency of flui-platform, flui-assets, flui-app and flui-cli. Up to four multi-thread runtimes can exist in one process:
- flui-app flui-io and flui-compute (crates/flui-app/src/app/execution.rs:327,343)
- the flui-assets bridge runtime (crates/flui-assets/src/registry/bridge.rs:66)
- the flui-platform BackgroundExecutor runtime (crates/flui-platform/src/executor.rs:42,66)

## Fit with the plan

H0 (beta):
- Track A7 ("1143 unwrap -> allowlist") is already met for unwrap (0 in production). What remains of A7 is unsafe auditing and the BUG: convention, so the roadmap item should be re-scoped rather than scheduled as a burn-down.
- The "AccessKit by default" goal conflicts with dependency weight: flui-platform's `a11y` feature is off by default because it adds about 66 crates on Linux (comment in crates/flui-platform/Cargo.toml features block).
- Deterministic golden and replay tests (E7, G3, G7) and principle 4's "stable IDs" are weakened by process-wide atomic ID counters (GlobalKey, Route, OverlayEntry, FocusNode, PipelineOwner, lanes, layer links), whose values depend on test order within the process. This is a hypothesis: I did not check which of these reach semantic snapshots.

H1:
- PlatformCapability plugins built outside the repo need a thin, backend-free capability-trait crate. Today any crate that names a capability trait links every backend, and platform types (android_activity) are re-exported from the facade.
- A2UI and agent determinism would benefit from the same realm-ownership ratchet.

H2 (perf, multi-window as the norm):
- Blocked structurally by the global FONT_SYSTEM mutex, which serializes shaping across realms and workers even though ADR-0027:121 lists font shaping as parallel work.
- Also blocked by the thread_local APP_RUNTIME host and the thread-local navigator, lane and GlobalKey registries. ADR-0027:60 itself says thread_local "could not express two realms on one thread", which is the AppKit case.
- Per-node Mutexes on the layout path (ChildManager, LayoutConstraintsCell, HeaderShrinkCell) are the contention the AGENTS.md frame-path rule forbids.

H3 (API freeze):
- Send+Sync supertraits on Listenable, Animation, CustomPainter, the delegates and ScrollPhysics are public contract. Removing a supertrait later is a breaking change for every implementor and caller, so the flip must happen before the freeze.
- A lock-typed public API (ElementBuildContext) and duplicate public types (flui_types::physics::*, flui_types::BoxConstraints) would be frozen too.

H4 / ecosystem:
- Third-party render-object catalogs will copy these patterns: Send+Sync delegates force Arc<Mutex>.
- `multiple-versions = allow` plus a 191-crate facade raises build time and binary size, which hurts adoption.

Delivery layers: the plan wants Material and Cupertino in official packages, yet the facade default is `material` (Cargo.toml:598). Outside this area's core, but it adds to default dependency weight.

## Strengths

- Panic policy works: 0 production `.unwrap()` across all crates (measure.py strips #[cfg(test)] items, comments and test files); no crate-level allow(clippy::unwrap_used) outside benches/examples; `flui_foundation::panic::is_internal_invariant`/`payload_text` centralise panic inspection
- unsafe is opt-in per site: unsafe_code = warn + -D warnings forces #[expect(unsafe_code)] per site/module; unsafe_op_in_unsafe_fn = deny; 17 of 27 crates contain zero unsafe (geometry, tree, objects, semantics, layer, interaction, widgets, material, cupertino, scheduler, animation, assets, painting (forbid), testing, macros, localizations, devtools)
- The one unsafe island in the render core (subtree_arena.rs) is file-scoped, documented with its aliasing invariant, exports only safe fns, and runs under Miri in CI (tasks.rs:474-480)
- Core trees follow ADR-0027: View (flui-view/src/view/view.rs:56), RenderObject (flui-rendering/src/traits/render_object.rs:178), ParentData carry no Send/Sync; PipelineCell is Rc<RefCell<PipelineOwner>> (flui-rendering/src/pipeline/owner/cell.rs:51); negative bounds pinned by compile-fail tests
- Zero `static mut` anywhere; nearly all remaining statics are atomics/Once; ID offset discipline via NonZeroUsize newtypes
- No file exceeds 2000 lines of production code (max 1784, tools/desktop-mcp/src/a11y/uia.rs; max in crates 1698, flui-view/src/tree/element_tree.rs), so the '46 files >2000' metric is mostly inline tests plus doc comments, not god-objects
- Platform types are mostly confined: outside flui-platform only flui-app/runner (android, web) and flui-hot-reload touch platform crates
- Lints encode rules (significant_drop_in_scrutinee, dbg/todo/unimplemented, unexpected_cfgs deny, unused_must_use deny) instead of review prose

## Problems

### The realm-ownership principle has no gate; its ratchet file is gone

- **Kind:** safety · **Severity:** high
- **Evidence:** roadmap.md:224 and :743 cite an ambient-reach ratchet in `runtime-contract.toml`. `git ls-files | grep -i contract` shows no such file, and `rg -i 'ambient|thread_local|OnceLock' tools/xtask/src` shows no static/thread_local check. Current inventory (scratchpad statics.txt):
- Stateful process statics: FONT_SYSTEM (flui-painting/src/text_layout/layout.rs:124), decode CACHE (flui-widgets/src/image/decode_cache.rs:96), ERROR_VIEW_BUILDER RwLock (flui-view/src/view/error.rs:41), TIME_DILATION (flui-scheduler/src/config.rs:43), AssetRegistry::global and INTERNER (flui-assets/src/registry/mod.rs:84, types/key.rs), hot-reload REQUEST_REBUILD and WORKER_BUILDS.
- thread_local registries: APP_RUNTIME (flui-app/src/app/runner/host.rs:46), PENDING_SECONDARY_WINDOW_* (secondary_window.rs), NAVIGATOR_COMMAND_TARGETS (flui-widgets/src/navigator/navigator.rs:90-91), LOCAL_LANES and ACTIVE_LANES (flui-interaction/src/routing/interaction_lane.rs:738-739), REGISTRY_STACK (flui-view/src/key/registry.rs).
- About 20 static AtomicU64/AtomicUsize ID counters.
- **Impact:** Principle 3 and ADR-0027 are enforced only by review, so any PR can add a global. This blocks H2 'multi-window as the norm' and multi-realm on one thread (the AppKit main thread): ADR-0027:60 itself says thread_local cannot express two realms on one thread. It also makes deterministic tests (E7, G3, G7) order-sensitive.
- **Direction:** Add `cargo xtask globals` as part of `checks`, backed by a checked-in allowlist (crate, item, reason, owner horizon). Pure ID counters and Once warnings are allowlisted. Any new static holding Mutex, RefCell, Lazy or OnceLock, or any thread_local!, fails the check. Then burn the list down by moving each item into SharedEngineServices or a UiRealm resource. Consider a clippy `disallowed_types`/`disallowed_macros` config for thread_local! outside flui-platform and flui-app/runner.

### The ADR-0027 !Send flip stopped at the core trees; protocol traits still force Send+Sync, and with it Arc<Mutex> and unsafe impls

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** ADR-0027:106 says UI callbacks are !Send + !Sync, and ADR-0027:55-56 says the bounds were 'forced by storage, not by use'. Yet these traits are still `pub trait …: Send + Sync`: Listenable (flui-foundation/src/notifier.rs:78), Animation<T> (flui-animation/src/animation.rs:68), TickerProvider (flui-scheduler/src/ticker.rs:94), CustomPainter, FlowDelegate, SingleChildLayoutDelegate, MultiChildLayoutDelegate, SliverGridDelegate (flui-rendering/src/delegates/*), ScrollPhysics (flui-widgets/src/scroll/scroll_physics.rs:167), HitTestTarget (flui-interaction/src/traits.rs:33), ViewKey (flui-foundation/src/key.rs:364), BuildDuringLayoutCell (flui-objects/src/layout/layout_constraints_cell.rs:64), Notification. ListenerCallback = Arc<dyn Fn()+Send+Sync> (notifier.rs:46). Consequences:
- AnimationController { inner: Arc<Mutex<…>> } (flui-animation/src/controller.rs:208), with the doc at 177-180 calling it 'a recorded, scoped exception until the engine-wide !Send flip lands'.
- TextEditingController Arc<Mutex<ControllerInner>> (flui-widgets/src/text/controller.rs:263).
- `unsafe impl Send/Sync for ObjectKey` only to satisfy ViewKey (flui-view/src/key/object_key.rs:47-50).
- A test-only `unsafe impl Send for BindingPtr` because add_semantics_enabled_listener demands Send+Sync on a !Send binding (flui-app/src/bindings/renderer_binding.rs:920-968).
Measured in production code: 398 Mutex/RwLock< and 1527 Arc< occurrences. Widgets has 93/267, view 38/85, interaction 36/249.
- **Impact:** Every frame pays for locks and atomic refcounts. Users implementing painters, delegates or physics must make them Send+Sync, which is H3 public contract: removing a supertrait after the freeze breaks every implementor. It also makes signals (ADR-0074, realm-scoped and !Send) and the Listenable world two incompatible reactivity models.
- **Direction:** Finish the flip before H3 in one breaking change, recorded by an ADR that supersedes the ADR-0027 exception. Drop Send+Sync from UI-side traits. Make ListenerCallback Rc<dyn Fn()>, and use Rc<RefCell>/Cell for controller internals. Keep Send only on the types ADR-0027 section 2 lists (Scene, WorkerJob, UiCommandSender). Pin the change with assert_not_impl_any! tests, and make ObjectKey hold a usize address so it needs no unsafe.

### Per-node locks on the build/layout path

- **Kind:** performance · **Severity:** medium
- **Evidence:** ChildManagerRegistry = Arc<Mutex<HashMap<RenderId, Arc<Mutex<dyn ChildManager>>>>> (flui-view/src/element/child_manager.rs:56); build_owner.rs:2364 collects Arc<Mutex<dyn ChildManager>>. LayoutConstraintsCell and HeaderShrinkCell each hold a parking_lot::Mutex<CellState> used during perform_layout (flui-objects/src/layout/layout_constraints_cell.rs:96, header_shrink_cell.rs:55). subtree_arena.rs imports parking_lot::Mutex (line 53). BuildOwner's rebuild inbox is Arc<Mutex<HashMap>> (flui-view/src/owner/build_owner.rs:83,495).
- **Impact:** This goes against the AGENTS.md rule that no lock guards per-node state touched inside perform_layout/paint. Today uncontended locks are cheap, but they make H2 parallel layout inside a realm (the plan's open question) a deadlock-risk refactor, and they are the reason those traits need Send+Sync.
- **Direction:** Once the !Send flip lands, replace them with RefCell/Cell owned by the realm. The only cross-thread entry point should be the owner inbox through UiCommandSender. A clippy `disallowed_types` rule (parking_lot::Mutex, std::sync::Mutex in flui-objects and flui-rendering outside a named allowlist) would pin it.

### The global FONT_SYSTEM mutex serializes text across realms and contradicts ADR-0027's parallel shaping

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>` (flui-painting/src/text_layout/layout.rs:124). It is reached from painting (font_resolve.rs 16 sites, layout.rs 8, text_painter/measure.rs), engine (glyph_atlas.rs, painter/mod.rs) and app (runtime.rs). ADR-0027:76 puts 'font service' in shared engine services and ADR-0027:121 lists 'font loading and shaping' as parallel work. flui-painting is layer 'substrate' and cannot see a realm or engine services.
- **Impact:** Blocks H2 (multi-window performance, a 1 MB editor) and principle 3. Tests that register fonts race each other: AGENTS.md itself names FONT_SYSTEM as a source of flakes that needs module locks. The Parley migration (ADR-0077) is the natural moment to fix it; if it is not fixed then, the global carries over into the new stack.
- **Direction:** Make the ADR-0077 spike's exit criterion include 'no process-global font state'. The font collection becomes a SharedEngineServices resource, passed into painting as an explicit `&mut FontContext`/handle, with a per-realm layout context and a shared, immutable-after-load font database. Put FONT_SYSTEM on the globals allowlist with an H0 deadline.

### flui-interaction's one use of a platform trait drags every platform backend into the render machine and everything above it

- **Kind:** layering · **Severity:** high
- **Evidence:** crates/flui-interaction/Cargo.toml:31 depends on flui-platform only for `use flui_platform::traits::PlatformTextInput` (src/text_input.rs:27; `rg flui_platform crates/flui-interaction/src` gives 1 hit). `cargo tree -p flui-rendering -e normal -i flui-platform` shows flui-platform -> flui-interaction -> flui-rendering. flui-platform brings tokio, windows 0.62, winit, accesskit and ui-events (`cargo tree -p flui-platform --depth 1`): 56 crates for the host target, 198 across targets. flui-rendering's dependency count jumps to 112 from flui-tree's 32.
- **Impact:** The headless render machine, flui-objects and flui-testing compile OS backends and a tokio runtime, which hurts test build time on the memory-limited host and CI. It blocks the H1 PlatformCapability model, where out-of-repo plugins should depend on a small capability API rather than on every backend. It also means the layer check passes even though the substrate is not substrate-weight.
- **Direction:** Split out a backend-free `flui-platform` API surface (traits, capability types and events, no OS dependencies) as the lower-layer crate or module that flui-interaction and plugins depend on. Backends stay in flui-platform and are wired only by flui-app. Record it in ADR-0041 and let `cargo xtask workspace` forbid rendering, objects and view from depending on backend crates transitively.

### Unsafe accounting: about 309 platform sites with no audit document, and no coverage on CI

- **Kind:** safety · **Severity:** medium
- **Evidence:** rg counts of `unsafe {`/`unsafe fn`/`unsafe impl`/`unsafe extern`, production only: macos 148, windows 137, android 23, ios 17, web 12, winit 2. Platform has 295 SAFETY comments for 384 'unsafe' tokens. Cargo.toml:398 has undocumented_unsafe_blocks = allow, and its comment lists 16+ undocumented sites. docs/safety-review.md (185 lines) covers only crates/flui-hot-reload/src/plugin.rs. AGENTS.md says Win32, AppKit, Android and iOS are clippy-only on CI, and Miri cannot execute FFI.
- **Impact:** The plan's 'unsafe behind audit and miri' (plan trajectory row 'Platforms') and H3 'security + unsafe audit' are not progressing in a measurable way. The largest unsafe body is exactly the code that CI never runs.
- **Direction:** Keep a per-backend unsafe ledger (a generated count per module in `cargo xtask checks`, ratcheting down) and turn on undocumented_unsafe_blocks per backend module via `#![warn]` once each is annotated. Prefer safe wrapper crates (windows-rs safe APIs, objc2 generated safe methods) where they exist. Require a live-run proof (the device-checks / live-smoke tooling) for any PR that touches unsafe in an unexecuted backend.

### Public API types still expose locks: ElementBuildContext takes and returns Arc<RwLock<ElementTree/BuildOwner>>

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** crates/flui-view/src/context/element_build_context.rs:50-53 has the fields; 128-135 has `pub fn tree(&self) -> &Arc<RwLock<ElementTree>>` and `pub fn build_owner(&self) -> &Arc<RwLock<BuildOwner>>`; 1071-1116 has a pub builder returning (Self, Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>). It is re-exported at flui-view/src/lib.rs:181. Production builds through `BuildCtx<'_>` (element_build_context.rs:786) with &mut ElementTree ownership (flui-view/src/binding.rs:519-522). Apart from its own module and tests, the only callers are unit tests in element_tree.rs:5408,5463. `test_only_set_global_key_registry` (lib.rs:128-131) is also pub with Arc<RwLock> params.
- **Impact:** This goes against the AGENTS.md rule 'a lock in a public signature makes callers part of the locking protocol'. It is unwired public surface that would be frozen at H3 and confuses third-party widget authors about which context is real.
- **Direction:** Make ElementBuildContext and its builder pub(crate), or move them behind flui-view's `testing` feature. Keep one public BuildContext implementation (BuildCtx). Add a check that no `pub fn` in flui-view, rendering or widgets names Mutex/RwLock (a disallowed_types clippy config on the public API, or an xtask grep as a stopgap).

### Globals hidden behind instance-looking APIs: time dilation, error view builder, image decode cache, asset registry

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** `UpdateScheduler::set_time_dilation(&self, …)` writes the process static (flui-scheduler/src/scheduler.rs:3270-3271 -> config.rs:43), and AnimationController reads it (flui-animation/src/controller.rs:1803,2419). `set_error_view_builder` writes the static ERROR_VIEW_BUILDER (flui-view/src/view/error.rs:41-55). AssetImage uses the static CACHE (flui-widgets/src/image/decode_cache.rs:96,103,133) even though ADR-0027:76 puts ImageCache in shared engine services. `AssetRegistry::global()` exists (flui-assets/src/registry/mod.rs:83), used only by examples and tests.
- **Impact:** Two windows or realms cannot differ (for example slow-motion in one devtools-inspected window). Tests leak state into each other. The API lies about scope, which matters for agents and the MCP protocol, which read behavior as per-realm.
- **Direction:** Move time dilation into realm and FrameClock configuration, error-view builder into the binding or theme, and the decode cache into SharedEngineServices with an explicit budget. Delete AssetRegistry::global. Each move is small, and together they empty most of the allowlist.

### Dead duplicate types in flui-types: physics simulations and BoxConstraints

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** Tolerance, SpringSimulation, SpringDescription, FrictionSimulation, BoundedFrictionSimulation, GravitySimulation and ClampedSimulation are defined in both flui-types/src/physics/* (1797 lines) and flui-animation/src/simulation.rs (Tolerance :31, SpringSimulation :329, FrictionSimulation :611). BoxConstraints is defined in both flui-types/src/layout/constraints.rs:37 (393 lines, re-exported layout/mod.rs:22) and flui-rendering/src/constraints/box_constraints.rs:41. `rg 'physics::|flui_types::.*BoxConstraints'` outside flui-types finds no production consumer. Widgets use flui_animation::simulation (flui-widgets/src/scroll/page_view.rs:45). The rendering doc example still names `flui_types::BoxConstraints` in an ```ignore block (flui-rendering/src/delegates/flow_delegate.rs:24).
- **Impact:** Two public types with the same name and different semantics would both be frozen at H3. Users and agents will import the wrong one, and the ignored doctest hides the mismatch.
- **Direction:** Delete flui_types::physics and flui_types::layout::BoxConstraints, or keep exactly one owner per concept at the lowest layer that needs it. Add an xtask check that fails when the same pub type name is defined in two framework crates, with an allowlist for deliberate cases.

### Dependency hygiene: 67 duplicated crates allowed, a default feature compiles unused winit, and the facade re-exports platform types

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** Cargo.lock: 763 packages, 67 names at more than one version (awk). deny.toml:103 has `multiple-versions = "allow"`. flui-platform `default = ["desktop"]` with `desktop = ["dep:winit"]`, but `rg 'feature = "desktop"' crates/flui-platform/src` gives 0 hits, and winit code is gated on `winit-backend` (traits/window.rs:110), so Windows and macOS default builds compile winit for nothing. The `web`, `wayland` and `x11` features are admitted no-ops or near no-ops (Cargo.toml comment 'no-op selector'). cosmic-text 0.19 alone brings two skrifa/read-fonts/font-types versions. objc2 0.5 (accesskit_macos 0.27) and 0.6 (own AppKit) both sit in the macOS graph. flui-hot-reload depends on `windows` directly (crates/flui-hot-reload/Cargo.toml:47). The facade re-exports `android_activity` (src/lib.rs:157).
- **Impact:** Build time, binary size and the H1 '16 KB .so' and store-size goals all suffer. The platform-type re-export becomes a semver hazard at H3: an android-activity major bump becomes a FLUI major bump.
- **Direction:** Set `multiple-versions = "warn"` with an explicit `skip` list that has reasons, and ratchet it. Remove winit from `desktop`, and delete the no-op features. Wrap android_activity::AndroidApp in a FLUI newtype in flui-platform. Route hot-reload's dlopen through flui-platform or libloading. Revisit the duplicates after the Parley migration.

### Several independent tokio runtimes in one process, with no single async owner

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** flui-app builds flui-io and flui-compute multi-thread runtimes (crates/flui-app/src/app/execution.rs:327,343). flui-assets' BridgeRuntime builds its own single-worker multi-thread runtime unless a Handle is ambient (crates/flui-assets/src/registry/bridge.rs:55-75). flui-platform's BackgroundExecutor lazily builds another one (crates/flui-platform/src/executor.rs:42,60-72). tokio is a normal dependency of platform, assets, app and cli. The plan's trajectory row says 'tokio without an explicit role -> explicit async model, tokio optional'.
- **Impact:** This undermines ADR-0047 (unified execution services), H2 'memory and startup within budget' (every runtime spawns threads), and a 'tokio optional' goal for wasm and embedded (H4).
- **Direction:** Keep one execution-services owner in flui-app. flui-platform and flui-assets receive a spawner or handle trait and never build runtimes. Put tokio behind a feature in the lower crates so that lower layers compile without it.

### The BUG: invariant convention is half-followed and not machine-checked

- **Kind:** safety · **Severity:** low
- **Evidence:** Multiline-aware scan of production code (scratchpad m2.py): 255 `expect("BUG…")`, up to 255 other literal messages and 51 non-literal ones. panic!: 11 BUG-prefixed against 50 others. The largest non-BUG counts are flui-app 73, widgets 48, rendering 42 and engine 22. Some flui-app hits are test-support modules (raster_test_support.rs, window_test_support.rs) that survive the filter, so the number is an upper bound. PANIC-POLICY.md makes the prefix a review bar only, and `is_internal_invariant` relies on it.
- **Impact:** Panic triage for users and agents (the BUG: prefix is how a caught panic is classified as a framework fault) is unreliable, and the devtools/MCP diagnostics that classify panics inherit the noise.
- **Direction:** Add a cheap xtask lint (syn-based: expect/panic! whose first literal does not start with 'BUG:' or name a try_ twin, outside cfg(test)), with a per-crate ratchet. Mark test-support modules #[cfg(any(test, feature = "testing"))] so they drop out.

### The file-size metric is misleading; the real smell is inline test modules and narrative comments inside production files

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** 49 tracked .rs files are over 2000 raw lines (wc), but 0 files have over 2000 production code lines, and 20 have over 1000 (scratchpad m3.py). Examples: flui-scheduler/src/scheduler.rs is 5650 raw lines, with 2545 comment lines and tests from line 3622; flui-view/src/owner/build_owner.rs is 6043 raw lines with 1520 code lines; flui-engine/src/renderer.rs is 5009 raw lines with tests from line 165. Workspace-wide: 599k raw lines, 175k comment lines, 240k production code lines. Comments often narrate history ('not the old bare Option', realm_dispatch.rs:32-33; 'Unstated-until-now invariant', renderer_binding.rs:937), which AGENTS.md discourages.
- **Impact:** Low runtime risk, but it slows review and agents: context windows fill with test code and history. The '46 files >2000' roadmap number drives the wrong refactor, which would be splitting production modules that are not actually too big.
- **Direction:** Measure production code size, not raw lines. Move large inline `mod tests` into sibling `*_tests.rs` or tests/ files (already the pattern in ui_realm/). Treat historical narration in doc comments as a review finding (state the invariant, not the history).

### Hand-rolled SIMD and other micro-optimisations with unsafe in low layers

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** flui-types/src/styling/color.rs:300-360 has an SSE2 `lerp_simd_sse` for a 4-channel u8 lerp behind the `simd` feature, with 4 unsafe blocks and #[expect(dead_code)] when the feature is off. flui-foundation/src/id.rs:148,314,365 has `pub const unsafe fn zip_unchecked/new_unchecked`; its own comment at id.rs:143 says no call site uses them yet.
- **Impact:** Unsafe surface with no measured benefit, and a public unsafe API in the lowest layer that becomes semver-frozen at H3.
- **Direction:** Delete the SIMD twin unless a bench shows a win. Make the unchecked ID constructors pub(crate), or remove them until a measured call site exists.

## Unwired or dead surface

- flui_types::physics::* (Tolerance, SpringSimulation, SpringDescription, FrictionSimulation, BoundedFrictionSimulation, GravitySimulation, ClampedSimulation; 1797 lines): no consumer outside flui-types, duplicated in flui-animation/src/simulation.rs
- flui_types::layout::BoxConstraints (flui-types/src/layout/constraints.rs:37): duplicate of flui_rendering::constraints::BoxConstraints, no production consumer
- flui_view::ElementBuildContext / ElementBuildContextBuilder (Arc<RwLock> based, flui-view/src/context/element_build_context.rs:50,1071): pub-exported, production uses BuildCtx<'_>
- flui_view::test_only_set_global_key_registry (flui-view/src/lib.rs:128): pub #[doc(hidden)] test hook in a production crate
- flui_assets::AssetRegistry::global() (flui-assets/src/registry/mod.rs:83): only examples/tests call it
- flui_foundation id `pub const unsafe fn zip_unchecked/new_unchecked` (flui-foundation/src/id.rs:148,314,365): the file's own comment says no call site yet
- flui-platform features `desktop` (enables winit but gates no code), `web` (documented no-op), `wayland`/`x11` (near no-op; only linux/window_ext.rs refers to wayland)
- flui-types `simd` color lerp (color.rs:300+), dead_code-expected when the feature is off
- 61 #[allow/expect(dead_code)] in non-test code (view 16, platform 7, app 7, widgets 6, interaction 6, rendering 5, engine 5), each a candidate unwired item
- Roadmap/plan references to `runtime-contract.toml` ambient-reach ratchet (roadmap.md:224,743): the file and its check no longer exist

## Open questions

- Which of the process-wide atomic ID counters (GlobalKey COUNTER, Route/OverlayEntry COUNTER, NEXT_FOCUS_NODE_ID, PIPELINE_ID_COUNTER, layer link NEXT_ID) reach semantic snapshots, the devtools/MCP protocol, or record/replay payloads? If any do, G3/G7 determinism needs realm-scoped IDs. Not verified.
- Was the ambient-reach ratchet (`runtime-contract.toml`) removed on purpose or lost in a refactor? The roadmap still cites it as the mechanism for FONT_SYSTEM removal. `git log --all -- '*runtime-contract*'` would answer this; I did not run history queries.
- Is the engine-wide !Send flip (the AnimationController doc's 'until the engine-wide !Send flip lands') tracked as an issue or ADR follow-up with a horizon? It must precede H3, because removing supertraits is breaking.
- Can the flui-platform API/backends split be a module-level feature gate (backends behind features, traits always on) instead of a new crate, given the 'layers, not micro-crates' decision? A trait-only crate is arguably a legitimate lower layer.
- Should Miri coverage extend to flui-view key/registry and to the flui-hot-reload scene ownership paths, and can platform FFI get at least a sanitizer (ASan/TSan) run on the Windows self-hosted host?
- Is tokio meant to remain a hard dependency of flui-platform and flui-assets (wasm targets exclude it by cfg), or should execution services be the only runtime owner per ADR-0047?

