# AGENTS.md — flui-app

Top-level application framework. Hosts owner-affine UI realms (`UiRealm`) and
the loop-scoped composition root (`AppRuntime`). ADR-0027's singleton
retirement is complete (#553): there is no process-global binding graph left
— `AppBinding` is deleted, not slimmed, and `RenderingFlutterBinding` /
`Scheduler` are realm-owned, not process-scoped. What remains is *hosting*
debt, not singleton debt — see `Key constraints` below.

## What lives here

- **UiRealm** — owner-affine widget tree (`WidgetsBinding`) + `RenderingFlutterBinding` + its own fresh `Scheduler` value + single-presentation gesture state + scoped GlobalKey/local post-frame activation + bounded command inbox — every one of these is constructed fresh per realm, never shared
- **AppRuntime / AppConfig** — the loop-scoped composition root: platform event-loop demux, `SharedEngineServices` (painting/accessibility, resolved once per owner thread), the frame-wake mechanism, the platform clipboard, and the single realm slot (`RealmId`-keyed; real 1..N hosting is issue #555)
- **RootRenderElement / RootRenderView** — root of the render/element tree
- **run_app / run_app_with_config** — the supported entry points for starting the app
- **run_direct** — experimental direct-engine escape hatch (no widget tree, no input); its bootstrap (window, GPU renderer, callback wiring) now runs inside `on_ready` (ADR-0039 slice 2), so it works on the winit backend too — still `experimental` by policy, not a supported cross-platform application entry point
- **embedder / overlay** — app-level subsystems (PORT-CHECK-OK-SP4 marked)
- **Re-exports** — `GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `WidgetsBinding` from constituent crates. `SemanticsBinding` (flui-semantics) is retired — deleted, not slimmed; semantics enablement + announce/event delivery is now the private, per-presentation `SemanticsHost` (`app/semantics_host.rs`), and the process-scoped accessibility-feature flags live on `AppRuntime`'s `SharedEngineServices`.

## Key constraints

- **Depends on ALL other crates** — flui-view, flui-rendering, flui-types, flui-foundation, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, plus flui-hot-reload behind the optional `hot-reload` feature
- **No design tokens live here.** Colours, typography, spacing, radius, motion, and any other design token are design-system concerns (`flui-material` / `flui-cupertino`, L7); this crate is L9 application composition. A token type appearing under `src/` is a review failure whatever it is named — the parked `AppTheme`/`AppColorScheme` surface was removed for exactly this reason ([ADR-0042](../../docs/adr/ADR-0042-theming-ownership.md)), and `just inventory-check` fails if a `theme` module reappears here. Appearance is per-presentation: the OS signal is `MediaQueryData::platform_brightness` on one window's tree, and the resolved theme is published by an in-tree inherited widget. The app shells that own theme selection (`WidgetsApp` / `MaterialApp` / `CupertinoApp`) are issue #573.
- **Platform features** — `desktop` (default), `android`, `ios`, `web`. Platform-specific entry points gated by `cfg(target_os)`.
- **`hot-reload` feature (off by default)** — `flui-hot-reload` is an *optional* dependency, so an ordinary production graph does not contain it. Everything that would name a `flui_hot_reload` item goes through `app::hot_reload`, which has desktop and Android implementations; web/iOS builds remain cfg-complete but install no reload driver. Add to that seam rather than reaching for the crate directly. The typed worker-plugin fields and builders on `AppConfig` exist only with this feature, so an application cannot configure a capability its build omitted.
- **Debug features** — `debug-overlay`, `performance-overlay` (both off by default)
- **Singleton retirement is complete; remaining debt is hosting-shaped.** Scheduler, renderer, widget state, gesture state, and GlobalKey identity are ALL realm-owned now — no test needs a serialization guard against shared binding state any more (each test constructs its own independent realm; see `two_realms_coexist_same_thread` / `two_realms_two_threads_no_shared_state` / `dropping_realm_a_cannot_wake_realm_b` / `cross_realm_duplicate_global_key_mounts_succeed_in_both` in `app/ui_realm.rs` for the coexistence proof). Gesture state currently models one presentation per realm and moves to `PresentationRuntime` only when a second real presentation consumer exists. What remains: `AppRuntime` hosts exactly one realm per owner thread (issue #555 grows that into real 1..N hosting), `Scheduler` still combines logical/physical/raster scheduling concerns (issue #556 splits it), and the production runners don't yet adopt the `RasterOwner` mailbox protocol (issue #559). A few genuinely process-global residuals remain by design, not omission — named per-symbol in `docs/runtime-contract.toml`'s ambient-reach ratchet (`flui-interaction`'s `global_timer_service`, `flui-assets`'s `Registry::global`, flui-painting's `FONT_SYSTEM`) — a test mutating one of those needs its own explicit serialization, not a shared lock from the now-deleted binding-singleton family.
- **Root scopes** — both attach paths wrap the app root in outer `GestureArenaScope` (the realm binding's shared `BindingDriven` arena) and inner `VsyncScope`; keep production input and recognizers on that same arena
- **flui-interaction `testing` feature** enabled in dev-dependencies for synthetic pointer events in tests
- **Android entry point** — `run_app_android` / `run_app_android_with_config` gated behind `cfg(target_os = "android")`
- **WASM** — `wasm-bindgen-futures` for `spawn_local` frame loop on wasm32
