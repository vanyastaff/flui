# Codebase map: widgets_catalogs_facade: flui-widgets, flui-material, flui-cupertino, flui-localizations, facade (src/, prelude, features), examples/

_Raw output of the `map:widgets_catalogs_facade` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How it works today (main cab06137d). flui-widgets (layer 6) is one crate of about 80k lines. That count includes about 14.3k lines of *_tests.rs/tests.rs files under src/ and 163 inline #[cfg(test)] blocks, so the audit's "82.7k non-test" figure is probably inflated (hypothesis). By size: navigator/ 27.5k (about 14.7k production), interaction/ 12.8k (focus, drag, gestures, shortcuts), text/ 8.6k (EditableText 4.7k, controller 3.0k), scroll/ 7.7k, layout/ 4.4k, overlay/ 2.9k, animated/, image/ (with a process-global decode cache), app/ (WidgetsApp, MediaQuery, InheritedTheme), localization/ (1.6k, sync-only contracts), and widget_state.rs (WidgetState/WidgetStateProperty). The crate root re-exports about 250 names flat and has a curated prelude that globs flui_view::prelude::*.

The modules are loosely coupled today. navigator uses overlay and animated. text uses interaction. scroll uses paint, localization and animated. app uses navigator and text. No gate enforces any of this. The 2026-09-23 decision replaced the crate split (A1) with an import-direction check in CI, and that check does not exist in xtask.

The `__private` seam (4 re-exports) was built for sibling crates (scrolling, navigation, text-editing) that were then rejected. Its only users are flui-widgets' own modules plus one test.

flui-material (layer 7, 26.9k lines) is a real M3 catalog: 41 modules, ThemeData with per-component theme fields, InkWell, the Material surface, Scaffold/AppBar/tabs/dialogs/snackbars/DataTable/TextField. It owns the ink, elevation and surface substrate. flui-cupertino (4.3k) has a theme, a button, a nav bar, page and tab scaffolds, and a route. flui-localizations (281 lines, layer 8 alone) supplies only an RTL language table for WidgetsLocalizations.

Neither design system uses __private or any doc(hidden) item. Both are written against the ordinary public API of 6-9 workspace crates (widgets, view, rendering, objects, interaction, scheduler, animation, foundation, types), every one pinned with an exact `=0.2.0-dev` version. ADR-0028 is enforced through `allowed-dependents` in the manifests, and a grep finds no code edge from core into a design system, only doc mentions.

The facade `flui` (layer 10) re-exports whole crates as modules (types, geometry, foundation, view, widgets, animation, app), curated authoring modules (rendering, painting, interaction, testing), and the optional catalogs. Its default feature is `material`, and the default prelude includes about 40 Material names. `TextField` in the prelude is the Material one; the base widget is `RawTextField`. `cargo xtask facade-combos` builds 12 isolated feature combinations.

examples/ mixes three kinds of program: about 8 facade-only author demos, engine/platform integration probes that import flui_engine, flui_layer and flui_platform directly, and device-check probes. All of them ship in the published `flui` archive (`include = /examples/**`).

The structural verdict. The ADR-0028 core-never-depends-on-a-design-system rule holds. But the owner's delivery model (Material and Cupertino as official packages in separate repos, raw primitives in core, themes as data, a machine-readable catalog for A2UI, stability tiers) is not yet reflected in the topology or the API surface:
- the design systems sit below the runtime and inside the default facade;
- they reach into the render layer;
- the shared substrate and the raw primitives ADR-0028 promises mostly do not exist;
- nothing in the catalog prepares for the H1 descriptor/A2UI layer or for the H3 API-tier split.

## Responsibilities and boundaries

flui-widgets owns the declarative, design-neutral catalog: layout, paint, interaction/focus, text editing, scrolling/slivers, navigator/overlay/hero, the localization contracts, WidgetState, InheritedTheme, WidgetsApp, and the shared test harness (feature `testing`).

flui-material and flui-cupertino own design opinion: tokens, themes and skinned components. In practice Material also owns mechanism that ADR-0028 places below both design systems:
- ink and the state overlay (InkWell composes Focus, Actions and GestureDetector);
- the elevation, clip and shadow surface (`Material` implements RenderView over flui_objects::RenderPhysicalShape);
- the button activation and a11y wiring.
Cupertino duplicates part of that mechanism and lacks the rest (no focus or keyboard activation).

flui-localizations owns global l10n implementations but in practice holds one RTL table. The per-language catalogs and ICU belong in the H1 i18n official package.

The facade should be the curated, tiered author surface. It actually re-exports whole crates, including flui_app's embedder and bindings modules and flui_widgets' `__private`. So it cannot mark tiers, and Material is part of the default prelude.

