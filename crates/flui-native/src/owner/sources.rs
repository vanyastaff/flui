//! What a loop keeps of its host's feature sources, as the owner sees it.
//!
//! A source reads the operating system for one feature and wakes the owner
//! when what it read changes. The host's table builds it, the loop holds it
//! while it runs and drops it when it stops, and the feature's position reaches
//! it through the loop. The owner names no feature (ADR-0151 §10): to the loop
//! a source is one of these traits, or a loop's own hook beside them.

use flui_platform::SystemPreferences;

use crate::error::PlatformError;

/// The system preference source a loop keeps for its host's preference
/// position (ADR-0172).
pub(crate) trait SystemPreferenceSource {
    /// The system's preferences, as `Platform::preferences` reports them.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the system cannot be read now, and
    /// [`PlatformError::PreferencesDeferred`] while the first read waits for
    /// the owner.
    fn sample(&self) -> Result<SystemPreferences, PlatformError>;

    /// The application delivered `values`, which it read through
    /// [`sample`](Self::sample) (`Platform::preferences_delivered`). A source
    /// that counts a value as accepted only once it is delivered (the AppKit
    /// and Android source) settles it here; every other source says, in its
    /// own no-op, why it has nothing to settle.
    fn delivered(&self, values: &SystemPreferences);
}
