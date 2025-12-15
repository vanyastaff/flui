//! RenderAligningShiftedBox trait - shifted box with alignment support.
//!
//! This module provides the `RenderAligningShiftedBox` trait which corresponds to
//! Flutter's `RenderAligningShiftedBox` class - an abstract class for one-child-layout
//! render boxes that use alignment to position their children.

use ambassador::delegatable_trait;
use flui_types::{Alignment, Offset, Size};

use super::RenderShiftedBox;

/// Text direction for resolving directional alignments.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `TextDirection` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Right-to-left text direction (e.g., Arabic, Hebrew).
    Rtl,
    /// Left-to-right text direction (e.g., English, Spanish).
    #[default]
    Ltr,
}

/// Trait for shifted boxes that use alignment to position the child.
///
/// RenderAligningShiftedBox extends RenderShiftedBox with alignment functionality:
/// - Stores alignment and text direction
/// - Resolves directional alignments based on text direction
/// - Provides `align_child()` method to set child offset after layout
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderAligningShiftedBox` in Flutter.
///
/// # Usage
///
/// Use when you need to:
/// - Align a child within a larger parent area
/// - Apply width/height factors to size relative to child
/// - Support various alignment configurations (including RTL)
///
/// # Example
///
/// ```ignore
/// impl RenderAligningShiftedBox for MyAlignedBox {
///     fn alignment(&self) -> Alignment {
///         self.alignment
///     }
///
///     fn set_alignment(&mut self, alignment: Alignment) {
///         if self.alignment != alignment {
///             self.alignment = alignment;
///             self.mark_needs_layout();
///         }
///     }
///
///     fn text_direction(&self) -> Option<TextDirection> {
///         self.text_direction
///     }
///
///     fn set_text_direction(&mut self, direction: Option<TextDirection>) {
///         if self.text_direction != direction {
///             self.text_direction = direction;
///             self.mark_needs_layout();
///         }
///     }
///
///     // ... other methods
/// }
/// ```
#[delegatable_trait]
pub trait RenderAligningShiftedBox: RenderShiftedBox {
    // ===== Alignment Properties =====

    /// Returns the alignment used to position the child.
    ///
    /// The x and y values of the alignment control the horizontal and vertical
    /// alignment, respectively:
    /// - x = -1.0: left edge of child aligned with left edge of parent
    /// - x = 1.0: right edge of child aligned with right edge of parent
    /// - x = 0.0: child centered horizontally
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.alignment` getter.
    fn alignment(&self) -> Alignment;

    /// Sets the alignment used to position the child.
    ///
    /// Setting this will trigger a layout update.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.alignment` setter.
    fn set_alignment(&mut self, alignment: Alignment);

    /// Returns the text direction used to resolve the alignment.
    ///
    /// This is required when using directional alignments.
    /// Returns `None` if no text direction is set.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.textDirection` getter.
    fn text_direction(&self) -> Option<TextDirection> {
        None
    }

    /// Sets the text direction used to resolve the alignment.
    ///
    /// This may be changed to `None`, but only after alignment has been
    /// changed to a value that does not depend on the direction.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.textDirection` setter.
    #[allow(unused_variables)]
    fn set_text_direction(&mut self, direction: Option<TextDirection>) {
        // Default: no-op, implementations should override
    }

    // ===== Size Factors =====

    /// Returns the width factor, if any.
    ///
    /// When set, the width is `child_width * width_factor`.
    /// Must be non-negative if set.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderPositionedBox.widthFactor`.
    fn width_factor(&self) -> Option<f32>;

    /// Returns the height factor, if any.
    ///
    /// When set, the height is `child_height * height_factor`.
    /// Must be non-negative if set.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderPositionedBox.heightFactor`.
    fn height_factor(&self) -> Option<f32>;

    // ===== Alignment Resolution =====

    /// Returns the resolved alignment.
    ///
    /// For simple `Alignment` values, this returns the alignment directly.
    /// For directional alignments, this resolves based on `text_direction()`.
    ///
    /// Subclasses should use this instead of `alignment()` directly when
    /// computing the child's offset.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.resolvedAlignment`.
    fn resolved_alignment(&self) -> Alignment {
        let alignment = self.alignment();
        // In Flutter, AlignmentGeometry.resolve(textDirection) handles RTL.
        // For simple Alignment, we flip x for RTL.
        match self.text_direction() {
            Some(TextDirection::Rtl) => Alignment::new(-alignment.x, alignment.y),
            _ => alignment,
        }
    }

    // ===== Child Alignment =====

    /// Computes the child offset based on alignment.
    ///
    /// Call this after laying out the child to compute its offset.
    ///
    /// # Arguments
    ///
    /// * `parent_size` - The size of this render object
    /// * `child_size` - The size of the child after layout
    ///
    /// # Returns
    ///
    /// The offset at which to position the child.
    fn compute_aligned_offset(&self, parent_size: Size, child_size: Size) -> Offset {
        self.resolved_alignment().along_offset(Offset::new(
            parent_size.width - child_size.width,
            parent_size.height - child_size.height,
        ))
    }

    /// Aligns the child within this render object.
    ///
    /// This method must be called after the child has been laid out and
    /// this object's own size has been set. It computes and returns the
    /// offset at which to position the child.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if:
    /// - There is no child
    /// - The child hasn't been laid out (has no size)
    /// - This object hasn't been laid out (has no size)
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.alignChild`.
    fn align_child(&self, own_size: Size, child_size: Size) -> Offset {
        debug_assert!(self.child().is_some(), "alignChild called without a child");
        self.compute_aligned_offset(own_size, child_size)
    }
}