Where the boundary leaks:
- **Runtime into the catalog.** flui-app, the runtime at layer 9, depends on flui-widgets and hardwires catalog widgets as root scopes: FocusRoot, GestureArenaScope and VsyncScope (ui_realm/attach.rs:6), MediaQuery (media_query_root.rs:9), and NavigatorCommand in the realm command channel (ui_realm/commands.rs:8). Runtime contracts therefore live in the catalog.
- **Catalog into the render layer.** SlideTransition's public signature needs flui_objects::TranslationFraction, which neither widgets nor the facade re-exports.
- **Platform policy in a core widget.** EditableText branches on TargetPlatform::current() at compile time.

What belongs elsewhere:
- the image decode cache → flui-assets or realm-owned;
- the root-scope widgets that flui-app mounts → a runtime-facing binding module below the catalog;
- the ink/elevation/surface substrate and the raw button/toggle behaviour → flui-widgets;
- Material, Cupertino and the l10n catalogs → above the runtime, off the facade default, eventually out of the repo.

## Key types and contracts

- flui_widgets::__private — doc-hidden, semver-exempt seam: generic_render_view_element!, AnchoredBox, enclosing_focus_parent, install_rect_provider, SaltingChildKey (crates/flui-widgets/src/__private.rs). Only internal consumers.
- flui_widgets::prelude — globs flui_view::prelude::* plus about 200 catalog names, gesture/pointer types, BoxConstraints and flui_types values (crates/flui-widgets/src/lib.rs:292-376).
- flui::prelude — flui_widgets::prelude::* + run_app/AppConfig/open_window + ~40 flui_material names under cfg(feature="material"); Material `TextField` wins, base widget is `RawTextField` (src/lib.rs:215-278).
- Facade features: default=["material"], cupertino, localizations, hot-reload, a11y, serde, signals (opt-in), testing, gpu-readback-tests (Cargo.toml:591-660); isolated combos in tools/xtask/src/tasks/facade.rs:19-30.
- ADR-0028 design-system decoupling: allowed-dependents = [flui-localizations, flui-app, flui] in flui-material/flui-cupertino [package.metadata.flui], checked by tools/xtask/src/workspace.rs:237-259.
- Layer table: widgets/testing/hot-reload=6, material/cupertino=7, localizations=8, app/cli/devtools=9, facade=10 (root Cargo.toml:86-102).
- InheritedTheme (flui-widgets) implemented by flui_material::Theme and CupertinoTheme; Theme::of / depend_on_fields with ThemeData::FIELD_* masks (flui-material/src/theme.rs:68-110); ThemeData is #[non_exhaustive] with pub Option<*ThemeData> component fields (theme_data.rs:750-791).
- WidgetState/WidgetStateProperty/WidgetStatesController in flui-widgets (widget_state.rs), consumed by Material InkWell and buttons.
- LocalizationsDelegate/WidgetsLocalizations/BoxedLocalizationsDelegate (flui-widgets localization/, sync-only) implemented by flui_localizations::GlobalWidgetsLocalizationsDelegate.
- RawTextField / EditableText / TextEditingController: the only 'Raw' primitive in the catalog (grep `pub struct Raw` → RawTextField only).
- PageRoute::back_gesture(bool) (navigator/page_route.rs:324): core behaviour that Cupertino only switches on (cupertino/route.rs:144); the raw-primitive pattern done right.
- flui_widgets::testing (feature `testing`) — shared LaidOut/harness reused by material/cupertino tests and re-exported as flui::testing::widgets.

## Dependencies

Outgoing, normal edges:
- **flui-widgets** → flui-view, flui-objects, flui-rendering, flui-animation, flui-scheduler, flui-foundation, flui-interaction, flui-types, flui-geometry, plus futures-core, bitflags, unicode-segmentation, parking_lot, thiserror, tracing. Optional: image, flui-assets (the asset-images/network-images features; flui-assets runs a background tokio runtime), futures-util, lru, flui-testing and flui-platform (both only under `testing`).
- **flui-material** → flui-widgets, flui-view, flui-types, flui-objects (RenderPhysicalShape, PathClipConfiguration in material.rs:83), flui-rendering (BoxConstraints, Canvas, RenderUpdateImpact, the RenderView protocol), flui-foundation, flui-animation, flui-interaction, flui-scheduler (LocalPostFrameHandle in scaffold_messenger.rs:212), tracing.
- **flui-cupertino** → flui-view, flui-widgets, flui-objects (only for TranslationFraction), flui-foundation, flui-types, flui-animation, tracing.
- **flui-localizations** → flui-widgets, flui-types.
- **Facade flui** → macros, types, geometry, foundation, view, widgets, animation, app, tree, painting, rendering, interaction, plus optional testing, material, cupertino, localizations, hot-reload. Its dev-dependencies pull in engine (dx12), platform, layer, objects, wgpu, image and objc2 for the examples.

