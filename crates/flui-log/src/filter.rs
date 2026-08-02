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

    /// The environment variable was present but was not valid Unicode.
    #[error("the `{env_var}` environment variable is not valid Unicode")]
    EnvironmentNotUnicode {
        /// Name of the consulted variable.
        env_var: String,
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

    /// Resolve the configuration into an [`EnvFilter`], reading the configured
    /// environment variable.
    ///
    /// A non-blank value in that variable wins; otherwise the configured
    /// directives are used. Either way exactly one filter is produced, and
    /// nothing downstream adds a second one.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Environment`] when the environment variable holds
    /// a directive string that does not parse, and [`FilterError::Configured`]
    /// when the configured directives do not.
    pub fn env_filter(&self) -> Result<EnvFilter, FilterError> {
        let from_environment = match &self.env_var {
            Some(env_var) => match std::env::var(env_var) {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotPresent) => None,
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err(FilterError::EnvironmentNotUnicode {
                        env_var: env_var.clone(),
                    });
                }
            },
            None => None,
        };

        self.env_filter_from(from_environment.as_deref())
    }

    /// Resolve the configuration against an explicitly supplied environment
    /// value instead of reading the process environment.
    ///
    /// `env_value` is what the configured variable holds, or `None` if it is
    /// unset. Two reasons this is public rather than a test hook: a host that
    /// already resolved its configuration elsewhere can feed it in directly,
    /// and the resolution rules become testable without mutating process-global
    /// state — `std::env::set_var` is `unsafe` in edition 2024 precisely
    /// because another thread may be reading the environment at the same time,
    /// which under a threaded test runner is not a hypothetical.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Environment`] when `env_value` does not parse,
    /// and [`FilterError::Configured`] when the configured directives do not.
    pub fn env_filter_from(&self, env_value: Option<&str>) -> Result<EnvFilter, FilterError> {
        // Trim once and use the *trimmed* value. Testing `value.trim()` for
        // emptiness and then parsing the untrimmed original made
        // `RUST_LOG=' info '` a hard error: a directive string is not
        // whitespace-tolerant, and a trailing newline or a shell quoting
        // accident is not a configuration mistake worth failing over.
        if let Some(env_var) = &self.env_var
            && let Some(directives) = env_value.map(str::trim)
            && !directives.is_empty()
        {
            return EnvFilter::try_new(directives).map_err(|source| FilterError::Environment {
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

    // --- environment resolution
    //
    // Driven through `env_filter_from` rather than `std::env::set_var`, so the
    // cases are deterministic, order-independent, and safe under a threaded
    // test runner. `tests/env_var_is_read_and_trimmed.rs` covers the wiring to
    // the real process environment in a process of its own.

    /// The maximum level a resolved filter admits — the observable difference
    /// between "the directives took effect" and "they were ignored".
    fn max_level(config: &FilterConfig, env_value: Option<&str>) -> Option<LevelFilter> {
        let filter = config
            .env_filter_from(env_value)
            .expect("the directives under test are valid");
        <EnvFilter as Layer<Registry>>::max_level_hint(&filter)
    }

    #[test]
    fn surrounding_whitespace_in_the_environment_value_is_trimmed() {
        // `RUST_LOG=' info,flui_view=trace '` — a trailing newline from a
        // `.env` file or a shell quoting accident. Testing the trimmed value
        // for emptiness and then parsing the untrimmed one made this a hard
        // `ParseLevelFilterError`.
        let config = FilterConfig::new("error");

        assert_eq!(
            max_level(&config, Some(" info,flui_view=trace ")),
            Some(LevelFilter::TRACE),
            "a padded environment value must resolve exactly as its trimmed form does"
        );
    }

    #[test]
    fn a_padded_value_resolves_identically_to_its_trimmed_form() {
        let config = FilterConfig::new("error");

        assert_eq!(
            max_level(&config, Some("\t info,wgpu=warn \n")),
            max_level(&config, Some("info,wgpu=warn")),
            "whitespace around the value must make no difference to the result"
        );
    }

    #[test]
    fn a_whitespace_only_value_falls_through_to_the_configured_directives() {
        // Blank is "unset", not "invalid": it must reach the configured
        // directives rather than produce an error.
        let config = FilterConfig::new("flui_view=debug");

        assert_eq!(
            max_level(&config, Some("   \t\n  ")),
            Some(LevelFilter::DEBUG),
            "a blank environment value must not override, and must not error"
        );
    }

    #[test]
    fn an_unset_variable_falls_through_to_the_configured_directives() {
        let config = FilterConfig::new("flui_view=debug");
        assert_eq!(max_level(&config, None), Some(LevelFilter::DEBUG));
    }

    #[test]
    fn a_genuinely_malformed_environment_value_is_still_reported() {
        // Trimming must not become "repair the value". Anything that is not
        // just surrounding whitespace still fails, and names the variable.
        let error = FilterConfig::new("info")
            .env_filter_from(Some(" =not a directive= "))
            .expect_err("`=not a directive=` must not parse, padded or otherwise");

        match error {
            FilterError::Environment { env_var, .. } => assert_eq!(env_var, "RUST_LOG"),
            other => panic!("expected `FilterError::Environment`, got {other:?}"),
        }
    }

    #[test]
    fn a_disabled_env_var_ignores_even_a_supplied_value() {
        let config = FilterConfig::new("warn").without_env_var();

        assert_eq!(
            max_level(&config, Some("trace")),
            Some(LevelFilter::WARN),
            "`without_env_var` must mean the configured directives always win"
        );
    }
}
