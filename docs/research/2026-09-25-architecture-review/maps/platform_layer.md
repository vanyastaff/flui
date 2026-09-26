# Codebase map: platform_layer (crates/flui-platform: Win32, AppKit, UIKit, Android, Web, winit fallback, headless, Linux stub)

_Raw output of the `map:platform_layer` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

flui-platform is one layer-2 crate (~46k non-test lines in src; ~339 `unsafe` sites) that holds three different things: (1) the OS-facing contract: `Platform`, `PlatformWindow`, `PlatformDisplay`, the input vocabulary (re-exported `ui-events`/`keyboard-types`), the `OwnerPlatform`/`SharedPlatform`/`PlatformProxy` owner capability (ADR-0039), `DataTransferSource` (ADR-0038) and the per-window capability accessors (`text_input`, `haptics`, `accessibility`); (2) every backend as a cfg-gated module (winit 8.3k lines, macos 9.5k, windows 6.5k, headless 2.8k, ios 2.5k, android 2.3k, web 1.3k, linux 1.2k as a stub plus the AT-SPI bridge); (3) cross-cutting runtime pieces: a tokio `BackgroundExecutor`, `Task`, callback registries (`shared/handlers.rs`, 1.6k lines), panic boundaries and key tables.

How it runs: `flui-app` calls `current_platform()`. That function first checks the process-global `FLUI_HEADLESS` env var, then picks a backend by `#[cfg]`: Win32 on Windows, AppKit on macOS, winit on Linux (`LinuxPlatform` is a public stub that panics), UIKit on iOS, and web on wasm. Android is built directly by `flui-app`'s Android runner. `Platform::run(self: Box<Self>, on_ready)` hands the app a `!Send` `OwnerPlatform` minted through a `pub(crate)` constructor, which seals the trait to this crate. After that, native events reach the app through about 15 per-window `Box<dyn FnMut + Send>` callback slots, plus a separate `Platform::on_window_event` stream that only the desktop runner uses. flui-app still carries separate desktop, Android, iOS and web runners.

The capability model is closed. Each OS service (IME, haptics, a11y) is a method on `PlatformWindow` that returns `Option<Arc<dyn Trait>>`, so adding one means editing the trait, every backend, flui-app and `LifecycleContext`. The IME trait has two push-only methods and holds no document state. Coverage is uneven: only the winit, macOS and headless backends implement `text_input`; the Win32 backend (the shipping Windows path and the B1 target) has no IME code at all. A11y adapters exist only for Win32, AppKit and AT-SPI, behind the non-default `a11y` feature. Haptics exist only in headless. File dialogs exist only on Win32 and nothing calls them. No backend has native menus. The `PlatformProxy` lane has a real transport only on winit.

All platform traits require `Send + Sync`, so every main-thread-affine native object gets `unsafe impl Send/Sync`: 26 impls across macOS, Win32 and web, plus `RawWindowHandle` in `src/window.rs`. The macOS backend marshals every method through `route_on_owner`. The trait docs themselves record rule violations: flui-app's frame wake calls `request_redraw` from arbitrary threads (#949).

The structural problem with the widest reach: `flui-interaction` depends on `flui-platform` with default features, only to name `PlatformTextInput`. Because of that, `flui-rendering`, `flui-view`, `flui-widgets` and any future plugin link winit, tokio's multi-thread runtime and `windows`: `cargo tree -p flui-rendering -i winit` and `-i tokio` both resolve through flui-interaction → flui-platform. The crate also carries a lot of dead or duplicate public surface: a second `Window` trait with its own `WindowId`, `RawWindowHandle` and `WindowState`; `PlatformEmbedder`; `PlatformCapabilities` with no consumer; `BasicVelocityTracker`; `as_winit`; window_ext traits that production code never reaches. Several backend-private rule modules are `pub` only to avoid dead-code lints on other targets. None of this can be frozen as the H3 API. The engineering inside the backends is strong: typed owner capability, typed errors, per-decision records with market lineage, the surface-status contract, the clipboard fix. The weak part is the boundary itself: one crate for the contract and all backends, a sealed trait, a closed capability set, and Send+Sync everywhere.

## Responsibilities and boundaries

What it should own: the OS boundary. That means windows, the event loop and owner thread, displays and DPI, translation of native input into the canonical ui-events vocabulary, the IME protocol, clipboard and data transfer, and a11y adapters (it takes an accesskit::TreeUpdate so it never names flui-semantics; this seam is clean). Also surface handles via raw-window-handle, the lifecycle and surface-availability signals, and shell services (open_url, dialogs, menus).