All in-repo edges use exact `=0.2.0-dev` pins.

Incoming edges:
- flui-app (layer 9) depends on flui-widgets for root scopes and NavigatorCommand. It is allowlisted to depend on Material/Cupertino but does not.
- The facade and examples depend on everything here. No other core crate depends on the design systems.

## Fit with the plan

**H0 (beta).**
- The catalog has the right shape for Notes (navigator, text, scroll, Material skin, shared harness).
- The A1 replacement is not implemented: there is no module import-direction gate and no concept→module map.
- The table-stakes gaps (Form, Slider, Dropdown, multiline editor) must land as raw primitives plus a skin. Today only RawTextField exists as a raw primitive; button, toggle and ink behaviour live inside Material and are not reused by Cupertino.
- The Router (A3/D1) lands on top of a 27k-line imperative Navigator that flui-app's command channel already names. That raises the cost of making Router primary.
- The custom render object extension point is reachable through flui::rendering, but no example or test builds a third-party catalog crate against `flui` alone.

**H1 (mobile, PlatformCapability, A2UI, themes as data, i18n).**
- Nothing in the catalog carries a descriptor or serde layer. There is no widget registry like RENDER_OBJECT_TYPES, ThemeData is not serializable, and props are closures and builders. G6 and flui-a2ui therefore need a new layer.
- flui-localizations is a 281-line stub in its own layer. It is planned to depend on the in-repo design systems, which would pull the i18n official package back into the repo.
- EditableText's compile-time TargetPlatform branch conflicts with the capability-seam rule and with web/mobile runtime detection.

**H2 (perf and scale).** The process-global decode cache and whole-ThemeData clones in Theme::of (29 call sites) are small, but they are the kind of state that complicates multi-window and realm isolation.

**H3 (API freeze, tiers).**
- The facade re-exports whole crates and puts Material in the default prelude.
- `__private` is pub on a published crate.
- There is no public-API baseline or semver-checks tool in xtask.
- Callback Send/Sync policy is inconsistent.
Each of these has to be settled before a Stable/Evolving split can be expressed.

**Delivery layers (core / official / community).**
- Topology contradicts the plan: the design systems sit at layers 7-8, below the runtime (9) and inside the core facade's default feature.
- Material depends on render-layer crates the facade does not expose.
- No ADR records "official packages in separate repos".
- Moving Material or Cupertino out today is feasible in the narrow sense, since they use no private or doc-hidden API. But it would force the public surface of about 9 crates into the stable contract, and it would make the core `flui` release depend on an out-of-repo crate unless the facade's default and prelude change.

**Principles.**
- No-global-state (P3) is violated by the image decode cache and a navigator id counter.
- "Make rules types, not reviews" is violated by the missing module gate.
- The deliberate, tested Flutter divergences recorded in widgets and material ARCHITECTURE.md are a strong fit with P5 and P7.

## Strengths

- ADR-0028 is enforced mechanically: allowed-dependents on both design-system manifests, checked by `cargo xtask workspace`. A grep finds no code edge from any core crate to flui_material or flui_cupertino, only doc-comment mentions.
- Material and Cupertino use no `__private` or doc(hidden) items. Everything they touch is ordinary public API, so an out-of-repo move is technically possible now (the problem is how much surface that API spans, not privacy).
- The facade's feature design is disciplined: a module whose feature is off is absent rather than empty, `required-features` is set on every catalog example, and `cargo xtask facade-combos` builds 12 isolated combinations so feature unification cannot hide a break (tools/xtask/src/tasks/facade.rs:19-30).
- Material and Cupertino test suites re-export the one shared harness, flui_widgets::testing, instead of carrying drifted copies.
- The raw-primitive pattern works where it has been applied: RawTextField, and the swipe-back behaviour living in core navigator (a pub(crate) back_gesture module plus PageRoute::back_gesture), which Cupertino only switches on (cupertino/route.rs:144).
- ThemeData is #[non_exhaustive] and offers field-granular dependencies (Theme::depend_on_fields with FIELD_* masks). That is the design correction for Flutter's Theme.of rebuilding half the tree.
- Coupling between modules inside flui-widgets is low today (navigator→overlay/animated, text→interaction, scroll→paint/localization), so a module-direction gate can be locked in cheaply.
- The mapping-decision records are exceptionally thorough: 20 in widgets, 3 in material, each naming the oracle, the choice, the rejected alternatives and a replacement test.

