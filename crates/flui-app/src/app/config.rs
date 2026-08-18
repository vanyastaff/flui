//! Application configuration.

#[cfg(feature = "hot-reload")]
use std::path::PathBuf;

use flui_log::AppIdentity;
use flui_types::{Size, geometry::px};

use super::execution::HostExecutors;
use super::frame_failure::FrameFailureHandler;
#[cfg(not(target_arch = "wasm32"))]
use super::lifecycle::ServiceDefinition;
#[cfg(not(target_os = "ios"))]
use super::runtime::ExitPolicy;

/// Default diagnostics policy for a managed application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticsProfile {
    /// Developer-oriented diagnostics with framework debug events enabled.
    Development,

    /// Shipped-application diagnostics limited to actionable warnings/errors.
    Production,
}

impl Default for DiagnosticsProfile {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Development
        } else {
            Self::Production
        }
    }
}

/// Application configuration.
///
/// Use this to customize the app window and behavior.
///
/// # Frame pacing has no field here
///
/// `vsync`/`target_fps` fields previously lived here, advisory-only and
/// never wired to the real present path — issue #556 removed the unwired
/// `vsync`/`target_fps` fields rather than leave a persistently misleading
/// shape a caller could reasonably expect to govern pacing. Both are
/// removed, not deprecated, and the checker mechanically enforces that
/// absence (`docs/runtime-contract.toml`'s `frame-config-effective-or-removed`
/// contract, `forbidden_pattern` entries below). `flui_engine::RasterOptions`
/// (`target_frame_rate`, `max_frames_in_flight`) is the SHAPE a future
/// production frame-pacing surface would take, not one that governs
/// anything today: `RasterOwner` reads its own `RasterOptions` field for
/// nothing (`crates/flui-engine/src/raster_owner.rs`'s own doc: "this module
/// never acts on it"), and `FrameClock::set_min_produce_interval`/
/// `set_max_in_flight` have zero callers outside this workspace's own test
/// code. There is no field here to remove-or-wire because there is no
/// consumer downstream to wire it to yet — that consumer is #559's job, not
/// a claim this removal gets to make in the meantime.
///
/// # Example
///
/// ```rust,ignore
/// let config = AppConfig::new()
///     .with_title("My App")
///     .with_size(1024, 768)
///     .with_resizable(true);
/// ```
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Process-level application identity used by native diagnostics sinks.
    ///
    /// This is deliberately independent from [`Self::title`]: a multi-window
    /// application has one identity and may give every window a different title.
    pub application_identity: AppIdentity,

    /// Diagnostics defaults for a managed application.
    pub diagnostics_profile: DiagnosticsProfile,

    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Window title.
    pub title: String,

    /// Initial window size.
    pub size: Size,

    /// Minimum window size.
    pub min_size: Option<Size>,

    /// Maximum window size.
    pub max_size: Option<Size>,

    /// Whether the window is resizable.
    pub resizable: bool,

    /// Whether to show the window decorations.
    pub decorations: bool,

    /// Whether to start in fullscreen mode.
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// has no fullscreen field to carry this into, and nothing calls
    /// `PlatformWindow::toggle_fullscreen` at startup to honor it. The intended
    /// consumer is the desktop bootstrap in `runner.rs`, once `WindowOptions`
    /// (or a post-creation `toggle_fullscreen` call keyed off this field)
    /// grows the plumbing end to end.
    pub fullscreen: bool,

    /// Whether to show the performance overlay — FPS, average frame time, and
    /// runtime tail-latency/counter telemetry drawn over the app's own content.
    ///
    /// Scope, deliberately narrow: the renderer
    /// (`flui_engine::wgpu::Backend::add_performance_overlay`) draws three rows
    /// but still ignores both the frame counter and the option mask, so
    /// `PerformanceOverlayOption` has no observable effect yet. The sampled
    /// interval is between *composited* frames — an idle frame produces no layer
    /// tree, so it is a repaint rate, not a wall-clock frame rate. The telemetry
    /// row reports p99 present/input latency, deferred/dropped counts, and
    /// whether input attribution was truncated by the bounded per-frame buffer.
    ///
    /// Flutter's `showPerformanceOverlay`. The bootstrap runner forwards this
    /// to `UiRealm::set_performance_overlay`, which is what actually starts
    /// the rolling frame-time window; the frame path then appends a
    /// `PerformanceOverlayLayer` as the root layer's last child. Off costs a
    /// cheap `None` check per frame; snapshot collection, percentile reduction,
    /// and line formatting run only while the overlay is enabled.
    ///
    /// `From<&AppConfig> for flui_platform::WindowOptions` drops this field —
    /// it is a compositing concern, not a window-creation one.
    pub show_performance_overlay: bool,

    /// Whether to enable debug paint.
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// drops this field and no paint-phase debug visualization reads it yet.
    /// Intended consumer: a future paint-phase hook analogous to Flutter's
    /// `debugPaintSizeEnabled`.
    pub debug_paint: bool,

    /// Optional hot-reload worker dylib path for host/worker apps.
    ///
    /// When unset, the desktop runner falls back to `FLUI_WORKER_PLUGIN` for
    /// CLI compatibility.
    #[cfg(feature = "hot-reload")]
    pub worker_plugin_path: Option<PathBuf>,

    /// Governs when the platform loop exits once every hosted window has
    /// closed. See [`ExitPolicy`]'s own doc for the drain-before-decide
    /// rule; `run_desktop`'s bootstrap installs this into the live winit
    /// event loop (issue #555's native-lifecycle wiring). Not present on
    /// iOS: [`ExitPolicy`]/`AppRuntime` are themselves not compiled there
    /// (`run_ios` never reads any field of this config beyond ignoring it
    /// entirely — see that function's own doc).
    #[cfg(not(target_os = "ios"))]
    pub exit_policy: ExitPolicy,

    /// Host-injected background executors (issue #557).
    ///
    /// `None` (the default): the runtime constructs its own worker pools,
    /// lazily, on first background spawn. `Some`: all background work routes
    /// to the host's pools and the default pools are **never** constructed —
    /// the seam by which FLUI embedded next to an engine that already owns a
    /// thread pool avoids oversubscribing the machine. See
    /// [`HostExecutors`]'s own doc for the contract layered on top.
    pub executors: Option<HostExecutors>,

    /// Optional embedder callback receiving every contained frame failure
    /// (issue #561's typed error route). `None` (the default): failures
    /// are still contained and surfaced through `tracing`; only the typed
    /// delivery is skipped. See [`FrameFailureHandler`]'s own doc for the
    /// re-entrancy contract the callback must honor.
    pub frame_failure_handler: Option<FrameFailureHandler>,

    /// Application services to start at bootstrap (issue #558) — durable,
    /// app-lifetime background work with a declared
    /// [`ServiceLifetime`](super::lifecycle::ServiceLifetime) (does the
    /// last window closing stop the app, or does the service keep it
    /// alive?) and a graceful-shutdown contract: at loop exit each service
    /// is cancelled cooperatively, given a bounded flush window, and
    /// joined with evidence. Started by the desktop bootstrap after the
    /// realm install resolves the loop's execution services; a start
    /// failure fails the bootstrap. Native-only today — the lifecycle
    /// layer has no wasm32 slice yet.
    #[cfg(not(target_arch = "wasm32"))]
    pub services: Vec<ServiceDefinition>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            application_identity: AppIdentity::new("FLUI App")
                .expect("BUG: the default application identity is valid"),
            diagnostics_profile: DiagnosticsProfile::default(),
            title: "FLUI App".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            min_size: None,
            max_size: None,
            resizable: true,
            decorations: true,
            fullscreen: false,
            show_performance_overlay: false,
            debug_paint: false,
            #[cfg(feature = "hot-reload")]
            worker_plugin_path: None,
            #[cfg(not(target_os = "ios"))]
            exit_policy: ExitPolicy::default(),
            executors: None,
            frame_failure_handler: None,
            #[cfg(not(target_arch = "wasm32"))]
            services: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the process-level identity used by native diagnostics sinks.
    pub fn with_application_identity(mut self, identity: AppIdentity) -> Self {
        self.application_identity = identity;
        self
    }

    /// Select development or production diagnostics defaults explicitly.
    pub fn with_diagnostics_profile(mut self, profile: DiagnosticsProfile) -> Self {
        self.diagnostics_profile = profile;
        self
    }

    /// Set the initial window size.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = Size::new(px(width as f32), px(height as f32));
        self
    }

    /// Set the minimum window size.
    pub fn with_min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some(Size::new(px(width as f32), px(height as f32)));
        self
    }

    /// Set the maximum window size.
    pub fn with_max_size(mut self, width: u32, height: u32) -> Self {
        self.max_size = Some(Size::new(px(width as f32), px(height as f32)));
        self
    }

    /// Set whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether to show window decorations.
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Set whether to start in fullscreen.
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Enable performance overlay.
    pub fn with_performance_overlay(mut self, show: bool) -> Self {
        self.show_performance_overlay = show;
        self
    }

    /// Enable debug paint.
    pub fn with_debug_paint(mut self, enabled: bool) -> Self {
        self.debug_paint = enabled;
        self
    }

    /// Set the policy governing when the platform loop exits once every
    /// hosted window has closed. See [`ExitPolicy`]'s own doc.
    #[cfg(not(target_os = "ios"))]
    pub fn with_exit_policy(mut self, policy: ExitPolicy) -> Self {
        self.exit_policy = policy;
        self
    }

    /// Set the hot-reload worker dylib path for host/worker apps.
    #[cfg(feature = "hot-reload")]
    pub fn with_worker_plugin_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.worker_plugin_path = Some(path.into());
        self
    }

    /// Inject the host's own background executors. See
    /// [`Self::executors`]'s doc for what this changes.
    pub fn with_executors(mut self, executors: HostExecutors) -> Self {
        self.executors = Some(executors);
        self
    }

    /// Register a typed frame-failure callback. See
    /// [`Self::frame_failure_handler`]'s doc and
    /// [`FrameFailureHandler`]'s re-entrancy contract.
    pub fn with_frame_failure_handler(mut self, handler: FrameFailureHandler) -> Self {
        self.frame_failure_handler = Some(handler);
        self
    }

    /// Register an application service to start at bootstrap. See
    /// [`Self::services`]'s doc for the lifetime and shutdown contract.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_service(mut self, service: ServiceDefinition) -> Self {
        self.services.push(service);
        self
    }
}

