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
//!     flui::run_app(Hello)
//! }
//! ```
//!
//! The root passed to [`run_app`] is any view, stateless or stateful, as
//! Flutter's `runApp` takes any widget. Create signals in
//! [`view::StatefulView::create_state`] with a [`view::LifecycleContext`], read
//! them during `build`, and write them from event callbacks. The
//! [`view::BuildContext`] deliberately does not provide lifecycle capabilities.
//!
//! # Starting an application
//!
//! [`run_app`]`(root)` starts the same way on every platform: on the desktop
//! it drives the native event loop until shutdown, on iOS it enters UIKit, on
//! the web it starts the browser runner and on Android it runs in the
//! Activity's window. It returns a [`LaunchError`] when the platform cannot
//! start, the system is below FLUI's floor or, on the desktop, the event loop
//! or the first window fails.
//!
//! An application that configures its run builds an [`Application`]: the
//! [`AppConfig`] (window configuration and application services) and the
//! capability providers it registers (overrides of the built-in providers and
//! plugins, ADR-0152), ended by [`Application::run`]. `run_app(root)` is
//! `Application::new().run(root)`.
//!
//! A project that also builds for Android marks the function it starts from
//! with [`main`]: the system loads an Android application as a library and
//! calls an exported `android_main`, which the attribute writes, so the
//! application writes no Android code. On every other platform the
//! attribute leaves the function as it is, and the binary's `fn main` calls
//! it:
//!
//! ```no_run
//! # use flui::prelude::*;
//! # #[derive(Clone, StatelessView)]
//! # struct Hello;
//! # impl StatelessView for Hello {
//! #     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView { Text::new("Hello") }
//! # }
//! // src/lib.rs
//! #[flui::main]
//! pub fn run() -> Result<(), flui::LaunchError> {
//!     flui::run_app(Hello)
//! }
//! ```
//!
#![cfg_attr(
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ),
    doc = "On the desktop, [`Application::on_ready`] gives an [`AppHandle`] for
cross-thread show and quit requests, and [`Application::run_resident`] starts an
application without a window, building each main window it later shows from a
factory. A successful show-command admission is not window completion: await its
returned request or poll `try_result` to obtain the window result."
)]
//!
//! # Choosing a catalog
//!
//! The base surface — [`widgets`], [`view`], [`animation`], [`run_app`], and
//! the non-Material half of [`prelude`] — needs no feature at all; that
//! includes the global widgets localizations in [`widgets`]. The design
//! systems are feature-selected:
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `material` | off | `flui::material` and the Material half of [`prelude`] |
//! | `cupertino` | off | `flui::cupertino` |
//! | `http` | off | the built-in `HttpCapability` provider, each host's own HTTP stack (`ureq` over rustls on Linux); off, an `HttpCapability` request is refused as not registered |
//! | `system` | off | `flui::services::system`: the launcher and file-dialog providers |
//! | `devtools` | off | re-exports `flui-devtools` as `devtools`: the frame profiler, timeline and inspector counters |
//! | `driver` | off | the driver agent in an optimized build; every debug build serves its windows to a driver launched with an endpoint (ADR-0239) |
//! | `native` | off | `flui::native`, the operating systems' window extensions (Evolving) |
//! | `unstable-windows` | off | `open_window`, `open_secondary_window` and `WindowPolicy`: windows beyond the first (unstable) |
//!
//! Nothing is on by default (ADR-0088 §6): an application names the catalog it
//! uses, e.g. `features = ["material"]`; with no feature the facade still gives
//! the full widget layer, navigation, focus, and media information. A module whose feature is off is
//! **absent**, not empty — `flui::cupertino` without the `cupertino` feature is
//! an unresolved-import error at the use site, which is the diagnostic you
//! want.
//! Hot reload is the official package `flui-hot-reload`, which an application
//! adds as a dependency of its own and installs with
//! `AppConfig::dev_reload`; the facade re-exports no package for it
//! (ADR-0311 §6).
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
//! `unstable-windows` publishes the windows beyond the first: `open_window`,
//! `open_secondary_window` and `WindowPolicy`. An application has one window
//! until ADR-0091 §3–§5 are built (`docs/gaps.md`), so they are absent without
//! the feature:
//!
#![cfg_attr(not(feature = "unstable-windows"), doc = "```compile_fail,E0432")]
#![cfg_attr(feature = "unstable-windows", doc = "```no_run")]
//! use flui::WindowPolicy;
//! ```
//!
//! # Using a design system
//!
//! [`prelude`] curates the everyday `flui-widgets` surface plus — when the
//! `material` feature is on — the common Material widgets (see [`prelude`]'s
//! own docs for the curation rule); the full catalogs live at `flui::material`
//! and `flui::cupertino`:
//!
#![cfg_attr(
    feature = "material",
    doc = r#"```no_run
