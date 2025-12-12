//! Integration of mixins with RenderObject and RenderBox traits
//!
//! This module provides blanket implementations that allow mixin types
//! (ProxyBox, ShiftedBox, LeafBox, etc.) to be used directly as RenderBox implementations.
//!
//! # Architecture
//!
//! ```text
//! Mixin Layer (Ambassador + Deref):
//!   ProxyBox<T>, ShiftedBox<T>, LeafBox<T>, ContainerBox<T, PD>
//!             ↓ (via blanket impl)
//! Protocol Layer:
//!   RenderBox<A: Arity> - perform_layout(), paint(), hit_test()
//!             ↓
//! Base Layer:
//!   RenderObject - debug_name(), visit_children()
//! ```
//!
//! # Arity Mapping
//!
//! Each mixin maps to a specific arity:
//! - `ProxyBox<T>` → `RenderBox<Single>` (exactly 1 child)
//! - `ShiftedBox<T>` → `RenderBox<Single>` (exactly 1 child)
//! - `AligningShiftedBox<T>` → `RenderBox<Single>` (exactly 1 child)
//! - `ContainerBox<T, PD>` → `RenderBox<Variable>` (0+ children)
//! - `LeafBox<T>` → `RenderBox<Leaf>` (0 children)
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use flui_rendering::mixins::*;
//! use flui_types::Color;
//!
//! // 1. Define data
//! #[derive(Clone, Debug)]
//! struct ColoredBoxData {
//!     color: Color,
//! }
//!
//! // 2. Create type alias
//! type RenderColoredBox = LeafBox<ColoredBoxData>;
//!
//! // 3. Implement mixin trait
//! impl RenderLeafBox for RenderColoredBox {
//!     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
//!         let size = constraints.biggest();
//!         self.set_size(size);
//!         size
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!         // Paint using self.color (via Deref!)
//!     }
//! }
//!
//! // ✨ Automatically implements:
//! // - RenderObject (via blanket impl)
//! // - RenderBox<Leaf> (via blanket impl)
//! // - Can be used in render tree!
//! ```

use crate::box_render::BoxHitTestResult;
use flui_foundation::Diagnosticable;
use flui_interaction::{HitTestEntry, HitTestTarget};
use flui_types::events::PointerEvent;
use flui_types::{BoxConstraints, Offset, Size};

use crate::{box_render::RenderBox, mixins::*, object::RenderObject, PaintingContext};
use flui_tree::arity::{Leaf, Single, Variable};

// ============================================================================
// Diagnosticable Blanket Implementations
// ============================================================================

impl<T: ProxyData + Send + Sync> Diagnosticable for ProxyBox<T> {}
impl<T: ProxyData + Send + Sync> Diagnosticable for ShiftedBox<T> {}
impl<T: ProxyData + Send + Sync> Diagnosticable for AligningShiftedBox<T> {}
impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> Diagnosticable
    for ContainerBox<T, PD>
{
}
impl<T: ProxyData + Send + Sync> Diagnosticable for LeafBox<T> {}

// ============================================================================
// HitTestTarget Blanket Implementations
// ============================================================================

impl<T: ProxyData + Send + Sync> HitTestTarget for ProxyBox<T> {
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {}
}

impl<T: ProxyData + Send + Sync> HitTestTarget for ShiftedBox<T> {
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {}
}

impl<T: ProxyData + Send + Sync> HitTestTarget for AligningShiftedBox<T> {
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {}
}

impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> HitTestTarget
    for ContainerBox<T, PD>
{
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {}
}

impl<T: ProxyData + Send + Sync> HitTestTarget for LeafBox<T> {
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {}
}

// ============================================================================
// RenderObject Blanket Implementations
// ============================================================================

/// Blanket impl: ProxyBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for ProxyBox<T> {
    fn debug_name(&self) -> &'static str {
        // Use type name of the wrapper
        std::any::type_name::<Self>()
    }
}

