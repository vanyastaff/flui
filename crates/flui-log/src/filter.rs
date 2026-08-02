//! Event filtering.
//!
//! # One filter, not two
//!
//! An [`EnvFilter`] is the *only* thing that decides whether an event reaches a
//! backend. The historical logger stacked a second `LevelFilter` beside the
//! `EnvFilter` and seeded it from a `Level` field that defaulted to `INFO`, so
//! `RUST_LOG=flui_view=trace` selected the events and the ceiling then threw
//! them away — a filter that could only ever subtract from what the user asked
//! for, and that no directive could raise.
//!
//! This module therefore has no level knob at all. A ceiling cannot be
//! configured because there is nowhere to configure one; the technical maximum
//! of each native backend is likewise pinned wide open (see
//! [`crate::backend`]).

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::ParseError;

/// Directives used when neither the environment nor the application says
/// otherwise.
///
/// `wgpu` is noisy at `info`, and its output is rarely what a FLUI author is
/// looking at.
pub const DEFAULT_DIRECTIVES: &str = "info,wgpu=warn";

/// Environment variable consulted before the configured directives.
pub const DEFAULT_ENV_VAR: &str = "RUST_LOG";

/// Why an [`EnvFilter`] could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FilterError {
    /// The environment variable held a directive string that does not parse.
    ///
    /// Surfaced rather than swallowed: silently falling back to the built-in
    /// default would leave an author staring at output that ignores the
    /// `RUST_LOG` they just set.
    #[error("the `{env_var}` environment variable holds an invalid filter directive: {source}")]
    Environment {
        /// Name of the consulted variable.
        env_var: String,
        /// The underlying parse failure.
        #[source]
        source: ParseError,
    },

    /// The configured directive string does not parse.
    #[error("the configured filter directive {directives:?} is invalid: {source}")]
    Configured {
        /// The rejected directive string.
        directives: String,
        /// The underlying parse failure.
        #[source]
        source: ParseError,
    },
}

/// Which directives select events, and which environment variable may override
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterConfig {
    directives: String,
    env_var: Option<String>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            directives: DEFAULT_DIRECTIVES.to_owned(),
            env_var: Some(DEFAULT_ENV_VAR.to_owned()),
        }
    }
}

impl FilterConfig {
    /// Build a configuration from an explicit directive string.
    ///
    /// The directive string uses [`EnvFilter`] syntax, e.g.
    /// `"info,wgpu=warn,flui_view=trace"`. It is not validated here — call
    /// [`FilterConfig::env_filter`] for that, so a caller sees the failure at
    /// the point it can act on it.
    #[must_use]
    pub fn new(directives: impl Into<String>) -> Self {
        Self {
            directives: directives.into(),
            env_var: Some(DEFAULT_ENV_VAR.to_owned()),
        }
    }

    /// Consult a different environment variable.
    #[must_use]
    pub fn with_env_var(mut self, env_var: impl Into<String>) -> Self {
        self.env_var = Some(env_var.into());
        self
    }

    /// Ignore the environment entirely; the configured directives always win.
    ///
    /// Useful for a test process, or a host that has already resolved its own
    /// configuration and does not want an inherited `RUST_LOG` to reinterpret
    /// it.
    #[must_use]
    pub fn without_env_var(mut self) -> Self {
        self.env_var = None;
        self
    }

    /// The configured directive string.
    #[inline]
    #[must_use]
    pub fn directives(&self) -> &str {
        &self.directives
    }

    /// The environment variable consulted before the directives, if any.
    #[inline]
    #[must_use]
    pub fn env_var(&self) -> Option<&str> {
        self.env_var.as_deref()
    }

    /// Resolve the configuration into an [`EnvFilter`].
    ///
    /// A non-empty value in the configured environment variable wins; otherwise
    /// the configured directives are used. Either way exactly one filter is
    /// produced, and nothing downstream adds a second one.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Environment`] when the environment variable holds
    /// a directive string that does not parse, and [`FilterError::Configured`]
    /// when the configured directives do not.
    pub fn env_filter(&self) -> Result<EnvFilter, FilterError> {
        if let Some(env_var) = &self.env_var
            && let Ok(value) = std::env::var(env_var)
            && !value.trim().is_empty()
        {
            return EnvFilter::try_new(&value).map_err(|source| FilterError::Environment {
                env_var: env_var.clone(),
                source,
            });
        }

        EnvFilter::try_new(&self.directives).map_err(|source| FilterError::Configured {
            directives: self.directives.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::{Layer, Registry};

    use super::*;

    #[test]
    fn default_directives_quieten_wgpu_only() {
        let config = FilterConfig::default();
        assert_eq!(config.directives(), "info,wgpu=warn");
        assert_eq!(config.env_var(), Some("RUST_LOG"));
    }

    #[test]
    fn a_trace_directive_survives_into_the_filters_own_ceiling() {
        // The regression this module exists to prevent: the resolved filter's
        // maximum must be what the directives asked for, so a `trace!` in
        // `flui_view` is reachable. With the historical second `LevelFilter`
        // the effective maximum was pinned at the configured `Level`.
        let filter = FilterConfig::new("info,flui_view=trace")
            .without_env_var()
            .env_filter()
            .expect("`info,flui_view=trace` is a valid directive string");

        assert_eq!(
            <EnvFilter as Layer<Registry>>::max_level_hint(&filter),
            Some(LevelFilter::TRACE)
        );
    }

    #[test]
    fn invalid_configured_directives_are_reported_not_swallowed() {
        let error = FilterConfig::new("=not a directive=")
            .without_env_var()
            .env_filter()
            .expect_err("`=not a directive=` must not parse");

        assert!(matches!(error, FilterError::Configured { .. }));
    }

    #[test]
    fn without_env_var_ignores_the_environment() {
        let config = FilterConfig::new("warn").without_env_var();
        assert_eq!(config.env_var(), None);
    }
}
