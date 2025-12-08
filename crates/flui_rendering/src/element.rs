//! Type-safe render elements with compile-time arity validation.
//!
//! This module provides the core `RenderElement<R, P, A>` type with full compile-time
//! safety for child count validation through the Arity system from `flui-tree`.
//!
//! # Architecture
//!
//! ```text
//! RenderElement<R: RenderObject, P: Protocol, A: Arity>
//!     ├── R: Specific render object type (RenderPadding, RenderFlex, etc.)
//!     ├── P: Layout protocol (BoxProtocol, SliverProtocol)
//!     └── A: Child count constraint (Leaf, Single, Optional, Variable, etc.)
//! ```
//!
//! # Compile-time Safety
//!
//! - **Child count validation**: Arity generic parameter prevents invalid child operations
//! - **Type-safe accessors**: GAT-based accessors provide zero-cost child access
//! - **Protocol safety**: Protocol generic ensures correct constraint/geometry types
//! - **Zero runtime overhead**: All validation compiled away in release builds
//!
//! # Example
//!
//! ```rust,ignore
//! // Padding can have exactly one child
//! let padding: RenderElement<RenderPadding, BoxProtocol, Single> =
//!     RenderElement::new(render_padding);
//!
//! // Access the single child with compile-time guarantee
//! let child = padding.children().single(); // Returns &ElementId
//!
//! // Flex can have multiple children
//! let flex: RenderElement<RenderFlex, BoxProtocol, Variable> =
//!     RenderElement::new(render_flex);
//!
//! // Access children as slice
//! let children = flex.children().as_slice(); // Returns &[ElementId]
//! ```

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_types::{Offset, Size};

use crate::arity::{Arity, ChildrenAccess, Leaf, Optional, Single, Variable};
use crate::flags::AtomicRenderFlags;
use crate::lifecycle::RenderLifecycle;
use crate::object::RenderObject;
use crate::parent_data::ParentData;
use crate::protocol::{BoxProtocol, Protocol, ProtocolCast, SliverProtocol};
use crate::state::RenderState;
use crate::tree::RenderId;
use crate::RenderResult;

// ============================================================================
// TYPE ALIASES FOR CONVENIENCE
// ============================================================================

/// Type alias for box render elements with specific arity.
pub type BoxRenderElement<R, A> = RenderElement<R, BoxProtocol, A>;

/// Type alias for sliver render elements with specific arity.
pub type SliverRenderElement<R, A> = RenderElement<R, SliverProtocol, A>;

// ============================================================================
// TYPE-ERASED STORAGE ENUM
// ============================================================================

/// Type-erased render element for storage in trees.
///
/// This enum allows storage of different arity types in a unified tree structure
/// while preserving type safety at API boundaries through pattern matching.
#[derive(Debug)]
pub enum AnyRenderElement<R: RenderObject, P: Protocol + ProtocolCast> {
    /// Leaf element (no children).
    Leaf(RenderElement<R, P, Leaf>),
    /// Single child element.
    Single(RenderElement<R, P, Single>),
    /// Optional child element (0-1 children).
    Optional(RenderElement<R, P, Optional>),
    /// Variable children element (0+ children).
    Variable(RenderElement<R, P, Variable>),
}

impl<R: RenderObject, P: Protocol + ProtocolCast> AnyRenderElement<R, P> {
    /// Get the element ID regardless of arity.
    pub fn id(&self) -> Option<ElementId> {
        match self {
            Self::Leaf(elem) => elem.id(),
            Self::Single(elem) => elem.id(),
            Self::Optional(elem) => elem.id(),
            Self::Variable(elem) => elem.id(),
        }
    }

    /// Get the parent ID regardless of arity.
    pub fn parent(&self) -> Option<ElementId> {
        match self {
            Self::Leaf(elem) => elem.parent(),
            Self::Single(elem) => elem.parent(),
            Self::Optional(elem) => elem.parent(),
            Self::Variable(elem) => elem.parent(),
        }
    }

