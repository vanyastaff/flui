//! MediaQuery - Window and device information
//!
//! Analogous to Flutter's MediaQuery, this provides information about the
//! current window size, device pixel ratio, and other platform metrics.

use flui_core::{BuildContext, Element};
use flui_types::Size;
use flui_view::{IntoElement, ProviderView};
use std::sync::Arc;

/// Media query data - Window and device information
///
/// Contains information about the current window and display metrics.
/// Analogous to Flutter's MediaQueryData.
///
/// # Example
///
/// ```rust,ignore
/// // In any widget:
/// let media = ctx.depend_on::<MediaQueryData>().unwrap();
/// let size = media.size;
/// let dpr = media.device_pixel_ratio;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQueryData {
    /// Window size in logical pixels
    pub size: Size,

    /// Device pixel ratio (physical pixels per logical pixel)
    ///
    /// - Retina/HiDPI displays: 2.0, 3.0, etc.
    /// - Standard displays: 1.0
    pub device_pixel_ratio: f32,

    /// Text scale factor for accessibility
    ///
    /// Default: 1.0
    /// User can increase for larger text
    pub text_scale_factor: f32,

    /// Whether the device is in portrait orientation
    ///
    /// - Portrait: width < height
    /// - Landscape: width >= height
    pub is_portrait: bool,
}

impl MediaQueryData {
    /// Create media query data from window size
    ///
    /// # Parameters
    ///
    /// - `size`: Window size in logical pixels
    /// - `device_pixel_ratio`: Physical pixels per logical pixel
    pub fn new(size: Size, device_pixel_ratio: f32) -> Self {
        Self {
            size,
            device_pixel_ratio,
            text_scale_factor: 1.0,
            is_portrait: size.width < size.height,
        }
    }

    /// Create with custom text scale factor
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.text_scale_factor = factor;
        self
    }

    /// Get size in physical pixels
    pub fn physical_size(&self) -> Size {
        Size::new(
            self.size.width * self.device_pixel_ratio,
            self.size.height * self.device_pixel_ratio,
        )
    }
}

impl Default for MediaQueryData {
    fn default() -> Self {
        Self::new(Size::new(800.0, 600.0), 1.0)
    }
}

/// MediaQuery provider - Provides window and device information
///
/// This provider exposes MediaQueryData to descendant widgets.
/// Analogous to Flutter's MediaQuery widget.
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::MediaQueryProvider;
///
/// MediaQueryProvider::new(
///     MediaQueryData::new(window_size, scale_factor),
///     child_widget,
/// )
/// ```
pub struct MediaQueryProvider {
    /// The media query data
    data: Arc<MediaQueryData>,

    /// Child element
    child: Option<Element>,
}

impl MediaQueryProvider {
    /// Create a new MediaQuery provider
    ///
    /// # Parameters
    ///
    /// - `data`: Window and device information
    /// - `child`: Child element
    pub fn new(data: MediaQueryData, child: Element) -> Self {
        Self {
            data: Arc::new(data),
            child: Some(child),
        }
    }

    /// Update the media query data
    ///
    /// Called when window resizes or DPI changes.
    pub fn update_data(&mut self, data: MediaQueryData) {
        self.data = Arc::new(data);
    }
}

impl std::fmt::Debug for MediaQueryProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaQueryProvider")
            .field("data", &self.data)
            .finish()
    }
}

impl ProviderView<MediaQueryData> for MediaQueryProvider {
    fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoElement {
        self.child.take().expect("MediaQueryProvider already built")
    }

    fn value(&self) -> Arc<MediaQueryData> {
        self.data.clone()
    }

    fn should_notify(&self, old_value: &MediaQueryData) -> bool {
        // Notify if size or DPI changed
        self.data.size != old_value.size
            || self.data.device_pixel_ratio != old_value.device_pixel_ratio
            || self.data.text_scale_factor != old_value.text_scale_factor
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::Element;

    #[test]
    fn test_media_query_data_creation() {
        let data = MediaQueryData::new(Size::new(1920.0, 1080.0), 2.0);
        assert_eq!(data.size, Size::new(1920.0, 1080.0));
        assert_eq!(data.device_pixel_ratio, 2.0);
        assert!(!data.is_portrait); // Landscape
    }

    #[test]
    fn test_portrait_detection() {
        let portrait = MediaQueryData::new(Size::new(600.0, 800.0), 1.0);
        assert!(portrait.is_portrait);

        let landscape = MediaQueryData::new(Size::new(800.0, 600.0), 1.0);
        assert!(!landscape.is_portrait);
    }

    #[test]
    fn test_physical_size() {
        let data = MediaQueryData::new(Size::new(400.0, 300.0), 2.0);
        let physical = data.physical_size();
        assert_eq!(physical, Size::new(800.0, 600.0));
    }

    #[test]
    fn test_text_scale_factor() {
        let data = MediaQueryData::new(Size::new(800.0, 600.0), 1.0).with_text_scale_factor(1.5);
        assert_eq!(data.text_scale_factor, 1.5);
    }

    #[test]
    fn test_should_notify() {
        let provider = MediaQueryProvider::new(
            MediaQueryData::new(Size::new(800.0, 600.0), 1.0),
            Element::empty(),
        );

        // Same data - no notification
        let same = MediaQueryData::new(Size::new(800.0, 600.0), 1.0);
        assert!(!provider.should_notify(&same));

        // Different size - notify
        let different_size = MediaQueryData::new(Size::new(1024.0, 768.0), 1.0);
        assert!(provider.should_notify(&different_size));

        // Different DPI - notify
        let different_dpi = MediaQueryData::new(Size::new(800.0, 600.0), 2.0);
        assert!(provider.should_notify(&different_dpi));
    }
}
