//! FLUI — a Flutter-inspired declarative UI framework for Rust with a
//! `wgpu`-backed GPU rendering engine.
//!
//! This crate is the **facade** over the FLUI workspace: it re-exports the
//! layered crates an application author needs, so a downstream consumer can
//! depend on `flui` alone (by path — FLUI is pre-release and not on
//! crates.io) instead of naming each layer.
//!
//! # Quick start
//!
//! ```no_run
//! use flui::prelude::*;
//!
//! #[derive(Clone, StatelessView)]
//! struct Hello;
//!
//! impl StatelessView for Hello {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Container::new()
//!             .color(Color::rgb(18, 18, 24))
//!             .child(Center::new().child(Text::new("Hello, FLUI!")))
//!     }
//! }
//!
//! fn main() -> Result<(), flui::LaunchError> {
//!     flui::run_app(Hello, flui::AppLaunch::new())?;
//!     Ok(())
//! }
//! ```
//!
//! The root passed to [`run_app`] implements [`view::StatelessView`]; compose
//! stateful children beneath it when the screen owns changing state. Create
//! signals in [`view::ViewState::init_state`] with a [`view::LifecycleContext`],
//! read them during `build`, and write them from event callbacks. The
//! [`view::BuildContext`] deliberately does not provide lifecycle capabilities.
//!
//! # Application entry points
//!
//! [`run_app`] takes the root and an [`AppLaunch`]: the [`AppConfig`] (window
//! configuration and application services) and the [`AppCapabilities`] the
//! application registers (overrides of the built-in providers and plugins,
//! ADR-0152). It returns a [`LaunchError`] when the registrations conflict or
//! the platform cannot start. On desktop it drives the native event loop until
//! shutdown; on iOS it enters UIKit; on the web it starts the browser runner.
//! On Android it panics: enable `native` and call
//! `flui::native::android::run_app_android` from `android_main`, passing the
//! system-provided `AndroidApp` and the same [`AppLaunch`].
//!
#![cfg_attr(
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ),
    doc = "For fallible desktop startup, use [`Application::new`] with a root factory and
call [`Application::run`], which returns `Result<(), AppRunError>`. The factory runs
on the owner thread and creates a fresh tree whenever the main window is reopened.
Use [`Application::on_ready`] to obtain an [`app::AppHandle`] for cross-thread show and
quit requests. A successful show-command admission is not window completion:
await its returned request or poll `try_result` to obtain the window result."
)]
//!
//! # Choosing a catalog
//!
//! The base surface — [`widgets`], [`view`], [`animation`], [`run_app`], and
//! [`prelude`] — needs no feature at all; that includes the global widgets
//! localizations in [`widgets`]. The design systems are feature-selected:
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `material` | off | `flui::material` and its everyday glob at `flui::material::prelude` |
//! | `cupertino` | off | `flui::cupertino` |
//! | `assets` | off | `flui::assets`: application-owned decoded file and network images |
//! | `hot-reload` | off | re-exports `flui-hot-reload` as `hot_reload`; install its hook with `AppConfig::with_dev_reload` |
//! | `devtools` | off | re-exports `flui-devtools` as `devtools`: the frame profiler, timeline and inspector counters |
//! | `driver` | off | the driver agent in an optimized build; every debug build serves its windows to a driver launched with an endpoint (ADR-0239) |
//! | `native` | off | `flui::native`, the operating systems' window extensions (Evolving) |
//!
//! Nothing is on by default (ADR-0315 §1): an application names the catalog it
//! uses, e.g. `features = ["material"]`; with no feature the facade still gives
//! the full widget layer, navigation, focus, and media information. A module whose feature is off is
//! **absent**, not empty — `flui::cupertino` without the `cupertino` feature is
//! an unresolved-import error at the use site, which is the diagnostic you
//! want.
//! Enabling `hot-reload` makes the tooling available; it does not install a
//! driver. Desktop and iOS poll an installed development hook, Android drives
//! its scene-frame hook, and the web runner drives no hook. The feature remains
//! compile-safe on web and iOS so workspace feature unification does not break
//! an otherwise supported cross-target build.
//!
//! Additional opt-in capabilities are `testing` (the `flui::testing` headless
//! harness, normally enabled in a development dependency) and `serde`
//! (serialization for framework value types). Native accessibility is not a
//! feature: on Windows, macOS and Linux every window offers its semantics tree
//! to the operating system's assistive technology through AccessKit.
//!
//! `native` publishes the escape hatch, `flui::native`: what one operating
//! system's windows offer beyond the window contract, in `flui::native::windows`
//! and `flui::native::macos`. It is Evolving rather than Stable (ADR-0089 §5),
//! so it is off by default and absent without the feature:
//!
#![cfg_attr(not(feature = "native"), doc = "```compile_fail,E0432")]
#![cfg_attr(feature = "native", doc = "```")]
//! use flui::native::HostWindow;
//! ```
//!
//! # Using a design system
//!
//! [`prelude`] curates the design-neutral application surface. Material owns
//! its everyday glob at `flui::material::prelude`; the full catalogs live at
//! `flui::material` and `flui::cupertino`:
//!
#![cfg_attr(
    feature = "material",
    doc = r#"```no_run