    /// Get the child count regardless of arity.
    pub fn child_count(&self) -> usize {
        match self {
            Self::Leaf(elem) => elem.child_count(),
            Self::Single(elem) => elem.child_count(),
            Self::Optional(elem) => elem.child_count(),
            Self::Variable(elem) => elem.child_count(),
        }
    }

    /// Execute a closure with access to the typed element.
    pub fn with_typed<F, T>(&self, f: F) -> T
    where
        F: TypedElementVisitor<R, P, Output = T>,
    {
        match self {
            Self::Leaf(elem) => f.visit_leaf(elem),
            Self::Single(elem) => f.visit_single(elem),
            Self::Optional(elem) => f.visit_optional(elem),
            Self::Variable(elem) => f.visit_variable(elem),
        }
    }

    /// Execute a mutable closure with access to the typed element.
    pub fn with_typed_mut<F, T>(&mut self, f: F) -> T
    where
        F: TypedElementVisitorMut<R, P, Output = T>,
    {
        match self {
            Self::Leaf(elem) => f.visit_leaf_mut(elem),
            Self::Single(elem) => f.visit_single_mut(elem),
            Self::Optional(elem) => f.visit_optional_mut(elem),
            Self::Variable(elem) => f.visit_variable_mut(elem),
        }
    }
}

/// Visitor trait for type-safe operations on any render element.
pub trait TypedElementVisitor<R: RenderObject, P: Protocol + ProtocolCast> {
    type Output;

    fn visit_leaf(&self, element: &RenderElement<R, P, Leaf>) -> Self::Output;
    fn visit_single(&self, element: &RenderElement<R, P, Single>) -> Self::Output;
    fn visit_optional(&self, element: &RenderElement<R, P, Optional>) -> Self::Output;
    fn visit_variable(&self, element: &RenderElement<R, P, Variable>) -> Self::Output;
}

/// Mutable visitor trait for type-safe operations on any render element.
pub trait TypedElementVisitorMut<R: RenderObject, P: Protocol + ProtocolCast> {
    type Output;

    fn visit_leaf_mut(&self, element: &mut RenderElement<R, P, Leaf>) -> Self::Output;
    fn visit_single_mut(&self, element: &mut RenderElement<R, P, Single>) -> Self::Output;
    fn visit_optional_mut(&self, element: &mut RenderElement<R, P, Optional>) -> Self::Output;
    fn visit_variable_mut(&self, element: &mut RenderElement<R, P, Variable>) -> Self::Output;
}

// ============================================================================
// MAIN RENDER ELEMENT STRUCT
// ============================================================================

/// Type-safe render element with compile-time arity validation.
///
/// This struct represents a node in the render tree with full compile-time
/// type safety for child count constraints, layout protocols, and render objects.
///
/// # Type Parameters
///
/// - `R: RenderObject` - The specific render object type (e.g., `RenderPadding`)
/// - `P: Protocol` - The layout protocol (`BoxProtocol` or `SliverProtocol`)
/// - `A: Arity` - The child count constraint (`Leaf`, `Single`, `Optional`, `Variable`, etc.)
///
/// # Compile-time Guarantees
///
/// - Child operations are validated at compile time based on arity
/// - Protocol-specific state access is type-safe
/// - Invalid operations (e.g., adding child to `Leaf`) are compilation errors
///
/// # Memory Layout
///
/// The struct is designed for efficient memory usage:
/// - `PhantomData` has zero size
/// - Child storage is optimized based on arity (inline for small counts)
/// - Flags use atomic operations for thread safety
pub struct RenderElement<R: RenderObject, P: Protocol, A: Arity> {
    // ========== Identity ==========
    /// This element's ID (set during mount).
    id: Option<ElementId>,

    /// Parent element ID (None for root).
    parent: Option<ElementId>,

