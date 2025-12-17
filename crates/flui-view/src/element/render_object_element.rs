//! RenderObjectElement - Elements that manage RenderObjects.
//!
//! This module implements Flutter's RenderObjectElement architecture:
//! - `RenderObjectElement` trait for elements that create RenderObjects
//! - `RenderTreeRootElement` for the root of the render tree
//! - Methods for attaching/detaching RenderObjects to the render tree
//!
//! # Flutter Architecture
//!
//! In Flutter, RenderObjectElement:
//! 1. Creates a RenderObject in `mount()`
//! 2. Calls `attachRenderObject()` which finds ancestor RenderObjectElement
//! 3. Ancestor's `insertRenderObjectChild()` adds child to render tree
//! 4. RenderTreeRootElement sets `pipelineOwner.rootNode = renderObject`

use crate::view::ElementBase;
use flui_foundation::ElementId;
use std::any::Any;
use std::sync::Arc;

/// Slot identifier for render object children.
///
/// Used by `insertRenderObjectChild` and `removeRenderObjectChild` to identify
/// which child slot is being modified.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RenderSlot {
    /// Single child slot (for SingleChildRenderObjectElement)
    Single,
    /// Indexed slot (for MultiChildRenderObjectElement)
    Index(usize),
    /// Named slot (for custom layouts)
    Named(String),
}

impl Default for RenderSlot {
    fn default() -> Self {
        Self::Single
    }
}

/// Trait for elements that manage RenderObjects.
///
/// This corresponds to Flutter's `RenderObjectElement` which:
/// - Creates RenderObjects from RenderObjectWidgets
/// - Manages RenderObject lifecycle (attach, detach)
/// - Handles parent-child relationships in the render tree
///
/// # Flutter Equivalent
///
/// ```dart
/// abstract class RenderObjectElement extends Element {
///   RenderObject get renderObject;
///   void attachRenderObject(Object? newSlot);
///   void detachRenderObject();
///   void insertRenderObjectChild(RenderObject child, Object? slot);
///   void moveRenderObjectChild(RenderObject child, Object? oldSlot, Object? newSlot);
///   void removeRenderObjectChild(RenderObject child, Object? slot);
/// }
/// ```
pub trait RenderObjectElement: ElementBase {
    /// Get the RenderObject as a type-erased reference.
    ///
    /// Returns None if the RenderObject hasn't been created yet (before mount).
    fn render_object_any(&self) -> Option<&dyn Any>;

    /// Get the RenderObject as a mutable type-erased reference.
    fn render_object_any_mut(&mut self) -> Option<&mut dyn Any>;

    /// Attach this element's RenderObject to the render tree.
    ///
    /// This method:
    /// 1. Finds the nearest ancestor RenderObjectElement
    /// 2. Calls `insertRenderObjectChild` on that ancestor
    ///
    /// For RenderTreeRootElement, this sets `pipelineOwner.rootNode` instead.
    ///
    /// # Arguments
    /// * `slot` - The slot identifier for this child in the parent
    fn attach_render_object(&mut self, slot: RenderSlot);

    /// Detach this element's RenderObject from the render tree.
    ///
    /// This calls `removeRenderObjectChild` on the ancestor RenderObjectElement.
    fn detach_render_object(&mut self);

    /// Insert a child RenderObject into this element's RenderObject.
    ///
    /// Called by child elements when they attach to the render tree.
    ///
    /// # Arguments
    /// * `child` - The child RenderObject (type-erased)
    /// * `slot` - Where to insert the child
    fn insert_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot);

    /// Move a child RenderObject from one slot to another.
    ///
    /// # Arguments
    /// * `child` - The child RenderObject to move
    /// * `old_slot` - Previous slot
    /// * `new_slot` - New slot
    fn move_render_object_child(
        &mut self,
        child: &dyn Any,
        old_slot: RenderSlot,
        new_slot: RenderSlot,
    );

    /// Remove a child RenderObject from this element's RenderObject.
    ///
    /// Called by child elements when they detach from the render tree.
    ///
    /// # Arguments
    /// * `child` - The child RenderObject to remove
    /// * `slot` - Which slot to remove from
    fn remove_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot);

    /// Find the nearest ancestor RenderObjectElement.
    ///
    /// Used by `attach_render_object` to find where to insert this RenderObject.
    fn find_ancestor_render_object_element(&self) -> Option<ElementId>;

    /// Set the ancestor RenderObjectElement reference.
    fn set_ancestor_render_object_element(&mut self, ancestor: Option<ElementId>);
}

/// Marker trait for root elements that bootstrap a new render tree.
///
/// RenderTreeRootElement is special in that it:
/// - Does NOT call insertRenderObjectChild on an ancestor
/// - Instead, sets pipelineOwner.rootNode = renderObject
/// - Creates its own PipelineOwner (or uses a provided one)
///
/// # Flutter Equivalent
///
/// ```dart
/// abstract class RenderTreeRootElement extends RenderObjectElement {
///   @override
///   void attachRenderObject(Object? newSlot) {
///     _slot = newSlot;
///     // Does NOT call ancestor.insertRenderObjectChild
///   }
/// }
/// ```
pub trait RenderTreeRootElement: RenderObjectElement {
    /// Get the PipelineOwner for this render tree.
    ///
    /// The RenderTreeRootElement owns or references the PipelineOwner
    /// that manages the render tree rooted at this element.
    fn pipeline_owner(&self) -> Option<Arc<dyn Any + Send + Sync>>;

    /// Set the PipelineOwner for this render tree.
    fn set_pipeline_owner(&mut self, owner: Arc<dyn Any + Send + Sync>);

    /// Attach the root RenderObject to the PipelineOwner.
    ///
    /// This sets `pipelineOwner.rootNode = renderObject`.
    fn attach_to_pipeline_owner(&mut self);

    /// Detach the root RenderObject from the PipelineOwner.
    ///
    /// This sets `pipelineOwner.rootNode = None`.
    fn detach_from_pipeline_owner(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_slot_default() {
        let slot = RenderSlot::default();
        assert_eq!(slot, RenderSlot::Single);
    }

    #[test]
    fn test_render_slot_variants() {
        let single = RenderSlot::Single;
        let indexed = RenderSlot::Index(5);
        let named = RenderSlot::Named("header".to_string());

        assert_eq!(single, RenderSlot::Single);
        assert_eq!(indexed, RenderSlot::Index(5));
        assert_eq!(named, RenderSlot::Named("header".to_string()));
    }
}
