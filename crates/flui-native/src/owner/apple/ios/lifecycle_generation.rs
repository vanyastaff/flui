//! A scene window's lifecycle generation: it moves on at every background,
//! foreground and detach, and a step that waited (a queued observation, a
//! scene installation) compares the one it carries with the window's to learn
//! whether the window moved on meanwhile.

use std::num::NonZeroU64;

use flui_foundation::{IdExhausted, IdMint};

/// One lifecycle generation of one window, minted by that window's
/// [`LifecycleGenerations`] and compared only with its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleGeneration(NonZeroU64);

/// The issuer of one window's lifecycle generations: none is issued twice, so
/// a stale observation never matches a later generation.
#[derive(Debug, Default)]
pub(crate) struct LifecycleGenerations(IdMint<NonZeroU64>);

impl LifecycleGenerations {
    /// The next generation.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once every generation was issued, from then on.
    pub(crate) fn next(&self) -> Result<LifecycleGeneration, IdExhausted> {
        self.0.next().map(LifecycleGeneration)
    }
}
