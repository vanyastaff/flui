//! What the tool that started the app does when a client asks it to reload (`run.reload`,
//! ADR-0401).
//!
//! `flui run` rebuilds the application and applies the build at a tier ADR-0393 names. A
//! client reaches it at the run endpoint its session file names, not at the app's endpoint: the
//! app cannot rebuild itself, and the call outlives the process a restart replaces. The answer
//! is the [`ReloadReport`] the run writes for every reload, whatever started it (a save, a key,
//! a call).

use crate::build::Diagnostics;

vocabulary! {
    /// How much of the running application a reload replaces (ADR-0393 §1), cheapest first.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ReloadTier {
        /// New code runs in the elements that are mounted; every `State` is kept.
        Reassemble => "reassemble",
        /// The application is composed again in the same process and windows; its state is not
        /// kept.
        Restart => "restart",
        /// A new process runs the new build.
        ProcessRestart => "process_restart",
    }
}

/// `run.reload`: rebuild the application and apply the build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ReloadParams {
    /// The cheapest tier the reload may apply; absent, the tool places the reload itself. The
    /// tool may apply a higher one, and never a lower one (ADR-0393 §2).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub at_least: Option<ReloadTier>,
}

/// What a reload came to, and how long it took from what started it to its outcome.
///
/// `{"outcome": {"applied": {"tier": "process_restart"}}, "elapsed_ms": 1640}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ReloadReport {
    /// What the reload came to.
    pub outcome: Reloaded,
    /// From the moment the run took what started the reload (a save, a key, or a call as it
    /// reached the run, a build it waited behind included) to its outcome.
    pub elapsed_ms: u64,
}

impl ReloadReport {
    /// `outcome`, reached `elapsed_ms` after the reload started.
    #[must_use]
    pub const fn new(outcome: Reloaded, elapsed_ms: u64) -> Self {
        Self {
            outcome,
            elapsed_ms,
        }
    }
}

/// What a reload came to.
///
/// `{"applied": {"tier": "reassemble"}}`, `"unchanged"`,
/// `{"failed": {"diagnostics": [ … ], "omitted": 0}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Reloaded {
    /// The build succeeded and the tool applied it at `tier`.
    #[non_exhaustive]
    Applied {
        /// The tier applied: the one asked for or a higher one.
        tier: ReloadTier,
    },
    /// The build succeeded with nothing new to apply: it is the build the running application
    /// was started from, so the application runs on as it was.
    Unchanged,
    /// The build failed with these errors. A reload that stops the application before it
    /// builds (a restart, or a platform that cannot replace a running build) leaves none
    /// running; any other leaves the running application as it was. A tool that does not read
    /// the compiler's messages for this target reports no errors.
    Failed(Diagnostics),
}

impl Reloaded {
    /// The build was applied at `tier`.
    #[must_use]
    pub const fn applied(tier: ReloadTier) -> Self {
        Self::Applied { tier }
    }
}