## Problems

### Design systems sit below the runtime and inside the core facade's default, so topologically Material is core

- **Kind:** workspace_topology · **Severity:** high
- **Evidence:** Root Cargo.toml:86-102 orders the layers design systems (7) < localizations (8) < application (9) < facade (10). flui-material/flui-cupertino allowed-dependents include "flui-app" (crates/flui-material/Cargo.toml [package.metadata.flui]), although flui-app has no such dependency (grep of crates/flui-app/Cargo.toml finds none). Facade: `default = ["material"]` (Cargo.toml features), the prelude re-exports ~40 flui_material names (src/lib.rs:262-270), and the docs describe it as Material-first. No ADR records the plan's 'official packages in separate repos' layer (grep of docs/adr for 'official'/'separate repo' finds only ADR-0028's Flutter context).
- **Impact:** The plan's delivery layers (core vs official packages) and the H3 tiers (Stable prelude vs Evolving Material) cannot be expressed. If Material moves out, the core `flui` crate's default build and prelude would depend on an out-of-repo package, which creates a release cycle between core and official packages. The allowlist also leaves the runtime free to grow a Material dependency.
- **Direction:** Write an ADR for delivery layers that supersedes the layer table. Put the runtime (flui-app) below the catalogs: a runtime/composition layer, then design systems and l10n as 'packages' above everything core. Drop flui-app from the design systems' allowed-dependents. Make the facade catalog-neutral by default: no default Material, and the prelude without Material, which can offer its own `flui_material::prelude`. Keep a thin in-repo 'flui-material' only while the release train needs it.

### Material/Cupertino depend on the public surface of about 9 lower crates, and the facade does not cover what they use

- **Kind:** extension_point · **Severity:** high
- **Evidence:** flui-material normal-depends on widgets, view, types, objects, rendering, foundation, animation, interaction and scheduler, all pinned `=0.2.0-dev` (crates/flui-material/Cargo.toml). Specific uses: `Material` implements RenderView over `flui_objects::{PathClipConfiguration, RenderPhysicalShape}` (material.rs:83, 208), `flui_rendering::RenderUpdateImpact` (material.rs:173-227), and `flui_scheduler::LocalPostFrameHandle` (scaffold_messenger.rs:212). The facade exposes no flui-objects concrete render objects (flui-objects is only a facade dev-dependency; src/rendering.rs exposes the protocol only). Cupertino adds flui-objects solely for `TranslationFraction` (flui-cupertino/Cargo.toml comment; route.rs:80).
- **Impact:** An official package outside the repo, or a community design system, cannot be written against `flui` alone. It has to pin 6-9 internal crates exactly, which turns their whole public surface into a de-facto stable contract (H3) and multiplies release coordination (H4: 10 community crates).
- **Direction:** Define one 'catalog author SDK' surface: facade modules or a flui::sdk that re-export what a design system needs, such as a PhysicalShape/Surface widget, RenderView authoring and post-frame handles. Require Material and Cupertino to depend only on `flui`, and add an xtask rule that the design systems' only normal in-repo dependency is the facade/SDK. That both proves the extension point and makes the out-of-repo move mechanical.

### ADR-0028's shared substrate and raw primitives do not exist; ink, surface and button behaviour live in Material, and Cupertino lacks them

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** ADR-0028 (docs/adr/ADR-0028-design-system-decoupling-contract.md) says ink/splash, surface tint, elevation and localizations live 'below both' and that 'InkWell already follows' the raw-primitive direction. In code, InkWell is flui-material/src/ink_well.rs (composing Focus + Actions + GestureDetector, lines 115, 390-460), and the surface/elevation is flui-material/src/material.rs. Grepping `pub struct Raw` in flui-widgets finds only RawTextField and RawTextFieldState. CupertinoButton composes GestureDetector + Semantics with no Focus/Actions (cupertino/src/button.rs:616-626) and its docs list focus ring and onFocusChange/autofocus as missing for want of a FocusableActionDetector equivalent (button.rs:34-43). The keyboard-activation a11y work (commit c2fa36fb6) therefore reaches Material only.
- **Impact:** This blocks roadmap C2 ('Raw primitives first, then the M3 skin'), C3/C4 parity and the 'equal test gates' goal, and it makes a11y-by-default (EAA, beta exit) a per-skin effort. Any third-party design system has to re-implement activation, focus and state-overlay behaviour. The code silently disagrees with an accepted ADR, which AGENTS.md defines as a defect.
- **Direction:** Move the design-neutral behaviour into flui-widgets as raw primitives: RawButton/FocusableActionDetector (focus, activation intents, WidgetStates, semantics), RawToggle/RawCheckbox/RawRadio/RawSwitch, a Surface (clip, elevation, shadow), and an ink/overlay mechanism. Rebuild Material InkWell/buttons and CupertinoButton on them. Either amend ADR-0028 or make the code match it, and add a test that Cupertino and Material buttons have identical keyboard/a11y behaviour.

