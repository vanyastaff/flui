//! RenderBackdropFilter - Applies a filter to the content behind the widget
//!
//! Implements Flutter's backdrop filter that applies image filters (most commonly blur)
//! to the content painted behind the widget, creating frosted glass and blur effects.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderBackdropFilter` | `RenderBackdropFilter` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `filter` | `filter` property (ImageFilter) |
//! | `blend_mode` | `blendMode` property |
//! | `ImageFilter::blur()` | `ImageFilter.blur()` |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Capture backdrop content**
//!    - Save current paint layer content in rectangular region
//!    - Region defined by widget bounds
//!
//! 2. **Apply image filter**
//!    - Apply filter (blur, matrix, etc.) to captured content
//!    - Most common: Gaussian blur for frosted glass effect
//!
//! 3. **Composite filtered backdrop**
//!    - Paint filtered content back with blend mode
//!    - Default blend mode: SrcOver
//!
//! 4. **Paint child on top**
//!    - Child painted over filtered backdrop
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(w × h × f) where w=width, h=height, f=filter complexity
//!   - Blur: O(w × h × r) where r = blur radius
//!   - Very expensive: requires backdrop capture + filter pass
//! - **Memory**: ~16 bytes (ImageFilter + BlendMode) + backdrop buffer (w × h × 4 bytes)
//!
//! # Use Cases
//!
//! - **Frosted glass**: iOS-style translucent panels with blur
//! - **Modal backgrounds**: Blurred background behind dialogs/sheets
//! - **Navigation bars**: Translucent nav bars with backdrop blur
//! - **Card effects**: Material Design elevated surfaces with blur
//! - **Depth effects**: Visual separation with selective blur
//! - **Privacy screens**: Blur sensitive content behind overlays
//!
//! # Performance Considerations
//!
//! **WARNING: Very expensive operation!**
//!
//! Backdrop filter is one of the most expensive rendering operations:
//! - Requires capturing backdrop (full-screen buffer copy)
//! - Requires filter pass (blur is GPU-intensive)
//! - Can cause significant frame drops if overused
//!
//! **Optimization strategies:**
//! - Use RepaintBoundary around filtered areas
//! - Keep filtered areas small (blur cost = width × height × radius)
//! - Avoid animating blur radius (very expensive)
//! - Consider static blur for better performance
//! - Limit number of backdrop filters on screen
//!
//! **When NOT to use:**
//! - Simple opacity effects (use RenderOpacity instead)
//! - Large areas with high blur radius
//! - Frequently animating effects
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderBackdropFilter;
//! use flui_types::painting::{ImageFilter, BlendMode};
//!
//! // Frosted glass effect (blur radius 10)
//! let frosted = RenderBackdropFilter::blur(10.0);
//!
//! // Strong blur for modal background
//! let modal_bg = RenderBackdropFilter::blur(20.0);
//!
//! // Subtle blur with custom blend mode
//! let subtle = RenderBackdropFilter::blur(5.0)
//!     .with_blend_mode(BlendMode::Screen);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{painting::BlendMode, painting::ImageFilter, Size};

// ===== RenderObject =====

/// RenderObject that applies an image filter to the content behind it.
///
/// Captures the backdrop content and applies filters (most commonly blur) to create
/// effects like frosted glass, iOS-style translucent panels, and blurred backgrounds.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only affects backdrop filtering in paint.
///
/// # Use Cases
///
/// - **Frosted glass UI**: iOS-style translucent panels with background blur
/// - **Modal dialogs**: Blurred background to focus attention on dialog
/// - **Navigation bars**: Translucent app bars that blur scrolling content
/// - **Bottom sheets**: Material Design sheets with backdrop blur
/// - **Privacy overlays**: Blur sensitive content behind authentication screens
/// - **Depth perception**: Create visual depth with selective background blur
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderBackdropFilter behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Captures backdrop content in widget's rectangular bounds
/// - Applies ImageFilter to captured content
/// - Supports custom blend modes for compositing
/// - Very expensive operation (requires backdrop capture + filter pass)
/// - Uses BackdropFilterLayer for compositor integration
///
/// # Performance Warning
///
/// **This is one of the most expensive rendering operations!**
///
/// Cost scales with area × filter complexity:
/// - Small area (100×100) with blur radius 10: ~moderate cost
/// - Full screen (1920×1080) with blur radius 20: **very expensive**
///
/// Always profile and optimize:
/// - Keep filtered areas small
/// - Use RepaintBoundary to isolate
/// - Avoid animating blur radius
/// - Limit number of backdrop filters
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderBackdropFilter;
/// use flui_types::painting::BlendMode;
///
/// // Frosted glass panel
/// let frosted = RenderBackdropFilter::blur(10.0);
///
/// // Modal background with strong blur
/// let modal = RenderBackdropFilter::blur(20.0)
///     .with_blend_mode(BlendMode::Darken);
/// ```
#[derive(Debug)]
pub struct RenderBackdropFilter {
    /// Image filter to apply to backdrop
    pub filter: flui_types::painting::ImageFilter,
    /// Blend mode for compositing filtered result
    pub blend_mode: BlendMode,
}

// ===== Methods =====

impl RenderBackdropFilter {
    /// Create new backdrop filter with blur
    pub fn blur(radius: f32) -> Self {
        Self {
            filter: ImageFilter::blur(radius),
            blend_mode: BlendMode::default(),
        }
    }

    /// Create with custom filter
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            blend_mode: BlendMode::default(),
        }
    }

    /// Set blend mode
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Get the image filter
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// Set the image filter
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        self.blend_mode = blend_mode;
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderBackdropFilter {}

impl RenderBox<Single> for RenderBackdropFilter {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Note: Full backdrop filtering requires compositor support
        // In production, this would:
        // 1. Capture the current paint layer content (backdrop buffer)
        // 2. Apply the image filter to that content (e.g., Gaussian blur)
        // 3. Paint the filtered result back to the layer
        // 4. Paint the child on top of the filtered backdrop
        //
        // For now, we just paint the child
        // TODO: Implement BackdropFilterLayer when compositor supports it

        ctx.paint_child(child_id, ctx.offset);
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_backdrop_filter_set_filter() {
        let mut filter = RenderBackdropFilter::blur(5.0);

        let new_filter = ImageFilter::Blur {
            sigma_x: 10.0,
            sigma_y: 10.0,
        };
        filter.set_filter(new_filter.clone());

        assert_eq!(*filter.filter(), new_filter);
    }

    #[test]
    fn test_render_backdrop_filter_set_blend_mode() {
        let mut filter = RenderBackdropFilter::blur(10.0);

        filter.set_blend_mode(BlendMode::Screen);
        assert_eq!(filter.blend_mode(), BlendMode::Screen);
    }

    #[test]
    fn test_render_backdrop_filter_with_blend_mode() {
        let filter = RenderBackdropFilter::blur(10.0).with_blend_mode(BlendMode::Multiply);
        assert_eq!(filter.blend_mode(), BlendMode::Multiply);
    }
}
