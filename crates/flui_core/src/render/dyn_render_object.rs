//! DynRender - Object-safe trait for heterogeneous Render storage
//!
//! This module provides the `DynRender` trait, which enables storing
//! different types of Renders in heterogeneous collections.
//!
//! # Design Pattern: Typed + Dynamic
//!
//! FLUI uses a two-level approach for Renders:
//!
//! 1. **Render** (typed trait) - Zero-cost concrete usage with arity constraints
//! 2. **DynRender** (this trait) - Object-safe for `Box<dyn DynRender>` storage
//!
//! This allows:
//! - **Compile-time safety** when working with concrete Render types
//! - **Runtime flexibility** for heterogeneous storage in ElementTree
//! - **Zero-cost abstractions** where types are known statically
//! - **Dynamic dispatch** only when necessary (e.g., tree traversal)
//!
//! # Why DynRender?
//!
//! The `Render` trait has associated types (`Arity`), which makes it not object-safe.
//! You cannot create `Box<dyn Render>` or store different Render types together.
//!
//! `DynRender` solves this by being object-safe - it doesn't have associated types.
//! All types that implement `Render` automatically implement `DynRender` via
//! a blanket implementation.
//!
//! # Usage Pattern
//!
//! ```rust,ignore
//! // Concrete types use Render (zero-cost)
//! impl Render for RenderParagraph {
//!     type Arity = LeafArity;
//!     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size { /* ... */ }
//! }
//!
//! // ElementTree stores heterogeneous Renders via DynRender
//! struct ElementTree {
//!     render_objects: Vec<Box<dyn DynRender>>,
//! }
//!
//! // Can downcast back to concrete types when needed
//! let paragraph = render_object.downcast_ref::<RenderParagraph>().unwrap();
//! ```
//!
//! # Naming Convention
//!
//! The `Dyn*` prefix follows Rust convention for object-safe trait variants.
//! See also: `DynWidget`, `DynElement` in other FLUI modules.

use std::any::Any;
use std::fmt;

use downcast_rs::{DowncastSync, impl_downcast};
use flui_engine::BoxedLayer;
use flui_types::constraints::BoxConstraints;
use flui_types::{Offset, Size};

use crate::element::{ElementId, ElementTree};

/// Object-safe base trait for all Renders
///
/// This trait is automatically implemented for all types that implement `Render`.
/// It provides the minimal object-safe interface needed for heterogeneous storage.
///
/// # Design Principles
///
/// 1. **Object Safety**: No associated types, no generic methods
/// 2. **Minimal Interface**: Only methods needed for tree operations
/// 3. **Downcast Support**: Can convert back to concrete types via `downcast_rs`
/// 4. **State Separation**: RenderState is stored separately in ElementTree
///
/// # When to Use Each Trait
///
/// - Use `Render` when working with concrete types (layout/paint implementation)
/// - Use `DynRender` when storing in heterogeneous collections
/// - Use `downcast_ref/mut` to convert from `DynRender` back to concrete type
///
/// # Example
///
/// ```rust,ignore
/// // Heterogeneous storage in ElementTree
/// let render_objects: Vec<Box<dyn DynRender>> = vec![
///     Box::new(RenderParagraph::new("Hello")),
///     Box::new(RenderImage::new(image)),
///     Box::new(RenderFlex::new()),
/// ];
///
/// // Later, downcast to concrete type
/// for render_obj in &render_objects {
///     if let Some(paragraph) = render_obj.downcast_ref::<RenderParagraph>() {
///         println!("Text: {}", paragraph.text);
///     }
/// }
/// ```
pub trait DynRender: DowncastSync + fmt::Debug + Send + Sync {
    // ========== Core Identity ==========

    /// Get the arity (child count constraint) for this Render
    ///
    /// Returns the arity as a runtime value:
    /// - `Some(0)` for LeafArity (no children)
    /// - `Some(1)` for SingleArity (exactly one child)
    /// - `None` for MultiArity (variable count)
    ///
    /// This is the runtime equivalent of the compile-time `Render::Arity` type.
    fn arity(&self) -> Option<usize>;

    /// Get the debug name for this Render
    ///
    /// Returns the type name for debugging and diagnostics.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    // ========== Layout ==========

    /// Compute intrinsic width for a given height
    ///
    /// Returns the minimum width this Render would prefer at the given height.
    /// Returns `None` if no intrinsic width is defined.
    ///
    /// # Arguments
    ///
    /// - `height`: The height constraint (may be infinite)
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Compute intrinsic height for a given width
    ///
    /// Returns the minimum height this Render would prefer at the given width.
    /// Returns `None` if no intrinsic height is defined.
    ///
    /// # Arguments
    ///
    /// - `width`: The width constraint (may be infinite)
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    // ========== Dynamic Layout & Paint ==========

    /// Perform layout dynamically (for pipeline use)
    ///
    /// This method creates the correctly-typed LayoutCx<Arity> and calls
    /// the typed `Render::layout()` method.
    ///
    /// Used by RenderPipeline for dynamic dispatch during the layout phase.
    ///
    /// # Arguments
    ///
    /// - `tree`: Reference to the element tree
    /// - `element_id`: This element's ID
    /// - `constraints`: Layout constraints from parent
    ///
    /// # Returns
    ///
    /// The size this Render computed
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        element_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size;