impl From<&AppConfig> for flui_platform::WindowOptions {
    fn from(config: &AppConfig) -> Self {
        flui_platform::WindowOptions {
            title: config.title.clone(),
            size: config.size,
            resizable: config.resizable,
            visible: true,
            decorated: config.decorations,
            min_size: config.min_size,
            max_size: config.max_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.title, "FLUI App");
        assert_eq!(config.application_identity.display_name(), "FLUI App");
        assert!(config.resizable);
        #[cfg(feature = "hot-reload")]
        assert!(config.worker_plugin_path.is_none());
    }

    #[test]
    fn test_builder_pattern() {
        let config = AppConfig::new()
            .with_title("Test App")
            .with_size(1024, 768)
            .with_resizable(false);

        assert_eq!(config.title, "Test App");
        assert_eq!(config.size.width, px(1024.0));
        assert_eq!(config.size.height, px(768.0));
        assert!(!config.resizable);
    }

    /// `with_service` appends in declaration order — the order the
    /// bootstrap starts them in.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn with_service_registers_in_declaration_order() {
        use super::super::lifecycle::ServiceLifetime;

        let config = AppConfig::new()
            .with_service(ServiceDefinition::new(
                "first",
                ServiceLifetime::StopsWithLastWindow,
                |_context| Box::pin(async {}),
            ))
            .with_service(ServiceDefinition::new(
                "second",
                ServiceLifetime::KeepsAppAlive,
                |_context| Box::pin(async {}),
            ));
        let names: Vec<&str> = config
            .services
            .iter()
            .map(ServiceDefinition::name)
            .collect();
        assert_eq!(names, ["first", "second"]);
        assert_eq!(
            config.services[1].lifetime(),
            ServiceLifetime::KeepsAppAlive
        );
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn test_worker_plugin_path() {
        let config = AppConfig::new().with_worker_plugin_path("target/debug/libworker.so");

        assert_eq!(
            config.worker_plugin_path,
            Some(PathBuf::from("target/debug/libworker.so"))
        );
    }
}
