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
//!   RenderBox<A: Arity> - layout(), paint(), hit_test()
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
//!     fn paint(&self, ctx: &mut dyn Any, offset: Offset) {
//!         // Paint using self.color (via Deref!)
//!     }
//! }
//!
//! // ✨ Automatically implements:
//! // - RenderObject (via blanket impl)
//! // - RenderBox<Leaf> (via blanket impl)
//! // - Can be used in render tree!
//! ```

use std::any::Any;

use flui_foundation::{DiagnosticsProperty, RenderId};
use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Offset, Size};

use crate::{
    box_render::RenderBox,
    context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext},
    mixins::*,
    object::RenderObject,
    RenderResult,
};
use flui_tree::arity::{Leaf, Single, Variable};

// ============================================================================
// RenderObject Blanket Implementations
// ============================================================================

/// Blanket impl: ProxyBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for ProxyBox<T> {
    fn debug_name(&self) -> &'static str {
        // Use type name of the wrapper
        std::any::type_name::<Self>()
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(RenderId)) {
        if let Some(child_id) = self.child().get() {
            visitor(child_id);
        }
    }
}

/// Blanket impl: ShiftedBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for ShiftedBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(RenderId)) {
        if let Some(child_id) = self.child().get() {
            visitor(child_id);
        }
    }
}

/// Blanket impl: AligningShiftedBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for AligningShiftedBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(RenderId)) {
        if let Some(child_id) = self.child().get() {
            visitor(child_id);
        }
    }
}

/// Blanket impl: ContainerBox<T, PD> implements RenderObject
impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> RenderObject for ContainerBox<T, PD> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(RenderId)) {
        for (child_id, _) in self.children().iter_with_data() {
            visitor(child_id);
        }
    }
}

/// Blanket impl: LeafBox<T> implements RenderObject
impl<T: ProxyData + Send + Sync> RenderObject for LeafBox<T> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(RenderId)) {
        // No children - leaf node
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
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Call mixin's perform_layout
        let size = self.perform_layout(&ctx.constraints);
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // Call mixin's paint (with type-erased context)
        RenderProxyBoxMixin::paint(self, &mut () as &mut dyn Any, Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        // Call mixin's hit_test
        RenderProxyBoxMixin::hit_test(self, &mut () as &mut dyn Any, ctx.position)
    }
}

/// Blanket impl: ShiftedBox<T> implements RenderBox<Single>
impl<T: ProxyData + Send + Sync> RenderBox<Single> for ShiftedBox<T>
where
    Self: RenderShiftedBox,
{
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        let size = self.perform_layout(&ctx.constraints);
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        RenderShiftedBox::paint(self, &mut () as &mut dyn Any, Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        RenderShiftedBox::hit_test(self, &mut () as &mut dyn Any, ctx.position)
    }
}

/// Blanket impl: AligningShiftedBox<T> implements RenderBox<Single>
impl<T: ProxyData + Send + Sync> RenderBox<Single> for AligningShiftedBox<T>
where
    Self: RenderAligningShiftedBox,
{
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        let size = self.perform_layout(&ctx.constraints);
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // AligningShiftedBox inherits paint from RenderShiftedBox
        <Self as RenderShiftedBox>::paint(self, &mut () as &mut dyn Any, Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        // AligningShiftedBox inherits hit_test from RenderShiftedBox
        <Self as RenderShiftedBox>::hit_test(self, &mut () as &mut dyn Any, ctx.position)
    }
}

/// Blanket impl: ContainerBox<T, PD> implements RenderBox<Variable>
impl<T: ProxyData + Send + Sync, PD: std::fmt::Debug + Send + Sync + 'static> RenderBox<Variable> for ContainerBox<T, PD>
where
    Self: RenderContainerBox<PD>,
{
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
        let size = self.perform_layout(&ctx.constraints);
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
        RenderContainerBox::paint(self, &mut () as &mut dyn Any, Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
        RenderContainerBox::hit_test(self, &mut () as &mut dyn Any, ctx.position)
    }
}

/// Blanket impl: LeafBox<T> implements RenderBox<Leaf>
impl<T: ProxyData + Send + Sync> RenderBox<Leaf> for LeafBox<T>
where
    Self: RenderLeafBox,
{
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
        let size = self.perform_layout(&ctx.constraints);
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
        RenderLeafBox::paint(self, &mut () as &mut dyn Any, Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Leaf>, result: &mut HitTestResult) -> bool {
        RenderLeafBox::hit_test(self, &mut () as &mut dyn Any, ctx.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Color;

    #[test]
    fn test_proxy_box_implements_render_object() {
        #[derive(Clone, Debug)]
        struct TestData {
            value: f32,
        }

        let proxy = ProxyBox::new(TestData { value: 42.0 });

        // Should implement RenderObject
        assert!(!proxy.debug_name().is_empty());

        // visit_children should work (no children yet)
        let mut count = 0;
        proxy.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_leaf_box_implements_render_object() {
        #[derive(Clone, Debug)]
        struct ColorData {
            color: Color,
        }

        let leaf = LeafBox::new(ColorData {
            color: Color::rgb(255, 0, 0),
        });

        // Should implement RenderObject
        assert!(!leaf.debug_name().is_empty());

        // visit_children should work (always 0 for leaf)
        let mut count = 0;
        leaf.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_container_box_implements_render_object() {
        use flui_foundation::RenderId;

        #[derive(Clone, Debug)]
        struct ContainerData {
            spacing: f32,
        }

        #[derive(Default, Clone, Debug)]
        struct ParentData {
            offset: Offset,
        }

        let mut container = ContainerBox::<ContainerData, ParentData>::new(ContainerData {
            spacing: 8.0,
        });

        // Add children
        container
            .children_mut()
            .push(RenderId::new(1), ParentData::default());
        container
            .children_mut()
            .push(RenderId::new(2), ParentData::default());

        // visit_children should iterate all children
        let mut count = 0;
        container.visit_children(&mut |_| count += 1);
        assert_eq!(count, 2);
    }
}