use flui::material::{AppBar, Scaffold, Theme, ThemeData};
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
    flui::run_app(MaterialHello)
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
//! | `material` | `flui-material` | `material` | Material Design theming + widget catalog |
//! | `cupertino` | `flui-cupertino` | `cupertino` | iOS-style theming + widget catalog |
//! | [`app`] | `flui-app` | — | `run_app` + bindings |
//! | `native` | `flui-native`, through `flui-app` | `native` | one operating system's window extensions |
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
pub mod semantics;
#[cfg(feature = "testing")]
pub mod testing;

pub use flui_animation as animation;
pub use flui_app as app;
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
/// open, ADR-0254 §4). Each system's module has an extension trait
/// implemented for `dyn HostWindow`, the window as the core host hands it out.
/// Its one method narrows the window to that system's view of it (`win32()`,
/// `appkit()`) and answers `None` for a window of any other host (headless,
/// another system), so nothing is asked of a window that cannot take it
/// and nothing panics. The views take and return FLUI types; a raw window or
/// display handle leaves only through `raw-window-handle`'s `HasWindowHandle`
/// and `HasDisplayHandle`, which every window implements.
///
/// What a window is created with on one system is a value of that system's
/// type in [`platform::os_options::OsWindowOptions`], given to
/// [`AppConfig::os_options`].
#[cfg(feature = "native")]
pub mod native {
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

/// Application configuration. Re-exported from [`app`] (`flui-app`).
pub use flui_app::app::AppConfig;
/// The errors a window request can fail with — what [`AppHandle`] reports and
/// what an [`Application`] window-error observer receives (and, under
/// `unstable-windows`, what `open_window` answers). Re-exported from [`app`]
/// (`flui-app`); desktop only, like the entry points that produce it.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::AppWindowError;
/// Whether an additional window joins the caller's UI runtime or gets its own.
/// Re-exported from [`app`] (`flui-app`); unstable, under `unstable-windows`
/// (see `open_window`), and absent on iOS, where no secondary-window entry
/// point exists.
#[cfg(all(not(target_os = "ios"), feature = "unstable-windows"))]
pub use flui_app::app::WindowPolicy;
/// Open an additional top-level window without widget content.
///
/// Unstable, under `unstable-windows` (see [`open_window`]). Desktop only.
/// Call on the owner thread while its event loop is live. Success can mean
/// creation was admitted for a later owner turn; this API does not provide a
/// completion receiver or a renderer. Use [`open_window`] for widget content.
/// See [`app::app::open_secondary_window`] for policy, deferred-failure, and
/// ownership details. Re-exported from [`app`] (`flui-app`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    feature = "unstable-windows"
))]
pub use flui_app::app::open_secondary_window;
/// Open an additional top-level window with mounted widget content.
///
/// Unstable: an application has one window unless it turns on the additive
/// `unstable-windows` feature, because ADR-0091 §3–§5 are not built yet
/// (`docs/gaps.md`). Without the feature this entry point,
/// [`open_secondary_window`] and [`WindowPolicy`] are absent.
///
/// Desktop only. Call on the owner thread while its event loop is live, with
/// [`WindowPolicy::Isolated`]. `WindowPolicy::Shared` is refused with
/// [`AppWindowError::UnsupportedPolicy`]. As with [`open_secondary_window`],
/// success can mean admission for deferred creation rather than completed
/// installation; see [`app::app::open_window`] for the error contract.
/// Re-exported from [`app`] (`flui-app`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    feature = "unstable-windows"
))]
pub use flui_app::app::open_window;
/// The handle a desktop [`Application`] gives its `on_ready` observer, which
/// shows the main window or quits from any thread, and why a request through
/// it fails. Re-exported from [`app`] (`flui-app`); desktop only.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::{AppControlError, AppHandle};
/// The function an application starts from on every platform; on Android it
/// writes the `android_main` the system calls. Re-exported from `flui-app`.
pub use flui_app::main;
/// Run `root` in a main window: `Application::new().run(root)`, as Flutter's
/// `runApp` takes the root widget. Re-exported from [`app`] (`flui-app`).
///
/// The root is any [`view::View`], stateless or stateful. It returns a
/// [`LaunchError`] when the platform cannot start, the system is below FLUI's
/// floor or the run fails; on the desktop it returns after shutdown.
pub use flui_app::run_app;
/// The builder of a configured run, and why a run fails. Re-exported from
/// [`app`] (`flui-app`).
pub use flui_app::{Application, BackendFailure, GpuBackend, LaunchError, UnsupportedSystem};
/// Registers a font's faces with the app, for measurement, paint and carets.
/// Re-exported from [`app`] (`flui-app`).
///
/// Call on the app's owner thread, before startup or from an app callback.
/// Registration on a worker thread does not update the running app: send the
/// bytes to its owner thread first. Successful registration updates each
/// UI runtime's text layout on its next frame; faces are never removed.
///
/// Invalid font bytes, repeated bytes, and reentry while the runtime is borrowed
/// return [`FontRegistrationError`] without adding a face. See
/// [`app::register_font`] for the individual error variants.
pub use flui_app::{FontRegistrationError, register_font};