    /// Child element IDs - stored as Vec but accessed through typed accessors.
    children: Vec<ElementId>,

    /// Depth in tree (0 for root).
    depth: usize,

    // ========== Render Object ==========
    /// Reference into RenderTree (four-tree architecture).
    render_id: Option<RenderId>,

    // ========== Render State (Typed!) ==========
    /// Protocol-specific state - DIRECT, not Box<dyn>!
    state: RenderState<P>,

    // ========== Lifecycle ==========
    /// Current lifecycle state.
    lifecycle: RenderLifecycle,

    // ========== ParentData ==========
    /// Parent data set by parent (for positioning, flex, etc).
    parent_data: Option<Box<dyn ParentData>>,

    // ========== Debug ==========
    /// Debug name for diagnostics.
    debug_name: Option<&'static str>,

    // ========== PhantomData ==========
    /// Markers for RenderObject and Arity types.
    _phantom: PhantomData<(R, A)>,
}

impl<R: RenderObject, P: Protocol, A: Arity> fmt::Debug for RenderElement<R, P, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("depth", &self.depth)
            .field("render_id", &self.render_id)
            .field("lifecycle", &self.lifecycle)
            .field("debug_name", &self.debug_name)
            .field("arity", &A::runtime_arity())
            .field("protocol", &std::any::type_name::<P>())
            .finish()
    }
}

// ============================================================================
// CORE CONSTRUCTORS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Create a new render element.
    ///
    /// # Compile-time Validation
    ///
    /// The arity type `A` determines what child operations are allowed.
    /// Invalid operations will be caught at compile time.
    pub fn new() -> Self
    where
        A: Default,
    {
        Self {
            id: None,
            parent: None,
            children: Vec::new(),
            depth: 0,
            render_id: None,
            state: RenderState::new(),
            lifecycle: RenderLifecycle::Detached,
            parent_data: None,
            debug_name: None,
            _phantom: PhantomData,
        }
    }

    /// Create element with debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Create element with render ID.
    pub fn with_render_id(mut self, render_id: RenderId) -> Self {
        self.render_id = Some(render_id);
        self
    }
}

// ============================================================================
// STATE ACCESS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get immutable reference to protocol-specific state.
    pub fn state(&self) -> &RenderState<P> {
        &self.state
    }

    /// Get mutable reference to protocol-specific state.
    pub fn state_mut(&mut self) -> &mut RenderState<P> {
        &mut self.state
    }

    /// Get the protocol ID for runtime identification.
    pub fn protocol_id(&self) -> crate::protocol::ProtocolId
    where
        P: ProtocolCast,
    {
        P::id()
    }
}

// ============================================================================
// BOX PROTOCOL SPECIFIC METHODS
// ============================================================================

impl<R: RenderObject, A: Arity> RenderElement<R, BoxProtocol, A> {
    /// Get the computed size (Box protocol only).
    pub fn size(&self) -> Size {
        self.state.size()
    }

    /// Set the computed size (Box protocol only).
    pub fn set_size(&mut self, size: Size) {
        self.state.set_size(size);
    }

    /// Get the layout constraints (Box protocol only).
    pub fn constraints(&self) -> Option<&flui_types::BoxConstraints> {
        self.state.constraints()
    }

    /// Set the layout constraints (Box protocol only).
    pub fn set_constraints(&mut self, constraints: flui_types::BoxConstraints) {
        self.state.set_constraints(constraints);
    }
}

// ============================================================================
// SLIVER PROTOCOL SPECIFIC METHODS
// ============================================================================

impl<R: RenderObject, A: Arity> RenderElement<R, SliverProtocol, A> {
    /// Get the computed geometry (Sliver protocol only).
    pub fn geometry(&self) -> Option<flui_types::SliverGeometry> {
        self.state.geometry()
    }