use flui::material::prelude::*;
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct MaterialHello;

impl StatelessView for MaterialHello {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            Scaffold::new()
                .app_bar(AppBar::new().title(Text::new("FLUI")))
                .body(Center::new().child(Text::new("Hello, Material!"))),
        )
    }
}

fn main() -> Result<(), flui::LaunchError> {
    flui::run_app(MaterialHello, flui::AppLaunch::new())
}
```"#
)]
//!
//! # Layers
//!
//! Each re-exported module is one workspace crate; the layering (no upward
//! edges) is documented in `docs/architecture.md`:
//!
//! | Module | Crate | Feature | Layer |
//! |---|---|---|---|
//! | [`geometry`] | `flui-foundation` | — | geometry values (`f64` logical pixels) |
//! | [`foundation`] | `flui-foundation` | — | keys, listenables, diagnostics |
//! | [`painting`] | `flui-painting` | — | custom painting; paint, style and text values |
//! | [`platform`] | `flui-platform` | — | platform contracts and values (brightness, locale, IME) |
//! | [`view`] | `flui-view` | — | View/Element tree |
//! | [`widgets`] | `flui-widgets` | — | user-facing widget catalog |
//! | [`animation`] | `flui-animation` | — | curves, tweens, tickers |
//! | `assets` | `flui-assets` | `assets` | decoded file and network images |
//! | `material` | `flui-material` | `material` | Material Design theming + widget catalog |
//! | `cupertino` | `flui-cupertino` | `cupertino` | iOS-style theming + widget catalog |
//! | [`app`] | selected `flui-app` contracts | — | cross-thread control and host executors |
//! | `native` | `flui-native`, through `flui-app` | `native` | OS window extensions and Android entry points |
//!
//! [`painting`], [`rendering`], and [`interaction`] expose selected authoring
//! contracts for custom drawing, render objects, and gestures. Arena storage,
//! the engine, and platform implementations remain outside this facade.
//! Enable `testing` in a development dependency for deterministic headless tests.
//! `flui::material` and `flui::cupertino` sit *above* [`widgets`] (ADR-0028's
//! design-system decoupling contract — `material --> widgets`,
//! `cupertino --> widgets`, never the reverse), which is why `flui` is on
//! that ADR's allowlist of crates permitted to depend on both: the facade is
//! the app-level aggregation point, not a core crate.

// Every public facade item is documented; keep it that way.
#![deny(missing_docs)]

pub mod interaction;
pub mod painting;
pub mod rendering;
#[cfg(feature = "testing")]
pub mod testing;

pub use flui_animation as animation;
/// Cross-thread application control and host-provided execution contracts.
///
/// Ordinary launch, configuration and window-creation items live at the crate
/// root and in [`prelude`]. This module deliberately does not expose the
/// runner, bindings, embedder or a second prelude.
pub mod app {
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub use flui_app::app::AppHandle;
    pub use flui_app::app::{
        ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture,
        SpawnError,
    };
}
/// Decoded image assets (`flui-assets`). Requires the `assets` feature.
#[cfg(feature = "assets")]
pub use flui_assets as assets;
/// The iOS-style design system (`flui-cupertino`). Requires the `cupertino`
/// feature.
#[cfg(feature = "cupertino")]
pub use flui_cupertino as cupertino;
/// The profiler, timeline and inspector counters (`flui-devtools`). Requires the `devtools`
/// feature.
#[cfg(feature = "devtools")]
pub use flui_devtools as devtools;
pub use flui_foundation as foundation;
pub use flui_foundation::geometry;
/// The typed self-description of a node, and its derive, for application-defined types.
pub use flui_foundation::inspect::Inspect;
/// Development hot-reload support. Requires the `hot-reload` feature.
#[cfg(feature = "hot-reload")]
pub use flui_hot_reload as hot_reload;
/// The Material Design system (`flui-material`). Requires the `material`
/// feature.
#[cfg(feature = "material")]
pub use flui_material as material;
/// OS services from capability crates (ADR-0154 §8), each behind the feature
/// of its name.
///
/// `system` (`flui-system`): the providers of the launcher, whose handle is
/// [`platform::launch::Launcher`], and the file dialogs. An application
/// registers `services::system::SystemPlugin`.
#[cfg(feature = "system")]
pub mod services {
    pub use flui_system as system;
}
/// What one operating system's windows offer beyond the window contract
/// (ADR-0254 §8). Requires the `native` feature; Evolving, not Stable
/// (ADR-0089 §5).
///
/// [`with_window`](native::with_window) runs a closure on the window a
/// presentation is drawn in, by the address a window request resolves to or a
/// close request names; a widget that holds only its `WindowHandle` has no
/// address to give it yet (that path comes with the window handler passed at
/// open, ADR-0254 §4). The Windows and macOS modules have an extension trait
/// implemented for `dyn HostWindow`, the window as the core host hands it out.
/// Its one method narrows the window to that system's view of it (`win32()`,
/// `appkit()`) and answers `None` for a window of any other host (headless,
/// another system), so nothing is asked of a window that cannot take it
/// and nothing panics. The views take and return FLUI types; a raw window or
/// display handle leaves only through `raw-window-handle`'s `HasWindowHandle`
/// and `HasDisplayHandle`, which every window implements. Android's module
/// carries the system-provided `AndroidApp` and its application entry point.
///
/// What a window is created with on one system is a value of that system's
/// type in [`platform::os_options::OsWindowOptions`], given to
/// [`AppConfig::with_os_options`].
#[cfg(feature = "native")]
pub mod native {
    #[cfg(target_os = "android")]
    pub use flui_app::native::android;
    #[cfg(target_os = "macos")]
    pub use flui_app::native::macos;
    #[cfg(windows)]
    pub use flui_app::native::windows;
    pub use flui_app::native::{HostWindow, with_window};
}
/// The platform contracts and input vocabulary (`flui-platform`).
///
/// [`input_capabilities`](flui_platform::input_capabilities) and
/// [`haptics`](flui_platform::haptics), the second haptic tier with its
/// requests and outcomes, are unstable and present only under the additive
/// `unstable-input` feature (ADR-0213 §5, ADR-0210 §9). Without it the modules
/// are private, not merely undocumented:
///
#[cfg_attr(not(feature = "unstable-input"), doc = "```compile_fail,E0603")]
#[cfg_attr(feature = "unstable-input", doc = "```")]
/// use flui::platform::input_capabilities::InputCapabilities;
/// ```
///
#[cfg_attr(not(feature = "unstable-input"), doc = "```compile_fail,E0603")]
#[cfg_attr(feature = "unstable-input", doc = "```")]
/// use flui::platform::haptics::HapticPattern;
/// ```
pub mod platform {
    pub use flui_platform::*;
    /// Handle-less input capabilities (ADR-0213); unstable.
    #[cfg(feature = "unstable-input")]
    pub mod input_capabilities {
        pub use flui_platform::input_capabilities::{InputCapabilities, PointerKindSet};
    }
    // Shadows the glob's module of the same name, so the unstable types are
    // unreachable through the facade without the feature.
    #[cfg(not(feature = "unstable-input"))]
    #[expect(
        hidden_glob_reexports,
        reason = "hiding the unstable module from the glob is the point (ADR-0213 §5)"
    )]
    mod input_capabilities {}
    /// Haptic patterns, requests and outcomes (ADR-0210); unstable.
    #[cfg(feature = "unstable-input")]
    pub mod haptics {
        pub use flui_platform::haptics::{
            ControlPoint, HapticCapabilities, HapticEvent, HapticOutcome, HapticPattern,
            HapticPatternBuilder, HapticPatternError, HapticRequest, HapticTicket, HapticTiming,
            Intensity, ParameterCurve, PatternPlayback, Sharpness, UnitRangeError,
        };
    }
    // The same shadowing for the second haptic tier (ADR-0210 §9).
    #[cfg(not(feature = "unstable-input"))]
    #[expect(
        hidden_glob_reexports,
        reason = "hiding the unstable module from the glob is the point (ADR-0210 §9)"
    )]
    mod haptics {}
}
pub use flui_widgets as widgets;

