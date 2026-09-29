//! Loading state machine for async asset loading.

use std::fmt;

/// The current loading state of an asset.
///
/// This enum tracks the lifecycle of an asset from initial request through
/// to loaded or error states. It's useful for UI feedback (loading indicators,
/// error messages) and retry logic.
///
/// # State Transitions
///
/// ```text
/// Pending → Loading → Loaded
///                  ↘ Error
/// ```
///
/// # Examples
///
/// ```
/// use flui_assets::LoadState;
///
/// let mut state = LoadState::Pending;
///
/// // Start loading
/// state = LoadState::Loading;
///
/// // Complete successfully
/// state = LoadState::Loaded;
///
/// // Check state
/// assert!(state.is_loaded());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoadState {
    /// Asset load has been requested but not started yet.
    Pending,

    /// Asset is currently being loaded.
    Loading,

    /// Asset has been successfully loaded.
    Loaded,

    /// Asset loading failed.
    Error,
}

impl LoadState {
    /// Returns `true` if the asset is pending.
    #[inline]
    pub const fn is_pending(self) -> bool {
        matches!(self, LoadState::Pending)
    }

    /// Returns `true` if the asset is currently loading.
    #[inline]
    pub const fn is_loading(self) -> bool {
        matches!(self, LoadState::Loading)
    }

    /// Returns `true` if the asset has been loaded successfully.
    #[inline]
    pub const fn is_loaded(self) -> bool {
        matches!(self, LoadState::Loaded)
    }

    /// Returns `true` if asset loading failed.
    #[inline]
    pub const fn is_error(self) -> bool {
        matches!(self, LoadState::Error)
    }

    /// Returns `true` if the asset is either loaded or errored (terminal states).
    #[inline]
    pub const fn is_complete(self) -> bool {
        matches!(self, LoadState::Loaded | LoadState::Error)
    }

    /// Returns `true` if the asset is either pending or loading (in progress).
    #[inline]
    pub const fn is_in_progress(self) -> bool {
        matches!(self, LoadState::Pending | LoadState::Loading)
    }
}

impl Default for LoadState {
    #[inline]
    fn default() -> Self {
        LoadState::Pending
    }
}

impl fmt::Display for LoadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadState::Pending => write!(f, "pending"),
            LoadState::Loading => write!(f, "loading"),
            LoadState::Loaded => write!(f, "loaded"),
            LoadState::Error => write!(f, "error"),
        }
    }
}
