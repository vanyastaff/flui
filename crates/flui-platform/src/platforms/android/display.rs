//! The Android display source.
//!
//! The one display this backend reports is still a placeholder: a fixed size
//! and density, with the refresh period unknown. Reading the real display
//! through JNI is a separate source; until then the snapshot makes the same
//! claim the backend always made, and no more.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayInfo, DisplayKey, DisplaySnapshot};

use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;

/// The placeholder display. `Display.getDisplayId` of the built-in screen is 0.
pub(super) fn placeholder_snapshot(
    source: &DisplaySource,
) -> Result<DisplaySnapshot, PlatformError> {
    let displays = source
        .mint(&DisplayKey::AndroidId(0))
        .map(|id| {
            vec![
                DisplayInfo::new(
                    id,
                    "Android Display",
                    Bounds::new(Point::new(0, 0), Size::new(1080, 2340)),
                    2.75,
                )
                .with_primary(true),
            ]
        })
        .unwrap_or_default();
    source.publish(displays)
}
