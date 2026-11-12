//! Event filtering.
//!
//! # One filter, not two
//!
//! A [`Targets`] filter is the *only* thing that decides whether an event
//! reaches a backend. The historical logger stacked a second `LevelFilter`
//! beside the directive filter and seeded it from a `Level` field that
//! defaulted to `INFO`, so `RUST_LOG=flui_view=trace` selected the events and
//! the ceiling then threw them away — a filter that could only ever subtract
//! from what the user asked for, and that no directive could raise.
//!
//! This module therefore has no level knob at all. A ceiling cannot be
//! configured because there is nowhere to configure one; the technical maximum
//! of each native backend is likewise pinned wide open (see
//! [`crate::backend`]).
//!
//! # Directives
//!
//! A directive string is a comma-separated list of `target=level` pairs and at
//! most one bare `level` for every other target, as `RUST_LOG` is usually
//! written: `"info,wgpu=warn,flui_view=trace"`. A target matches itself and the
//! module paths below it. Span and field directives (`[span{field=value}]`)
//! and regular expressions are not supported, so an application does not link
//! a regex engine to read its log level.

use tracing_subscriber::filter::{ParseError, Targets};

/// Directives used when neither the environment nor the application says
/// otherwise.
///
/// `wgpu` is noisy at `info`, and its output is rarely what a FLUI author is
/// looking at.
pub const DEFAULT_DIRECTIVES: &str = "info,wgpu=warn";

/// Environment variable consulted before the configured directives.
pub const DEFAULT_ENV_VAR: &str = "RUST_LOG";

/// Why a filter could not be built.
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
        /// Why the directives were refused.
        #[source]
        source: DirectiveError,
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
        /// Why the directives were refused.
        #[source]
        source: DirectiveError,
    },
}

/// Which directives select events, and which environment variable may override
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FilterConfig {
    /// The directive string (see [Directives](self#directives)). It is not validated
    /// until [`FilterConfig::targets`] resolves it.
    pub directives: String,
    /// The environment variable consulted before the directives, if any.
    pub env_var: Option<String>,
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
    /// The directive string is a list of `target=level` pairs (see
    /// [Directives](self#directives)), e.g.
    /// `"info,wgpu=warn,flui_view=trace"`. It is not validated here — call
    /// [`FilterConfig::targets`] for that, so a caller sees the failure at
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
    pub fn env_var(mut self, env_var: impl Into<String>) -> Self {
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

    /// Resolve the configuration into a [`Targets`] filter, reading the configured
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
    pub fn targets(&self) -> Result<Targets, FilterError> {
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

        self.targets_from(from_environment.as_deref())
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
    pub fn targets_from(&self, env_value: Option<&str>) -> Result<Targets, FilterError> {
        // Trim once and use the *trimmed* value. Testing `value.trim()` for
        // emptiness and then parsing the untrimmed original made
        // `RUST_LOG=' info '` a hard error: a directive string is not
        // whitespace-tolerant, and a trailing newline or a shell quoting
        // accident is not a configuration mistake worth failing over.
        if let Some(env_var) = &self.env_var
            && let Some(directives) = env_value.map(str::trim)
            && !directives.is_empty()
        {
            return parse_directives(directives).map_err(|source| FilterError::Environment {
                env_var: env_var.clone(),
                source,
            });
        }

        parse_directives(&self.directives).map_err(|source| FilterError::Configured {
            directives: self.directives.clone(),
            source,
        })
    }
}

/// Why a directive string was refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DirectiveError {
    /// A directive selects by span or field (`target[span{field=value}]=level`),
    /// which the `target=level` filter does not support. Refused rather than
    /// read as a target of that literal name, which would select nothing.
    #[error("`{directive}` selects by span or field; write `target=level`")]
    SpanOrField {
        /// The refused directive.
        directive: String,
    },

    /// A directive does not parse, such as an unknown level.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Parses `directives` (see [Directives](self#directives)).
fn parse_directives(directives: &str) -> Result<Targets, DirectiveError> {
    if let Some(directive) = directives
        .split(',')
        .map(str::trim)
        .find(|directive| directive.contains(['[', ']', '{', '}']))
    {
        return Err(DirectiveError::SpanOrField {
            directive: directive.to_owned(),
        });
    }
    Ok(directives.parse::<Targets>()?)
}