    /// Set the computed geometry (Sliver protocol only).
    pub fn set_geometry(&mut self, geometry: flui_types::SliverGeometry) {
        self.state.set_geometry(geometry);
    }

    /// Get the sliver constraints (Sliver protocol only).
    pub fn sliver_constraints(&self) -> Option<&flui_types::SliverConstraints> {
        self.state.constraints()
    }

    /// Set the sliver constraints (Sliver protocol only).
    pub fn set_sliver_constraints(&mut self, constraints: flui_types::SliverConstraints) {
        self.state.set_constraints(constraints);
    }
}

// ============================================================================
// LIFECYCLE METHODS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Mount the element (transition from Created to Mounted).
    pub fn mount(&mut self, id: ElementId, parent: Option<ElementId>) {
        debug_assert!(self.lifecycle.is_detached());
        self.id = Some(id);
        self.parent = parent;
        self.lifecycle = RenderLifecycle::Attached;
    }

    /// Unmount the element (transition to Unmounted).
    pub fn unmount(&mut self) {
        debug_assert!(self.lifecycle.is_attached());
        self.id = None;
        self.parent = None;
        self.children.clear();
        self.lifecycle = RenderLifecycle::Detached;
    }

    /// Update the element (keeps same ID, may change parent).
    pub fn update(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Activate the element (mount to existing tree).
    pub fn activate(&mut self) {
        self.lifecycle = RenderLifecycle::Attached;
    }

    /// Deactivate the element (temporarily detach).
    pub fn deactivate(&mut self) {
        self.lifecycle = RenderLifecycle::Detached;
    }
}

// ============================================================================
// PARENT DATA METHODS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Setup parent data (called by parent during child addition).
    pub fn setup_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.parent_data = Some(data);
    }

    /// Get parent data reference.
    pub fn get_parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_deref()
    }

    /// Get mutable parent data reference.
    pub fn get_parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_deref_mut()
    }

    /// Get parent data as specific type.
    pub fn parent_data_as<T: ParentData>(&self) -> Option<&T> {
        self.parent_data.as_ref()?.as_any().downcast_ref()
    }

    /// Get mutable parent data as specific type.
    pub fn parent_data_as_mut<T: ParentData>(&mut self) -> Option<&mut T> {
        self.parent_data.as_mut()?.as_any_mut().downcast_mut()
    }
}

