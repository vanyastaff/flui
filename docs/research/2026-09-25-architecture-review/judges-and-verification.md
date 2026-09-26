# judge_engineer

```json
{
  "scores": [
    {
      "design": "safety_correctness_first",
      "score": 8.2,
      "strongest": "The most sound design to implement. Its capability seam is object-safe: a hidden `capability_erased(TypeId)` on the sealed trait plus a `LifecycleContextExt`. That matters because `LifecycleContext` is used as `&dyn` at 122 sites and `BuildContext` is sealed (crates/flui-view/src/context/build_context.rs:106,377). It says outright that the root-scope widgets flui-app imports have to move down if the runtime sits below widgets (crates/flui-app/src/app/ui_realm/attach.rs:6 imports FocusRoot, GestureArenaScope and VsyncScope). Its 'one implementation per contract' principle lines up with the duplicated `HeadlessBinding::pump_frame` (crates/flui-testing/src/lib.rs:955). The gate table has a 'today' column. It also proposes a `Writer` token so ADR-0074's run-time guard becomes a compile-time type.",
      "weakest": "Five new crates, and the arithmetic is muddled ('four are added' followed by a list of five). Re-packing ElementId into GenId buys little: it is already a generational NonZeroU64 (crates/flui-foundation/src/id.rs:1163). Deleting HeadlessRenderer in favour of 'wgpu to a caller texture' makes headless CI depend on a GPU or lavapipe until the CPU backend lands. Putting runtime at T4, below widgets, costs an extra migration step that the ai_native and ecosystem placements avoid."
    },
    {
      "design": "ecosystem_evolution_first",
      "score": 7.8,
      "strongest": "The best long-horizon shape. Tier metadata (stable, internal, official, tool) sits next to `layer` and is checked by xtask. Only three crates carry a semver promise: flui, platform-api and protocol. P9 says a seam counts only once a second implementation passes its conformance kit. The runtime sits in the spine tier and the root-scope binding views move into flui-view, which matches the real app→widgets coupling. It spot-checked the claims it leans on (text_input.rs:27, Cargo.toml:598 default=[\"material\"], platform default=[\"desktop\"]). The H0-H4 table names which seams each horizon needs.",
      "weakest": "`fn capability<C>` is a generic method declared directly on `LifecycleContext`, which breaks object safety for the 122 `&dyn LifecycleContext` users. The DX sample calls `cx.signal(0usize)` inside `StatelessView::build`, which breaks the enforced 'signals never created in build' rule (ADR-0074). The facade default keeps `signals` as a feature even though the same design says signals should be default-on and non-optional. The crate count stays at about 24, so maintenance load barely drops."
    },
    {
      "design": "performance_first",
      "score": 7.6,
      "strongest": "The strongest cost model. Every O(tree) step inside a frame is tied to a file:line and a fix: paint.rs mints fresh layer ids, raster_lane.rs:354 always sends Full damage, there is a global topology sync, a whole-slab scan and deep dyn_clone of view configs. Retained layer identity is argued as the one contract change that must land before the freeze. Deterministic work counters gate each PR. flui-text is a real reach seam: cosmic-text is currently reachable from 17 crates through painting. The GPU-free engine modules it wants to extract do have 0 wgpu references (verified: layer_walk, layer_render, dispatch, command_renderer and damage have none; layer_state_stack mentions wgpu only in comments).",
      "weakest": "flui-protocol is placed under packages/, yet core flui-testing is supposed to use its query and golden model, so the layering contradicts itself. The capability method is again a generic method on a dyn-used trait. flui-devtools is folded into the protocol package, which mixes wire types with a server. Five new crates plus four deletions add up to a heavy H0 for a bus-factor-1 project."
    },
    {
      "design": "ai_native_first",
      "score": 7.3,
      "strongest": "It makes the machine-readable contract a real low layer: flui-protocol at T1, with serde and accesskit only. That frees it from the `publish = false` bin-only desktop-mcp (tools/desktop-mcp/Cargo.toml:11,14). `#[derive(Catalog)]` doubles as a stable element identity to replace TypeId. The runtime is placed correctly, above view and widgets, so there is no root-scope migration. The capability API uses `impl dyn LifecycleContext`, which is object-safe. It treats MCP as a projection of the FLUI wire protocol rather than the base, which suits MCP's churn.",
      "weakest": "It keeps flui-tree and wants ElementTree to implement a trait trio that has no generic consumer, which is speculative work. Much of the plan hangs on the protocol and catalog (a derive over roughly 1.3k items, egui_inspection co-design), all marked hypothesis. It pushes the protocol freeze and inspector machinery into H0, where they compete with the user loop. The fate table is thorough, but the performance and platform sections mostly repeat the other designs."
    },
    {
      "design": "dx_first",
      "score": 7,
      "strongest": "It measures cost by the path from `flui create` to a working app. The evidence is concrete: run_app requires `StatelessView + Clone` (verified at crates/flui-app/src/app/runner/mod.rs:216), the clipboard accessor is dead (verified at runtime.rs:1632-1638 as expect(dead_code)), and the prelude leaks internals. It replaces the `runtime-internals` feature with `#[doc(hidden)] __runtime`; feature unification turns that feature on everywhere, as flui-app/Cargo.toml:90 shows. It has good gates (reach facts, globals, module DAG, release-check), and its test finders share the agent query model.",
      "weakest": "flui-runtime sits at T3, below widgets at T4, yet UiRealm imports FocusRoot, VsyncScope, MediaQuery and NavigatorCommand from flui-widgets. Only NavigatorCommand is addressed, so the extraction as drawn does not compile without an unstated move. `capability::<C>()` is a generic method on the dyn-used sealed trait. Removing the Flutter-branded names and adding many derives (Route, Store, Catalog, RenderView, flui::main) all in H0 widens the scope."
    },
    {
      "design": "minimalist",
      "score": 6,
      "strongest": "It is honest about maintenance cost: P8 says the crate count is a cost, and P9 deletes unwired pub items. Several merges check out against the code. flui-log's allowed-dependents are only app, cli and facade, and flui-cli does not use it (0 references). flui-semantics is already re-exported as `flui_rendering::semantics` (flui-rendering/src/lib.rs:80). flui-animation depends only on types, foundation, scheduler and macros. Leaving a single semver-checked surface (the facade) is the cheapest route to H3.",
      "weakest": "It puts the realm and frame transaction inside flui-view, below widgets, without dealing with the root-scope widgets UiRealm needs (attach.rs:6, media_query_root.rs:9, commands.rs:8). It folds every OS backend into flui-app, making one crate of about 90k lines with unsafe code and runtime wiring mixed together. The flagship sample creates a signal in `build` (`cx.signal(0)`), which breaks ADR-0074. It deletes StateCell and StateHandle outright, against the already-taken decision. It drops flui-assets from core without a replacement image or IO seam for H2. Merging animation into scheduler widens scheduler's rebuild fan-out."
    }
  ],
  "winner": "safety_correctness_first. It turns principles into types, lints and manifest facts. Its extension seam is the only object-safe one other than ai_native's, and it states the migration steps it depends on. Graft onto it: ecosystem's tier metadata and three-crate semver promise, performance's work counters, flui-text and retained-identity sequencing, and ai_native's placement of the runtime above widgets.",
  "best_crate_topology": "The best base is ecosystem_evolution_first's tiers: V values, C contracts, S substrate, R render, K spine+runtime, H hosts. Each has a reach fact, and tier metadata (stable, internal, official, tool) is checked by `cargo xtask workspace`. Changes to make: (1) Place flui-runtime explicitly above flui-widgets in the spine tier, following the ai_native order view → widgets → runtime → testing → platform → app. Moving FocusRoot, GestureArenaScope, VsyncScope and MediaQuery down into flui-view first is the costlier alternative, since flui-app/src/app/ui_realm/attach.rs:6 and media_query_root.rs:9 import them today. (2) Put the reactive graph core in a small data-structure-only module of flui-foundation or a tiny flui-reactive crate, so rendering and animation can hold phase-typed subscribers. (3) Merge flui-tree's Arity, Slot and Depth into foundation or rendering and delete the trait trio. Delete flui-localizations. Fold flui-log into flui-app, which the minimalist design correctly shows is safe. (4) Add flui-text only if the Parley spike keeps shaping separable from painting; otherwise keep it as a painting module. (5) Keep flui-protocol as a core contract crate so flui-testing can depend on it, not as a package (fixes performance_first's inconsistency). (6) Have the scene contract live in flui-layer, taking the engine's GPU-free modules (0 wgpu references, verified), plus a flui-engine-cpu peer. (7) Keep all OS backends in a separate flui-platform crate at the H tier, not inside flui-app.",
  "ideas_to_graft": [
    "Split flui-platform-api from flui-platform, with a `forbid-reach` manifest fact checked against `cargo tree -e normal`. All six designs agree on this. The only edge is `use flui_platform::traits::PlatformTextInput` at crates/flui-interaction/src/text_input.rs:27, plus a dev/testing-feature use in widgets.",
    "An object-safe capability seam: the sealed `capability_erased(TypeId)` plus a `LifecycleContextExt::capability::<C>()` (safety design), or an `impl dyn LifecycleContext` inherent method (ai_native). Never a generic method on the trait itself, because `&dyn LifecycleContext` appears 122 times.",
    "Tier metadata (stable, internal, official, tool). Official packages may depend on `flui` only. Semver-checks apply only to flui, platform-api and protocol (ecosystem).",
    "P9: an extension seam counts as existing only when a second implementation passes its conformance kit (wgpu + CPU raster; clipboard + haptics + dialogs; flui-objects + an out-of-tree RenderSliver fixture).",
    "One frame transaction in flui-runtime, used by both flui-app and flui-testing. This retires HeadlessBinding::pump_frame (flui-testing/src/lib.rs:955) and supersedes ADR-0041's 'two entry points' gate.",
    "Retained per-boundary layer identity plus a damage diff in flui-layer, presented through a retained target and a blit. Land it before the render protocol freezes, because it changes what paint produces (performance).",
    "Deterministic per-PR work counters (elements built, layout passes, damage area, allocations) on the virtual clock, with wall time kept as a nightly trend. Also fix bench-collect skipping required-features benches.",
    "Replace the `runtime-internals` Cargo feature with `#[doc(hidden)] pub mod __runtime`. Feature unification enables it in every app (crates/flui-app/Cargo.toml:90).",
    "`cargo xtask globals` allowlist ratchet. runtime-contract.toml is absent from git (verified: git ls-files finds 0 matches).",
    "Make the reactive graph realm-owned and move it below rendering, with phase-typed subscribers (element rebuild, needs_layout, needs_paint).",
    "Pull-model IME text-store contract in platform-api, with TSF on Windows.",
    "A `Writer` or `rx` token passed only to callbacks and effects, which turns the no-writes-in-build guard into a type.",
    "Fold flui-log into flui-app; the minimalist design shows this is safe (allowed-dependents are only app, cli and facade, and cli has 0 uses).",
    "`#[derive(Catalog)]` as the single source for G6, the A2UI catalog, llms.txt and a stable element type name (ai_native).",
    "Put the internal version pins in `[workspace.dependencies]`, publish with `cargo publish --workspace`, and gate with `xtask release-check`."
  ],
  "ideas_to_reject": [
    "A generic `fn capability<C>()` declared directly on `LifecycleContext` (dx_first, performance_first, minimalist, ecosystem). It breaks object safety for the dyn-used trait.",
    "Creating signals inside `build` in the DX samples (minimalist `cx.signal(0)` in `Counter::build`, ecosystem in `StatelessView::build`). This contradicts the enforced ADR-0074 rule.",
    "Putting the realm or frame transaction in flui-view (minimalist), or putting runtime below widgets without first moving the root-scope widgets it imports (dx_first T3).",
    "Moving every OS backend into flui-app (minimalist). That makes one crate of about 90k lines mixing unsafe FFI with runtime wiring and loses the separate unsafe and CI boundary.",
    "Merging flui-animation into flui-scheduler and flui-geometry into flui-types just to cut the crate count. The rebuild fan-out and churn outweigh one fewer publish unit.",
    "Deleting StateCell and StateHandle outright (minimalist). Demote them to `flui::view::state` instead, per the decision already taken.",
    "Keeping flui-tree and making ElementTree implement the trait trio as the 'protocol walker' (ai_native). There is no consumer; this is speculative abstraction.",
    "flui-protocol as a package above the facade while core flui-testing consumes it (performance_first).",
    "Re-packing ElementId into a generic GenId purely for uniformity. It is already generational (id.rs:1163); only LayerId and SemanticsId need it.",
    "Deleting HeadlessRenderer before flui-engine-cpu exists (safety). CI would then need a GPU.",
    "Dropping flui-assets from core with no IO or image seam to replace it (minimalist)."
  ],
  "notes": "I read the designs against the scratchpad context and plan and spot-checked them read-only against main cab06137d. Verified: interaction→platform is a single trait import (text_input.rs:27); flui-widgets reaches flui-platform only through its testing feature and dev-deps (Cargo.toml:94,124); the facade has `default = [\\\"material\\\"]` (Cargo.toml:598); flui-app enables `runtime-internals` (flui-app/Cargo.toml:90); desktop-mcp is `publish = false` with bin only; `pub use ::wgpu` is at flui-engine/src/lib.rs:229; the engine's layer_walk, layer_render, dispatch, command_renderer and damage have 0 wgpu references; runtime-contract.toml is absent; the clipboard accessor is `expect(dead_code)` (runtime.rs:1632); run_app requires `StatelessView + Clone` (runner/mod.rs:216); ElementId is already generational (id.rs:1163); APP_RUNTIME is thread-local (host.rs); rendering depends on semantics and uses animation as a dev-dep only; UiRealm imports FocusRoot, GestureArenaScope, VsyncScope, MediaQuery and NavigatorCommand from flui-widgets.\n\nThe cross-design defect that matters most for the implementer is object safety. `LifecycleContext` (sealed through `BuildContext`) is taken as `&dyn` at 122 sites, so four designs' capability sketches would not compile as written. The second is where the runtime sits relative to widgets. Two designs place the runtime below widgets without moving the root scopes it imports (dx_first; minimalist, which puts it in flui-view); safety puts it at T4, below widgets, but states the move.\n\nAll six converge on the same core moves: platform-api split, runtime extraction, realm-owned signals, raster contract plus a CPU backend, curated facade, !Send flip, globals gate. So the choice among them comes down to seam soundness and sequencing, not direction.\n\nUnverified hypotheses carried by the designs: the swapchain scissor leaving stale pixels, the cost of the host font scan at cold start, and duplicate upper-stack builds from `testing` features. I did not build the workspace. The cratesio MCP server failed to connect, so I could not check whether the crate names are available."
}
```

# judge_app_and_plugin_author

```json
{
  "scores": [
    {
      "design": "ecosystem_evolution_first",
      "score": 8.5,
      "strongest": "It is the only design that treats third-party packages as a first-class concern. Every crate carries tier metadata (stable/internal/official/tool) that xtask checks. Only 3 crates carry a semver promise (flui, flui-platform-api, flui-protocol). The P9 rule says an extension point exists only once a second implementation passes its conformance kit. P10 bans pre-1.0 upstream types from Stable signatures, and today the facade re-exports android_activity at src/lib.rs:155-157. It also adds `flui::sdk`, which Material must compile against as proof that the official-package seam works, and `flui verify` badges for community crates.",
      "weakest": "Two of its sketches would not compile. `fn capability<C: PlatformCapability>` is a generic method on LifecycleContext, which is always used as `&dyn LifecycleContext` (crates/flui-view/src/context/build_context.rs:377), so the trait stops being object-safe. The hello-world calls `cx.signal(0)` inside a StatelessView `build`, which ADR-0074's guard forbids. The design also keeps about 24 published internal crates, so crates.io users still see noise even though the banner says they are internal."
    },
    {
      "design": "safety_correctness_first",
      "score": 8,
      "strongest": "It gets the plugin seam right. The capability lookup is an erased `capability_erased(TypeId)` plus a `LifecycleContextExt` typed extension, so the sealed dyn trait stays usable and the capability set is open. It adds a `CapabilityProvider` override, `Unsupported` as a thiserror type with a stable `ID`, and `flui::sdk` with an xtask rule that a package's only in-repo normal dependency is `flui`. Plugin authors can trust the S1 rule of one implementation per contract: tests drive the same runtime, and there is a single BuildContext. It also scopes Stable to about 8 crates, following Slint's internal/api split.",
      "weakest": "Too many crates stay on the promise list: 8 Stable crates, including reactive, geometry and types, which widens the H3 freeze. It adds both flui-reactive and flui-raster-cpu. The DX sketch is heavier (StatefulView + create_state + ViewState + Writer token), and the design says less about community distribution: no badges, and no tier metadata beyond forbid-reach."
    },
    {
      "design": "dx_first",
      "score": 8,
      "strongest": "It gives the best app-author path. `#[flui::main]` replaces the per-OS run_app_* functions, and `run_app` accepts any View kind (today it requires `StatelessView + Clone`, crates/flui-app/src/app/runner/mod.rs:214-216). Signals are canonical, and every callback receives an `rx` it does not have to capture. It adds Router, a Form bound to stores, and a WidgetTester whose finders share the agent protocol's query model. Packages get a curated facade with an `sdk` module and a catalog-neutral prelude. Material is dropped from default, which today is `default = [\"material\"]`.",
      "weakest": "Its `capability<C>` is also a generic method on a dyn-used trait. It deletes flui-hot-reload and flui-localizations before Subsecond or the i18n package is proven. It sketches a federated plugin shape but has no tier metadata, so nothing checks that an official package depends only on `flui`. Its plan to drop Flutter-branded names is cosmetic compared with the structural items."
    },
    {
      "design": "ai_native_first",
      "score": 7,
      "strongest": "The `#[derive(Catalog)]` contract gives community widgets a stable NAME, a JSON Schema, and examples that compile as tests. That makes third-party catalogs agent-visible and usable from A2UI automatically. Its `Query`, `Action` and outline types are shared by `flui test` and `flui mcp`. Capabilities appear in a protocol `capabilities` listing, and `impl dyn LifecycleContext` is object-safe.",
      "weakest": "It keeps flui-tree and gives it a new job (a generic tree walker), which is weaker than merging it. It is thinner on the package author's path: there is no sdk-only rule enforced as manifest data, and no stability-tier metadata. The catalog derive burden across about 1.3k widget items falls on package authors. Protocol-first pushes H0 scope toward agent features ahead of the Windows app loop."
    },
    {
      "design": "performance_first",
      "score": 6.5,
      "strongest": "Its extension points are safe for performance by construction. Plugins must take part in identity and damage, custom shaders join the prewarm set, and conformance checks include perf invariants. Its `flui-text` split and `flui-platform-api` reach facts cut the plugin dependency graph from about 198 crates to about 30. It has one `App` entry point.",
      "weakest": "It offers the least for the plugin or package author. Stability promises are vague (it freezes several module tiers). It makes no sdk-only enforcement for packages, and its DX sketch has wrong signatures (`init_state` returning Self). It adds 5 crates, with flui-raster and flui-text as new publish units, and it merges devtools into protocol. Seams are justified by perf rather than by what an ecosystem needs."
    },
    {
      "design": "minimalist",
      "score": 6.5,
      "strongest": "It has one semver crate (the facade), one state primitive, a `packages/` directory that depends only on `flui` with caret requirements (so a later repo split is a `git mv`), and the P9 rule that unwired pub surface is deleted. The lean surface is easy for app authors to learn.",
      "weakest": "It merges OS backends into flui-app (about 90k lines), which sends the 'backend edits rebuild' cost and the unsafe budget into the composition root. It deletes flui-assets from core and deletes StateCell outright. It merges semantics into rendering and animation into scheduler, which are churn with no ecosystem gain. It puts a generic `capability<C>` on a sealed dyn trait. Its DX sketch creates a signal in `build`. `Stateful::init(&self, cx: &mut LifecycleCx)` changes the context model with no ADR path."
    }
  ],
  "winner": "ecosystem_evolution_first",
  "best_crate_topology": "Use ecosystem_evolution_first's topology: named tiers V/C/S/R/K/H with forbid-reach facts, plus orthogonal `tier = stable|internal|official|tool` metadata checked by `cargo xtask workspace`, and a Stable promise on only flui, flui-platform-api and flui-protocol. Change three things. (1) Adopt safety_correctness_first's `flui::sdk` rule as a gate: an official package's only normal in-repo dependency is `flui`, with platform-api and protocol allowed for plugins. Today flui-material depends on 10 internal crates (crates/flui-material/Cargo.toml:25-49). (2) Keep the reactive core in flui-foundation, not a separate flui-reactive crate, and keep flui-assets in core (runtime-agnostic, as ecosystem proposes), not deleted as minimalist proposes. (3) Leave OS backends in flui-platform (tier H), not in flui-app. Merge flui-tree into foundation/rendering, delete flui-localizations, and move material, cupertino, devtools, mcp and hot-reload (after Subsecond) into `packages/` in the monorepo with caret requirements on `flui`. Everything else can stay an exact-pinned internal train generated from [workspace.dependencies] (145 hard-coded =0.2.0-dev pins in crates/*/Cargo.toml today).",
  "ideas_to_graft": [
    "safety_correctness_first: an object-safe capability seam. `LifecycleContext` gets `#[doc(hidden)] fn capability_erased(TypeId)` plus a typed `LifecycleContextExt::capability::<C>()`, and a `CapabilityProvider<C>` gives an override hook from day one. Evidence: LifecycleContext is used as `&dyn` (build_context.rs:377), so a generic method would break dyn dispatch.",
    "ecosystem: P8/P9/P10 as written principles. The facade is the only promise, each seam is proven by a second implementation plus its conformance kit, and no pre-1.0 upstream type appears in a Stable signature. Enforce it with a cargo-public-api snapshot on the 3 Stable crates from H0.",
    "ecosystem + safety: `flui::sdk` as an Evolving author surface, sized by exactly what Material and Cupertino import, with Material compiling against `flui` only as the H0 proof.",
    "dx_first: `#[flui::main]` plus `App::new(any View)` replaces run_app_* and the `StatelessView + Clone` root bound (runner/mod.rs:214-216).",
    "dx_first + ai_native: one query model (`find(role).label().activate()`) and one outline format shared by WidgetTester, semantic goldens and `flui mcp`, so a failing agent scenario and a failing test are the same artifact.",
    "ai_native: `#[derive(Catalog)]` with a stable NAME/JSON Schema/examples-as-tests, so community widgets are automatically agent- and A2UI-visible, and TreeObserver carries names instead of TypeId.",
    "all: a conformance kit `flui::testing::rendering::check_box/check_sliver`, plus a facade fixture with a custom RenderSliver. The current tests/fixtures/*.rs contain zero RenderSliver, so third-party slivers are unproven.",
    "ecosystem: `flui verify` badges computed from the conformance kits (builds against the current train, harness passes, semantics declared, unsafe budget) instead of a curation committee.",
    "all: catalog-neutral facade default. Remove `default = [\"material\"]` (Cargo.toml features) and the whole-crate `pub use flui_x as x` aliases (src/lib.rs:125-152). The CLI template adds material explicitly.",
    "all: a `!Send` flip of callback bounds before H3. Plugin and app callbacks must accept Rc captures. Today page_view.rs still threads `Arc<dyn Curve + Send + Sync>` through public widget config.",
    "ecosystem: a pull-based text-store IME contract in flui-platform-api with TSF first on Windows. It is a platform-parity prerequisite, because Win32 ships with no IME path.",
    "minimalist: P9 'unwired pub surface is deleted at next minor', plus making the clipboard the first client of the capability seam. The clipboard is installed but unreachable today (runtime.rs:738-746)."
  ],
  "ideas_to_reject": [
    "Generic `fn capability<C>()` directly on the sealed dyn-used LifecycleContext (dx_first, ecosystem, minimalist). It is not object-safe, so use the erased method plus an Ext trait.",
    "Creating signals inside `build` in the canonical sketches (minimalist, ecosystem). This contradicts ADR-0074's run-time guard and would teach the wrong idiom in the first example.",
    "minimalist: moving all OS backends into flui-app. It turns the composition root into about 90k lines of unsafe-heavy code, which plugin authors (who need only the contract) would never touch, but which every app rebuild pays for.",
    "minimalist: merging animation into scheduler and semantics into rendering. This churns the internal layers with no benefit to app or plugin authors.",
    "minimalist: deleting StateCell and removing assets from core before signals and the replacement loaders are proven. It leaves app authors without an escape hatch or image loading during the migration.",
    "dx_first / ai_native: deleting flui-hot-reload and flui-devtools before the Subsecond spike and the protocol server exist. Mark them publish=false instead, and delete once replaced.",
    "Multiple semver-promised crates beyond facade + platform-api + protocol (safety's ~8, performance's module tiers). Each extra promise is a freeze liability for a bus-factor-1 project.",
    "Separate repos for official packages now (all designs rightly reject it). Use `packages/` with caret requirements on `flui` until release cadence diverges.",
    "ai_native: keeping flui-tree as a generic inspector trait crate. That adds a publish unit to serve one consumer, and the protocol walker can live in runtime.",
    "dx_first: removing `column!`/Flutter names as an H0 priority. It is cosmetic compared with the seam work."
  ],
  "notes": "I verified these claims at main cab06137d:\n- The facade is `default = [\\\"material\\\"]`, `signals` is off by default, and src/lib.rs:125-157 re-exports whole crates, including `flui_app::android_activity`.\n- `run_app_impl` requires `V: View + StatelessView + Clone` (crates/flui-app/src/app/runner/mod.rs:214-216).\n- flui-interaction imports `flui_platform::traits::PlatformTextInput` (text_input.rs:27). This is the edge that leaks backends into the headless stack.\n- flui-material depends on widgets, view, types, objects, rendering, foundation, animation, interaction, scheduler and painting (Cargo.toml:25-69). The \"packages depend on flui only\" rule is therefore a real migration, not a relabel.\n- BuildContext is sealed (build_context.rs:19,106). `LifecycleContext: BuildContext` (line 377) is sealed transitively and is used as `&dyn`. This is the basis for my object-safety objection to three designs' capability sketches (a compile-level hypothesis; I did not build a prototype).\n- The facade fixtures in tests/fixtures contain zero RenderSliver.\n- The clipboard is stored as `Arc<Mutex<Option<Arc<dyn Clipboard>>>>` in runtime.rs:746.\n\nThe pin count I measured is 145 `=0.2.0-dev` occurrences in crates/*/Cargo.toml, not the designs' 172. The difference may come from the root and the tools; I did not recount.\n\nAll six designs converge on the same core moves: platform-api split, runtime extraction, a curated facade with a catalog-neutral prelude, signals canonical and realm-owned, the `!Send` flip, a PlatformCapability seam, conformance kits, flui-protocol, and packages/ in the monorepo. So the ranking from this lens depends on stability-tier discipline and the correctness of the plugin seam.\n\nWindows-first parity: every design names TSF, but none makes a Windows IME run plus a Narrator run an H0 gate for plugins. Recommend adding it.\n\nI did not verify:\n- the Subsecond status on Windows;\n- whether cargo-public-api snapshots are practical over the facade's re-export depth;\n- whether crates.io names are available (the cratesio MCP failed to connect)."
}
```

# judge_owner_maintainer

```json
{
  "scores": [
    {
      "design": "ecosystem_evolution_first",
      "score": 8.2,
      "strongest": "It turns the plan's delivery layers and H3 stability tiers into manifest data that xtask checks: [package.metadata.flui] tier = stable|internal|official|tool, and official packages may depend only on `flui`. Only three crates carry a semver promise (facade, platform-api, protocol), which is the smallest governance and semver-checks load that still supports plugins and agents. The P9 rule (a seam exists only once a second implementation passes its conformance kit) gives agents and outside contributors an objective way to finish work without the author, which the H3 exit asks for. It ties each H0 seam to an H0 exit item. It also points out the contradiction in the plan between 'separate repos' and 'one release train', and resolves it with caret dependencies on the facade and a repo split only when a package's cadence diverges.",
      "weakest": "Fourteen H0 breaking items, with no sequencing beyond 'items 1-3 and 10 first', is more than a WIP limit of 2 tracks can carry. The crate count stays at about 24, plus a new flui-engine-cpu. The facade default keeps a `signals` feature while the design also says signals are canonical, which is a small inconsistency."
    },
    {
      "design": "safety_correctness_first",
      "score": 7.6,
      "strongest": "S1 ('one implementation per contract': one frame transaction, one BuildContext, one raster lowering, one protocol schema) is the rule that most improves how agents work in the repo, because tests then exercise production paths. It has the best adoption mechanics for a solo maintainer: land each gate with an allowlist, then shrink the allowlist, so each gate fits one PR and one WIP slot. It adds ADR front-matter validation, structured evidence records instead of BETA.md prose, and makes the no-build-time-writes rule a type (`&mut Writer` only in callbacks).",
      "weakest": "Topology cost. It adds five core crates (platform-api, reactive, protocol, runtime, raster-cpu), turns five crates into packages, lands at 26 units, and has about 8 semver-promised crates to check at H3. Its own crate arithmetic is inconsistent ('four are added' followed by a list of five)."
    },
    {
      "design": "minimalist",
      "score": 7.2,
      "strongest": "It treats maintenance load as the primary constraint. P8 ('crate count is a cost', and a new crate needs an ADR naming its second consumer) and P9 ('unwired pub surface is deleted') are the right governance rules for a single maintainer working with agents. It puts semver-checks on one crate (the facade), makes adopting ecosystem crates the default, uses a packages/ workspace so that leaving the repo later is just a `git mv`, and has the clearest H3 story.",
      "weakest": "Several of its merges add work or coupling without adding H0 value. Merging geometry into types raises rebuild fan-out on a crate 24 crates already rebuild. Animation into scheduler and semantics into rendering are churn with no exit item behind them. Moving OS backends into flui-app makes a crate of 90k+ lines, and moving the frame transaction into flui-view makes view the god crate. Moving assets out of core is questionable. Deleting StateCell/StateHandle contradicts the decided 'setState stays as low level'. Deleting the CLI test/analyze commands is a visible DX regression. The log-into-app merge claims flui-cli does not use flui-log; the manifest lists cli as an allowed dependent (crates/flui-log/Cargo.toml:66), so that claim needs checking."
    },
    {
      "design": "ai_native_first",
      "score": 7,
      "strongest": "It gives the plan's 2030 picture ('one catalog, three consumers') a concrete home. flui-protocol sits low in the graph. Tests and agents share one Query/Action/outline model, so a failing agent scenario and a failing test are the same artifact. #[derive(Catalog)] generates G6, A2UI, llms.txt and the `flui create` AGENTS.md index. P8 ('one concept, one home, found by a machine') targets exactly the duplication that agents replicate.",
      "weakest": "It adds workload that has no H0 exit behind it. 'Give flui-tree a job' makes ElementTree implement traits that nothing consumes today. Making the FLUI wire protocol primary with MCP as a projection departs from the plan's 'over MCP/AccessKit' wording and needs an ADR. The runtime crate also hosts the inspector and reload modules, which risks rebuilding a god crate. It has the heaviest H0 extraction list (protocol, platform-api, runtime and state all at once)."
    },
    {
      "design": "dx_first",
      "score": 6.9,
      "strongest": "'The facade is the product' and 'generated, not hand-written' (the catalog, llms.txt, crate map and ADR index come from code). It notes the stale hand-written docs (llms.txt, book state.md:45), which are what mislead agents today. It sets one canonical way per concept, has an exit proof that uses the same finders in `flui mcp` and in `flui test`, and adds release-check plus workspace.dependencies version pins.",
      "weakest": "It keeps about 27 core publish units, adds five new crates, and renames flui-testing to flui-test purely for churn. Twelve breaking changes plus runtime, protocol, engine-cpu, a damage producer and four gates all sit in H0, which works against the WIP limit and the bus factor. It says little about governance and tiers beyond public-api snapshots."
    },
    {
      "design": "performance_first",
      "score": 6.4,
      "strongest": "Deterministic per-PR count gates (elements built, layout nodes, damage area, allocations) are the cheapest way to meet principle 5 for performance, and they are noise-free, which suits agent contributors. It gives the only explicit bus-factor sequencing: runtime and platform-api first, then identity and damage, then the !Send flip and signals. It records an explicit 'no intra-realm parallel layout' decision.",
      "weakest": "It pulls H2 work, the retained layer tree and damage, to the top of the H0 breaking list, against the plan's rule that H0 is the developer loop. It adds flui-text and flui-raster as new crates on top of runtime, platform-api and protocol, a split that is justified by rebuild fan-out and not by an exit item. It says the least about governance, tiers, packages or agent workflow."
    }
  ],
  "winner": "ecosystem_evolution_first. From the owner and maintainer view it is the only design that encodes the plan's own governance model (core, official and community delivery layers, and the Stable, Evolving and Experimental tiers) as xtask-checked manifest data. It limits the semver promise to three crates and gives agents an objective definition of done for extension work: conformance kits plus the second-implementation rule. It should be run with safety_correctness_first's gate-first ratchet mechanics, performance_first's sequencing, and minimalist's P8 and P9 crate and surface discipline, so that the H0 break list fits the WIP limit of 2 tracks.",
  "best_crate_topology": "Use ecosystem_evolution_first's topology as the base: six named tiers V/C/S/R/K/H, each with forbid-reach facts, plus a `tier` metadata key checked next to `layer`. Change it as follows.\n\n(1) Keep the new crates to the four that the designs agree on and that each have an H0 consumer:\n- flui-platform-api (C). It removes the flui-interaction→flui-platform edge, which I verified at crates/flui-interaction/src/text_input.rs:27.\n- flui-protocol (C). It lifts the protocol out of tools/desktop-mcp, which is publish=false (tools/desktop-mcp/Cargo.toml:11).\n- flui-runtime (K). It replaces the duplicated HeadlessBinding transaction.\n- The reactive graph core as a module in flui-foundation, not a new flui-reactive or flui-state crate.\n\n(2) The CPU rasteriser starts as publish=false (ai_native's suggestion) or as a `cpu` module or feature of flui-engine. It is not a published core crate until H2 promotes it to the software fallback. The raster contract moves into flui-layer. Reject performance_first's separate flui-text and flui-raster crates.\n\n(3) Merge flui-tree into foundation or rendering, and delete flui-localizations. Keep flui-log, flui-assets, flui-semantics, flui-animation and flui-geometry as separate crates; reject minimalist's merges, because they are churn with no exit item behind them.\n\n(4) Keep the OS backends in flui-platform (H tier), not inside flui-app, so that the unsafe ledger and the crates CI never executes stay behind a crate boundary. flui-app shrinks to runners.\n\n(5) Move Material, Cupertino, devtools, mcp and hot-reload to a packages/ directory in this repo. They depend only on `flui` and its `sdk` module, which xtask enforces. Repos split only when a package's cadence diverges.\n\n(6) Put internal versions in [workspace.dependencies], use `cargo publish --workspace`, and add a release-check CI job. Semver-checks cover only the facade, platform-api and protocol.\n\n(7) Declare the flui-widgets module DAG in manifest metadata and enforce it with an xtask gate.",
  "ideas_to_graft": [
    "Tier metadata ([package.metadata.flui] tier = stable|internal|official|tool) checked by `cargo xtask workspace`, with official packages allowed to depend on `flui` only (ecosystem). It turns the plan's delivery layers and H3 stability tiers into a gate, not a review item.",
    "Gate-first ratchets (safety). Land each new check (globals, BUG: prefix, undocumented_unsafe per module, disallowed_types for Mutex on the frame path, module direction) together with a reasoned allowlist in the same PR, then shrink the allowlist. Each gate becomes a one-PR task an agent can finish, and the rules stop depending on the author's review.",
    "P8 'crate count is a cost: a new crate needs an ADR naming its second consumer or its compile/semver seam', and P9 'unwired pub surface is deleted at the next minor unless the PR names the follow-up' (minimalist). Both encode AGENTS.md's most common defect class as policy.",
    "P9 from ecosystem: an extension point counts as existing only once a second implementation passes its conformance kit (wgpu+CPU, clipboard+haptics+dialogs, flui-objects plus an out-of-tree fixture, desktop plus in-process protocol backend).",
    "Sequencing for bus factor 1 (performance): flui-runtime extraction and the platform-api split first, because they unblock tests and plugins. Then layer identity and damage. Then the !Send flip and realm signals. Spread the 11-14 breaking changes across H0 under the WIP-2 limit instead of listing them as one H0 block.",
    "forbid-reach manifest facts generalised from the existing TREE_FACTS (tools/xtask/src/tasks/facade.rs), so headless and wasm purity is checked instead of accidental.",
    "Generated rather than hand-written agent context (dx, ai_native): #[derive(Catalog)] drives G6, A2UI, llms.txt and the `flui create` AGENTS.md index. Also generate the concept→module map, docs/crates.md and the ADR index. ADR front matter (status enum, symmetric Supersedes) is validated by xtask.",
    "Structured evidence records (docs/evidence/*.toml written by `xtask device` and protocol runs) instead of prose in BETA.md, which makes principle 5 machine-checkable.",
    "One query model for `flui test` and `flui mcp`, with the outline format doubling as the semantic-golden file format, so a failing agent scenario and a failing test are the same artifact.",
    "Deterministic per-PR count gates for performance (elements built, layout passes, damage area, allocations), with wall time kept as a nightly trend. Also fix bench-collect skipping feature-gated benches.",
    "An xtask check that fails any Cargo feature with zero cfg sites. It removes the no-op features, e.g. flui-platform default = [\"desktop\"] at crates/flui-platform/Cargo.toml:300-303, which only pulls in winit.",
    "A curated facade with an enumerated, catalog-neutral prelude, a `flui::sdk` module for package authors, a cargo-public-api snapshot in `checks` from H0, and Material removed from the facade default (verified at Cargo.toml:598). Replace the runtime-internals feature (switched on for every app at crates/flui-app/Cargo.toml:90) with #[doc(hidden)] modules.",
    "A write-token type (`&mut Writer` available only in callbacks and effects) so ADR-0074's no-writes-in-build run-time guard becomes a compile-time rule (safety)."
  ],
  "ideas_to_reject": [
    "Minimalist's merges geometry→types, animation→scheduler, semantics→rendering and log→app, and removing assets from core. Each is a multi-crate churn PR that no H0 or H1 exit needs. geometry→types also raises rebuild fan-out on a crate 24 crates already rebuild. The log merge rests on 'cli doesn't use it', while the manifest lists flui-cli as an allowed dependent (crates/flui-log/Cargo.toml:66).",
    "Minimalist's move of the OS backends into flui-app (a crate of 90k+ lines) and of the frame transaction into flui-view. Both recreate the god-crate problem that the runtime extraction is meant to solve, and they blur the unsafe and unexecuted-backend boundary.",
    "Deleting StateCell/StateHandle outright (minimalist). It contradicts the decision that setState stays as the low level. Demote them out of the prelude instead.",
    "Performance_first's separate flui-text and flui-raster crates in H0. Put the raster contract in flui-layer, and put text inside painting behind the Parley ADR, until measured fan-out justifies a crate.",
    "Ai_native's 'give flui-tree a job' by making ElementTree implement the traits. That is work with no consumer; merge the crate instead.",
    "Renaming flui-testing to flui-test (dx). It is churn with no user value before the first publish.",
    "Deleting the CLI test/analyze/format wrappers without a replacement (minimalist). Change them to emit per-item NDJSON instead.",
    "Any design's framing of 11-14 breaking changes as one H0 block. It violates the WIP limit of 2 tracks with 1 platform slot. Order the changes by exit dependency.",
    "Retained layer identity and damage as breaking change #1 in H0 (performance_first). Settle the contract before H3, but schedule the work after the runtime and platform-api cuts; the H0 exit is the developer loop, not performance.",
    "Separate repos for official packages before 1.0. All six designs agree on this, so the owner should amend plan.md's delivery-layers line ('separate repos, one release train') to 'packages/ in this repo, caret on flui, split repos when cadence diverges'.",
    "More than about three semver-promised crates at H3 (safety proposes about 8). Every promised crate multiplies the semver-checks and LTS load for a single maintainer."
  ],
  "notes": "I spot-checked the claims the scores depend on, at main cab06137d.\n- The interaction→platform edge is real: `use flui_platform::traits::PlatformTextInput;` at crates/flui-interaction/src/text_input.rs:27.\n- The facade default is `default = [\"material\"]` (Cargo.toml:598).\n- flui-platform has `default = [\"desktop\"]` with `desktop = [\"dep:winit\"]` (crates/flui-platform/Cargo.toml:300,303).\n- flui-app turns on flui-view's `runtime-internals` for every app (crates/flui-app/Cargo.toml:90).\n- tools/desktop-mcp is `publish = false` and has no [lib] section (tools/desktop-mcp/Cargo.toml:11).\n- `git ls-files | grep runtime-contract` returns nothing, so the ambient-reach ratchet file the roadmap cites really is gone.\n- flui-log's allowed-dependents includes flui-cli (crates/flui-log/Cargo.toml:66), which undercuts minimalist's log→app merge rationale.\n- There is still a process marker in the root manifest: Cargo.toml:33 contains 'Catalog.1 slice', which supports the designs' marker-gate item.\n\nOwner-level observations:\n\n(a) All six designs converge on one core set: the platform-api split, flui-runtime extraction superseding the ADR-0041 gate, a curated catalog-neutral facade, globals, module-direction and reach gates, the !Send flip before H3, a realm-owned signal graph with phase-typed subscribers, a raster contract with a CPU backend (superseding the engine's 'nothing pluggable' stance), deleting flui-localizations and folding flui-tree, Subsecond, a packages/ directory in this repo, and one owner thread with no intra-realm parallel layout. The owner can treat that set as decided and debate only the deltas.\n\n(b) Three planned items need explicit plan or ADR edits that every design implies: the separate-repos wording in plan.md, ADR-0078's 'one method per capability' clause (it blocks out-of-repo plugins, which is an H1 exit), and ADR-0027's 'realms may execute concurrently' verdict (not implemented).\n\n(c) The biggest owner risk is scheduling, not design. No design maps its break list onto the WIP-2 limit properly; performance_first comes closest. The next step is to turn the consensus set into a sequenced H0 track list in TASKS/ROADMAP, each track with its gate landed first.\n\nI did not build anything and did not verify crate-size or rebuild fan-out numbers (for example, 16 crates rebuilt per Win32 edit); those are taken from the designs."
}
```

# verify_feasibility_topology

```json
{
  "lens": "feasibility_topology",
  "issues": [
    {
      "claim_or_decision": "§2.5.6 facade opt-in features `devtools` and `hot`, while flui-devtools and flui-hot-reload become `packages/` that normally depend on `flui` (§2.2 official tier, §2.4)",
      "problem": "This cannot be expressed in Cargo. A facade feature that pulls in a package, when that package depends on the facade, is a dependency cycle. Cargo rejects the cycle even when the edge is optional and the feature is off, because the lockfile is resolved with all features. The synthesis avoided this for Material by removing its facade feature, but kept `devtools` and `hot`. Today's graph already has `flui-app -> flui-hot-reload` (crates/flui-app/Cargo.toml) and a facade feature, `hot-reload = [\"dep:flui-hot-reload\", ..., \"flui-app/hot-reload\"]` (Cargo.toml:630). Once hot-reload depends on `flui`, both edges close a cycle through `flui -> flui-app`.",
      "evidence": "I reproduced it in the scratchpad with a minimal workspace: `a` has `b` as an optional dependency behind feature `hot`, and `b` depends on `a`. `cargo metadata` fails with: error: cyclic package dependency: package `a v0.1.0` depends on itself (exit 101). Current edges: Cargo.toml:519, 554 and 630; crates/flui-app/Cargo.toml lists flui-hot-reload as a dependency.",
      "severity": "blocking",
      "fix": "Drop `devtools` and `hot` as facade features. Apps add the package directly, or the CLI template adds it under `[target.'cfg(debug_assertions)']` or `dev-dependencies`. The runtime exposes only the `RealmObserver` and `DevReloadHook` traits. Remove the flui-app -> flui-hot-reload edge in the same PR that moves hot-reload into packages, not later. Add a check to `cargo xtask workspace` that no core crate names a `tier-kind = official` crate, even optionally."
    },
    {
      "claim_or_decision": "§2.2 and §3.3: flui-testing moves above flui-runtime, which sits above flui-widgets, and absorbs `flui_widgets::testing`. The goal is for tests to drive the real transaction (S1).",
      "problem": "flui-widgets' own unit tests inside src/ cannot use a harness that lives above flui-widgets. When a crate dev-depends on something that depends back on it, the `#[cfg(test)]` build links a second copy of that crate. The runtime installs the root scopes from that second copy (FocusRoot, GestureArenaScope, VsyncScope and MediaQuery, which the synthesis keeps in widgets). Code under test in `crate::` then looks up inherited scopes by TypeIds from the first copy and finds nothing, or hits type mismatches. The synthesis does not account for this. It means 22 unit-test files must become integration tests, or a second in-crate harness has to survive, which contradicts S1.",
      "evidence": "`grep -rln 'crate::testing' crates/flui-widgets/src`, excluding src/testing, finds 22 files (for example animated/ticker_mode.rs, interaction/focus.rs, interaction/shortcuts.rs, app/widgets_app.rs). The harness is 2,144 lines (src/testing.rs plus src/testing/*.rs). Root scopes come from widgets: crates/flui-app/src/app/ui_realm/attach.rs:6 `use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};`. The TypeId-mismatch outcome is a hypothesis based on how Cargo handles dev-dependency cycles; I did not compile it.",
      "severity": "major",
      "fix": "Budget moving those 22 unit-test modules into `crates/flui-widgets/tests/`, or keep a thin harness below the runtime inside widgets that drives the runtime transaction through a trait object defined in flui-view. Name that cost in W2/W3, and add a gate forbidding `flui_testing::WidgetTester` in `src/` of any crate below the runtime."
    },
    {
      "claim_or_decision": "§4 D4: the reactive graph core moves to flui-foundation as data structures only. Layout and Paint subscribers let rendering and animation read signals. The hello-world keeps `count.get(cx)` and `count.update(w, ..)`.",
      "problem": "If rendering is to subscribe, the `Signal<T>` handle has to live in foundation or below rendering, not only the graph. But `Signal::get` is an inherent method taking `&dyn crate::BuildContext`, a flui-view type. Inherent impls can only be written in the crate that defines the type, and foundation cannot name BuildContext. So the migration needs a foundation-level read-context trait, with BuildContext as a subtrait and upcasting at call sites, or an extension trait that users must import. The synthesis does neither, and its 'blast radius is one PR' estimate ignores this. Separately, `BuildContext::reactive()` hands `build` the whole `Reactive`, which has `set`. The 'compile error' guarantee of the `Writer` token therefore also requires removing that method.",
      "evidence": "crates/flui-view/src/reactive/mod.rs:648 defines `pub struct Signal`. :752 `pub fn get(self, cx: &dyn crate::BuildContext) -> T`. :774 `pub fn set(self, r: &Reactive, ..)`. :68 `use crate::owner::ExternalBuildScheduler`. crates/flui-view/src/context/build_context.rs:132 `fn reactive(&self) -> crate::reactive::Reactive;`.",
      "severity": "major",
      "fix": "Put `trait ReadScope` (current subscriber, graph handle) in foundation next to Signal. Make BuildContext a subtrait of ReadScope and implement read contexts for layout and paint in rendering. Remove `BuildContext::reactive()` in the same PR. State in D4 that the handle type moves, not just the core, and prototype it before W3."
    },
    {
      "claim_or_decision": "§2.3 flui-scheduler: 'AsyncDriver moves to the runtime'. §3.5 and §8 acquire a `Spawner` capability in `init_state` via `cx.spawner()`. flui-runtime (K) sits above flui-view and flui-widgets.",
      "problem": "This inverts the layering. `LifecycleContext` lives in flui-view and returns the concrete type, so a view trait method cannot name a type defined in flui-runtime above it. The same applies to `cx.spawner()`. The erased `capability_erased` path avoids the problem for capabilities, but the synthesis presents Spawner and AsyncDriver as first-class methods.",
      "evidence": "crates/flui-view/src/context/build_context.rs:406 `fn async_driver(&self) -> Option<crate::AsyncDriver>;`. The type is defined at crates/flui-scheduler/src/async_driver.rs:463 and re-exported at flui-scheduler/src/lib.rs:160. The runtime above widgets is §2.1.",
      "severity": "major",
      "fix": "Keep the `!Send` AsyncDriver and Spawner types in flui-scheduler (S), with the runtime only owning the instance. Or define `trait Spawner` in flui-view or foundation, implement it in the runtime, and return `Rc<dyn Spawner>`. Apply the same rule to every realm resource the synthesis puts 'in the runtime' but reaches through LifecycleContext (GlobalKey scope, FontContext)."
    },
    {
      "claim_or_decision": "§3.5 and D13: '`RasterOwner` moves from engine to runtime'. The K tier is labelled 'headless, wasm-clean' and the R tier says 'only engine reaches wgpu'.",
      "problem": "RasterOwner is written against `RasterBackend`, whose methods return `EngineError`, and that error carries wgpu types. Moving the file into flui-runtime would drag flui-engine, and so wgpu, into the K tier's normal graph. That violates the tier's reach fact unless the backend trait and its error are first moved into flui-layer as backend-neutral types. The synthesis does not list that preparatory step. The file is also 4,365 lines.",
      "evidence": "crates/flui-engine/src/raster_owner.rs:57-58 `use crate::error::EngineError; use crate::raster::{PresentDisposition, RasterBackend};`. crates/flui-engine/src/raster.rs:100-111 `pub trait RasterBackend: Send { fn render_scene(&mut self, scene: &Scene) -> Result<PresentDisposition, EngineError>; ... }`. `grep -c wgpu crates/flui-engine/src/error.rs` returns 18. `wc -l raster_owner.rs` returns 4365.",
      "severity": "major",
      "fix": "Make `RasterBackend`, `PresentDisposition` and a wgpu-free `RasterError` part of the flui-layer raster contract (§5.1) before moving RasterOwner. flui-engine then maps its `EngineError` into that error. Add `forbid-reach = [\"wgpu\"]` for flui-runtime in the same PR."
    },
    {
      "claim_or_decision": "P10 ('No pre-1.0 upstream type in a Stable signature') alongside `flui-platform-api` (Stable), which contains `PlatformAccessibility (takes an accesskit::TreeUpdate)`, and `flui-protocol` (Stable) with 'AccessKit role and action vocabulary (via accesskit::Role)'",
      "problem": "This contradicts itself. Two of the three Stable crates are specified to expose accesskit 0.25, a pre-1.0 crate, in public signatures. P10's own gate, a cargo-public-api snapshot of the Stable crates, would reject the design as written. H1 plugins would then be coupled to accesskit's release cadence.",
      "evidence": "crates/flui-platform/src/traits/accessibility.rs:34 `use accesskit::{ActionRequest, TreeUpdate};` and :72 `fn publish(&self, update: TreeUpdate);`. Cargo.toml:144 `accesskit = \"0.25\"`. Synthesis §2.2 table rows for platform-api and protocol, §11.1, and §1 P10.",
      "severity": "major",
      "fix": "Either keep `PlatformAccessibility` in the internal backend crate (H), which is the only consumer, and give flui-protocol its own role and action enums with a tested 1:1 map to accesskit, or list accesskit in P10 as an explicit, reasoned exception."
    },
    {
      "claim_or_decision": "D8 and §2.3 totals: 'semver promises drop from 28 potential units to 3', and 'Small H3 freeze'",
      "problem": "The public surface does not shrink. It moves into the facade. §2.6 keeps `pub mod view/state/widgets/rendering/painting/interaction/animation/geometry/testing/sdk`, each a curated re-export of internal crates. Every re-exported item becomes a semver promise of `flui`, and a change in an 'internal' crate breaks semver whenever the item is re-exported. `sdk` is labelled Evolving but sits inside the Stable `flui`, so once cargo-semver-checks starts gating at H3, every change to `sdk` needs a major bump of the facade. The crate count falls, but the freeze covers roughly the same number of items.",
      "evidence": "src/lib.rs:126-152 currently re-exports whole crates (`pub use flui_widgets as widgets`, `flui_view as view`, `flui_app as app`, and so on). Synthesis §2.6 lists the curated modules and marks `sdk` as Evolving. Material's current reach into internals: `grep -rhoE 'flui_(rendering|objects|scheduler|interaction|painting)::...' crates/flui-material/src crates/flui-cupertino/src` shows RenderPhysicalShape, RenderTable, DrawOp, BoxConstraints, BoxProtocol, Canvas, RenderUpdateImpact, LocalPostFrameHandle and more. `sdk` must re-export all of these.",
      "severity": "major",
      "fix": "Measure the facade surface (the synthesis already lists the cargo-public-api spike) before claiming a small freeze. Put `sdk` in a separately versioned crate (for example a `flui-sdk` contract crate with `tier-kind = stable-evolving`) or behind `unstable`, so it can break without a major bump of `flui`. Restate D8 as 'three crates, N items', with N measured."
    },
    {
      "claim_or_decision": "§15 W1 puts the platform-api split next to all the H0 gates, and W2 then has 'runtime extraction, OwnerHost replacing APP_RUNTIME TLS'",
      "problem": "The code says OwnerHost cannot replace the thread-local until the platform callback surface stops requiring `Send`. The W1 split therefore has to redesign the callback contract across every backend (Win32, AppKit, UIKit, Android, web, winit), plus the owner-loop state machine and the pull-based IME. That is not a mechanical move, and its cost is not reflected in a one-wave slot shared with six gates. Only Linux/headless runs in CI, so the other backends are verified only by clippy. The effort estimate is optimistic, and the dependency should be stated. This is a hypothesis about effort; the dependency itself is documented in code.",
      "evidence": "crates/flui-app/src/app/runner/host.rs, the APP_RUNTIME doc comment: 'The platform callback surface still requires `Send`, so the `!Send` realm this holds remains in owner TLS until that seam is retired (ADR-0027 follow-up 5)'. crates/flui-platform/src/traits/platform.rs:319-698 has `Box<dyn Fn.. + Send>` callbacks: set_exit_policy_hook, on_quit, on_window_event, on_open_urls, spawn, and others. flui-platform is 46,302 lines; flui-app is 52,092 lines, with realm_dispatch.rs at 7,149.",
      "severity": "major",
      "fix": "Split W1 into two steps. First, the mechanical crate split (a trait move plus re-export, one PR). Second, retiring `Send` on the callbacks per backend, which is the prerequisite for W2. Make W2's OwnerHost item depend explicitly on that second step."
    },
    {
      "claim_or_decision": "§4 and §8: every UI callback receives `&mut Writer` (`on_press(move |w| ..)`), while §14 item 7 lists this as one reactive-core step",
      "problem": "This changes the signature of every callback setter in the base catalog and the design systems, not just the reactive core. The effort and churn are not in the migration plan or the `flui migrate` data.",
      "evidence": "`grep -rhoE 'pub fn on_[a-z_]+' crates/flui-widgets/src crates/flui-material/src crates/flui-cupertino/src | wc -l` returns 92.",
      "severity": "minor",
      "fix": "List the callback-signature migration as its own §14 item, sequenced with the `!Send` flip (item 9), which touches the same 92 setters. Consider a `Writer` obtained from the event context rather than a new closure parameter, to limit churn."
    },
    {
      "claim_or_decision": "§2.5.6: facade `default = []`, Material is not a facade feature, and the CLI template adds flui-material",
      "problem": "Today the facade defaults to Material. The README and crate docs rely on `cargo add flui` bringing Material, and `cargo xtask facade-combos` together with `[[example]] required-features` depend on the `material` and `cupertino` facade features. Removing them breaks the example/feature-combination gate and the documentation contract. The synthesis lists none of this in §14.",
      "evidence": "Cargo.toml:598 `default = [\"material\"]`, with the comment at :593-597 ('cargo add flui keeps working exactly as the README ... teach it'). Cargo.toml:605-606 `material = [\"dep:flui-material\"]`, `cupertino = [...]`. AGENTS.md: examples using material/cupertino need `required-features` (`cargo xtask facade-combos` relies on it).",
      "severity": "minor",
      "fix": "Add to §14 item 6: examples that use Material move to packages/flui-material/examples, or keep a root dev-dependency on flui-material (a dev-dependency cycle is allowed). Rewrite facade-combos accordingly and update the README and lib docs in the same PR."
    },
    {
      "claim_or_decision": "§6.3: '`LifecycleContext` is taken as `&dyn` at 122 sites'",
      "problem": "The cited number is off. The conclusion, that a generic method on the trait would break object safety, still holds.",
      "evidence": "`grep -rn '&dyn LifecycleContext' crates src examples --include=*.rs | wc -l` returns 136.",
      "severity": "minor",
      "fix": "Cite 136, or name the command the count came from."
    }
  ],
  "confirmed_strong_points": [
    "Placing the runtime above widgets is the correct low-cost cut. crates/flui-app/src/app/ui_realm/attach.rs:6 imports FocusRoot, GestureArenaScope and VsyncScope from flui_widgets, and commands.rs:8 imports NavigatorCommand. 22 flui-app src files import flui_widgets.",
    "Packages depending only on `flui` is feasible for the derives. flui-macros already resolves paths through proc_macro_crate and falls back to `flui` (crates/flui-macros/src/runtime_path.rs:21-24), and `impl_render_view!` uses `$crate::`.",
    "Moving the GPU-free lowering into flui-layer is feasible. `grep -c wgpu` finds 0 references in layer_walk.rs, layer_render.rs, dispatch.rs, command_renderer.rs and damage.rs (2 in layer_state_stack.rs, in comments). LayerRender is generic over `R: CommandRenderer + LayerStateStack` (layer_render.rs:32), and its imports are only flui_layer, flui_painting and flui_types. flui-layer already depends on painting, so no new edge is needed.",
    "The platform-api split reaches only one production import below the app: crates/flui-interaction/src/text_input.rs:27 `use flui_platform::traits::PlatformTextInput`. The other use is flui-widgets' test harness (src/testing/harness.rs:157,170). `cargo tree -i flui-platform -e normal` confirms that this one edge pulls platform into interaction, rendering, objects, view, widgets and testing.",
    "Material and Cupertino reach into internals only modestly, so porting them to `flui` plus `sdk` is plausible. The bulk of their references are flui_widgets (233), flui_types (200) and flui_view (120). Rendering (26), objects (5), interaction (3), scheduler (2) and painting (9) are a short, enumerable list.",
    "The claim that `__private` has no external consumer is verified. Uses are inside flui-widgets src plus its own tests/anchored_box.rs.",
    "The 145 hand-written `=0.2.0-dev` pins are verified (`grep -rhoE 'version = \"=0.2.0-dev\"' crates/*/Cargo.toml | wc -l`). The `runtime-internals` feature is enabled by flui-app, flui-testing and flui-hot-reload, which supports D11.",
    "Deleting the flui-tree trait trio is cheap. There are 8 impls (RenderTree, LayerTree, SemanticsTree) and no generic `T: TreeNav` consumer outside flui-tree; the few call sites (for example flui-semantics/src/owner.rs:561 and flui-rendering accessors.rs:1101) can become inherent methods.",
    "The erased-method-plus-extension-trait capability seam matches the code: LifecycleContext is sealed through BuildContext (build_context.rs:106,377), and a blanket `impl<T: LifecycleContext + ?Sized>` extension is orphan-safe and object-safe.",
    "The second frame driver exists as claimed: flui-testing/src/lib.rs:955 `pub fn pump_frame`. `desktop = [\"dep:winit\"]` in flui-platform has 0 `feature = \"desktop\"` cfg sites."
  ]
}
```

# verify_invariants_contracts

```json
{
  "lens": "invariants_contracts",
  "issues": [
    {
      "claim_or_decision": "§8 hello world and §4: signals are created in a new `ViewState::create(cx: &dyn LifecycleContext) -> Self` hook ('NOT in build'), and P1 says 'lifecycle is sacred'",
      "problem": "The synthesis adds a lifecycle hook that does not exist and does not record it. Today `create_state(&self)` takes no context. ADR-0074 creates signals in `init_state`, owned by the mounted element, using `Option<Signal>`. A `create(cx)` that returns `Self` would have to run after the element exists, so that the slot can be element-owned and released on unmount, and before `init_state`. That merges Flutter's createState and initState. It also changes what the ADR-0078 rule 'capabilities only in init_state/did_change_dependencies' covers, since it adds a third hook that receives LifecycleContext. The hook also gets no `&View`, so state cannot be seeded from the widget config. None of D1-D16 records this, and it is the P1 contract the synthesis calls sacred.",
      "evidence": "crates/flui-view/src/binding.rs:1866 `fn create_state(&self) -> Self::State` (no context); crates/flui-view/src/element/future_builder.rs:299 `fn init_state(&mut self, ctx: &dyn LifecycleContext)`; docs/adr/ADR-0074-realm-scoped-signals.md:121-126 ('cx.signal(value) from init_state ... owned by that element'), :236-240 (Option<Signal> idiom); AGENTS.md rule table row 1 (init_state/did_change_dependencies only).",
      "severity": "major",
      "fix": "Keep ADR-0074's `init_state` idiom in the sketches. Alternatively, add a decision record for a `create(cx, &View)` hook that states its ordering relative to mount, init_state and did_change_dependencies, supersedes ADR-0078's hook list, and has a test that asserts the ordering."
    },
    {
      "claim_or_decision": "§4: the `&mut Writer` token 'turns ADR-0074's run-time guard into a compile error'; the effects phase runs as input → build → effects → layout; 'invalidation is coalesced per frame'",
      "problem": "The synthesis treats the effects phase as settled, but ADR-0075 is still Proposed. It lists unresolved requirements, including #6: a LayoutBuilder-scoped drain in the same frame can absorb an effect's write, so 'lands in the next frame' is false. The synthesis makes this worse, because §3.3 moves lazy-child builds inside sliver layout, which adds more in-layout build drains. The synthesis never cites ADR-0075. The Writer-token claim also skips writes from computations: ADR-0075 requirement 4, WrittenDuringCompute, needs a run-time guard. It also skips nested synchronous callbacks fired during build. The run-time guard therefore cannot be removed, and ADR-0078 calls it authoritative.",
      "evidence": "docs/adr/ADR-0075-derived-state-and-effects.md:3 (Status: Proposed), :34 (build → effects → layout), :58-63 (req 5 wake-only path, req 6 same-frame absorb), :52-54 (WrittenDuringCompute); docs/adr/ADR-0078-rules-live-in-types-and-lints.md:70 (run-time guard authoritative); synthesis §3.3 lazy children built inside layout.",
      "severity": "major",
      "fix": "Make ADR-0075 the vehicle for the effects phase and Writer. State which builds inside layout may observe an effect write. Keep the run-time guard as the backstop, and describe the Writer as an ergonomic narrowing, not a replacement."
    },
    {
      "claim_or_decision": "§8 Forms sketch: `Router::of(w).push(AppRoute::Home)`, which uses the Writer as the lookup context",
      "problem": "An inherited lookup (`X::of`) resolves by the element's position in the tree. A Writer carries no element identity, so `Router::of(w)` either reaches a realm-global or primary-presentation router, or it cannot be implemented. With multiple presentations or nested routers, this reproduces the defect the synthesis itself cites: SignalWrite and NavigatorCommand already target the primary presentation. It silently breaks the Flutter `Navigator.of(context)` nearest-ancestor contract.",
      "evidence": "crates/flui-app/src/app/ui_realm/presentations.rs:357-358 (`widgets()` = `self.presentations.primary().widgets()`); crates/flui-app/src/app/ui_realm/commands.rs:450-455 (SignalWrite routed through that); crates/flui-widgets/src/navigator/navigator.rs static NAVIGATOR_COMMAND_TARGETS (global command-target table).",
      "severity": "minor",
      "fix": "Capture a router handle in `init_state` (`Router::handle(cx)`) and use it from the callback. Otherwise, pass a positioned context alongside the Writer. Record that navigation targets the nearest ancestor router."
    },
    {
      "claim_or_decision": "§1 P3/§10: the `cargo xtask globals` gate scans `static` of Mutex/RefCell/OnceLock/LazyLock and `thread_local!`, and lists TIME_DILATION among the violations; §3.2: OwnerHost 'owned value, replaces thread_local APP_RUNTIME'",
      "problem": "As specified, the gate would not catch one of its own listed violations. TIME_DILATION is a plain `static AtomicU64` holding process-wide mutable configuration, not an ID counter. Atomic statics are outside the scan patterns. The global inventory is also incomplete: it omits the loop-owned TLS PENDING_SECONDARY_WINDOW_OPENS and PENDING_SECONDARY_WINDOW_COMPLETIONS, the hot-reload REQUEST_REBUILD LazyLock<Mutex>, the assets INTERNER, and the platform callback statics (macOS SINK/Q, iOS DELEGATE_STATE, windows OWNER). Replacing APP_RUNTIME with an owned value is asserted without saying how OS callback trampolines reach the host. host.rs states the realm stays in TLS because the platform callback surface requires it. Some ambient reach is therefore permanent on AppKit/Win32, and 'no global state' needs an explicit carve-out, not an implied zero.",
      "evidence": "crates/flui-scheduler/src/config.rs:43,94 (static TIME_DILATION: AtomicU64, store()); crates/flui-app/src/app/runner/secondary_window.rs:459,469; crates/flui-hot-reload/src/dispatch.rs:24; crates/flui-app/src/app/runner/host.rs:25-47 (comment: 'the !Send realm this holds remains in owner TLS until that seam is retired'); grep of static Atomic* shows 24 atomic statics, including KEY_COUNTER/UNIQUE_KEY_COUNTER (flui-foundation/src/key.rs) and GlobalKey COUNTER.",
      "severity": "major",
      "fix": "Have the gate scan every `static` whose type is not a pure monotonic ID counter, including Atomic* holding configuration. Seed the allowlist from an actual scan, not the prose list. Record a named, permanent exception class for OS-callback trampolines, stating which single TLS cell they reach."
    },
    {
      "claim_or_decision": "D16: 'flui-protocol primary, MCP a projection ... Amends ADR-0080'",
      "problem": "This is a reversal of ADR-0080, not an amendment. ADR-0080 decides that MCP is the transport, AccessKit the vocabulary, and a spec of FLUI's own only for what MCP lacks. It leaves only the in-process transport open. Making a custom protocol primary with MCP as a projection changes the accepted contract. Under the ADR policy that needs a new ADR with Supersedes, or the code will silently disagree with ADR-0080.",
      "evidence": "docs/adr/ADR-0080-agent-protocol-desktop-contract.md:10-14 ('MCP as the transport, AccessKit as the vocabulary ... a specification of our own only for what MCP lacks'), :131-136 ('Not decided here: The in-process backend's transport').",
      "severity": "minor",
      "fix": "Write the protocol ADR as `Supersedes: ADR-0080` (the transport clause), with Superseded-by added to ADR-0080."
    },
    {
      "claim_or_decision": "§11 item 8 and the H0 exit: the in-process backend and the UIA backend 'must produce identical outlines' on windows-latest",
      "problem": "Hypothesis, not verified: this invariant is probably unattainable as stated. UIA exposes a smaller, lossy set of control types. The desktop-mcp role enum is a reduced AccessKit subset with a `native_role` fallback. The OS tree also contains frame and title-bar nodes that the realm's semantics tree lacks. Pinning 'identical' outlines would force either a lossy normalization, which is then the real contract and is undefined, or a flaky gate.",
      "evidence": "tools/desktop-mcp/src/a11y/role.rs:11-40 ('The native name (Node::native_role) is reported alongside, so a role this list does not cover is still visible'); the enum holds about 25 roles against AccessKit's full Role set.",
      "severity": "minor",
      "fix": "Define a normalized projection: roles folded to the UIA-expressible subset, OS chrome filtered. Assert equality of that projection, and record the projection in the protocol ADR."
    },
    {
      "claim_or_decision": "D13: 'ADR-0045 accepted as mode-agnostic; Inline permanent on macOS and wasm'",
      "problem": "ADR-0045 is Proposed, and its acceptance condition is that surface acquisition and present run on the raster thread. Declaring it accepted while making macOS and wasm permanently inline rewrites the acceptance criterion. That change needs a superseding or revised ADR, not a status flip, or the ADR text will contradict the code on two platforms.",
      "evidence": "docs/adr/ADR-0045-raster-lane.md:3 ('Status: Proposed — accepted when surface acquisition and present run on the raster thread and ...').",
      "severity": "minor",
      "fix": "Revise ADR-0045 explicitly: a new acceptance criterion per platform, with the macOS wgpu-hal pin (#653) as the recorded reason."
    },
    {
      "claim_or_decision": "§1: 'ADR-0041's no flui-runtime until two entry points is already satisfied' by HeadlessBinding::pump_frame",
      "problem": "ADR-0041's gate needs a managed entry point and an embedded or host-driven one, plus a measurable dependency reduction for a consumer. A test harness is neither an embedder nor a production consumer, and AGENTS.md says test callers do not count as wiring. The gate is not satisfied. It is being overridden, which is legitimate but must be argued as a supersession, not presented as a fact.",
      "evidence": "docs/adr/ADR-0041-workspace-topology-contract.md:73-77; AGENTS.md Review guidelines 'Unwired surface ... test, example and bench callers don't count'.",
      "severity": "minor",
      "fix": "Word D2 as superseding ADR-0041's runtime gate. Give the reason (a duplicated transaction) and name the measurable dependency reduction, for example flui-testing no longer re-implementing the frame."
    },
    {
      "claim_or_decision": "§1/D4: ADR-0074's placement 'puts the graph per BuildOwner' and so needs superseding",
      "problem": "The framing is wrong, though the conclusion is right. ADR-0074 is titled and accepted as realm-scoped ('the realm's graph ... dropped with the realm'). The per-presentation graph, where SignalWrite always goes to the primary presentation, is code disagreeing with an accepted ADR. By the repo's ADR policy that is a defect to fix in code, plus a clarification of the ambiguous 'lives beside BuildOwner' sentence, not a supersession. Framing it as a supersession hides an existing contract violation.",
      "evidence": "docs/adr/ADR-0074-realm-scoped-signals.md:1, :127-130; crates/flui-app/src/app/ui_realm/commands.rs:450-455 + presentations.rs:357-358 (primary presentation only).",
      "severity": "minor",
      "fix": "Record this as a current ADR-0074 conformance defect, with a test that fails today: a signal written from window B re-renders a reader in window B. Supersede only the 'foundation module' placement detail if that is really new."
    }
  ],
  "confirmed_strong_points": [
    "ID claims check out: ElementId is a generational NonZeroU64 (crates/flui-foundation/src/id.rs:1163), and RenderId, RealmId and DataTransferId are GenId. LayerId, SemanticsId and ViewId are plain reusable-slab `Id` (id.rs:640-715 `plain:` section). The AGENTS.md:138 'ID offset' row really does contradict the code, which it describes as 1-based NonZeroUsize for ElementId/RenderId. Moving LayerId and SemanticsId to GenId is justified.",
    "ADR-0027's 'Multiple realms may execute concurrently' (ADR-0027:17-18) is contradicted by the single thread_local APP_RUNTIME that hosts all realms (crates/flui-app/src/app/runner/host.rs:25-47). Amending it to 'isolated, not concurrent' is correct.",
    "ADR-0078 does say 'A new capability is a method on LifecycleContext' and LifecycleContext is sealed (ADR-0078:64-65). There are 122 `dyn LifecycleContext` occurrences (grep count), so the object-safe erased method plus Ext trait is the right shape and keeps build-phase safety.",
    "The SignalWrite command resolves the graph through the primary presentation only (commands.rs:450-455 → presentations.rs:357-358), which confirms the cross-window signal defect.",
    "RenderObject<P> has no Send bound (crates/flui-rendering/src/traits/render_object.rs:178), yet `RenderView::RenderObject` requires `Send + Sync` (crates/flui-view/src/view/render.rs:451). That confirms the S2/!Send-flip inconsistency the synthesis targets.",
    "The globals the synthesis names exist: FONT_SYSTEM, the decode CACHE, ERROR_VIEW_BUILDER, TIME_DILATION (flui-scheduler/src/config.rs:43), AssetRegistry::global (flui-assets/src/registry/mod.rs:83), APP_RUNTIME, NAVIGATOR_COMMAND_TARGETS, the interaction LOCAL/ACTIVE_LANES and REGISTRY_STACK (flui-view/src/key/registry.rs:204). No runtime-contract.toml is tracked (git ls-files).",
    "The effects-phase ordering build → effects → layout matches ADR-0075:34, and the synthesis correctly refuses signal creation in build, consistent with ADR-0074:121-126."
  ]
}
```

# verify_concurrency_perf

```json
{
  "lens": "concurrency_perf",
  "issues": [
    {
      "claim_or_decision": "§2.1/§2.4 and D1/§9: official packages (Material, Cupertino, devtools, mcp, hot-reload) may depend only on `flui` + platform-api + protocol, while 'a Win32 backend edit rebuilds only platform, app and the facade' and 'plugins depend on about 30 crates, not about 198'.",
      "problem": "The facade depends on flui-app, flui-engine and wgpu, and nothing in §2.5/§2.6 puts those behind a feature. A package that depends only on `flui` therefore gets the runners, the backends, wgpu and naga as normal dependencies. That is a compile-time regression for every package. It also means every platform-backend or engine edit rebuilds all packages, which contradicts the stated 3-crate rebuild target and the ~30-crate plugin graph. The ~30 figure can only hold for a provider crate that depends on platform-api alone. The app-facing half of a plugin needs `LifecycleContextExt`, which the synthesis places in flui-view and so is reached through the facade.",
      "evidence": "`cargo tree -e normal` unique-crate counts: flui 191, flui-material 127. `cargo tree -p flui-material -i wgpu/naga/flui-app/flui-engine` returns nothing today, while `-p flui -i <each>` hits all four. So the rule 'packages depend on flui' adds wgpu, naga, the engine and the app to Material's graph.",
      "severity": "major",
      "fix": "Split the facade's authoring surface from its host surface. Either (a) put `flui-app`/`flui-engine` behind a default-on `runtime`/`host` facade feature that packages disable with `default-features = false`, or (b) let official packages depend on an authoring facade such as `flui::sdk` re-exported from a host-free crate. Add a `forbid-reach` fact: packages must not reach wgpu, flui-engine, flui-app or any OS crate. Re-measure the rebuild fan-out with `cargo build --timings` before claiming 3 crates."
    },
    {
      "claim_or_decision": "§3.3/§9: 'layout, with lazy children built *inside* sliver layout, replacing the up-to-10/6-pass fixpoint', with a per-PR budget of '1 layout pass', together with §3.1 'PipelineOwner<Idle>::set_children … enforce arity and depth' as the only way to change topology.",
      "problem": "These contradict each other. Building a child inside sliver layout mounts elements and inserts render objects while the pipeline is in the Layout typestate and the `Rc<RefCell<PipelineOwner>>` is mutably borrowed. §3.1, however, allows topology mutation only on `PipelineOwner<Idle>`. The synthesis gives no mechanism for this reentrancy: an invokeLayoutCallback-style scoped mutation API, a way for rendering to call back into flui-view without an upward edge, or a RefCell borrow discipline. Without one, 'inside layout' is either a RefCell double-borrow panic or a quiet return of the fixpoint, and the '1 pass' budget is not reachable. Today's code is explicitly built around between-pass servicing for this reason.",
      "evidence": "crates/flui-rendering/src/pipeline/owner/cell.rs:51 `pub struct PipelineCell(Rc<RefCell<PipelineOwner>>)`; crates/flui-rendering/src/pipeline/owner/actions.rs:12 `impl PipelineOwner<Idle>`; crates/flui-view/src/owner/build_owner.rs:2233-2264 `service_child_requests(&mut self, tree: &mut ElementTree, pipeline: &PipelineCell)` and `service_child_requests_between_passes`; crates/flui-view/src/owner/layout_builder.rs:64,74 (MAX_LAYOUT_BUILD_PASSES=10, MAX_LAZY_BAND_PASSES=6).",
      "severity": "major",
      "fix": "Specify the mechanism before promising the budget. One option is a `LayoutCallbackScope` token on `PipelineOwner<Layout>` that allows subtree-local `set_children` under the laid-out parent only (Flutter's invokeLayoutCallback contract), reaching the element side through an object-safe `ChildManager` handle that the runtime installs (no upward crate edge). Pin it with a test that a lazy band settles in 1 pass. Until then, state the budget as 'passes ≤ N, trending to 1'."
    },
    {
      "claim_or_decision": "§5.3/§5.4: one `GpuContext` per app owning one device/queue, pipeline cache and a shared glyph atlas, while §3.5 threads the raster lane on Win32/Linux.",
      "problem": "Raster lanes are per window today (one `RasterLane`/`RasterOwner` per surface address). If N threaded lanes share one GpuContext-owned glyph and image atlas, the atlas needs a lock or a single raster thread. That is a new contended lock on the raster frame path. The synthesis only removes the shaping lock and says nothing about how atlas or queue submission is synchronized across lanes, or whether there is one raster thread per app. It also leaves open how the §17 'one stalled window' spike interacts with a shared atlas. (Hypothesis on the contention cost; the missing ownership decision itself is verified.)",
      "evidence": "crates/flui-app/src/app/raster_lane.rs:1-40 (lane wraps one RasterOwner, keyed by a surface address; tests at :586-768 construct `RasterLane::new(backend, test_address(), 640, 480)` per surface); synthesis §5.4 'glyph and image atlases' inside GpuContext, §3.5 'threaded on Win32 and Linux'.",
      "severity": "major",
      "fix": "Decide in the ADR-0045 addendum whether there is one raster thread per GpuContext that serves all presentations (atlas stays single-owner and lock-free), or per-window lanes with per-lane atlas staging plus an owner-merged upload. Add a counter or bench for atlas contention to the §17 multi-window spike."
    },
    {
      "claim_or_decision": "§9: 'Deterministic counts are gated per PR on the virtual clock', with a budgets table (idle 0 frames, layout nodes ≤ band, elements built ≤ 3, 0 relayout on opacity animation) and `cargo xtask perf` using `pump_counted()`.",
      "problem": "None of the counting infrastructure exists, and none of it is scheduled. No `pump_counted` or per-phase node counters exist in rendering, view or testing, and bench-collect skips the GPU damage bench that backs the 2901/56 µs baseline. Nor does the W1-W8 plan (§15) contain a perf-harness item, so the budgets are not measurable during H0. The §14/§15 structural cuts (runtime extraction, !Send flip, retained layers) would then land with no regression gate on the invariant they are meant to deliver.",
      "evidence": "grep for `pump_counted|layout_count|nodes_laid_out|PipelineStats` in crates/*/src matches only flui-devtools and build_owner.rs; tools/xtask/src/bench.rs:36-38 'Targets with `required-features` (the GPU readback benches) are skipped'; docs/adr/ADR-0061-damage-needs-layer-identity.md:31 (2901 µs / 56 µs baseline); §15 wave table has no perf item.",
      "severity": "major",
      "fix": "Add 'phase counters in PipelineOwner/BuildOwner plus `cargo xtask perf` with idle and 10k-list scenarios' to W1 next to the other gates. Make counter tests a ratchet (record current counts, allow only decreases) so the runtime extraction and the !Send flip are measured against them."
    },
    {
      "claim_or_decision": "§3.2: 'Intra-realm parallel layout is a non-goal… The types already exclude it: `RenderObject` is not `Send` (`traits/render_object.rs:178`)'.",
      "problem": "This is only half true, and it contradicts §10 of the same synthesis. The `RenderObject` trait has no Send supertrait, but every render object created through `RenderView` must be `Send + Sync`, and `ViewportOffset` is `Send + Sync`. What actually excludes parallel layout today is `PipelineCell` being `Rc<RefCell<..>>`, not the render-object types. So the 'types already exclude it' argument does not establish that the `!Send` flip is cheap: the flip must remove these bounds, which is a break for every third-party RenderView.",
      "evidence": "crates/flui-view/src/view/render.rs:451 `type RenderObject: flui_rendering::traits::RenderObject<Self::Protocol> + Send + Sync + 'static;`; crates/flui-rendering/src/view/viewport_offset.rs:57 `pub trait ViewportOffset: Debug + Send + Sync`; crates/flui-rendering/src/traits/render_box.rs:485 metadata `Arc<dyn Any + Send + Sync>`.",
      "severity": "minor",
      "fix": "Reword §3.2 to cite `PipelineCell` (cell.rs:51) as the actual exclusion and list `RenderView::RenderObject: Send + Sync`, `ViewportOffset: Send + Sync` and the metadata `Arc<dyn Any + Send + Sync>` as items of the §14 item-9 flip, which must land before a stable `flui::rendering`."
    },
    {
      "claim_or_decision": "§5.2: the default presenter renders into 'a persistent retained target plus a blit' to get partial damage, since wgpu has no buffer age.",
      "problem": "Its cost is not budgeted. It adds a full-surface copy and a second surface-sized texture per presentation on every frame, including Full-damage frames (scroll, resize, animation). Those are exactly the frames the 100k-fling and p99 budgets measure, and on tile-based mobile GPUs a full blit is a bandwidth cost that can exceed the savings from partial raster. (Hypothesis: not measured; the benefit baseline 2901→56 µs is from a bench that bench-collect currently skips.)",
      "evidence": "Synthesis §5.2 and §9 budgets; tools/xtask/src/bench.rs:36-38 skips the required-features damage bench; crates/flui-app/src/app/raster_lane.rs:354 always emits `DamageRegion::Full` today, so there is no measured partial-damage path to compare against.",
      "severity": "minor",
      "fix": "Make the retained target conditional: skip it and render straight to the swapchain when damage is Full or covers more than a threshold of the surface. Add blit cost and extra memory per window to the §17 'Retained layer identity + differ' spike's success metric."
    },
    {
      "claim_or_decision": "§4/D4: the reactive graph core goes into flui-foundation 'so a paint or layout subscriber needs no new edge'; its blast radius is to be measured 'when it lands'.",
      "problem": "foundation is the widest fan-out crate in the stack. Putting an actively iterated graph core (Clean/Check/Dirty, intrusive slab links, write journal, store triggers) there means every edit to it rebuilds every flui crate in the facade graph. This cuts against §9's 'compile time is a performance budget'. A leaf crate just above foundation would have nearly the same consumers but would spare geometry/types/macros/scheduler edges. The P8 argument ('no new publish unit') trades a publish unit for iteration cost during exactly the H0 waves (W3) when the graph changes most.",
      "evidence": "`cargo tree -p flui -e normal -i flui-foundation` lists 17 distinct flui crates depending on foundation; `cargo tree -p flui-foundation` graph is 20 unique crates (it is the base).",
      "severity": "minor",
      "fix": "Put the core in an internal module that is gated by a build measurement: land it in its own internal crate during W3 while it churns, and fold it into foundation only if `cargo build --timings` shows no fan-out difference. Alternatively, record the expected rebuild count as an acceptance criterion in the effects ADR."
    }
  ],
  "confirmed_strong_points": [
    "Per-node locks on the frame path are real and correctly cited: ChildManagerRegistry is `Arc<Mutex<HashMap<RenderId, Arc<Mutex<dyn ChildManager>>>>>` (crates/flui-view/src/element/child_manager.rs:56), and LayoutConstraintsCell holds a `Mutex<CellState>` (crates/flui-objects/src/layout/layout_constraints_cell.rs:96).",
    "Realms are serialized, not concurrent: a single `thread_local! APP_RUNTIME: RefCell<AppRuntime>` hosts them (crates/flui-app/src/app/runner/host.rs:25-47). Amending ADR-0027 to 'isolated, not concurrent' is correct.",
    "Damage is always Full on the production path (crates/flui-app/src/app/raster_lane.rs:354), and the ADR-0061 baseline of 2901 µs vs 56 µs exists (docs/adr/ADR-0061-damage-needs-layer-identity.md:31).",
    "The scheduler is lock-heavy: `Mutex<Option<FrameTiming>>`, `Mutex<FrameBudget>`, etc. at crates/flui-scheduler/src/scheduler.rs:743-760, with 70 Mutex mentions in the file. The owner-local core plus a Send waker is justified.",
    "The process-global text lock is real: `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>` (crates/flui-painting/src/text_layout/layout.rs:124), shared with the engine glyph path. A per-realm FontContext is correctly targeted.",
    "PipelineCell is `Rc<RefCell<PipelineOwner>>` (crates/flui-rendering/src/pipeline/owner/cell.rs:51), so declaring intra-realm parallel layout a non-goal matches the actual ownership model.",
    "The raster lane already uses a mode-agnostic mailbox protocol ('threading the lane later changes who calls pump, not what a frame is', raster_lane.rs:1-13), which supports D13.",
    "cargo-deny `multiple-versions = \"allow\"` (deny.toml:103) and bench-collect skipping required-features benches (tools/xtask/src/bench.rs:36) are accurately reported.",
    "Per-crate `testing` features that chain into rendering/layer exist (flui-objects `testing = [\"flui-rendering/testing\"]`, flui-rendering `testing = [\"flui-layer/testing\"]`), so the duplicate-build hypothesis is plausible and worth the planned spike."
  ]
}
```

# verify_plan_and_dx_reality

```json
{
  "lens": "plan_and_dx_reality: does the proposed architecture carry H0-H4, and would counter / todo-form / two-screen nav / plugin compile in principle in the proposed API; is it simpler than today",
  "issues": [
    {
      "claim_or_decision": "P10 (no pre-1.0 upstream type in a Stable signature) together with D8/§6: flui-platform-api and flui-protocol are Stable, and PlatformAccessibility 'takes an accesskit::TreeUpdate', protocol uses 'accesskit::Role', NativeContext exposes 'HWND and NSWindow via raw-window-handle', and the input vocabulary is ui-events",
      "problem": "Two of the three Stable crates are specified to name the very pre-1.0 types P10 bans. As written, the tier ADR and P10 cannot both pass their own gate, the cargo-public-api snapshot. Wrapping these types is not free. It means an owned FLUI role and action enum plus a TreeUpdate DTO, which contradicts §11.1 ('AccessKit stored natively in flui-protocol', desktop-mcp's copied role enum deleted). It also means NativeContext handing out opaque raw pointers, which makes every out-of-repo plugin (the H1 exit) unsafe code with no typed seam.",
      "evidence": "Cargo.toml:144 `accesskit = \"0.25\"`, Cargo.toml:233 `raw-window-handle = \"0.6\"`, Cargo.toml:169 `objc2 = 0.6.4`, Cargo.toml:301 `ui-events = \"0.3\"`. Synthesis §2.2 table (platform-api/protocol rows), §6.1, §6.3 NativeContext, §11.1.",
      "severity": "major",
      "fix": "Choose explicitly. (a) Keep AccessKit, raw-window-handle and the like as named, versioned exceptions under P10 and accept one Stable-crate major per upstream break. Or (b) define owned FLUI vocabulary types and make NativeContext a set of per-OS typed accessors in unstable/cfg-gated modules outside the Stable promise. Record which in the tier ADR, and drop the claim that the protocol stores AccessKit natively if (b) is chosen."
    },
    {
      "claim_or_decision": "D8: 'Semver promises drop from 28 potential units to 3'; the internal tier is 'not semver-checked'; flui::rendering is Stable",
      "problem": "The H0 extension point (third-party RenderBox/RenderSliver) is a Stable promise over trait signatures defined in internal crates. RenderBox's methods take types from flui-rendering (PaintCx, BoxHitTestContext, CursorIcon, SemanticsConfiguration, RenderInvalidationHandle), flui-types (Size, Matrix4, TextBaseline), foundation (Diagnosticable) and tree (Arity). Re-exporting them through `flui` does not shrink the promise. Any change in these 'internal, exact-pinned' crates breaks the Stable facade. The '3 crates' number therefore relabels the semver surface without reducing it, and the H3 freeze covers most of V/S/R in practice.",
      "evidence": "crates/flui-rendering/src/traits/render_box.rs:3-9 (imports flui_tree::Arity, flui_types::Size, hit_testing::CursorIcon), :359 `paint(&self, ctx: &mut crate::context::PaintCx<'_, Self::Arity>)`, :485 `metadata() -> Option<Arc<dyn Any + Send + Sync>>`, :508 `&mut crate::semantics::SemanticsConfiguration`, :589 `attach(&mut self, handle: RenderInvalidationHandle)`. Synthesis §2.3 totals and D8.",
      "severity": "major",
      "fix": "State the real Stable surface as 'the transitive public-type closure of flui's Stable modules', and run the public-api snapshot over that closure, not over 3 crate names. Move the cargo-public-api re-export-depth spike (§17) ahead of W1, because D8 depends on its result. Also note that `metadata()` returning `Arc<dyn Any + Send + Sync>` contradicts S2 and must be flipped before any freeze."
    },
    {
      "claim_or_decision": "§8 hello world: `impl ViewState<Counter> for CounterState { fn create(cx: &dyn LifecycleContext) -> Self { Self { count: cx.signal(0) } } ... }` with only `#[derive(Clone, StatefulView)] struct Counter;`",
      "problem": "This does not compile against any stated plan. Today state is created by `StatefulView::create_state(&self)`, which has no context and is called when the element behaviour is constructed, before the element is mounted and before it has an ElementId. Owner-scoped signals need that ElementId (`signal_owned_by(owner: ElementId, ..)`). A `create(cx: &dyn LifecycleContext)` therefore requires moving state construction to mount time, and it drops the view config, so `NoteView(id)` cannot seed its state from props. Neither the move nor a two-phase state appears in §14. The derive also cannot infer `type State`. Today's example still needs `impl StatefulView { type State = ..; fn create_state }`.",
      "evidence": "crates/flui-view/src/view/stateful.rs:76-84 (`fn create_state(&self) -> Self::State`, no ctx), :112 init_state gets LifecycleContext; crates/flui-view/src/element/behavior.rs:693-698 (`StatefulBehavior::new(view)` calls `view.create_state()` before mount); crates/flui-view/src/reactive/mod.rs:285-288,334 (widget-owned signals are created from init_state with the element as owner); examples/counter.rs (today: StatefulView impl + StateCell + bind in init_state).",
      "severity": "major",
      "fix": "Either keep `create_state(&self)` and create signals in `init_state` (`self.count = Some(cx.signal(0))`, or a `Lazy`/`Signal::uninit` slot), or add a §14 item: 'state construction moves to mount: `fn create(view: &V, cx: &dyn LifecycleContext) -> Self`', with the element-construction reorder and its lifecycle-ordering test. Fix the sketch to show `type State` or document what the derive infers."
    },
    {
      "claim_or_decision": "§4/§8 Writer token: `on_press(move |w| ...)`, `.on_press(move |w| if signup.validate(w) { Router::of(w).push(AppRoute::Home) })`",
      "problem": "(1) `Router::of(w)` is an inherited-ancestor lookup, which needs a tree position. A `&mut Writer` handed to a callback has none, so this cannot resolve which Router, for example with nested routers or two windows. The router handle has to be resolved in build (`let router = Router::<AppRoute>::of(cx)`) and captured, and `of` needs the route type parameter. (2) The Writer changes the signature of every public callback in the catalog, from `Fn()`/`Fn(T)` to `Fn(&mut Writer, ..)`. That affects 91 `pub fn on_*` in widgets/material/cupertino plus every recognizer in flui-interaction. This breaking change is missing from §14 (item 7 only says 'Writer'). (3) The claim 'turns the run-time guard into a compile error' holds only if no BuildContext path yields a Writer. Today `BuildContext::reactive()` hands the writable graph to build, and it has to be removed as well.",
      "evidence": "crates/flui-widgets/src/interaction/gesture_detector.rs:225 `on_tap(callback: impl Fn() + 'static)`; crates/flui-material/src/elevated_button.rs:71 `on_pressed(impl Fn() + 'static)`; `grep -rhoE 'pub fn on_[a-z_]+\\(' crates/flui-{widgets,material,cupertino}/src | wc -l` → 91; crates/flui-view/src/context/build_context.rs:132 `fn reactive(&self) -> Reactive` on BuildContext; reactive/mod.rs:783 `update(self, r: &Reactive, ..)`.",
      "severity": "major",
      "fix": "Rewrite the nav sketch to resolve the router in build and capture a Copy handle. Add an explicit §14 item for the callback-signature migration with a `flui migrate` rule, and remove `BuildContext::reactive()` in the same change. Compare the new counter and todo against today's honestly: the proposed counter is the same length as today's StateCell version and adds a parameter to every closure. The gain is compile-time write safety, not brevity."
    },
    {
      "claim_or_decision": "§2.2/§2.4: `official` crates may have exactly these normal in-repo dependencies: flui, flui-platform-api, flui-protocol; flui-a2ui is official and 'reads the existing catalog'; Material ported to flui + sdk is the H0 seam proof",
      "problem": "For the H1 exit (a Notes form arrives through A2UI, rendered with Material), flui-a2ui has to construct Material components from JSON. That needs either a flui-a2ui → flui-material edge, which the official rule forbids, or a runtime catalog registry that Material populates. Rust has no link-time auto-registration without inventory/linkme, so the app must register it by hand. The same applies to plugin 'endorsed defaults from target dependencies': a cfg'd dependency selects code but registers nothing, so every plugin still needs `App::new(..).capability::<C>(provider)` in user code. Neither mechanism is specified, and both carry the 'plugin is just a crate' and 'same catalog, three consumers' promises.",
      "evidence": "Synthesis §2.1 'official crates may have exactly these normal in-repo dependencies', §6.3 'The endorsed default comes from [target.cfg] dependencies ... override hook', §7 H1 row flui-a2ui; plan.md:35 ('плагин — обычный crate с #[cfg(target_os)]'), plan.md:16 (H1 exit: form from the model through A2UI).",
      "severity": "major",
      "fix": "Specify the registration mechanism once: an explicit `App::with(plugin)`/`App::catalog(material::catalog())` builder, or a `linkme` distributed slice with its wasm and static-lib caveats. Allow official→official edges where declared (a2ui → material), or make A2UI catalogs values passed in by the app."
    },
    {
      "claim_or_decision": "H1/H2 external GPU content (plan: 'wgpu texture / third-party renderer as a widget') via Layer::External + TextureRegistry, while flui-engine drops `pub use ::wgpu` and wgpu interop sits behind `unstable-wgpu-interop`",
      "problem": "The only way a user can hand FLUI a wgpu texture is an unstable feature on an internal crate. Under the tier model it can never reach Stable while wgpu is in the signature. The plan's H2 extension point therefore has no Stable home, and the synthesis does not say it is Experimental forever. Separately, P10 lists 'wgpu 30' as pre-1.0, but wgpu is at 30.0. The real concern is major-version churn, not pre-1.0, so the rule's stated criterion is wrong.",
      "evidence": "Cargo.toml:224 `wgpu = { version = \"30.0\" ...}`; synthesis §2.2 flui-engine row ('wgpu interop behind unstable-wgpu-interop'), P10 list, §7 external GPU row.",
      "severity": "minor",
      "fix": "Rephrase P10 as 'no upstream type whose major changes more often than our Stable cadence'. Declare external-GPU interop a permanently Evolving `flui::sdk::gpu` module that re-pins its wgpu major on each train, and document that in the H2 exit."
    },
    {
      "claim_or_decision": "D7: official packages live in `packages/` of this repo, not separate repos (overrides plan.md:31)",
      "problem": "This overrides an explicit owner decision in the plan that the user asked the review to follow. In the same workspace, `packages/*` resolve `flui` by path, so the 'Material compiled against flui + sdk proves the seam' check runs against unreleased HEAD, not against a published train. The H0 proof then does not show what an out-of-repo package experiences. **(hypothesis:** would need the package manifests to confirm path resolution.)",
      "evidence": "plan.md:31 ('Официальные пакеты (flui-* в отдельных репо, один релизный поезд)'); synthesis §1 departures, D7, §2.4.",
      "severity": "minor",
      "fix": "Present D7 to the owner as a proposed amendment, not a decision. If adopted, put packages/ in a separate cargo workspace that depends on `flui` by version via `[patch]` only in dev, and have CI build it against the last published train."
    }
  ],
  "confirmed_strong_points": [
    "The object-safe capability seam (erased method plus an Ext trait) is right: BuildContext and LifecycleContext are sealed and object-safe today (crates/flui-view/src/context/build_context.rs:17-19,106,377), so a generic trait method would break the `&dyn` call sites. The Ext pattern compiles in principle (`Rc<dyn Any>` downcast to `C::Handle: Clone + 'static`).",
    "Signal<T> is already Copy (reactive/mod.rs:656-661), so `let count = self.count; move |..| count.update(..)` capturing into `'static` non-Send callbacks compiles. Today's callbacks are already `impl Fn() + 'static` without Send (gesture_detector.rs:225, elevated_button.rs:71), so the '!Send callbacks' convention matches most of the catalog. The exception is flui-widgets/src/semantics/mod.rs:354, which is `Send + Sync` and is correctly flagged.",
    "Rejecting `cx.signal(0)` inside build matches the current run-time guard (reactive/mod.rs:30-37 CreatedDuringBuild/WrittenDuringBuild).",
    "The signals feature is real (crates/flui-view/Cargo.toml:123; Cargo.toml:645), so 'signals are not a feature' is a concrete, needed change.",
    "The facade re-export of android_activity is real (src/lib.rs:157) and correctly identified as a Stable-surface leak.",
    "The SignalSender cross-thread path already exists (reactive/mod.rs:810-820), so the plugin and IO-lane result delivery sketched in §3.5 builds on shipped code."
  ]
}
```

