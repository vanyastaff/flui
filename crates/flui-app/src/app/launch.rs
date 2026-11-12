//! The application builder every platform starts a run from, and how starting
//! fails.

use flui_engine::BackendFailure;
use flui_platform::capability::{Capability, CapabilityProvider, Plugin};
use flui_platform::{Environment, OsRelease};
use flui_view::View;

use super::{AppConfig, capability::AppCapabilities};

/// An application before it runs: its configuration and the capability
/// providers it registers (ADR-0152), on every platform.
///
/// [`run_app`](crate::run_app)`(root)` is `Application::new().run(root)`: the
/// default configuration and the built-in providers alone. An application that
/// configures its run builds one and ends it with [`run`](Self::run); one that
/// lives without a window ends it with `run_resident` (desktop). The builder
/// runs on the thread that will own the application.
///
/// ```rust,ignore
/// Application::new()
///     .config(AppConfig::new().with_title("My App"))
///     .plugin(MyPlugin)
///     .run(MyRoot)?;
/// ```
#[must_use = "an application does nothing until it runs; end the chain with `run`"]
pub struct Application {
    pub(crate) config: AppConfig,
    pub(crate) capabilities: AppCapabilities,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(crate) ready: Option<super::application::ReadyObserver>,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(crate) window_error: Option<super::application::WindowErrorObserver>,
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Application {
    /// The default configuration and the built-in providers alone.
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
            capabilities: AppCapabilities::default(),
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            ready: None,
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            window_error: None,
        }
    }

    /// Run with `config`: the window, the exit policy and the loop's services.
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }

    /// Provide the capability `C` with `provider`, in place of the built-in
    /// provider and of any plugin's (ADR-0152). A widget acquires it with
    /// `ctx.capability::<C>()` in a lifecycle hook.
    ///
    /// Overriding one capability twice fails [`run`](Self::run) with
    /// [`LaunchError::CapabilityConflict`] before the platform starts.
    pub fn capability<C: Capability>(mut self, provider: impl CapabilityProvider<C>) -> Self {
        self.capabilities = std::mem::take(&mut self.capabilities).capability::<C>(provider);
        self
    }

    /// Register the providers of `plugin`. A plugin provides what neither the
    /// application nor the framework does; two plugins providing one
    /// capability fail [`run`](Self::run) with
    /// [`LaunchError::CapabilityConflict`] before the platform starts.
    pub fn plugin(mut self, plugin: impl Plugin) -> Self {
        self.capabilities = std::mem::take(&mut self.capabilities).plugin(plugin);
        self
    }

    /// Run `root` in the main window until the application quits.
    ///
    /// On the desktop it drives the native event loop and returns after
    /// shutdown, once the loop's services are released; each main window,
    /// the first and any a show request reopens, mounts a fresh tree from a
    /// clone of `root`. On iOS it enters UIKit, on the web it starts the browser
    /// runner, and on Android it runs in the Activity's window the
    /// `#[flui::main]` entry was handed.
    ///
    /// # Errors
    ///
    /// [`LaunchError::CapabilityConflict`] when the registrations conflict,
    /// found before the platform starts; [`LaunchError::UnsupportedSystem`]
    /// below FLUI's floor; [`LaunchError::Platform`] when the platform cannot
    /// start or its event loop fails; [`LaunchError::NoAndroidEntry`] on
    /// Android outside a `#[flui::main]` function; and on the desktop the
    /// first window's and the services' failures.
    ///
    /// # Panics
    ///
    /// With the application's own panic, when nothing is left to go on after
    /// it (ADR-0258 §2): a destructor or `on_quit` that panics at the exit, or
    /// the close of the last window. The exit first writes what the
    /// application persisted and stops its services and pools; the panic then
    /// resumes, so the process exits with 101 and the panic's text, printed
    /// once.
    pub fn run<V>(self, root: V) -> Result<(), LaunchError>
    where
        V: View + Clone + 'static,
    {
        super::runner::run(self, root)
    }
}