### The module-boundary gate that replaced the crate split (A1) was never implemented

- **Kind:** layering · **Severity:** high
- **Evidence:** roadmap 'Решения 2026-09-23' (the 2026-09-23 decisions): an import-direction check between modules in the gates (text/, scroll/, navigator/ see the base, not each other), a concept→module map in AGENTS.md, and a duplicate detector in CI. `cargo xtask checks` runs fmt, typos, taplo, docs-links, workspace, toolchain, wgsl, paths-filter and font-assets (tools/xtask/src/tasks/checks.rs:100-120). A grep of tools/xtask/src for an import/module-boundary check finds nothing, and AGENTS.md has no concept map. Current coupling measured by grepping `crate::<module>`: navigator→animated, overlay; text→interaction, layout, paint; scroll→animated, localization, paint; app→navigator, text, layout, localization.
- **Impact:** The decision to keep one crate was justified by this guard. Without it the ~80k-line crate has no enforced internal architecture, and every new widget (Form, Slider, Router, multiline editor, all H0) can add cross-feature edges unnoticed, which is the 'spaghetti' A1 was meant to stop.
- **Direction:** Add an xtask check, folded into `checks`, that parses `use crate::…` and `super::` edges in flui-widgets and rejects edges not in a declared DAG (for example base = layout/paint/flex/stack/…, then interaction, text, scroll, overlay→navigator, then app). Alternatively use per-module `pub(in …)` visibility plus a tiny module manifest. Lock the current edges now while they are few.

### `__private` is a vestigial, semver-exempt public seam on a published crate with no external consumer

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** crates/flui-widgets/src/__private.rs: its doc says it exists for 'a sibling crate (scrolling, navigation, text editing)', and those crates were rejected on 2026-09-23. Grep of crates/src/examples/tools for `__private` shows the only users are flui-widgets itself (navigator/hero.rs:93, navigator/subtree.rs:71, text/editable_text.rs:893, 896, 1161, 1195, 1886, scroll/viewport.rs:14, sliver_list.rs:12, sliver_fill_viewport.rs:11, sliver_main_axis_group.rs:11) plus tests/anchored_box.rs. The facade re-exports `flui_widgets as widgets`, so `flui::widgets::__private` is reachable.
- **Impact:** It is unwired surface, AGENTS.md's most common defect. Before H3 it is a hole in the stability story: anything published with `pub` will be depended on. It also signals to agents that a crate split is still intended.
- **Direction:** Make the items `pub(crate)`, turn `generic_render_view_element!` into a crate-local macro, and delete the module. If a design system genuinely needs one of these items (for example AnchoredBox for an anchored menu), promote it to a documented public API through the author SDK instead.

### Navigation investment runs against the plan: a 27k-line imperative Navigator, already bound into the runtime command channel

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** crates/flui-widgets/src/navigator/ is 27,457 lines (about 14.7k production), a third of the crate. ARCHITECTURE.md mapping decisions §4-§13 are devoted to named routes. flui-app's realm command channel imports `flui_widgets::NavigatorCommand` (crates/flui-app/src/app/ui_realm/commands.rs:8). The plan makes a declarative Router primary, with URL as the source of truth, and push/pop as a thin facade (roadmap A3/D1, plan 'Навигация' (navigation) row).
- **Impact:** Every additional Navigator feature raises the cost of making Router primary. The runtime and agent command surface (UiCommand) is shaped around imperative navigator commands, so the Router migration crosses the widgets and app layers and possibly the MCP protocol. The risk is Flutter's Navigator 1.0/2.0 split, which the roadmap explicitly names as a pain to avoid.
- **Direction:** Freeze Navigator features. Write the Router ADR with Navigator as its implementation detail: the route stack stays, and the public names become Router/RouteConfig. Replace NavigatorCommand in the runtime with a design-neutral navigation intent (for example a URL or route path) that Router interprets, so the runtime and the agent protocol never name Navigator.

