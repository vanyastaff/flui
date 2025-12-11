//! Aligning mixin — applies alignment to shifted child
//!
//! This module provides AligningShiftedBox<T> for render objects that align their child
//! within available space (e.g., RenderAlign, RenderCenter).
//!
//! # Pattern
//!
//! ```rust,ignore
//! // 1. Define your data
//! #[derive(Clone, Debug)]
//! pub struct AlignData {
//!     pub width_factor: Option<f32>,
//!     pub height_factor: Option<f32>,
//! }
//!
//! // 2. Type alias
//! pub type RenderAlign = AligningShiftedBox<AlignData>;
//!
//! // 3. MUST override perform_layout, can use align_child() helper
//! impl RenderShiftedBox for RenderAlign {
//!     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
//!         let shrink_w = self.width_factor.is_some(); // self.width_factor via Deref!
//!         // ... layout child
//!         self.align_child(child_size, size); // Helper from RenderAligningShiftedBox!
//!         self.set_size(size);
//!         size
//!     }
//! }
//!
//! // AUTO: paint(), hit_test() apply child_offset automatically!
//! // AUTO: align_child() helper for computing offset from alignment!
//! ```

use std::ops::{Deref, DerefMut};

use ambassador::{delegatable_trait, Delegate};
use flui_types::{Alignment, BoxConstraints, Offset, Size};
use flui_types::prelude::TextDirection;

use crate::children::{Child, BoxChild};
use crate::protocol::{Protocol, BoxProtocol};

// Re-export from proxy.rs and shifted.rs
use super::proxy::{HasChild, HasBoxGeometry, ProxyData};
use super::shifted::{HasOffset, ShiftedBase, RenderShiftedBox};

// Import ambassador macros
use super::proxy::{ambassador_impl_HasChild, ambassador_impl_HasBoxGeometry};
use super::shifted::ambassador_impl_HasOffset;

// ============================================================================
// Part 1: Delegatable Trait - HasAlignment
// ============================================================================

/// Trait for accessing alignment and text direction (delegatable)
#[delegatable_trait]
pub trait HasAlignment {
    fn alignment(&self) -> Alignment;
    fn set_alignment(&mut self, alignment: Alignment);
    fn text_direction(&self) -> Option<TextDirection>;
    fn set_text_direction(&mut self, dir: Option<TextDirection>);

    /// Resolve alignment for RTL/LTR (default: return alignment as-is)
    ///
    /// TODO: Handle AlignmentDirectional when needed
    fn resolved_alignment(&self) -> Alignment {
        self.alignment()
    }
}

// ============================================================================
// Part 2: Base Struct - AligningBase<P>
// ============================================================================

/// Base for aligning render objects (internal use)
///
/// Contains ShiftedBase + alignment fields
#[derive(Debug)]
pub struct AligningBase<P: Protocol> {
    pub(crate) shifted: ShiftedBase<P>,
    pub(crate) alignment: Alignment,
    pub(crate) text_direction: Option<TextDirection>,
}

impl<P: Protocol> Default for AligningBase<P>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            shifted: ShiftedBase::default(),
            alignment: Alignment::CENTER,
            text_direction: None,
        }
    }
}

// Implement delegatable traits by forwarding to shifted
impl<P: Protocol> HasChild<P> for AligningBase<P> {
    fn child(&self) -> &Child<P> {
        self.shifted.child()
    }

    fn child_mut(&mut self) -> &mut Child<P> {
        self.shifted.child_mut()
    }
}

// Box specialization - delegate geometry to shifted
impl HasBoxGeometry for AligningBase<BoxProtocol> {
    fn size(&self) -> Size {
        self.shifted.size()
    }

    fn set_size(&mut self, size: Size) {
        self.shifted.set_size(size);
    }
}

// Delegate offset to shifted
impl<P: Protocol> HasOffset for AligningBase<P> {
    fn child_offset(&self) -> Offset {
        self.shifted.child_offset()
    }

    fn set_child_offset(&mut self, offset: Offset) {
        self.shifted.set_child_offset(offset);
    }
}

// Implement HasAlignment for AligningBase
impl<P: Protocol> HasAlignment for AligningBase<P> {
    fn alignment(&self) -> Alignment {
        self.alignment
    }

    fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    fn set_text_direction(&mut self, dir: Option<TextDirection>) {
        self.text_direction = dir;
    }
}

// ============================================================================
// Part 3: Generic AligningShiftedBox<T> with Ambassador + Deref
// ============================================================================

