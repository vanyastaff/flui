//! What to log, as whom, and in what shape.

use tracing::Subscriber;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, Layer as _, Registry};

use crate::backend::PlatformLayer;
use crate::filter::{FilterConfig, FilterError};
use crate::identity::AppIdentity;
use crate::ownership::LogBridgePolicy;

/// Shape of the desktop formatter's output.
///
/// Consulted only by the desktop backend; the native sinks have their own,
/// non-negotiable formats.
///
/// Non-exhaustive because one variant is behind a feature flag, so this enum's
/// shape is not a constant of the API — any crate anywhere in the graph
/// enabling `hierarchical` adds a variant to everyone's build through feature
/// unification. Requiring the wildcard arm up front is what stops that from
/// turning into a downstream compile error nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DesktopFormat {
    /// One line per event.
    #[default]
    Compact,

    /// A span tree printed when each root span closes, via `tracing-forest`.
    ///
    /// Readable for a nested build/layout/paint pass, useless for streaming
    /// output — nothing is printed until the root span ends.
    #[cfg(feature = "hierarchical")]
    Hierarchical,
}

/// Everything FLUI's default backend needs to know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogConfig {
    identity: AppIdentity,
    filter: FilterConfig,
    desktop_format: DesktopFormat,
    log_bridge_policy: LogBridgePolicy,
}

impl LogConfig {
    /// Start from the defaults: an unidentified application, `info,wgpu=warn`
    /// overridable by `RUST_LOG`, and compact desktop output.
    pub fn builder() -> LogConfigBuilder {
        LogConfigBuilder {
            config: Self::default(),
        }
    }

    /// How the application names itself to the native sinks.
    #[inline]
    #[must_use]
    pub fn identity(&self) -> &AppIdentity {
        &self.identity
    }

    /// Which events are selected.
    #[inline]
    #[must_use]
    pub fn filter(&self) -> &FilterConfig {
        &self.filter
    }

    /// Shape of the desktop formatter's output.
    #[inline]
    #[must_use]
    pub fn desktop_format(&self) -> DesktopFormat {
        self.desktop_format
    }

    /// Whether managed setup may install the `log` compatibility bridge.
    #[inline]
    #[must_use]
    pub fn log_bridge_policy(&self) -> LogBridgePolicy {
        self.log_bridge_policy
    }

    /// Resolve the filter. Equivalent to `config.filter().env_filter()`.
    ///
    /// # Errors
    ///
    /// Propagates the [`FilterError`] from
    /// [`FilterConfig::env_filter`](crate::FilterConfig::env_filter).
    pub fn env_filter(&self) -> Result<EnvFilter, FilterError> {
        self.filter.env_filter()
    }

    pub(crate) fn without_env_override(&self) -> Self {
        let mut fallback = self.clone();
        fallback.filter = FilterConfig::new(self.filter.directives()).without_env_var();
        fallback
    }

    /// Build the platform's default sink as a composable layer.
    #[must_use]
    pub fn platform_layer<S>(&self) -> PlatformLayer<S>
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        PlatformLayer::platform_default(self)
    }

    /// Build FLUI's default subscriber: a registry and a filtered native sink.
    ///
    /// Nothing global happens here. The result is an ordinary value that a
    /// caller can stack further layers onto before handing it to
    /// [`install_subscriber`](crate::install_subscriber).
    ///
    /// ```rust
    /// use flui_log::LogConfig;
    /// use tracing_subscriber::layer::SubscriberExt as _;
    ///
    /// let subscriber = LogConfig::default().subscriber()?;
    /// // The application's own layer sits above FLUI's default stack.
    /// let _with_extra = subscriber.with(tracing_subscriber::filter::LevelFilter::TRACE);
    /// # Ok::<(), flui_log::FilterError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] when the directives (or the environment variable
    /// overriding them) do not parse. No subscriber is built in that case.
    pub fn subscriber(
        &self,
    ) -> Result<
        impl Subscriber + for<'lookup> LookupSpan<'lookup> + Send + Sync + 'static + use<>,
        FilterError,
    > {
        let filter = self.env_filter()?;
        let platform = PlatformLayer::platform_default(self);
        Ok(Registry::default().with(platform.with_filter(filter)))
    }
}

/// Builder for [`LogConfig`].
#[derive(Debug, Clone)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct LogConfigBuilder {
    config: LogConfig,
}

impl LogConfigBuilder {
    /// Set how the application names itself to the native sinks.
    pub fn identity(mut self, identity: AppIdentity) -> Self {
        self.config.identity = identity;
        self
    }

    /// Set which events are selected.
    pub fn filter(mut self, filter: FilterConfig) -> Self {
        self.config.filter = filter;
        self
    }

    /// Set the filter from a directive string, keeping the default
    /// `RUST_LOG` override.
    pub fn directives(mut self, directives: impl Into<String>) -> Self {
        self.config.filter = FilterConfig::new(directives);
        self
    }

    /// Set the shape of the desktop formatter's output.
    pub fn desktop_format(mut self, format: DesktopFormat) -> Self {
        self.config.desktop_format = format;
        self
    }

    /// Set whether managed setup may install the `log` compatibility bridge.
    pub fn log_bridge_policy(mut self, policy: LogBridgePolicy) -> Self {
        self.config.log_bridge_policy = policy;
        self
    }

    /// Finish the configuration.
    #[must_use]
    pub fn build(self) -> LogConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AppleBundleId, UNIDENTIFIED_APPLE_SUBSYSTEM};

    #[test]
    fn defaults_are_the_documented_ones() {
        let config = LogConfig::default();
        assert_eq!(config.filter().directives(), "info,wgpu=warn");
        assert_eq!(config.filter().env_var(), Some("RUST_LOG"));
        assert_eq!(config.desktop_format(), DesktopFormat::Compact);
        assert_eq!(config.log_bridge_policy(), LogBridgePolicy::Auto);
        assert_eq!(
            config.identity().apple_subsystem(),
            UNIDENTIFIED_APPLE_SUBSYSTEM
        );
    }

    #[test]
    fn the_builder_carries_every_field_through() {
        let identity = AppIdentity::new("My Game")
            .expect("a display name may contain spaces")
            .with_apple_bundle_id(AppleBundleId::new("com.example.mygame").expect("reverse-DNS"));

        let config = LogConfig::builder()
            .identity(identity)
            .directives("warn,flui_view=trace")
            .log_bridge_policy(LogBridgePolicy::Inherit)
            .build();

        assert_eq!(config.identity().display_name(), "My Game");
        assert_eq!(config.identity().apple_subsystem(), "com.example.mygame");
        assert_eq!(config.filter().directives(), "warn,flui_view=trace");
        assert_eq!(config.log_bridge_policy(), LogBridgePolicy::Inherit);
    }

    #[test]
    fn two_subscribers_can_coexist_as_plain_values() {
        // Construction is not installation. Two differently-configured
        // subscribers existing side by side is what lets a host build one,
        // inspect it, stack onto it, and decide separately whether it should
        // become the process-global one. (That it installs nothing is proved
        // behaviourally in `tests/inherit_never_touches_the_global_slot.rs`,
        // which owns its own process.)
        let first = LogConfig::default()
            .subscriber()
            .expect("the default directives parse");
        let second = LogConfig::builder()
            .directives("trace")
            .build()
            .subscriber()
            .expect("`trace` parses");

        // Both are usable as thread-local defaults, which they could not be if
        // building one had consumed the global slot.
        tracing::subscriber::with_default(first, || tracing::info!("first"));
        tracing::subscriber::with_default(second, || tracing::trace!("second"));
    }
}