    /// Perform paint dynamically (for pipeline use)
    ///
    /// This method creates the correctly-typed PaintCx<Arity> and calls
    /// the typed `Render::paint()` method.
    ///
    /// Used by RenderPipeline for dynamic dispatch during the paint phase.
    ///
    /// # Arguments
    ///
    /// - `tree`: Reference to the element tree
    /// - `element_id`: This element's ID
    /// - `offset`: Painting offset from parent
    ///
    /// # Returns
    ///
    /// The layer tree produced by this Render
    fn dyn_paint(&self, tree: &ElementTree, element_id: ElementId, offset: Offset) -> BoxedLayer;

    // ========== Lifecycle ==========

    /// Called when this Render is attached to the tree
    ///
    /// Override to perform initialization when added to the ElementTree.
    fn attach(&mut self) {
        // Default: no-op
    }

    /// Called when this Render is detached from the tree
    ///
    /// Override to perform cleanup when removed from the ElementTree.
    fn detach(&mut self) {
        // Default: no-op
    }

    /// Dispose of this Render
    ///
    /// Called when the Render is permanently removed.
    /// Override to clean up resources (textures, handles, etc.).
    fn dispose(&mut self) {
        // Default: no-op
    }

    // ========== Downcasting ==========

    /// Get as Any for downcasting
    ///
    /// This enables `downcast_ref`/`downcast_mut` via the `DowncastSync` trait.
    fn as_any(&self) -> &dyn Any;

    /// Get as Any (mutable) for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Enable downcasting for DynRender trait objects
impl_downcast!(sync DynRender);

/// Boxed Render trait object
///
/// Commonly used for heterogeneous collections of Renders.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::BoxedRender;
///
/// let children: Vec<BoxedRender> = vec![
///     Box::new(RenderParagraph::new("Hello")),
///     Box::new(RenderImage::new(image)),
/// ];
/// ```
pub type BoxedRender = Box<dyn DynRender>;

// ========== Blanket Implementation ==========

/// Blanket implementation: all Render types are also DynRender
///
/// This automatically provides DynRender for any type implementing Render.
/// The implementation bridges between the typed Render API and the dynamic
/// DynRender API.
impl<T> DynRender for T
where
    T: crate::render::RenderObjectTrait + fmt::Debug,
{
    fn arity(&self) -> Option<usize> {
        // Arity no longer has CHILD_COUNT constant
        // Return None for now - this needs refactoring
        None
    }

    fn debug_name(&self) -> &'static str {
        <T as crate::Render>::debug_name(self)
    }

    fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        <T as crate::Render>::intrinsic_width(self, height)
    }

    fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        <T as crate::Render>::intrinsic_height(self, width)
    }

    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        element_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        use crate::render::LayoutCx;
        let mut cx = LayoutCx::<T::Arity>::new(tree, element_id, constraints);
        <T as crate::Render>::layout(self, &mut cx)
    }

    fn dyn_paint(&self, tree: &ElementTree, element_id: ElementId, offset: Offset) -> BoxedLayer {
        use crate::render::PaintCx;
        let cx = PaintCx::<T::Arity>::new(tree, element_id, offset);
        <T as crate::Render>::paint(self, &cx)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutCx, LeafArity, MultiArity, PaintCx, Render, SingleArity};
    use flui_engine::{BoxedLayer, ContainerLayer};
    use flui_types::Size;

    // Test Renders
    #[derive(Debug)]
    struct TestLeaf;

    impl Render for TestLeaf {
        type Arity = LeafArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestSingle;

    impl Render for TestSingle {
        type Arity = SingleArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(200.0, 200.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl Render for TestMulti {
        type Arity = MultiArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(300.0, 300.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_arity_runtime_values() {
        let leaf: Box<dyn DynRender> = Box::new(TestLeaf);
        let single: Box<dyn DynRender> = Box::new(TestSingle);
        let multi: Box<dyn DynRender> = Box::new(TestMulti);

        assert_eq!(leaf.arity(), Some(0));
        assert_eq!(single.arity(), Some(1));
        assert_eq!(multi.arity(), None);
    }

    #[test]
    fn test_heterogeneous_storage() {
        let render_objects: Vec<Box<dyn DynRender>> = vec![
            Box::new(TestLeaf),
            Box::new(TestSingle),
            Box::new(TestMulti),
        ];

        assert_eq!(render_objects.len(), 3);
        assert_eq!(render_objects[0].arity(), Some(0));
        assert_eq!(render_objects[1].arity(), Some(1));
        assert_eq!(render_objects[2].arity(), None);
    }

    #[test]
    fn test_downcast() {
        let render_obj: Box<dyn DynRender> = Box::new(TestLeaf);

        // Successful downcast
        assert!(render_obj.downcast_ref::<TestLeaf>().is_some());

        // Failed downcast
        assert!(render_obj.downcast_ref::<TestSingle>().is_none());
    }

    #[test]
    fn test_debug_names() {
        let leaf: Box<dyn DynRender> = Box::new(TestLeaf);

        let name = leaf.debug_name();
        assert!(name.contains("TestLeaf"));
    }

    #[test]
    fn test_lifecycle_methods() {
        let mut render_obj: Box<dyn DynRender> = Box::new(TestLeaf);

        // These should not panic (default no-op implementations)
        render_obj.attach();
        render_obj.detach();
        render_obj.dispose();
    }
}