impl std::fmt::Debug for Application {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Application")
            .field("config", &self.config)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

/// Why an application could not start, or why its run ended in failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchError {
    /// Two capability registrations the run cannot choose between: two
    /// plugins, or two overrides, for one capability, or two capabilities
    /// sharing a name. Found before the platform starts.
    #[error("capability registration failed: {0}")]
    CapabilityConflict(#[source] flui_runtime::capability::CapabilityConflict),
    /// The system is below FLUI's floor (ADR-0244 §3). Nothing started.
    #[error("this system cannot run FLUI: {0}")]
    UnsupportedSystem(#[source] UnsupportedSystem),
    /// The platform could not be initialized, or its event loop failed.
    #[error("platform startup or event loop failed: {source}")]
    Platform {
        /// Original native failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
    /// The main window opened at startup could not be installed.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[error("initial main window failed: {0}")]
    InitialWindow(#[source] super::AppWindowError),
    /// The `on_ready` observer unwound before the first window was created.
    #[error("application on_ready callback panicked")]
    OnReadyPanicked,
    /// A configured application service failed to start.
    #[error("application service startup failed: {source}")]
    Service {
        /// Original service startup failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
    /// On Android, the run was not started from a `#[flui::main]` function:
    /// the system hands its `AndroidApp` only to the `android_main` that
    /// attribute writes, so there is no window to run in.
    #[error("no Android entry: start the application from a `#[flui::main]` function")]
    NoAndroidEntry,
}

/// Why this system cannot run FLUI: it is below the floor FLUI states for its
/// environment (ADR-0244 §3). Found before anything is shown; a release the
/// platform could not read is not below the floor.
///
/// A release is named for a message and a log, never compared: what an
/// application may branch on is a capability, not a version.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UnsupportedSystem {
    /// The system runs a release older than FLUI's floor there.
    #[error("{environment} {found} is below FLUI's floor, {floor}")]
    #[non_exhaustive]
    OsBelowFloor {
        /// The environment whose floor this is.
        environment: Environment,
        /// The release the system reported.
        found: OsRelease,
        /// The lowest release FLUI runs on there.
        floor: OsRelease,
    },
    /// Something the floor is made of, rather than a release, is missing: a
    /// Wayland global, an X11 extension, a graphics API.
    #[error("{environment} lacks {requirement}, which FLUI needs")]
    #[non_exhaustive]
    MissingRequirement {
        /// The environment whose floor this is.
        environment: Environment,
        /// What was looked for.
        requirement: &'static str,
    },
    /// No GPU backend gave a device: neither the one the target prefers nor
    /// its fallback (GLES 3.0, or WebGL2 on the web). Each backend tried is
    /// named with why it failed.
    #[error(
        "no supported GPU: {}",
        tried.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    #[non_exhaustive]
    NoSupportedGpu {
        /// Each backend tried, in order, with why it failed.
        tried: Vec<BackendFailure>,
    },
}

impl UnsupportedSystem {
    /// The refusal `error` is, or `error` itself when it is another failure.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "every failure but the two refusals stays a platform failure, a variant added later included"
    )]
    pub(crate) fn from_platform(
        error: flui_native::PlatformError,
    ) -> Result<Self, flui_native::PlatformError> {
        match error {
            flui_native::PlatformError::UnsupportedOs {
                environment,
                found,
                floor,
            } => Ok(Self::OsBelowFloor {
                environment,
                found,
                floor,
            }),
            flui_native::PlatformError::MissingRequirement {
                environment,
                requirement,
            } => Ok(Self::MissingRequirement {
                environment,
                requirement,
            }),
            other => Err(other),
        }
    }

    /// The refusal a renderer failure is, when no GPU backend gave a device.
    pub(crate) fn from_renderer(error: &(dyn std::error::Error + 'static)) -> Option<Self> {
        if let flui_engine::EngineError::NoSupportedGpu { tried } =
            error.downcast_ref::<flui_engine::EngineError>()?
        {
            Some(Self::NoSupportedGpu {
                tried: tried.clone(),
            })
        } else {
            None
        }
    }
}

impl LaunchError {
    /// A platform failure: the refusal of a system below FLUI's floor, or
    /// any other failure as [`Platform`](Self::Platform).
    pub(crate) fn from_platform(error: flui_native::PlatformError) -> Self {
        match UnsupportedSystem::from_platform(error) {
            Ok(unsupported) => Self::UnsupportedSystem(unsupported),
            Err(error) => Self::Platform {
                source: std::sync::Arc::new(error),
            },
        }
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl LaunchError {
    /// The startup window's failure: the refusal of a system no GPU backend
    /// gave a device (ADR-0244 §1), or the window failure itself.
    pub(crate) fn initial_window(error: super::AppWindowError) -> Self {
        if let super::AppWindowError::Renderer { source } = &error
            && let Some(unsupported) = UnsupportedSystem::from_renderer(source.as_ref())
        {
            return Self::UnsupportedSystem(unsupported);
        }
        Self::InitialWindow(error)
    }
}