### No descriptor/data layer in the catalog for the G6 machine-readable catalog, A2UI or themes-as-data

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** There is no widget registry analogous to RENDER_OBJECT_TYPES (grep for WIDGET_TYPES/WidgetCatalog/catalog.json finds nothing). flui-material and flui-cupertino have no serde (grep finds nothing), and flui-widgets' `serde` feature only forwards flui-types/serde. Widget props are builder methods and `Arc<dyn Fn …>`/`impl Fn` closures (for example lib.rs re-exports DragTargetAccept/…; animated_size.rs:116). Nothing mentions A2UI in crates/. ThemeData is a Rust struct of Option<*ThemeData> (theme_data.rs:752-791), not data.
- **Impact:** It blocks H1's flui-a2ui (the G6 catalog equals the A2UI catalog), themes-as-data (H1), Figma token import (H3) and the '3 consumers of one catalog' 2030 vision. It makes G6's 'catalog JSON generated from rustdoc' the only path, and that cannot describe callbacks or semantics bindings.
- **Direction:** Decide early, in an ADR, on a descriptor layer that sits beside widgets rather than in them: a `WidgetSpec`/schema per catalog widget (props, events as named actions, semantics role), registered like RENDER_OBJECT_TYPES with a harness test that each registered widget round-trips. Make theme tokens serde-able value types, with ThemeData built from tokens. The A2UI renderer then maps specs to widgets without making every widget serde.

### The facade re-exports whole crates and has no public-API baseline, so H3 stability tiers are not expressible

- **Kind:** api_dx · **Severity:** high
- **Evidence:** src/lib.rs has `pub use flui_view as view; pub use flui_widgets as widgets; pub use flui_app as app; pub use flui_foundation as foundation; pub use flui_types as types`. flui_app exposes `pub mod embedder`, `pub mod bindings` (crates/flui-app/src/lib.rs:46-48). flui_widgets::prelude globs `flui_view::prelude::*` (lib.rs:296). Grep of tools/xtask/src and .github/workflows for semver-checks/public-api finds nothing. Plan H3 exit: 'semver-checks green for 3 consecutive minors', with tiers Stable (prelude, View/State/Element, layout, Router, signals, base catalog), Evolving (Material/Cupertino, devtools) and Experimental (`unstable`).
- **Impact:** Every pub item in 5+ crates is reachable through `flui::` and would become part of the frozen API by default. Without a baseline, accidental surface growth (for example `__private`, embedder internals) is invisible until the freeze.
- **Direction:** Curate the facade: an explicit list per module instead of whole-crate aliases, with internals behind an `unstable`/`runtime-internals` feature. Add `cargo xtask public-api` (cargo-public-api or semver-checks against a committed baseline) as a `checks` step now, while breaks are cheap, so every surface change becomes a reviewed diff.

### The widget surface leaks lower-layer types it does not re-export (SlideTransition)

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** crates/flui-widgets/src/transitions/slide_transition.rs:8 takes `Animation<flui_objects::TranslationFraction>`. Neither flui-widgets' lib.rs, transitions/mod.rs (only re-exports the widget/state) nor the facade re-exports TranslationFraction, and the facade does not re-export flui-objects. flui-cupertino had to add a flui-objects dependency solely for this type (flui-cupertino/Cargo.toml comment; route.rs:80).
- **Impact:** A facade-only app cannot name the type SlideTransition needs, and design systems pick up extra internal edges. It is an instance of the general problem that nothing checks that the author surface is closed under its own signatures.
- **Direction:** Re-export TranslationFraction from flui-widgets. Add a check (rustdoc JSON or public-api) that every type appearing in a public signature reachable from `flui::` is itself nameable through `flui::`.

### Process-global state in the catalog (image decode cache, id counters)

- **Kind:** safety · **Severity:** medium
- **Evidence:** `static CACHE: LazyLock<DecodedImageCache>` guarded by Mutexes (crates/flui-widgets/src/image/decode_cache.rs:96); tests need `static TEST_CACHE_LOCK` (line 205) to serialize. `static NEXT_NAVIGATOR_COMMAND_TARGET_ID: AtomicU64` (navigator/navigator.rs:88). thread_local default builders in animated/animated_switcher.rs:102-115. The image path also spins a background tokio runtime through flui-assets (Cargo.toml comments on asset-images).
- **Impact:** This violates principle 3 (ratchet ambient reach to 0), leaks cache contents and memory budget across realms and windows (H2 multi-window as the norm), and makes test isolation depend on locks. The roadmap had also planned to move the image cache into flui-assets.
- **Direction:** Make the decode cache a realm-owned resource handed in through the image-provider capability (LifecycleContext), or move it to flui-assets behind an injected handle. Replace global id counters with realm-scoped allocators. Add the cache to the runtime-contract globals ratchet.