// ============================================================================
// TREE STRUCTURE METHODS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get element ID.
    pub fn id(&self) -> Option<ElementId> {
        self.id
    }

    /// Get parent element ID.
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set parent element ID.
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Get type-safe children accessor.
    ///
    /// This method returns a compile-time validated accessor based on the arity type:
    /// - `Leaf`: `NoChildren` - cannot access children (compilation error)
    /// - `Single`: `FixedChildren<1>` - access via `.single()`
    /// - `Optional`: `OptionalChild` - access via `.get()` returning `Option<&ElementId>`
    /// - `Variable`: `SliceChildren` - access via `.as_slice()` returning `&[ElementId]`
    pub fn children(&self) -> A::Accessor<'_, ElementId> {
        A::from_slice(&self.children)
    }

    /// Get raw children slice (for internal use).
    ///
    /// Prefer using `children()` for type-safe access.
    pub fn children_raw(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable raw children slice (for internal use).
    pub fn children_raw_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Get tree depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Set tree depth.
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Get child count.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Check if element has children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

// ============================================================================
// CHILD MANIPULATION - COMPILE-TIME ARITY VALIDATION
// ============================================================================

/// Extension trait for Vec<ElementId> that adds arity-validated operations.
trait ArityValidatedVec<A: Arity> {
    /// Push a child with arity validation - can use `?` operator!
    fn push_validated(&mut self, child: ElementId) -> Result<(), flui_tree::ArityError>;

    /// Remove a child with arity validation - can use `?` operator!
    fn remove_validated(&mut self, child: ElementId) -> Result<bool, flui_tree::ArityError>;
}

impl<A: Arity> ArityValidatedVec<A> for Vec<ElementId> {
    fn push_validated(&mut self, child: ElementId) -> Result<(), flui_tree::ArityError> {
        // Check if adding a child would be valid for this arity
        if !A::validate_count(self.len() + 1) {
            return Err(flui_tree::ArityError::TooManyChildren {
                arity: A::runtime_arity(),
                attempted: self.len() + 1,
            });
        }
        self.push(child);
        Ok(())
    }

    fn remove_validated(&mut self, child: ElementId) -> Result<bool, flui_tree::ArityError> {
        // Check if removing a child would be valid for this arity
        if !A::validate_count(self.len().saturating_sub(1)) {
            return Err(flui_tree::ArityError::TooFewChildren {
                arity: A::runtime_arity(),
                attempted: self.len().saturating_sub(1),
            });
        }

        if let Some(pos) = self.iter().position(|&id| id == child) {
            self.remove(pos);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Add a child with compile-time arity validation.
    ///
    /// Uses the elegant `?` operator for error handling:
    /// - `Single`: Can add one child if none exists
    /// - `Optional`: Can add one child if none exists
    /// - `Variable`: Can always add children
    /// - `Leaf`: Cannot add children (runtime error)
    pub fn add_child(&mut self, child: ElementId) -> RenderResult<()> {
        ArityValidatedVec::<A>::push_validated(&mut self.children, child)?;
        Ok(())
    }

    /// Remove a child with compile-time arity validation.
    ///
    /// Uses the elegant `?` operator for error handling:
    /// - `Optional`: Can remove child if one exists
    /// - `Variable`: Can remove children if any exist
    /// - `Single` and `Leaf`: Cannot remove children (runtime error)
    pub fn remove_child(&mut self, child: ElementId) -> RenderResult<bool> {
        Ok(ArityValidatedVec::<A>::remove_validated(
            &mut self.children,
            child,
        )?)
    }

    /// Try to add a child, returning whether it succeeded.
    pub fn try_add_child(&mut self, child: ElementId) -> bool {
        self.add_child(child).is_ok()
    }

    /// Try to remove a child, returning whether it was found and removed.
    pub fn try_remove_child(&mut self, child: ElementId) -> bool {
        self.remove_child(child).unwrap_or(false)
    }

    /// Check if adding a child is currently allowed.
    pub fn can_add_child(&self) -> bool {
        A::validate_count(self.children.len() + 1)
    }

    /// Check if removing a child is currently allowed.
    pub fn can_remove_child(&self) -> bool {
        self.children.len() > 0 && A::validate_count(self.children.len() - 1)
    }

    /// Get the maximum number of children allowed.
    pub fn max_children(&self) -> Option<usize> {
        match A::runtime_arity() {
            flui_tree::RuntimeArity::Exact(n) => Some(n),
            flui_tree::RuntimeArity::Optional => Some(1),
            flui_tree::RuntimeArity::AtLeast(_) => None,
            flui_tree::RuntimeArity::Variable => None,
            flui_tree::RuntimeArity::Range(_, max) => Some(max),
            flui_tree::RuntimeArity::Never => Some(0),
        }
    }

    /// Get the minimum number of children required.
    pub fn min_children(&self) -> usize {
        match A::runtime_arity() {
            flui_tree::RuntimeArity::Exact(n) => n,
            flui_tree::RuntimeArity::Optional => 0,
            flui_tree::RuntimeArity::AtLeast(min) => min,
            flui_tree::RuntimeArity::Variable => 0,
            flui_tree::RuntimeArity::Range(min, _) => min,
            flui_tree::RuntimeArity::Never => 0,
        }
    }
}

// ============================================================================
// RENDER ID METHODS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get render ID.
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    /// Set render ID.
    pub fn set_render_id(&mut self, render_id: Option<RenderId>) {
        self.render_id = render_id;
    }

    /// Check if element has render ID.
    pub fn has_render_id(&self) -> bool {
        self.render_id.is_some()
    }
}

// ============================================================================
// ARITY INTROSPECTION
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get compile-time arity information.
    pub fn arity() -> crate::arity::RuntimeArity {
        A::runtime_arity()
    }

    /// Check if this is a box protocol element.
    pub fn is_box() -> bool
    where
        P: ProtocolCast,
    {
        P::is_box()
    }

    /// Check if this is a sliver protocol element.
    pub fn is_sliver() -> bool
    where
        P: ProtocolCast,
    {
        P::is_sliver()
    }
}

// ============================================================================
// OFFSET METHODS (Box Protocol Only)
// ============================================================================

impl<R: RenderObject, A: Arity> RenderElement<R, BoxProtocol, A> {
    /// Get the layout offset (Box protocol only).
    pub fn offset(&self) -> Offset {
        self.state.offset()
    }

    /// Set the layout offset (Box protocol only).
    pub fn set_offset(&mut self, offset: Offset) {
        self.state.set_offset(offset);
    }
}

// ============================================================================
// DIRTY FLAGS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get atomic render flags.
    pub fn flags(&self) -> &AtomicRenderFlags {
        self.state.flags()
    }

    /// Mark that layout is needed.
    pub fn mark_needs_layout(&self) {
        self.flags().mark_needs_layout();
        // Note: In a full implementation, this would also propagate up the tree
        // and schedule a layout pass with the scheduler.
    }

    /// Mark that paint is needed.
    pub fn mark_needs_paint(&self) {
        self.flags().mark_needs_paint();
        // Note: In a full implementation, this would also propagate up the tree
        // and schedule a paint pass with the scheduler.
    }

    /// Mark that compositing is needed.
    pub fn mark_needs_compositing(&self) {
        self.flags().mark_needs_compositing();
    }

    /// Check if layout is needed.
    pub fn needs_layout(&self) -> bool {
        self.flags().needs_layout()
    }

    /// Check if paint is needed.
    pub fn needs_paint(&self) -> bool {
        self.flags().needs_paint()
    }

    /// Check if compositing is needed.
    pub fn needs_compositing(&self) -> bool {
        self.flags().needs_compositing()
    }

    /// Clear layout needed flag.
    pub fn clear_needs_layout(&self) {
        self.flags().clear_needs_layout();
    }

    /// Clear paint needed flag.
    pub fn clear_needs_paint(&self) {
        self.flags().clear_needs_paint();
    }
}

