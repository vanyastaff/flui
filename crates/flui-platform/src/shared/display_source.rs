//! The display state every native backend keeps: the registry that issues its
//! [`DisplayId`]s and the snapshot it last published.

use flui_platform_api::{DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot};
use parking_lot::Mutex;

use crate::error::PlatformError;

/// One platform instance's display identities and last published snapshot.
///
/// A backend mints ids for the outputs it sampled, hands the resulting
/// [`DisplayInfo`] list to [`publish`](Self::publish) and returns what that
/// answers from `Platform::display_snapshot`. An unchanged sample answers the
/// previous snapshot with its revision, so a consumer that compares revisions
/// sees no change.
pub(crate) struct DisplaySource {
    registry: DisplayRegistry,
    last: Mutex<DisplaySnapshot>,
}

impl DisplaySource {
    pub(crate) fn new() -> Self {
        Self {
            registry: DisplayRegistry::new(),
            last: Mutex::new(DisplaySnapshot::empty()),
        }
    }

    /// The id for `key`, or `None` once the id space is used up: that output is
    /// then left out of the snapshot and its window reports no display.
    pub(crate) fn mint(&self, key: &DisplayKey) -> Option<DisplayId> {
        match self.registry.mint(key) {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::warn!(%error, "an output is left out of the display snapshot");
                None
            }
        }
    }

    /// Publish a fresh sample: the previous snapshot when nothing differs,
    /// otherwise the next revision.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the revision space is used up; the last
    /// snapshot stays published.
    pub(crate) fn publish(
        &self,
        displays: Vec<DisplayInfo>,
    ) -> Result<DisplaySnapshot, PlatformError> {
        let mut last = self.last.lock();
        let next = last
            .successor(displays)
            .map_err(|error| PlatformError::Displays {
                message: error.to_string(),
            })?;
        *last = next.clone();
        Ok(next)
    }
}