Where the boundary leaks:
(a) Contract and implementation are one crate. Consumers that need only a trait (flui-interaction for PlatformTextInput, future PlatformCapability plugins, flui-testing) link every backend's dependencies plus tokio.
(b) It owns runtime concerns that belong to the runtime/app layer: a tokio BackgroundExecutor and Task (`Platform::background_executor` is a required method that executor.rs itself calls "slated for removal"), plus the FLUI_HEADLESS env-var policy.
(c) Test infrastructure sits in the production public API: HeadlessPlatform, MockWindow, FakeTextInput, FakeHaptics, FakeAccessibility and HeadlessOwnerTurns are exported at the crate root.
(d) Backend-specific rule modules are public: shared::hwnd_affinity, keys, keys_macos, visibility, scroll, panic_boundary, events, gestures.
(e) The app runner knows each backend: flui-app/src/app/runner has android.rs (704 lines), ios.rs (551), web.rs (438) and desktop.rs (809), and realm_dispatch.rs uses target-conditional dead_code expects. So the lifecycle is not abstracted by the contract, and a new platform means editing flui-app.
(f) The Linux a11y adapter lives in the `linux` stub module but is consumed by the winit backend.
(g) Velocity tracking (BasicVelocityTracker) and gesture normalization partly overlap with flui-interaction.

What belongs elsewhere: the executor and Task belong in the runtime (flui-app AppRuntime). The headless and fake backends belong behind a `testing` feature or in a test-support module. Heavy optional shell services (xdg-portal dialogs, notifications, camera and so on) belong in capability crates (official packages) built on a light contract crate.

## Key types and contracts

- trait Platform: Send + Sync + 'static (traits/platform.rs:253): run(self: Box<Self>, PlatformReadyCallback) -> Result<(), PlatformError>, open_window/active_window/displays (owner-affine but still public on the trait), clipboard(), data_transfer(), on_quit/on_reopen/on_window_event/on_open_urls, set_exit_policy_hook, set_wake_deadline_hook, prompt_for_paths/prompt_for_new_path (Win32-only), capabilities(), background_executor(). De-facto sealed: platform.rs:243-252
- OwnerPlatform (!Send, pub(crate) new at traits/owner.rs:83): the owner-thread capability from ADR-0039, handed to on_ready. SharedPlatform is the Send+Sync fence. PlatformProxy carries only open_window/request_quit/wake and never closures; only winit implements ProxyTransport (winit/platform.rs:2535)
- WindowOpen::{Ready, Pending(PendingWindow)} + ClaimSlot/ClaimHandle (flui-foundation): at-most-once window-creation handoff
- trait PlatformWindow: Send + Sync (traits/window.rs:169): about 45 methods including ~15 on_* callback registrations (FIFO + CallbackLease in shared/handlers.rs), capability accessors text_input()/haptics()/accessibility() -> Option<Arc<dyn _>>, window_handle/display_handle (raw-window-handle), on_surface_status_change(bool) (Android surface contract, ADR-0063), execution_state(), and a feature-conditional as_winit()
- trait PlatformTextInput (traits/text_input.rs): set_ime_allowed + set_ime_cursor_area only. flui_types::ImeEvent is winit-shaped (ADR-0030 §1); the ownership side is flui_interaction::TextInputOwner (ADR-0037 §5)
- trait PlatformAccessibility (traits/accessibility.rs): publish(accesskit::TreeUpdate), is_active, activation and action listeners. This is the layer-2/layer-3 seam
- trait PlatformHaptics: perform(HapticFeedback) + as_any (ADR-0031)
- trait PlatformCapabilities + Desktop/Mobile/WebCapabilities (traits/capabilities.rs): static bool flags, no consumer outside the crate. Not the plan's PlatformCapability plugin concept
- PlatformInput (ui-events PointerEvent/KeyboardEvent + DragDropEvent), DispatchEventResult
- DataTransferSource/DataTransferOffer (ADR-0038), Clipboard (text-only) and ClipboardItem: three overlapping clipboard surfaces
- Duplicate surface: src/window.rs trait Window + window::WindowId + window::RawWindowHandle + WindowState + WindowBuilder, implemented by WindowsWindow (windows/window.rs:1318) and MacOSWindow (macos/window.rs:1548) next to PlatformWindow
- current_platform() -> Result<Box<dyn Platform>, PlatformError> (lib.rs): env var first, then cfg selection. headless_platform()

## Dependencies

Out (normal): flui-types, flui-foundation (OwnerAffinity, DataTransferId, ClaimSlot), accesskit 0.25, ui-events 0.3, keyboard-types, cursor-icon, dpi, raw-window-handle 0.6, parking_lot, thiserror, tracing, web-time, static_assertions, bitflags. tokio (rt-multi-thread) on every non-wasm target. winit 0.30 by DEFAULT through the `desktop` feature (Cargo.toml `desktop = ["dep:winit"]`), even though no source file checks `feature = "desktop"` and winit code compiles only under `winit-backend`. Per target: windows 0.62 (+accesskit_windows 0.35 under a11y); objc2/objc2-foundation/objc2-app-kit/dispatch/dispatch2/core-graphics (+accesskit_macos); objc2-ui-kit/objc2-quartz-core/block2 on iOS; android-activity 0.6 (native-activity)/libc on Android; wasm-bindgen/web-sys/console_error_panic_hook on wasm; accesskit_unix (~66 crates, zbus) on Linux under a11y. The winit-backend feature adds arboard, crossbeam-channel and ui-events-winit. `cargo tree -p flui-platform -e normal` shows 56 unique crates on this Windows host.