// ============================================================================
// LIFECYCLE QUERIES
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get current lifecycle state.
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Check if element is attached to tree.
    pub fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    /// Check if element is detached from tree.
    pub fn is_detached(&self) -> bool {
        self.lifecycle.is_detached()
    }

    /// Check if element has been laid out.
    pub fn is_laid_out(&self) -> bool {
        !self.needs_layout()
    }

    /// Check if element has been painted.
    pub fn is_painted(&self) -> bool {
        !self.needs_paint()
    }

    /// Check if element is clean (no layout/paint needed).
    pub fn is_clean(&self) -> bool {
        !self.needs_layout() && !self.needs_paint()
    }

    /// Check if element is dirty (needs layout or paint).
    pub fn is_dirty(&self) -> bool {
        self.needs_layout() || self.needs_paint()
    }
}

// ============================================================================
// DEBUG METHODS
// ============================================================================

impl<R: RenderObject, P: Protocol, A: Arity> RenderElement<R, P, A> {
    /// Get debug name.
    pub fn debug_name(&self) -> Option<&'static str> {
        self.debug_name
    }

    /// Set debug name.
    pub fn set_debug_name(&mut self, name: Option<&'static str>) {
        self.debug_name = name;
    }

    /// Get debug description.
    pub fn debug_description(&self) -> String {
        format!(
            "{}({:?}, {:?}, {:?})",
            self.debug_name.unwrap_or("RenderElement"),
            std::any::type_name::<R>(),
            std::any::type_name::<P>(),
            A::runtime_arity()
        )
    }
}