/// Everything an application author needs in scope to write widget code:
/// the widget catalog prelude, [`run_app`], and — with the `material` feature
/// on — the everyday Material widgets.
///
/// # Two halves
///
/// The **base half** ([`flui_widgets::prelude`] plus [`run_app`]) is always
/// present. It is design-system-neutral, so an application that selects no
/// catalog still writes ordinary screens off `use flui::prelude::*`.
///
/// The **Material half** is `#[cfg(feature = "material")]`. Turning the
/// feature off removes those names from the glob rather than replacing them
/// with stubs; a build that was relying on them fails at the use site.
///
/// # Curation rule (Material half)
///
/// This module answers "what does *every* app touch" — not "what does
/// `flui-material` export." A type belongs here when an app author reaches
/// for it while writing ordinary screens: layout/text/interaction primitives
/// (via [`flui_widgets::prelude`]), theming (`Theme`, `ThemeData`,
/// `ColorScheme`, `TextTheme`), the app shell (`Scaffold`, `AppBar`,
/// `Drawer`), the standard button family, and the common
/// data-display/feedback/navigation widgets a Material screen composes
/// (`Card`, `ListTile`, `Dialog`, `SnackBar`, `NavigationBar`, the tab
/// family, …).
///
/// It does **not** include: `*State` handles an app never constructs
/// directly (`InkWellState`, `TabBarState`, …), component-theme override
/// structs (`AppBarThemeData` and friends — advanced per-widget
/// customization, reach them at `flui::material`), or `flui-material`
/// internals like the raw M3 type-scale table (`english_like_2021`).
///
/// **`TextField` names the Material type, unambiguously** — `flui-widgets`'
/// own theme-free stand-in is named [`flui_widgets::RawTextField`], not
/// `TextField`, specifically so this glob never has to choose between two
/// same-named types or make `TextField`'s meaning depend on whether the
/// `material` feature happens to be on. See this module's own
/// `## Mapping decisions` entry in `ARCHITECTURE.md` for the two earlier
/// revisions this superseded (first an omission, then an explicit-shadow
/// that solved the ambiguity but broke feature-additivity). The Cupertino
/// catalog has no such collision (every type is `Cupertino`-prefixed), but
/// its surface is app-shell-shaped rather than everyday-widget-shaped
/// (`CupertinoPageScaffold`, `CupertinoTabScaffold`, …), so it stays at
/// `flui::cupertino` rather than joining this glob.
pub mod prelude {
    pub use flui_app::app::AppConfig;
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub use flui_app::app::AppWindowError;
    #[cfg(all(not(target_os = "ios"), feature = "unstable-windows"))]
    pub use flui_app::app::WindowPolicy;
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32"),
        feature = "unstable-windows"
    ))]
    pub use flui_app::app::{open_secondary_window, open_window};
    pub use flui_app::{Application, LaunchError, register_font, run_app};
    #[cfg(feature = "material")]
    pub use flui_material::{
        AlertDialog, AppBar, BackButton, Card, Checkbox, Chip, ColorScheme, DefaultTabController,
        Dialog, Divider, Drawer, ElevatedButton, FilledButton, FilterChip, FloatingActionButton,
        IconButton, InkWell, ListTile, Material, MaterialApp, NavigationBar, NavigationDestination,
        OutlinedButton, Radio, Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle,
        ScaffoldMessengerScope, SnackBar, Switch, Tab, TabBar, TabBarView, TabController,
        TextButton, TextField, TextFormField, TextTheme, Theme, ThemeData, ThemeMode,
        VerticalDivider, show_dialog,
    };
    pub use flui_widgets::prelude::*;
}