/// The view layer (`flui-view`): View/Element, `BuildContext`, keys,
/// signals and the widgets binding.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_view::__runtime`, the composition roots' seam (ADR-0081 §4), which
/// is not application surface:
/// `thread_boundary_ui` pins the module-privacy diagnostic alongside valid
/// facade view and internal composition-root callers.
///
/// ```compile_fail,E0603
/// use flui::view::__runtime;
/// ```
pub mod view {
    pub use flui_view::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0081 §4): the composition roots' seam is not facade surface"
    )]
    mod __runtime {}
}

/// Application configuration. Re-exported from `flui-app`.
pub use flui_app::app::AppConfig;
/// The errors a window request can fail with — what [`open_window`] and
/// [`app::AppHandle`] report, and what an [`Application`] window-error observer
/// receives. Re-exported from `flui-app`; desktop only, like the entry points
/// that produce it.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::AppWindowError;
/// Whether an additional window joins the caller's UI runtime or gets its own.
/// Re-exported from `flui-app`; absent on iOS, where no secondary-window entry
/// point exists.
#[cfg(not(target_os = "ios"))]
pub use flui_app::app::WindowPolicy;
/// Open an additional top-level window without widget content.
///
/// Desktop only. Call on the owner thread while its event loop is live.
/// Success can mean creation was admitted for a later owner turn; this API
/// does not provide a completion receiver or a renderer. Use [`open_window`]
/// for widget content. See [`flui_app::app::open_secondary_window`] for policy,
/// deferred-failure, and ownership details. Re-exported from `flui-app`.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::open_secondary_window;
/// Open an additional top-level window with mounted widget content.
///
/// Desktop only. Call on the owner thread while its event loop is live, with
/// [`WindowPolicy::Isolated`]. `WindowPolicy::Shared` is refused with
/// [`AppWindowError::UnsupportedPolicy`]. As with [`open_secondary_window`],
/// success can mean admission for deferred creation rather than completed
/// installation; see [`flui_app::app::open_window`] for the error contract.
/// Re-exported from `flui-app`.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::open_window;
/// The resident-application builder: a reusable main-window factory and
/// windowless startup. Its cross-thread control handle is [`app::AppHandle`].
/// Re-exported from `flui-app`; desktop only.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::{AppControlError, AppRunError, Application, StartupWindow};
/// The application entry point — builds the tree, opens a window, and drives
/// the frame loop with an [`AppLaunch`]'s configuration and capabilities.
/// Re-exported from `flui-app`.
///
/// The root must implement [`view::StatelessView`]. It returns a
/// [`LaunchError`] when the capability registrations conflict or the platform
/// cannot start; on desktop it returns after shutdown and panics on an
/// event-loop failure. On Android it panics: use
/// `flui::native::android::run_app_android` with the system's `AndroidApp`.
pub use flui_app::run_app;
/// What [`run_app`] and Android's native entry point start a run with, the
/// capability registrations they carry, and why starting fails. Re-exported
/// from `flui-app`.
pub use flui_app::{
    AppCapabilities, AppLaunch, BackendFailure, GpuBackend, LaunchError, UnsupportedSystem,
};
/// Registers a font's faces with the app, for measurement, paint and carets.
/// Re-exported from `flui-app`.
///
/// Call on the app's owner thread, before startup or from an app callback.
/// Registration on a worker thread does not update the running app: send the
/// bytes to its owner thread first. Successful registration updates each
/// UI runtime's text layout on its next frame; faces are never removed.
///
/// Invalid font bytes, repeated bytes, and reentry while the runtime is borrowed
/// return [`FontRegistrationError`] without adding a face. See
/// [`flui_app::register_font`] for the individual error variants.
pub use flui_app::{FontRegistrationError, register_font};

/// The design-neutral application prelude: [`flui_widgets::prelude`], the
/// application entry points and their ordinary configuration types.
///
/// This module is independent of Cargo features. Enabling a design system
/// never adds names here or changes what an existing name resolves to. An
/// application that wants Material's everyday glob imports both preludes:
///
/// ```rust
/// # #[cfg(feature = "material")]
/// use flui::material::prelude::*;
/// use flui::prelude::*;
/// ```
///
/// The design-neutral editing primitives are named
/// [`flui_widgets::RawTextField`] and [`flui_widgets::RawTextFormField`], so
/// the two globs do not make Material's `TextField` or `TextFormField`
/// ambiguous. Cupertino's app-shell-shaped catalog remains at `flui::cupertino`.
pub mod prelude {
    pub use flui_app::app::AppConfig;
    #[cfg(not(target_os = "ios"))]
    pub use flui_app::app::WindowPolicy;
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub use flui_app::app::{AppWindowError, open_secondary_window, open_window};
    pub use flui_app::{AppCapabilities, AppLaunch, LaunchError, register_font, run_app};
    pub use flui_widgets::prelude::*;
}
