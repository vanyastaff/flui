//! What a frame became once a backend drew it.
//!
//! [`PresentDisposition`] is the answer a window slot of the raster executor
//! ([`crate::RasterExecutor`]) and the renderer of a scene
//! drawn outside one (`Renderer::render_scene`) give for each frame.

/// What a backend did with the frame it was handed.
///
/// A three-state answer rather than the `bool` this used to be, because the
/// two ways a frame fails to reach `present()` are not the same event and
/// have opposite consequences for the caller's pacing:
///
/// - [`NoDamage`](Self::NoDamage) — nothing was owed. The caller's work is
///   genuinely done and the loop may park.
/// - [`NotShown`](Self::NotShown) — content *was* owed and could not be put
///   on screen. The work was consumed producing the scene and the screen
///   never saw it, so a caller that treats this as "done" loses the frame
///   permanently: on a stack whose redraw rate is bounded by its own
///   fallback deadline (ADR-0058), a frame classified this way is the last
///   wake the loop gets, and a cold start that hits it is a window that
///   stays blank with nothing left to come back for.
///
/// A backend that cannot distinguish the two reports
/// [`NoDamage`](Self::NoDamage) for every skipped frame — the conservative
/// answer, since it never invents a retry the backend did not ask for.
///
/// Deliberately **not** `#[non_exhaustive`], unlike the sibling protocol
/// types around it. Every consumer of this value has to decide what an
/// answer means for pacing, and the bug this type was introduced to fix was
/// exactly a *silent* conflation of two of them: a wildcard arm that folded
/// an unknown future state into "nothing owed" would institutionalise the
/// same mistake for the next state. Exhaustive means a fourth answer cannot
/// be added without the compiler naming every site that must classify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "every site that classifies a frame names each disposition, and a new one must not compile until each does"
)]
pub enum PresentDisposition {
    /// The frame reached `present()`.
    Presented,
    /// Nothing was owed: the backend had no damage to paint.
    NoDamage,
    /// Content was owed and could not be shown — the surface reported
    /// itself unavailable (occluded), its owner had released it for the
    /// duration of a suspend, or the frame unwound before it could
    /// present.
    NotShown,
}

impl PresentDisposition {
    /// Whether the frame reached the screen.
    #[must_use]
    pub fn was_shown(self) -> bool {
        matches!(self, Self::Presented)
    }

    /// Whether the frame owed content it never managed to show, and so
    /// needs a retry rather than being counted as finished.
    #[must_use]
    pub fn is_withheld(self) -> bool {
        matches!(self, Self::NotShown)
    }
}
