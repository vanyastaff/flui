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
            return EnvFilter::try_new(with_aliases(directives)).map_err(|source| {
                FilterError::Environment {
                    env_var: env_var.clone(),
                    source,
                }
            });
        }

        EnvFilter::try_new(with_aliases(&self.directives)).map_err(|source| {
            FilterError::Configured {
                directives: self.directives.clone(),
                source,
            }
        })
    }
}

/// Spellings of tracing targets that predate the `flui.<subsystem>` convention, each with the
/// target that replaces it. They are honored for one release: a directive naming an old spelling
/// is read as naming the new one as well.
///
/// The old spelling stays in the filter too, because an event that sets no target of its own
/// still carries its module path (`flui_animation::controller`).
pub const TARGET_ALIASES: &[(&str, &str)] = &[
    ("flui::reconcile", "flui.tree.reconcile"),
    ("flui_animation", "flui.animation"),
    ("flui::signals", "flui.signals"),
    ("flui::a11y", "flui.a11y"),
];

/// `directives` with the replacement of every aliased target appended.
///
/// `flui_animation::diagnostics=debug` also admits `flui.animation.diagnostics=debug`. A bare
/// level (`info`) and a directive that names no old target pass through unchanged.
fn with_aliases(directives: &str) -> String {
    let mut out = directives.to_owned();
    let mut depth = 0_u32;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (at, character) in directives.char_indices() {
        match character {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                pieces.push(&directives[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    pieces.push(&directives[start..]);
    for piece in pieces {
        let piece = piece.trim();
        let target_end = piece.find(['=', '[']).unwrap_or(piece.len());
        let target = &piece[..target_end];
        for (old, new) in TARGET_ALIASES {
            let Some(rest) = target.strip_prefix(old) else {
                continue;
            };
            if !(rest.is_empty() || rest.starts_with("::")) {
                continue;
            }
            out.push(',');
            out.push_str(new);
            out.push_str(&rest.replace("::", "."));
            out.push_str(&piece[target_end..]);
        }
    }
    out
}