### The callback threading policy of the public catalog API is inconsistent

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** crates/flui-widgets/src/lib.rs:60-64 has `#![expect(clippy::arc_with_non_send_sync)]` with the comment 'Do not restore `Send + Sync` to UI callbacks'. But ARCHITECTURE.md §1 changed DragTarget callbacks to `Arc<dyn Fn … + Send + Sync>` and §17 makes Semantics action handlers `Send + Sync`. animated_size.rs:101 takes `curve: impl Curve + Send + Sync` while :116 takes `on_end: impl Fn() + 'static`. By rough grep, 38 non-test `pub fn`/`pub type` lines require Send+Sync and 39 builder methods take non-Send closures.
- **Impact:** Users cannot predict which closures may capture Rc state. That is an API freeze hazard (H3), because changing a bound later is breaking. It also muddies the ADR-0027 owner-local model that realm-scoped signals (A4) build on.
- **Direction:** Decide once in an ADR (an ADR-0027 addendum): UI callbacks are owner-local (`Rc`/no Send) unless they cross a documented thread boundary, such as the hit-test payload or the semantics/a11y adapter, which gets an explicit Send wrapper type. Apply the rule across the catalog and add a static_assertions test per exception.

### flui-localizations is a 281-line stub occupying its own layer, planned to depend on the in-repo design systems

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** crates/flui-localizations/src: lib.rs 57 + global_widgets_localizations.rs 224 lines. The only behaviour is an RTL language table, with strings identical to DefaultWidgetsLocalizations (lib.rs docs). Layer 8 exists only for it (Cargo.toml layers). docs/crates.md:88 says it will depend on flui-material/flui-cupertino for Global{Material,Cupertino}Localizations. Roadmap A5 says to merge it into the catalogs' substrate, and the plan puts i18n (ICU4X) in official packages. The widgets-side contracts (localization/, 1.6k lines) are sync-only.
- **Impact:** It adds a layer and a crate without value today. Its planned edges would chain the i18n official package to the in-repo design systems, and the ICU4X work (H1) has no designated home.
- **Direction:** Fold the RTL table into flui-widgets' localization module (the contract owner) and delete the crate and layer 8. Let each design-system package own its localization contract and default English. Create the real l10n catalogs later as the H1 official i18n package over ICU4X, depending on the design-system packages from outside the core.

### The runtime is hard-wired to the catalog crate's scope widgets

- **Kind:** layering · **Severity:** medium
- **Evidence:** flui-app (layer 9) normal-depends on flui-widgets (crates/flui-app/Cargo.toml:112-117). Production uses: `flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope}` (app/ui_realm/attach.rs:6), `MediaQuery, MediaQueryData` (app/media_query_root.rs:9) and `NavigatorCommand` (ui_realm/commands.rs:8).
- **Impact:** Focus root, gesture arena, vsync and media-query injection are runtime contracts, but they are defined in the catalog. Any change to those widgets is a runtime change. An alternative catalog or an embedder cannot supply its own. This is also why the runtime sits above the catalog in the layer order (see the first problem).
- **Direction:** Move the root-scope inherited views (FocusRoot, GestureArenaScope, VsyncScope, MediaQuery data injection) into flui-view or a small 'binding widgets' module below the catalog. Have the runtime depend only on that, and route navigation through a catalog-neutral intent.

### The catalog keeps two state idioms heading into the freeze (Listenable controllers vs signals)

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** The facade `signals` feature is opt-in and not default (Cargo.toml features comment: 'stays an opt-in until the #1090 field-mask registry lands'). Material uses flui_foundation::ChangeNotifier/Listenable/ListenerId (7-9 uses each, per the grep counts). Controllers such as TextEditingController, ScrollController, TabController and PageController are Flutter-style listenables. No catalog widget accepts a Signal.
- **Impact:** The plan makes realm-scoped signals the canonical state layer (A4, Stable tier), but the whole public catalog, including the skins, is built on the Flutter controller idiom. Either the catalog API changes after beta or two idioms get frozen together.
- **Direction:** Before C1/C2 widen the catalog, decide in the ADR-0075 track how catalog widgets accept state: for example `impl Into<Binding<T>>` accepting a value, a Signal or a controller. Pilot it on RawTextField and Checkbox so new widgets are born with the final shape.

### A core widget branches on the compile-time platform, against ADR-0028's capability-seam rule