/// Generic aligning shifted render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
///
/// # Automatic Features
///
/// - **HasChild** via Ambassador delegation to `base`
/// - **HasBoxGeometry** via Ambassador delegation to `base`
/// - **HasOffset** via Ambassador delegation to `base`
/// - **HasAlignment** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access
/// - **align_child()** helper method
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct AlignData {
///     pub width_factor: Option<f32>,
///     pub height_factor: Option<f32>,
/// }
///
/// pub type RenderAlign = AligningShiftedBox<AlignData>;
///
/// impl RenderAlign {
///     pub fn new(alignment: Alignment) -> Self {
///         let mut this = AligningShiftedBox::new(AlignData::default());
///         this.set_alignment(alignment);
///         this
///     }
/// }
///
/// impl RenderShiftedBox for RenderAlign {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Layout child
///         let child_size = child.layout(constraints);
///         // Use helper to calculate offset from alignment
///         self.align_child(child_size, container_size);
///         self.set_size(container_size);
///         container_size
///     }
/// }
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
#[delegate(HasOffset, target = "base")]
#[delegate(HasAlignment, target = "base")]
pub struct AligningShiftedBox<T: ProxyData> {
    base: AligningBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> AligningShiftedBox<T> {
    /// Create new AligningShiftedBox with data
    pub fn new(data: T) -> Self {
        Self {
            base: AligningBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access
impl<T: ProxyData> Deref for AligningShiftedBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for AligningShiftedBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 4: RenderAligningShiftedBox - Mixin Trait
// ============================================================================

/// Mixin trait for aligning shifted Box render objects
///
/// Extends RenderShiftedBox with alignment helper.
///
/// **IMPORTANT:** Still need to override `perform_layout` from RenderShiftedBox!
///
/// # Example
///
/// ```rust,ignore
/// impl RenderShiftedBox for RenderAlign {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Layout child
///         let child_size = child.layout(constraints);
///         let size = Size::new(100.0, 100.0);
///
///         // Use align_child() helper!
///         self.align_child(child_size, size);
///
///         self.set_size(size);
///         size
///     }
/// }
/// ```
pub trait RenderAligningShiftedBox: RenderShiftedBox + HasAlignment {
    /// Calculate and set child_offset based on alignment
    ///
    /// This is a helper method that computes the offset needed to align
    /// a child of `child_size` within a container of `container_size`
    /// according to the current alignment settings.
    fn align_child(&mut self, child_size: Size, container_size: Size) {
        let offset = self.resolved_alignment().calculate_offset(child_size, container_size);
        self.set_child_offset(offset);
    }
}

// Blanket impl: all AligningShiftedBox<T> get RenderAligningShiftedBox
impl<T: ProxyData> RenderAligningShiftedBox for AligningShiftedBox<T> {}

// Also need to implement RenderShiftedBox (still panics by default)
impl<T: ProxyData> RenderShiftedBox for AligningShiftedBox<T> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!(
            "perform_layout must be overridden for AligningShiftedBox<{}>",
            std::any::type_name::<T>()
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone, Debug)]
    struct TestData {
        width_factor: Option<f32>,
    }

    #[test]
    fn test_aligning_shifted_box_creation() {
        let aligning = AligningShiftedBox::new(TestData { width_factor: Some(0.5) });
        assert_eq!(aligning.width_factor, Some(0.5)); // Deref works!
    }

    #[test]
    fn test_aligning_shifted_box_deref() {
        let mut aligning = AligningShiftedBox::new(TestData { width_factor: None });

        // Read via Deref
        assert_eq!(aligning.width_factor, None);

        // Write via DerefMut
        aligning.width_factor = Some(1.0);
        assert_eq!(aligning.width_factor, Some(1.0));
    }

    #[test]
    fn test_aligning_shifted_box_child_access() {
        let aligning = AligningShiftedBox::new(TestData::default());

        // HasChild trait methods work via Ambassador
        assert!(!aligning.has_child());
        assert!(aligning.child().is_none());
    }

    #[test]
    fn test_aligning_shifted_box_geometry() {
        let mut aligning = AligningShiftedBox::new(TestData::default());

        // HasBoxGeometry trait methods work via Ambassador
        let size = Size::new(100.0, 50.0);
        aligning.set_size(size);
        assert_eq!(aligning.size(), size);
    }

    #[test]
    fn test_aligning_shifted_box_offset() {
        let mut aligning = AligningShiftedBox::new(TestData::default());

        // HasOffset trait methods work via Ambassador
        let offset = Offset::new(5.0, 10.0);
        aligning.set_child_offset(offset);
        assert_eq!(aligning.child_offset(), offset);
    }

    #[test]
    fn test_aligning_shifted_box_alignment() {
        let mut aligning = AligningShiftedBox::new(TestData::default());

        // HasAlignment trait methods work via Ambassador
        let alignment = Alignment::TOP_LEFT;
        aligning.set_alignment(alignment);
        assert_eq!(aligning.alignment(), alignment);
    }

    #[test]
    fn test_aligning_shifted_box_text_direction() {
        let mut aligning = AligningShiftedBox::new(TestData::default());

        // HasAlignment includes text_direction
        assert_eq!(aligning.text_direction(), None);
        aligning.set_text_direction(Some(TextDirection::Rtl));
        assert_eq!(aligning.text_direction(), Some(TextDirection::Rtl));
    }

    #[test]
    fn test_align_child_helper() {
        let mut aligning = AligningShiftedBox::new(TestData::default());

        // Set alignment to center
        aligning.set_alignment(Alignment::CENTER);

        // Align a 50x50 child in a 100x100 container
        let child_size = Size::new(50.0, 50.0);
        let container_size = Size::new(100.0, 100.0);

        aligning.align_child(child_size, container_size);

        // Should be centered at (25, 25)
        let offset = aligning.child_offset();
        assert_eq!(offset.dx, 25.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    #[should_panic(expected = "perform_layout must be overridden")]
    fn test_aligning_shifted_box_perform_layout_panics_by_default() {
        let mut aligning = AligningShiftedBox::new(TestData::default());
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Should panic because perform_layout is not overridden
        aligning.perform_layout(&constraints);
    }
}
