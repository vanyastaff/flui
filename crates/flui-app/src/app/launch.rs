//! What a platform entry point starts a run with, and how starting fails.

use flui_engine::BackendFailure;
use flui_platform::{Environment, OsRelease};

use super::{AppConfig, capability::AppCapabilities};

/// What a run starts with: its configuration and the capability providers it
/// registers (ADR-0152). Each platform's entry point takes one, so an option
/// added here reaches every platform without a new entry point.
///
/// ```rust,ignore
/// flui::run_app(
///     MyRoot,
///     AppLaunch::new()
///         .config(AppConfig::new().with_title("My App"))
///         .capabilities(AppCapabilities::new().plugin(MyPlugin)),
/// )?;
/// ```
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AppLaunch {
    pub(crate) config: AppConfig,
    pub(crate) capabilities: AppCapabilities,
}

impl AppLaunch {
    /// The default configuration and the built-in providers alone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run with `config`.
    #[must_use = "the builder returns the updated launch; assign or chain it"]
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }

    /// Register `capabilities`: the application's overrides and plugins.
    #[must_use = "the builder returns the updated launch; assign or chain it"]
    pub fn capabilities(mut self, capabilities: AppCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// Why a platform entry point could not start the application.
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
    /// The platform could not be initialized.
    #[error("platform startup failed: {source}")]
    Platform {
        /// Original native failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
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
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    pub(crate) fn from_platform(error: flui_native::PlatformError) -> Self {
        match UnsupportedSystem::from_platform(error) {
            Ok(unsupported) => Self::UnsupportedSystem(unsupported),
            Err(error) => Self::Platform {
                source: std::sync::Arc::new(error),
            },
        }
    }
}