- **Kind:** flutter_divergence · **Severity:** low
- **Evidence:** crates/flui-widgets/src/text/editable_text.rs:1500-1583: `word_jump_modifier(TargetPlatform)` and `TargetPlatform::current()` (compile-time cfg(target_os)); its own doc (lines 1513-1521) notes that web needs runtime detection. ADR-0028: 'a core widget never branches on cfg!(target_os) or theme.platform'.
- **Impact:** Keyboard conventions are wrong on web running on macOS, and the code silently disagrees with an accepted ADR (a defect per AGENTS.md). As H1 adds iOS and Android text editing, more such branches will accrete.
- **Direction:** Introduce a platform-conventions capability (keymap, selection style, scroll physics) resolved at runtime from flui-platform and injected via an inherited view or LifecycleContext. Design systems or the app can then override it.

### Examples and docs do not reflect or prove the catalog architecture

- **Kind:** docs · **Severity:** low
- **Evidence:** Among the top-level examples, 8 use only `flui` (counter, todo, widgets_gallery, material_demo, multi_window_demo, a11y_probe, workload_probe, sliver_demo). Others import flui_engine, flui_layer and flui_platform directly (color_filter_demo, filter_demo, scene_render, platform_window, wgpu_window, window_features) or are device probes. The facade `include = ["/examples/**"]` publishes them. docs/crates.md:79-80 describes Material as a 'theming foundation' and Cupertino as 'one component', and root Cargo.toml:33-34 has the same plus 'Catalog.1 slice' markers. src/lib.rs has 'Ship bar (wave 4)' and flui-widgets lib.rs:54 has 'wave 3', both process markers AGENTS.md forbids. flui-cupertino and flui-localizations have no ARCHITECTURE.md, so Cupertino's divergences have no `## Mapping decisions` home.
- **Impact:** New contributors and agents get a wrong picture of what Material and Cupertino are. There is no worked example of an external catalog or extension crate, the proof H1/H4 need ('5 plugins built outside the repo', community crates). The published facade archive carries internal probes.
- **Direction:** Split examples into author demos (facade-only, published) and internal probes (moved under tools/ or an unpublished examples crate). Add an `examples/third_party_catalog` crate that builds a mini design system against `flui` only. Refresh crates.md and the manifest comments, remove the wave markers, and add a Cupertino ARCHITECTURE.md.

## Unwired or dead surface

- flui_widgets::__private (generic_render_view_element!, AnchoredBox, enclosing_focus_parent, install_rect_provider, SaltingChildKey): pub and doc-hidden, with no consumer outside flui-widgets.
- The flui-app entry in flui-material/flui-cupertino `allowed-dependents` / `allowed-dev-dependents`: flui-app has no such dependency, so it is an unused allowance.
- flui-localizations::GlobalWidgetsLocalizations: behaviour limited to the RTL_LANGUAGES table, with strings identical to DefaultWidgetsLocalizations. Global{Material,Cupertino}Localizations are promised in docs/crates.md:88 but absent.
- The facade `signals` feature and flui-widgets `signals`: opt-in, and no catalog widget or design system consumes Signal (verify: grep hits in widgets src appear to be unrelated uses of the word 'signal').
- The ADR-0028 'shared substrate' and 'text-selection chrome is injected' seams: no selection-controls seam or ink substrate exists in flui-widgets (grep for SelectionControls/TextSelectionToolbar finds only a controller.rs mention).
- The docs/crates.md 'formal flui facade … target decomposition' wording and the Catalog.1 slice comments in root Cargo.toml: stale descriptions of the surface.

## Open questions

- Should the runtime (flui-app) really sit above the design systems? A layer order of core runtime < design-system packages < facade/app-template would make 'Material as an official package' a topology fact rather than a convention. It requires moving root-scope widgets out of flui-widgets.
- Is the facade meant to be the only dependency of official packages (an SDK surface), or may official packages pin internal crates? This decides whether ~9 crates enter the Stable tier at H3.
- Does `flui` keep `default = ["material"]` after beta? The plan puts Material in the Evolving tier and the prelude in Stable, and those two cannot both hold while Material is in the default prelude.
- Where does the A2UI/G6 descriptor layer live: per-widget derive in flui-widgets, a separate descriptor module, or the flui-a2ui package? And does it cover Material components or only the raw catalog?
- Should Navigator become Router's internal stack, and should the runtime/agent command channel carry navigation intents (URLs) instead of NavigatorCommand? The decision affects flui-app, the MCP protocol and 27k lines of widgets code.
- Hypothesis to verify: the '82.7k non-test lines' audit figure counts the ~14k lines of *_tests.rs under src/ and inline #[cfg(test)] modules. The production size of flui-widgets may be about 55-60k, which matters for the one-crate decision.
- Should the module-direction gate be an xtask source parser, or should module boundaries be expressed with `pub(in crate::…)` visibility so the compiler enforces them (the AGENTS.md 'types, not reviews' preference)?

