//! Structured errors for the layer crate.
//!
//! Added alongside the `catch_unwind` plumbing on
//! `Scene::fire_composition_callbacks` and the `Result`-returning
//! `SceneBuilder::pop`.
//!
//! The error variants are narrow on purpose -- the crate's failure surface
//! is small (programmer error: unknown id / stack underflow / orphan
//! relationship in `LinkRegistry`; runtime: composition callback poison).
//! `anyhow::Error` is never returned from this crate's public API.

use flui_foundation::LayerId;
use thiserror::Error;

use crate::layer::LayerLink;

/// Errors surfaced by the layer crate.
#[derive(Debug, Error)]
pub enum LayerError {
    /// A [`LayerId`] passed to a lookup is not present in the tree.
    ///
    /// Typically signals a stale id leaking from a previous scene.
    #[error("unknown layer id {id:?} in tree")]
    UnknownLayerId {
        /// The id that was not found.
        id: LayerId,
    },

    /// `SceneBuilder::pop` was called on an empty stack.
    ///
    /// Programmer error in the paint phase. Use
    /// [`SceneBuilder::try_pop`] for the panic-free probe form.
    ///
    /// [`SceneBuilder::try_pop`]: crate::compositor::SceneBuilder::try_pop
    #[error("scene builder stack underflow; pop called with empty stack")]
    BuilderStackUnderflow,

    /// A leader is registered in the [`LinkRegistry`] but no follower is
    /// linked to it.
    ///
    /// [`LinkRegistry`]: crate::link_registry::LinkRegistry
    #[error("leader {link:?} has no registered follower")]
    OrphanedLeader {
        /// The link with no follower.
        link: LayerLink,
    },

    /// A follower references a [`LayerLink`] whose leader is not registered.
    #[error("follower {follower:?} references unregistered leader {link:?}")]
    OrphanedFollower {
        /// The follower id.
        follower: LayerId,
        /// The link with no registered leader.
        link: LayerLink,
    },

    /// A composition callback panicked inside `catch_unwind`.
    ///
    /// The panic payload is unwound; the carrier message is best-effort
    /// rendering of the closure type name.
    #[error("composition callback panicked: {panic_type}")]
    CallbackPoisoned {
        /// Best-effort description of the panic payload.
        panic_type: &'static str,
    },
}

/// Result type alias for the layer crate's public API.
pub type LayerResult<T> = Result<T, LayerError>;