In: flui-app (the composition root; turns on winit-backend for Linux; forwards a11y), flui-interaction (same layer 2, ADR-0037 edge, default features; for PlatformTextInput only), flui-widgets (optional under `testing`, plus a dev-dependency), and the workspace root for examples. The facade (src/) does not re-export flui-platform, so platform extension traits (MacOSWindowExt, liquid glass, tiling, WindowsWindowExt) have no production path to app code. Transitively, through flui-interaction: flui-rendering, flui-objects, flui-view, flui-widgets, flui-material/cupertino, flui-testing. `cargo tree -p flui-rendering -e normal -i winit` gives winit ← flui-platform ← flui-interaction ← flui-rendering, and the same for tokio.

## Fit with the plan

H0 (beta, three desktops + web):
- The B1 exit needs Windows with Narrator and a Japanese IME. current_platform() always picks WindowsPlatform on Windows (lib.rs cfg(windows) arm), and that backend has no IME code (no WM_IME_*, no text_input(); `grep -rn 'fn text_input' platforms` hits only headless/macos/winit). The only Windows backend with IME (winit) is never selected in production. So H0/B1 is blocked on either writing IMM32/TSF for Win32 or switching Windows to winit.
- B2/F8 (native menus and file dialogs) has no home: no menu code on any backend, and dialogs exist only on Win32 with no caller.
- Web as a beta candidate has neither IME nor a11y: the canvas backend has no hidden input element and no DOM a11y mirror.
- "AccessKit on by default at beta" collides with the non-default a11y feature and the ~66-crate AT-SPI stack.