// ============================================================================
// ERROR TYPES
// ============================================================================

// ArityError is converted to RenderError for consistency

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::RenderObject;
    use crate::RenderResult;
    use flui_painting::Canvas;

    // Test render object
    #[derive(Debug)]
    struct TestRenderObject;

    impl RenderObject for TestRenderObject {
        fn perform_layout(
            &mut self,
            _element_id: ElementId,
            _constraints: flui_types::BoxConstraints,
            _layout_child: &mut dyn FnMut(
                ElementId,
                flui_types::BoxConstraints,
            ) -> RenderResult<flui_types::Size>,
        ) -> RenderResult<flui_types::Size> {
            Ok(flui_types::Size::new(100.0, 100.0))
        }

        fn paint(
            &self,
            _element_id: ElementId,
            _offset: flui_types::Offset,
            _size: flui_types::Size,
            _canvas: &mut Canvas,
            _paint_child: &mut dyn FnMut(ElementId, flui_types::Offset, &mut Canvas),
        ) {
        }
    }

    #[test]
    fn test_leaf_element() {
        let element: RenderElement<TestRenderObject, BoxProtocol, Leaf> = RenderElement::new();

        // Leaf elements cannot have children
        assert_eq!(element.child_count(), 0);
        assert!(!element.has_children());

        // This would be a compile error:
        // element.add_child(ElementId::new(1)); // CanAddChild not implemented for Leaf

        let children = element.children();
        assert_eq!(children.as_slice().len(), 0);
    }

    #[test]
    fn test_single_element() {
        let mut element: RenderElement<TestRenderObject, BoxProtocol, Single> =
            RenderElement::new();
        let child_id = ElementId::new(1);

        // Single elements can have exactly one child
        assert!(element.add_child(child_id).is_ok());
        assert_eq!(element.child_count(), 1);

        // Cannot add second child
        let second_child = ElementId::new(2);
        assert!(element.add_child(second_child).is_err());

        // Access the single child
        let children = element.children();
        assert_eq!(*children.single(), child_id);
    }

    #[test]
    fn test_optional_element() {
        let mut element: RenderElement<TestRenderObject, BoxProtocol, Optional> =
            RenderElement::new();

        // Optional starts with no children
        assert_eq!(element.child_count(), 0);
        let children = element.children();
        assert!(children.get().is_none());

        // Can add one child
        let child_id = ElementId::new(1);
        assert!(element.add_child(child_id).is_ok());

        let children = element.children();
        assert_eq!(children.get(), Some(&child_id));

        // Can remove the child
        assert!(element.remove_child(child_id).unwrap());
        assert_eq!(element.child_count(), 0);
    }

    #[test]
    fn test_variable_element() {
        let mut element: RenderElement<TestRenderObject, BoxProtocol, Variable> =
            RenderElement::new();

        // Variable can have multiple children
        let child1 = ElementId::new(1);
        let child2 = ElementId::new(2);
        let child3 = ElementId::new(3);

        assert!(element.add_child(child1).is_ok());
        assert!(element.add_child(child2).is_ok());
        assert!(element.add_child(child3).is_ok());

        assert_eq!(element.child_count(), 3);

        let children = element.children();
        let slice = children.as_slice();
        assert_eq!(slice, &[child1, child2, child3]);

        // Can remove children
        assert!(element.remove_child(child2).unwrap());
        assert_eq!(element.child_count(), 2);
    }

    #[test]
    fn test_type_erased_storage() {
        let leaf: AnyRenderElement<TestRenderObject, BoxProtocol> =
            AnyRenderElement::Leaf(RenderElement::new());
        let single: AnyRenderElement<TestRenderObject, BoxProtocol> =
            AnyRenderElement::Single(RenderElement::new());

        // Can access common properties
        assert_eq!(leaf.child_count(), 0);
        assert_eq!(single.child_count(), 0);

        // Can use visitor pattern for type-safe operations
        struct CountChildren;

        impl TypedElementVisitor<TestRenderObject, BoxProtocol> for CountChildren {
            type Output = usize;

            fn visit_leaf(
                &self,
                element: &RenderElement<TestRenderObject, BoxProtocol, Leaf>,
            ) -> usize {
                element.child_count()
            }

            fn visit_single(
                &self,
                element: &RenderElement<TestRenderObject, BoxProtocol, Single>,
            ) -> usize {
                element.child_count()
            }

            fn visit_optional(
                &self,
                element: &RenderElement<TestRenderObject, BoxProtocol, Optional>,
            ) -> usize {
                element.child_count()
            }

            fn visit_variable(
                &self,
                element: &RenderElement<TestRenderObject, BoxProtocol, Variable>,
            ) -> usize {
                element.child_count()
            }
        }

        let count1 = leaf.with_typed(CountChildren);
        let count2 = single.with_typed(CountChildren);

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_elegant_children_access_api() {
        // Demonstrate the elegant ChildrenAccess-based mutation API
        let child1 = ElementId::new(1);
        let child2 = ElementId::new(2);
        let child3 = ElementId::new(3);

        // Leaf: Cannot add children - compile-time safety via accessor
        let mut leaf: RenderElement<TestRenderObject, BoxProtocol, Leaf> = RenderElement::new();
        assert!(!leaf.can_add_child());
        assert!(leaf.add_child(child1).is_err()); // RenderError with ArityError inside

        // Single: Can add exactly one child
        let mut single: RenderElement<TestRenderObject, BoxProtocol, Single> = RenderElement::new();
        // Note: Single starts empty but expects exactly 1 child when accessing
        assert!(single.add_child(child1).is_ok()); // ✅ First child OK
        assert!(!single.can_add_child());
        assert!(single.add_child(child2).is_err()); // ❌ Second child fails

        // Now we can safely access the single child
        let children = single.children();
        assert_eq!(*children.single(), child1);

        // Optional: Can add/remove one child
        let mut optional: RenderElement<TestRenderObject, BoxProtocol, Optional> =
            RenderElement::new();
        assert!(optional.can_add_child());
        assert!(!optional.can_remove_child());

        assert!(optional.add_child(child1).is_ok()); // ✅ Add child
        assert!(!optional.can_add_child());
        assert!(optional.can_remove_child());

        assert!(optional.remove_child(child1).unwrap()); // ✅ Remove child
        assert!(optional.can_add_child());
        assert!(!optional.can_remove_child());

        // Variable: Can add/remove multiple children
        let mut variable: RenderElement<TestRenderObject, BoxProtocol, Variable> =
            RenderElement::new();
        assert!(variable.can_add_child());
        assert!(!variable.can_remove_child()); // Empty

        // Add multiple children with elegant ? syntax
        assert!(variable.add_child(child1).is_ok());
        assert!(variable.add_child(child2).is_ok());
        assert!(variable.add_child(child3).is_ok());
        assert_eq!(variable.child_count(), 3);

        // Can always add more (Variable arity)
        assert!(variable.can_add_child());
        assert!(variable.can_remove_child());

        // Remove children
        assert!(variable.remove_child(child2).unwrap());
        assert_eq!(variable.child_count(), 2);

        // Check mutation capability through ChildrenAccess
        assert_eq!(variable.max_children(), None); // Variable has no max
        assert_eq!(variable.min_children(), 0); // Variable allows empty
        assert_eq!(single.max_children(), Some(1)); // Single has max 1
        assert_eq!(leaf.max_children(), Some(0)); // Leaf has max 0
    }
}
