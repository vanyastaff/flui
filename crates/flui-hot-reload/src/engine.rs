//! Flutter-parity hot reload engine — tiers, outcomes, and host/worker contract.
//!
//! See `docs/designs/2026-06-28-flutter-parity-hot-reload.md` for the full plan.

/// How a code change is applied at runtime (Flutter vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotReloadTier {
    /// Re-run `build()` on the retained element tree; preserve `State`.
    HotReload,

    /// Remount the root widget; dispose all state; keep the host process alive.
    HotRestart,

    /// Kill and respawn the process (`flui run` full restart).
    FullRestart,
}

impl HotReloadTier {
    /// Whether element-tree `State` objects should survive this tier.
    #[must_use]
    pub const fn preserves_state(self) -> bool {
        matches!(self, Self::HotReload)
    }

    /// Whether the host OS process stays alive.
    #[must_use]
    pub const fn preserves_process(self) -> bool {
        !matches!(self, Self::FullRestart)
    }
}

/// Result of attempting a hot reload operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotReloadOutcome {
    /// Reload succeeded at the requested tier.
    Applied(HotReloadTier),

    /// Requested hot reload but degraded (e.g. worker layout fingerprint changed).
    Degraded {
        /// Tier the caller requested.
        requested: HotReloadTier,
        /// Tier actually applied.
        applied: HotReloadTier,
        /// Human-readable reason for the degradation.
        reason: String,
    },

    /// Reload failed; caller should retry after fixing build errors.
    Failed {
        /// Human-readable failure reason.
        reason: String,
    },
}

/// Environment variables for the host/worker split (Phase B).
pub mod env {
    /// Path to the reloadable worker dylib (`my_app_logic.dll`).
    pub const WORKER_PLUGIN: &str = "FLUI_WORKER_PLUGIN";

    /// Legacy scene plugin path (scene-only hot reload, not widget parity).
    pub const SCENE_PLUGIN: &str = "FLUI_SCENE_PLUGIN";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_reload_preserves_state() {
        assert!(HotReloadTier::HotReload.preserves_state());
        assert!(!HotReloadTier::HotRestart.preserves_state());
    }
}