H1 (mobile, PlatformCapability plugins, A2UI):
- Plugins are structurally blocked. Platform is sealed (#560). Capabilities are a closed set of accessors on PlatformWindow. PlatformProxy forbids closures and carries three verbs. OwnerPlatform exposes no native context (Activity/JNI, UIViewController, HWND/NSWindow beyond raw-window-handle). A plugin crate would have to depend on the whole backend crate plus tokio.
- Mobile IME needs a document-state protocol (Android InputConnection, iOS UITextInput). The push-only 2-method trait cannot express it; macOS already answers attributedSubstringForProposedRange with nil (macos/text_input.rs:430-446).
- Android uses NativeActivity. Hypothesis: GameActivity would be needed for proper soft-keyboard IME.

H2 (performance, multi-window): the owner-lane/proxy groundwork (ADR-0039) is a good base. But request_redraw has no worker-side verb (#949), and Send+Sync windows put marshaling costs and unsafe code on every call.

H3 (1.0 stability tiers): the public surface cannot be frozen as it stands. It has two Window traits, public test fakes, `pub` rule modules for dead-code avoidance, feature-conditional trait methods (as_winit), a no-op `web`/`wayland`/`x11` features, and a PlatformCapabilities name that clashes with the planned PlatformCapability.

H4 (embedded/kiosk, community backends): impossible while Platform is sealed and the contract lives in the same crate as the backends.

Delivery layers: the plan puts OS plugins and devtools in official packages and a community catalog above that. That needs a light, stable contract crate at the bottom, which does not exist yet.

Principles:
- Principle 3 (no global state): violated only where the OS forces it (clipboard_lock) plus the FLUI_HEADLESS env switch.
- Principle 5 (proof, not claims): violated by the README and lib.rs status tables.
- "Make rules types": the ADR-0039 capability follows it, but the Send+Sync trait bounds let violations compile.

## Strengths

- The ADR-0039 owner capability is a real compile-time fence. OwnerPlatform is !Send and minted only by a backend. SharedPlatform is the explicit Send+Sync subset. The PlatformProxy lane has typed rejection that returns the payload. The ClaimSlot at-most-once window handoff avoids leaked windows in every ordering. Rust-idiomatic, better than Flutter's untyped platform-thread convention.
- Typed error taxonomy (PlatformError, BootstrapError, OpenWindowError, ProxySendError, WaitError) with no anyhow in the public API, and a fallible on_ready that stops the loop on bootstrap failure.
- The layer-2/layer-3 a11y seam is clean. The platform speaks accesskit::TreeUpdate and never semantics types, so translation happens on the producing side, and inbound NodeIds are the stable semantics ids (traits/accessibility.rs).
- Canonical W3C input vocabulary (ui-events/keyboard-types). The winit keyboard path delegates entirely to ui-events-winit (Masonry's bridge), and a cross-backend key-agreement test covers the hand-written Win32/AppKit tables.
- Capability discovery through Option<Arc<dyn _>> (text_input/haptics/accessibility) instead of panicking default methods. Unsupported is expressible, just not open.
- Carefully reasoned lifecycle contracts with market lineage and tests: on_surface_status_change (Android surface before-signal, ADR-0063), AppKit display-pass re-arm, UIKit scene ownership (ADR-0073), exit-policy hooks, reopen, per-window execution state.
- Target-gated dependencies keep each OS build to its own binding stack. The macOS and iOS backends share objc2, and the cocoa/objc stack is gone.
- The headless backend lets the whole framework run deterministically in CI. The Win32 clipboard use-after-free was root-caused and fixed with a message-only owner window, and platform-windows now runs the suite on windows-latest (ci.yml:1049) in heavy runs.
- unsafe is scoped per backend module (#![expect(unsafe_code)] per FFI island, not crate-wide). The Win32 backend is nearly fully SAFETY-documented (133 sites / 130 SAFETY).

## Problems

### Contract and all backends in one crate: flui-interaction pulls winit, tokio and windows into rendering, view and widgets

- **Kind:** workspace_topology · **Severity:** high
- **Evidence:** crates/flui-interaction/Cargo.toml:31 depends on flui-platform (default features) only for PlatformTextInput (flui-interaction/src/text_input.rs:27). `cargo tree -p flui-rendering -e normal -i winit` gives winit ← flui-platform ← flui-interaction ← flui-rendering, and `-i tokio` gives the same. flui-platform/Cargo.toml `default = ["desktop"]`, `desktop = ["dep:winit"]`, but no source file references feature "desktop", so winit is compiled into every Windows/macOS build and never used. `cargo tree -p flui-platform` shows 56 unique crates. docs/FOUNDATIONS.md:254 and docs/crates.md:31 justify the edge (ADR-0037) without considering this cost.
- **Impact:** Every crate above layer 2, and every future PlatformCapability plugin, links the OS backend stack and a tokio runtime. That inflates compile times on a memory-limited host, blurs the wasm story, makes the render protocol depend on the windowing library, and prevents a light, stable contract crate for official and community packages (H1, H3 tiers, H4).
- **Direction:** Split flui-platform into a contract crate (traits, input/IME/a11y vocabulary, owner capability plus an explicit backend minting seam, conformance test kit; deps only types/foundation/ui-events/accesskit/raw-window-handle, no tokio or winit) at layer 1-2, and a backends crate at the composition-root level used only by flui-app. Keep backends as modules of the backends crate, the way winit's platform_impl and wgpu-hal do. Record it as an ADR superseding the relevant part of ADR-0037. Remove the no-op `desktop` edge immediately.

### Platform is sealed: no external backend, embedder or test/replay backend is possible

- **Kind:** extension_point · **Severity:** high
- **Evidence:** traits/platform.rs:243-252: 'de-facto sealed … An external-embedder minting seam is design work tracked separately (#560)'. OwnerPlatform::new and PlatformProxy::new are pub(crate) (traits/owner.rs:83, :491). ADR-0039 §1 says the same.
- **Impact:** Blocks H4 embedded/kiosk and community backends (OpenHarmony, a native Wayland crate, a record/replay backend for G7 in flui-testing), and it forces every backend into this crate. As a result the crate's size, unsafe count and CI matrix grow with every platform, with bus factor 1.
- **Direction:** Design the #560 seam now, while breaking changes are cheap. Put a `BackendContext`/`OwnerPlatform::for_backend(...)` minting API in the contract crate, marked unsafe or behind a documented `backend-api` feature, with an obligations checklist enforced by a conformance suite (the existing tests/contract.rs generalized into a reusable harness that any backend runs).

### No PlatformCapability extension point; capabilities are a closed set of accessors, and the name clashes with PlatformCapabilities

- **Kind:** extension_point · **Severity:** high
- **Evidence:** PlatformWindow hard-codes text_input()/haptics()/accessibility() (traits/window.rs:333-352). Each new service means editing the trait, every backend, flui-app and LifecycleContext (ADR-0078). PlatformProxy carries only open_window/request_quit/wake and 'never carries closures' (ADR-0039 §3). OwnerPlatform exposes no native context: AndroidPlatform::app() exists (android/mod.rs:193) but Platform is consumed by run(). traits/capabilities.rs PlatformCapabilities (static bool flags) has zero consumers outside the crate (grep for supports_touch, should_coalesce_pointer_moves and similar finds nothing).
- **Impact:** The plan's H1 extension point (typed native capability registration, plugin as an ordinary crate with #[cfg(target_os)], typed Unsupported) has no mechanism. The five out-of-repo plugins in the H1 exit are not buildable. The name collision will confuse API docs and the stability tiers.
- **Direction:** Add a typed capability registry per realm or presentation, e.g. `trait PlatformCapability: 'static { type Handle; fn attach(ctx: &BackendNativeContext) -> Result<Self::Handle, Unsupported>; }`. Give it an owner-thread execution primitive (a typed OwnerTask trait object executed on the owner lane, not an ad-hoc closure channel) and per-OS native context accessors (JNI VM/Activity, UIViewController, HWND/NSWindow). Delete PlatformCapabilities/Desktop/Mobile/WebCapabilities, or rename them to PlatformTraits and wire them to a consumer. Record it in an ADR before H1.

### The shipping Win32 backend lags the 'fallback' winit backend on IME, proxy lane and loop hooks

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** Capability grep over src/platforms: `fn text_input` in headless/macos/winit only; `fn set_exit_policy_hook` in headless/macos/winit; `fn set_wake_deadline_hook` in android/ios/macos/winit; `impl ProxyTransport` only winit/platform.rs:2535. No WM_IME_*, Imm* or TSF code in platforms/windows (grep empty). lib.rs current_platform: cfg(windows) always returns WindowsPlatform. The lib.rs status table still rates Windows 'Production 10/10'. Journal 2026-09-24: Windows live checks found five product bugs and the Japanese-IME step is pending.
- **Impact:** Blocks the H0/B1 exit criterion (Windows + Narrator + Japanese IME) and B5 (IME on three desktops). The backend that has these features on Windows is never selected in production.
- **Direction:** Decide per OS which backend ships (see the backend-matrix problem). If Win32 stays, IMM32/TSF composition and the ProxyTransport, exit-policy and wake hooks are B1 prerequisites. Make a backend capability matrix a generated, tested artifact (a conformance test per capability per backend) instead of prose tables.

### The IME contract is push-only and winit-shaped, with no document-state protocol

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** traits/text_input.rs: the trait has only set_ime_allowed and set_ime_cursor_area; its scope note says it does not model a text buffer or selection. macos/text_input.rs:430-446 answers attributedSubstringForProposedRange with nil/NSNotFound because the platform holds no text. ADR-0030 §1: 'flui_types::ImeEvent is winit-shaped'. Android backend uses android-activity with the `native-activity` feature (Cargo.toml).
- **Impact:** Reconversion, dictation, autocorrect, the macOS character palette, TSF on Windows, Android InputConnection (soft keyboard, predictive text) and iOS UITextInput all need the OS to query surrounding text and selection. The multiline editor (B4) and the mobile H1 exit therefore hit a protocol wall. Hypothesis: NativeActivity limits soft-keyboard IME on Android, and GameActivity/GameTextInput would be needed.
- **Direction:** Redesign PlatformTextInput around a document-state client, like Flutter's TextInputClient/editing state or Masonry/ui-events' IME with surrounding text: the owner exposes text, selection, composing range and char rects, and the backend pulls from it synchronously on the owner thread. Do this before the multiline editor lands, and record it as an ADR superseding ADR-0030 §1. Evaluate GameActivity for Android as part of F5.

### Send + Sync on Platform, PlatformWindow and PlatformDisplay forces unsafe Send/Sync on thread-affine native objects, and the rule is violated in production

- **Kind:** safety · **Severity:** high
- **Evidence:** 26 `unsafe impl Send/Sync` across macOS (accessibility, platform, text_input, window), Win32 (accessibility, platform, window) and web (clipboard, display, executor, platform, window), plus RawWindowHandle in src/window.rs:327-332. web/window.rs:38-39 has 'SAFETY: WASM is single-threaded' (unsound under wasm atomics/threads). traits/window.rs:131-240 says `Send + Sync` 'makes the wrong call compile from anywhere' and documents #949: flui_app's frame wake handle calls request_redraw from whichever thread completed a future, and headless and web run user callbacks synchronously on the caller's thread. PlatformProxy has no redraw verb, and only winit supplies a transport. Platform::open_window/displays/quit stay public trait methods callable on the Send+Sync Box<dyn Platform> before run().
- **Impact:** This is the structural source of much of the unsafe count and of per-call owner-lane marshaling on macOS (route_on_owner in every body). It turns the ADR-0039 type fence into documentation, a correctness risk once H2 introduces raster/IO lanes, and it stops web from adopting wasm threads.
- **Direction:** Make native window objects owner-owned and !Send, reachable only through OwnerPlatform. Hand out a Send `WindowHandle` proxy with a closed verb set (redraw, close, set_title, …) routed through a mandatory per-backend transport. Remove owner-affine methods from the public Platform trait (move them onto a backend-only trait). This removes most unsafe Send/Sync impls instead of documenting them.

### Duplicate and dead public surface that cannot be frozen for 1.0

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** src/window.rs (633 lines): a second `trait Window`, `window::WindowId`, a hand-rolled `RawWindowHandle` duplicating raw-window-handle, WindowState and WindowBuilder, implemented by WindowsWindow (windows/window.rs:1318) and MacOSWindow (macos/window.rs:1548) next to PlatformWindow. External grep finds no user of flui_platform::window::Window. traits/embedder.rs PlatformEmbedder/PlatformSpecificEvent/AndroidEvent/IosEvent has zero implementors. BasicVelocityTracker/TimestampProvider (traits/input.rs:305-387) have no consumer and duplicate flui-interaction/src/processing/velocity.rs. as_winit (traits/window.rs:769) has no external caller. prompt_for_paths/open_url/reveal_path/keyboard_layout/on_open_urls/compositor_name have no caller outside the crate.
- **Impact:** API noise, maintenance cost on every backend (each Window-trait body is a second implementation), and confusion for contributors and agents. Every item here would become Stable/Evolving debt at H3.
- **Direction:** Delete src/window.rs's Window family, PlatformEmbedder, BasicVelocityTracker and as_winit. Keep the shell services only once a consumer is wired through LifecycleContext. Enforce this with a public-API snapshot (cargo-public-api or semver-checks baseline) for the contract crate.

### Backend-private modules and test fakes exposed as public API

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** shared/mod.rs: hwnd_affinity, keys, keys_macos, visibility, events, gestures, scroll and panic_boundary are `pub`, with the comment that 'pub is what keeps a Linux-tested, Windows-consumed rule module warning-free everywhere'. lib.rs re-exports FakeAccessibility, FakeHaptics, FakeTextInput, HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns, HeadlessPlatform and MockWindow at the crate root. PlatformHaptics::as_any exists only so tests can downcast to FakeHaptics.
- **Impact:** CI limits (only Linux executes) leak into the public API. Internal tables become semver surface, and production traits carry test-only downcast methods, which blocks H3 tiering.
- **Direction:** Make them pub(crate), with #[cfg(any(test, target_os=…))] or `#[cfg_attr(not(target_os=…), allow(dead_code))]`. Move the headless/fake backend behind a `testing` feature (or into the contract crate's test-kit module) and drop as_any from capability traits in favor of a test-kit accessor.

### The backend matrix is doubled and unowned: winit option on Win/macOS, LinuxPlatform stub, no-op features

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** The winit-backend feature is 'optional on Windows/macOS' (platforms/mod.rs), but current_platform never selects it there (lib.rs cfg(windows) and cfg(macos) arms). It is still exercised in CI by platform-windows `--all-features` (ci.yml:1080). LinuxPlatform is public and panics with unimplemented! (linux/mod.rs, 17 unimplemented!/todo! sites in platforms). Cargo.toml features `web`, `wayland`, `x11` are self-described or verifiably no-ops; linux/window_ext.rs (562 lines) is gated on them. The plan's architecture table asks 'winit-fallback after 1.0?'.
- **Impact:** Two Windows and two macOS implementations to keep correct, while the one with more features (winit) never ships there and the shipping one lacks IME. Linux, a B2 beta target, rides on a backend labelled 'fallback' with a dead native stub beside it. CI minutes and review attention are spent on configurations no user runs.
- **Direction:** Record one backend per OS in an ADR. Suggested: winit is the Linux backend for H0-H2 (rename from 'fallback', move the AT-SPI bridge under it, delete LinuxPlatform/window_ext and the wayland/x11 features until a native backend is actually scheduled). Native Win32/AppKit/UIKit/Android/web elsewhere, with the winit option on Windows/macOS removed or kept only as a dev/test feature. Revisit winit 0.31 and ui-events-winit's winit ^0.30 pin as an explicit risk.

### The platform→app lifecycle contract is not uniform, so flui-app carries per-OS runners

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** Two event channels: Platform::on_window_event(WindowEvent) versus about 15 per-window callbacks. realm_dispatch.rs:111-125 says window-event variants 'are produced only by the desktop runner … mobile and web runners drive their lifecycle from platform callbacks instead' and uses target-conditional expect(dead_code). flui-app/src/app/runner has android.rs 704, ios.rs 551, web.rs 438, desktop.rs 809 lines. Android bypasses current_platform (lib.rs Android arm returns an error, and runner/android.rs:100 builds AndroidPlatform::new(app)).
- **Impact:** Adding a platform (H1 mobile maturity, H4 embedded) means editing the composition root. Lifecycle semantics (pause/resume, surface loss, visibility) drift per runner and are tested per runner. That makes the desktop-first and then mobile ordering more expensive than it has to be.
- **Direction:** Define one platform lifecycle state machine and event stream in the contract crate (app lifecycle, window execution state, surface status, visibility, appearance) that every backend emits and one runner consumes. Provide entry points (android_main, UIApplicationMain, wasm start) as thin macros or functions in the backend crate that call the same runner.

### A11y adapters are desktop-only and off by default; no plan for web/mobile adapters

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** `fn accessibility` exists only in headless/macos/windows/winit (capability grep). The a11y feature is off by default (Cargo.toml features; the AT-SPI note says ~66 crates). iOS/Android/web have no adapter (roadmap F6 acknowledges iOS). The Linux bridge lives in platforms/linux/accessibility.rs, a stub module, but is consumed by winit/window.rs:37.
- **Impact:** The plan's 'AccessKit on by default at beta (EAA)', 'semantics golden per widget' and 'AccessKit vocabulary = agent protocol' assume a default-on path. Web as a beta candidate has no screen-reader story (canvas only).
- **Direction:** Make a11y default-on for Win/macOS (cheap adapters) with Linux AT-SPI behind a feature or in an official package. Schedule a web DOM/ARIA mirror (or accesskit's web adapter if available) and Android/iOS adapters as explicit H1 items. Move the Linux bridge under the backend that uses it.

### A tokio runtime and a non-cancelling Task live in the platform contract

- **Kind:** layering · **Severity:** medium
- **Evidence:** Cargo.toml: tokio rt-multi-thread on every non-wasm target. executor.rs:12-19: BackgroundExecutor kept 'only as the Platform::background_executor compatibility surface (slated for removal)'. Platform::background_executor is still a required trait method (platform.rs:259). docs/crates.md:37 says platform 'loses BackgroundExecutor/PlatformExecutor'. ADR-0039 §2: prompt_for_paths returns flui_platform::Task 'whose drop does not cancel; that type must not spread'.
- **Impact:** The plan's 'explicit async model, tokio optional' cannot be met while the lowest OS layer mandates tokio. It is also a second executor beside AppRuntime's pools, and it adds tokio to every crate above interaction.
- **Direction:** Delete background_executor, PlatformExecutor, BackgroundExecutor and Task from the contract now. Backends needing a helper thread (Win32 dialogs on STA, the clipboard owner) own a std::thread. Async results go through the runtime's Task/Worker model (ADR-0049).

### Menus, dialogs, tray and notifications have no architectural home

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** No NSMenu/CreateMenu/menu code in any backend (grep only hits a web event comment). prompt_for_paths/prompt_for_new_path are implemented only in windows/platform.rs:1765 and have no caller outside flui-platform. ADR-0039 §2 defers 'consolidating prompts onto the framework's async mechanism … its own ADR'. The roadmap schedules F8 (native menus, file dialogs, context menus: Win32, AppKit, xdg-portal) in B2.
- **Impact:** B2 exit (native menus and file dialogs on three desktops) has no contract to implement against. xdg-portal brings zbus and would bloat the core crate if added in place.
- **Direction:** Write the ADR first. Put the app menu bar as data (a declarative menu model owned by the realm and rendered natively by a backend capability), and file dialogs and notifications as async capabilities through the PlatformCapability mechanism. Put the Linux portal implementation in a capability crate (an official package) on top of the contract crate.

### Oversized backend files; the owner lane, wake and exit policy are reimplemented per backend

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** wc -l: winit/platform.rs 3046, macos/window.rs 3034, headless/platform.rs 2837, windows/platform.rs 2113, windows/window.rs 2099, shared/handlers.rs 1577. Owner/loop machinery per backend: winit/control.rs 883; macos owner_lane.rs 381 + wake_pump.rs 400 + loop_control.rs 547; windows/owner_control.rs 153; ios/native_owner.rs. Exit-policy and wake hooks exist on only a subset (capability grep).
- **Impact:** The same lifecycle behavior (deferred window open, quit fencing, exit policy, wake deadline) is written 4-6 times and drifts, and each backend gets its own ARCHITECTURE entry instead of one shared state machine. Contributors cannot hold a 3k-line window file in their head, which conflicts with the goal of an external contributor finishing a feature without the author (H3).
- **Direction:** Extract a backend-agnostic owner/loop core (admission gate, deferred open queue, quit fence, exit-policy evaluation, wake-deadline scheduling) into the contract crate as a reusable state machine driven by a small per-OS `NativeLoop` trait. Then split window files by concern (geometry, input, appearance, lifecycle).

### The unsafe footprint is concentrated in backends that never execute in CI

- **Kind:** safety · **Severity:** medium
- **Evidence:** Measured `unsafe {|fn|impl` / SAFETY counts: macos 148/121, windows 133/130, android 23/15, ios 17/14, web 12/6, winit 2/2, other 4. lib.rs:150-163 says macos/android/web are 'NOT yet at that bar'. AGENTS.md: only Linux/headless executes in CI; Win32 runs only in the heavy platform-windows job (ci.yml:1049-1052, `if: needs.plan.outputs.heavy == 'true'`). macOS/iOS/Android/web are clippy-only (cross-typecheck).
- **Impact:** The roadmap's A7 unsafe ratchet and the plan's 'unsafe under audit and miri' cannot be verified for about 200 sites that never run. Soundness regressions on macOS (the only beta-candidate platform) land unseen.
- **Direction:** Turn on clippy::undocumented_unsafe_blocks per backend module with a counted ratchet in cargo xtask. Separate pure logic from FFI so the logic runs under miri on Linux (the shared/ rule modules already follow this pattern). Add at least a scheduled macOS runner job for the AppKit backend's tests and probes. The per-backend-crate split would give each backend its own unsafe budget.

### Status and doc drift inside the crate

- **Kind:** docs · **Severity:** low
- **Evidence:** README.md table says Win32 is 'Native, full featured' and android/web are 'stubs'. lib.rs doc table says Windows is 'Production 10/10 … no executing coverage in CI', while platform-windows exists, and Android/Web are 'Stub 2/10', while android/mod.rs is an 831-line working backend. traits/window.rs:220-226 says macOS request_redraw is unsound, but macos/window.rs:1020 routes it through route_on_owner. Cargo.toml comments reference the 'cocoa/objc stack' that ARCHITECTURE.md says is gone, and build.rs has an objc 0.2 cfg shim. Process markers 'slice-3' (traits/owner.rs:740,744), 'slice-2' (platform.rs:196, owner.rs:589) and 'wave 4' (lib.rs:165) violate AGENTS.md. ARCHITECTURE.md:3-5: 'a full crate architecture writeup is deferred'; its 930 lines are only mapping decisions.
- **Impact:** Violates plan principle 5 (proof, not claims). Agents and contributors get contradictory status. There is no map of the contract for a new backend author.
- **Direction:** Replace the status tables with a generated capability/evidence matrix (from conformance tests plus dated BETA.md evidence). Write the missing contract overview section: module map, owner model, event flow, capability discovery. Remove the markers and stale comments.

### Backend selection through a process-global env var

- **Kind:** api_dx · **Severity:** low
- **Evidence:** lib.rs current_platform: `if std::env::var("FLUI_HEADLESS").is_ok()` runs first. Cargo.toml [[test]] headless is a separate binary because the tests `unsafe env::set_var` it.
- **Impact:** A small violation of plan principle 3 (explicit, not ambient). Tests have to mutate process-global state, and a stray env var silently turns a production app headless.
- **Direction:** Make backend choice explicit (an AppBuilder/runner parameter or a test-kit entry point). Keep the env var only in the flui CLI or the test harness.

## Unwired or dead surface

- src/window.rs: trait Window, window::WindowId, window::RawWindowHandle (with unsafe Send/Sync), WindowState, WindowBuilder. Implemented by WindowsWindow and MacOSWindow; no consumer outside the crate
- traits/embedder.rs: PlatformEmbedder, PlatformSpecificEvent, AndroidEvent, IosEvent, WebEvent. Zero implementors, zero consumers
- traits/capabilities.rs: PlatformCapabilities, DesktopCapabilities, MobileCapabilities, WebCapabilities. Platform::capabilities() has no caller outside flui-platform (the flui-app/engine hits are the GPU renderer's own capabilities())
- traits/input.rs: BasicVelocityTracker, TimestampProvider, SystemTimestamp. No consumer; duplicates flui-interaction's velocity tracker
- PlatformWindow::as_winit (feature-conditional trait method). Only the winit impl defines it; no caller
- Platform/SharedPlatform: prompt_for_paths, prompt_for_new_path (Win32-only implementation), open_url, reveal_path, open_path, keyboard_layout, on_keyboard_layout_change, on_open_urls, compositor_name. No production caller
- Platform-specific window extension traits: macos window_ext (MacOSWindowExt, liquid_glass.rs 320 lines, window_tiling.rs 526), windows/window_ext.rs (413), linux/window_ext.rs (562, gated on no-op wayland/x11 features). Not reachable from flui-app or the facade
- LinuxPlatform (public, every method unimplemented!) and the `wayland`, `x11`, `web` Cargo features (no-op)
- The `desktop` default feature (enables dep:winit but gates no code)
- The winit-backend configuration on Windows/macOS: compiled and tested in CI, never selected by current_platform()
- PlatformHaptics: only the headless FakeHaptics implements it; no real backend
- PlatformProxy transport: ClosedTransport (Unsupported) on every backend except winit
- Platform::background_executor / BackgroundExecutor / Task: documented as a compatibility surface slated for removal

## Open questions

- Windows for B1: implement IMM32/TSF plus the proxy transport in the native Win32 backend, or ship winit on Windows (it already has IME, the proxy lane and exit/wake hooks)? This decides whether 'one backend per OS' means native or winit on Windows.
- Is winit the long-term Linux backend (drop the LinuxPlatform stub) or a bridge to a native Wayland/X11 backend? What is the plan when winit 0.31 lands and ui-events-winit still pins ^0.30?
- Should the contract/backends split be two crates (contract at L1-2, backends at the composition-root layer) or a contract crate plus one crate per backend? The first matches 'layers, not micro-crates'; the second enables per-backend CI, unsafe budgets and community backends. It depends on how #560 (the external minting seam) is resolved.
- Challenge to ADR-0037's interaction→platform edge: is there any reason flui-interaction needs more than a trait that could live in a dependency-free contract crate?
- What is the PlatformCapability owner-thread execution primitive, given ADR-0039's 'never carries closures' rule: a typed OwnerTask trait object, a per-capability command enum, or a registry keyed by TypeId?
- IME redesign: adopt a Flutter-like TextEditingState pull model, or follow the ui-events/Masonry IME shape with surrounding text? Hypothesis to verify: whether GameActivity is required for Android soft-keyboard IME.
- Web: is a hidden-textarea IME bridge plus a DOM/ARIA a11y mirror in scope for the H0 'web beta candidate', or is web explicitly keyboard/IME-limited until H1?
- Can PlatformWindow drop Send+Sync (owner-owned !Send windows plus a Send handle proxy) without breaking the raster lane's (ADR-0045) need to hold raw-window-handle surfaces off the owner thread?
- Measured on this Windows host only (cargo tree, grep counts). macOS/iOS/Android/web behavior claims come from code reading, and cross-typecheck was not run in this review.