/// Blanket impl: ShiftedBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for ShiftedBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Blanket impl: AligningShiftedBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for AligningShiftedBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Blanket impl: ContainerBox<T, PD> implements RenderObject
impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> RenderObject
    for ContainerBox<T, PD>
{
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Blanket impl: LeafBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for LeafBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ============================================================================
// RenderBox Blanket Implementations
// ============================================================================

/// Blanket impl: ProxyBox<T> implements RenderBox<Single>
///
/// Maps the mixin's perform_layout/paint to RenderBox protocol.
impl<T: ProxyData + Send + Sync> RenderBox<Single> for ProxyBox<T>
where
    Self: RenderProxyBoxMixin,
{
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Call mixin's perform_layout
        RenderProxyBoxMixin::perform_layout(self, &constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Call mixin's paint
        RenderProxyBoxMixin::paint(self, ctx, offset);
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Call mixin's hit_test
        RenderProxyBoxMixin::hit_test(self, result, position)
    }

    fn size(&self) -> Size {
        HasBoxGeometry::size(self)
    }
}

/// Blanket impl: ShiftedBox<T> implements RenderBox<Single>
impl<T: ProxyData + Send + Sync> RenderBox<Single> for ShiftedBox<T>
where
    Self: RenderShiftedBox,
{
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        RenderShiftedBox::perform_layout(self, &constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        RenderShiftedBox::paint(self, ctx, offset);
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        RenderShiftedBox::hit_test(self, result, position)
    }

    fn size(&self) -> Size {
        HasBoxGeometry::size(self)
    }
}

/// Blanket impl: AligningShiftedBox<T> implements RenderBox<Single>
impl<T: ProxyData + Send + Sync> RenderBox<Single> for AligningShiftedBox<T>
where
    Self: RenderAligningShiftedBox,
{
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // RenderAligningShiftedBox extends RenderShiftedBox, use its perform_layout
        <Self as RenderShiftedBox>::perform_layout(self, &constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // AligningShiftedBox inherits paint from RenderShiftedBox
        <Self as RenderShiftedBox>::paint(self, ctx, offset);
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // AligningShiftedBox inherits hit_test from RenderShiftedBox
        <Self as RenderShiftedBox>::hit_test(self, result, position)
    }

    fn size(&self) -> Size {
        HasBoxGeometry::size(self)
    }
}

/// Blanket impl: ContainerBox<T, PD> implements RenderBox<Variable>
impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> RenderBox<Variable>
    for ContainerBox<T, PD>
where
    Self: RenderContainerBox<PD>,
{
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        RenderContainerBox::perform_layout(self, &constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        RenderContainerBox::paint(self, ctx, offset);
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        RenderContainerBox::hit_test(self, result, position)
    }

    fn size(&self) -> Size {
        HasBoxGeometry::size(self)
    }
}

/// Blanket impl: LeafBox<T> implements RenderBox<Leaf>
impl<T: ProxyData + Send + Sync> RenderBox<Leaf> for LeafBox<T>
where
    Self: RenderLeafBox,
{
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        RenderLeafBox::perform_layout(self, &constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        RenderLeafBox::paint(self, ctx, offset);
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        RenderLeafBox::hit_test(self, result, position)
    }

    fn size(&self) -> Size {
        HasBoxGeometry::size(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Color;

    #[test]
    fn test_proxy_box_implements_render_object() {
        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        struct TestData {
            value: f32,
        }

        let proxy = ProxyBox::new(TestData { value: 42.0 });

        // Should implement RenderObject
        assert!(!proxy.debug_name().is_empty());
    }

    #[test]
    fn test_leaf_box_implements_render_object() {
        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        struct ColorData {
            color: Color,
        }

        let leaf = LeafBox::new(ColorData {
            color: Color::rgb(255, 0, 0),
        });

        // Should implement RenderObject
        assert!(!leaf.debug_name().is_empty());
    }

    #[test]
    fn test_container_box_implements_render_object() {
        use flui_foundation::RenderId;

        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        struct ContainerData {
            spacing: f32,
        }

        #[derive(Default, Clone, Debug)]
        #[allow(dead_code)]
        struct ParentData {
            offset: Offset,
        }

        let mut container =
            ContainerBox::<ContainerData, ParentData>::new(ContainerData { spacing: 8.0 });

        // Add children
        container
            .children_mut()
            .push(RenderId::new(1), ParentData::default());
        container
            .children_mut()
            .push(RenderId::new(2), ParentData::default());

        // Should implement RenderObject
        assert!(!container.debug_name().is_empty());
        // Children are stored in ContainerBox, not RenderObject trait
        assert_eq!(container.children().len(), 2);
    }
}
