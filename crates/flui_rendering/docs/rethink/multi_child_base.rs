//! Multi-child render object base infrastructure
//!
//! Provides reusable components for RenderObjects that have multiple children.
//! Automatically handles child_count tracking for cache invalidation.

use flui_core::{BoxConstraints, DynRenderObject, ElementId, ParentData};
use flui_types::{Offset, Size};
use crate::RenderFlags;

/// Entry for a child in multi-child layout
///
/// Contains the child render object, its parent data, and computed offset
#[derive(Debug)]
pub struct ChildEntry<P: ParentData> {
    /// The child render object
    pub render_object: Box<dyn DynRenderObject>,
    
    /// Parent data for layout communication
    pub parent_data: P,
    
    /// Offset after layout
    pub offset: Offset,
}

impl<P: ParentData> ChildEntry<P> {
    /// Create new child entry
    pub fn new(render_object: Box<dyn DynRenderObject>, parent_data: P) -> Self {
        Self {
            render_object,
            parent_data,
            offset: Offset::ZERO,
        }
    }
}

/// Common state for multi-child render objects
///
/// This struct contains all the common fields that every multi-child
/// RenderObject needs, plus automatic child_count tracking.
///
/// # CRITICAL: child_count tracking
///
/// This struct AUTOMATICALLY includes child_count in cache keys!
/// This solves the CRITICAL TODO for RenderFlex/Stack/IndexedStack.
///
/// # Memory Layout
///
/// ```text
/// MultiChildState<P>:
/// ├── element_id: Option<ElementId>     (16 bytes)
/// ├── children: Vec<ChildEntry<P>>      (24 bytes)
/// ├── size: Size                         (8 bytes)
/// ├── constraints: Option<BoxConstraints>(40 bytes)
/// └── flags: RenderFlags                 (1 byte)
/// Total: 89 bytes (padded to 96) + Vec heap allocation
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_rendering::core::MultiChildState;
/// use flui_rendering::FlexParentData;
///
/// pub struct RenderFlex {
///     state: MultiChildState<FlexParentData>,
///     direction: Axis,
///     main_axis_alignment: MainAxisAlignment,
/// }
///
/// impl RenderFlex {
///     pub fn new(direction: Axis) -> Self {
///         Self {
///             state: MultiChildState::new(),
///             direction,
///             main_axis_alignment: MainAxisAlignment::Start,
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct MultiChildState<P: ParentData> {
    /// Element ID for cache invalidation
    pub element_id: Option<ElementId>,
    
    /// Children with parent data
    pub children: Vec<ChildEntry<P>>,
    
    /// Current size after layout
    pub size: Size,
    
    /// Current constraints
    pub constraints: Option<BoxConstraints>,
    
    /// State flags (needs_layout, needs_paint, boundaries)
    pub flags: RenderFlags,
}

impl<P: ParentData> MultiChildState<P> {
    /// Create new state with no children
    #[inline]
    pub const fn new() -> Self {
        Self {
            element_id: None,
            children: Vec::new(),
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    /// Create with ElementId for caching
    #[inline]
    pub const fn with_element_id(element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            children: Vec::new(),
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    // ========================================================================
    // Element ID Management
    // ========================================================================
    
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }
    
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
    
    // ========================================================================
    // Children Management
    // ========================================================================
    
    /// Get child count
    ///
    /// This is used in cache keys to detect structural changes!
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
    
    /// Check if has any children
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
    
    /// Add child to the end
    ///
    /// Automatically marks as needing layout
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>, parent_data: P) {
        self.children.push(ChildEntry::new(child, parent_data));
        self.flags.mark_needs_layout();
    }
    
    /// Insert child at index
    ///
    /// Automatically marks as needing layout
    pub fn insert_child(
        &mut self,
        index: usize,
        child: Box<dyn DynRenderObject>,
        parent_data: P,
    ) {
        self.children.insert(index, ChildEntry::new(child, parent_data));
        self.flags.mark_needs_layout();
    }
    
    /// Remove child at index
    ///
    /// Automatically marks as needing layout
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn DynRenderObject>> {
        if index < self.children.len() {
            let entry = self.children.remove(index);
            self.flags.mark_needs_layout();
            Some(entry.render_object)
        } else {
            None
        }
    }
    
    /// Clear all children
    ///
    /// Automatically marks as needing layout
    pub fn clear_children(&mut self) {
        if !self.children.is_empty() {
            self.children.clear();
            self.flags.mark_needs_layout();
        }
    }
    
    /// Get child entry by index
    #[inline]
    pub fn child(&self, index: usize) -> Option<&ChildEntry<P>> {
        self.children.get(index)
    }
    
    /// Get mutable child entry by index
    #[inline]
    pub fn child_mut(&mut self, index: usize) -> Option<&mut ChildEntry<P>> {
        self.children.get_mut(index)
    }
    
    // ========================================================================
    // Layout State
    // ========================================================================
    
    #[inline]
    pub fn size(&self) -> Size {
        self.size
    }
    
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }
    
    #[inline]
    pub fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }
    
    #[inline]
    pub fn set_constraints(&mut self, constraints: BoxConstraints) {
        self.constraints = Some(constraints);
    }
    
    // ========================================================================
    // Flags Management
    // ========================================================================
    
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }
    
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }
    
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.flags.mark_needs_layout();
    }
    
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.flags.mark_needs_paint();
    }
    
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.clear_needs_layout();
    }
    
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.clear_needs_paint();
    }
    
    #[inline]
    pub fn flags(&self) -> RenderFlags {
        self.flags
    }
    
    #[inline]
    pub fn flags_mut(&mut self) -> &mut RenderFlags {
        &mut self.flags
    }
    
    // ========================================================================
    // Common Visitor Patterns
    // ========================================================================
    
    /// Visit all children (immutable)
    pub fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        for child in &self.children {
            visitor(&*child.render_object);
        }
    }
    
    /// Visit all children (mutable)
    pub fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        for child in &mut self.children {
            visitor(&mut *child.render_object);
        }
    }
    
    // ========================================================================
    // Common Paint Patterns
    // ========================================================================
    
    /// Paint all children at their computed offsets
    ///
    /// This is the most common paint pattern for multi-child layouts
    pub fn paint_children(&self, painter: &egui::Painter, offset: Offset) {
        for child in &self.children {
            let child_offset = offset + child.offset;
            child.render_object.paint(painter, child_offset);
        }
    }
    
    /// Paint children with custom offset calculation
    pub fn paint_children_with<F>(
        &self,
        painter: &egui::Painter,
        offset: Offset,
        compute_offset: F,
    )
    where
        F: Fn(Offset, &ChildEntry<P>) -> Offset,
    {
        for child in &self.children {
            let child_offset = compute_offset(offset, child);
            child.render_object.paint(painter, child_offset);
        }
    }
    
    // ========================================================================
    // Common Hit Test Patterns
    // ========================================================================
    
    /// Hit test children in reverse order (front to back)
    ///
    /// This is the most common hit test pattern for multi-child layouts
    pub fn hit_test_children(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
    ) -> bool {
        // Test in reverse order (front to back)
        for child in self.children.iter().rev() {
            let local_position = position - child.offset;
            if child.render_object.hit_test(result, local_position) {
                return true;
            }
        }
        false
    }
    
    /// Hit test children with custom position adjustment
    pub fn hit_test_children_with<F>(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
        adjust_position: F,
    ) -> bool
    where
        F: Fn(Offset, &ChildEntry<P>) -> Offset,
    {
        // Test in reverse order (front to back)
        for child in self.children.iter().rev() {
            let local_position = adjust_position(position, child);
            if child.render_object.hit_test(result, local_position) {
                return true;
            }
        }
        false
    }
}

impl<P: ParentData> Default for MultiChildState<P> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Macros
// ============================================================================

/// Macro to delegate common methods to MultiChildState
///
/// This eliminates the need to write boilerplate delegation code in every
/// multi-child RenderObject.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderFlex {
///     state: MultiChildState<FlexParentData>,
///     direction: Axis,
/// }
///
/// impl RenderFlex {
///     delegate_to_multi_child_state!(state, FlexParentData);
///     
///     // Now you only need to implement specific methods
///     pub fn set_direction(&mut self, direction: Axis) { ... }
/// }
/// ```
#[macro_export]
macro_rules! delegate_to_multi_child_state {
    ($field:ident, $parent_data:ty) => {
        /// Get element ID for caching
        #[inline]
        pub fn element_id(&self) -> Option<flui_core::ElementId> {
            self.$field.element_id()
        }
        
        /// Set element ID for caching
        #[inline]
        pub fn set_element_id(&mut self, element_id: Option<flui_core::ElementId>) {
            self.$field.set_element_id(element_id)
        }
        
        /// Get child count (CRITICAL for cache invalidation!)
        #[inline]
        pub fn child_count(&self) -> usize {
            self.$field.child_count()
        }
        
        /// Check if has any children
        #[inline]
        pub fn is_empty(&self) -> bool {
            self.$field.is_empty()
        }
        
        /// Add child to the end
        #[inline]
        pub fn add_child(
            &mut self,
            child: Box<dyn flui_core::DynRenderObject>,
            parent_data: $parent_data,
        ) {
            self.$field.add_child(child, parent_data)
        }
        
        /// Insert child at index
        #[inline]
        pub fn insert_child(
            &mut self,
            index: usize,
            child: Box<dyn flui_core::DynRenderObject>,
            parent_data: $parent_data,
        ) {
            self.$field.insert_child(index, child, parent_data)
        }
        
        /// Remove child at index
        #[inline]
        pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn flui_core::DynRenderObject>> {
            self.$field.remove_child(index)
        }
        
        /// Clear all children
        #[inline]
        pub fn clear_children(&mut self) {
            self.$field.clear_children()
        }
        
        /// Get current size
        #[inline]
        pub fn size(&self) -> flui_types::Size {
            self.$field.size()
        }
        
        /// Get current constraints
        #[inline]
        pub fn constraints(&self) -> Option<flui_core::BoxConstraints> {
            self.$field.constraints()
        }
        
        /// Check if needs layout
        #[inline]
        pub fn needs_layout(&self) -> bool {
            self.$field.needs_layout()
        }
        
        /// Check if needs paint
        #[inline]
        pub fn needs_paint(&self) -> bool {
            self.$field.needs_paint()
        }
        
        /// Mark as needing layout
        #[inline]
        pub fn mark_needs_layout(&mut self) {
            self.$field.mark_needs_layout()
        }
        
        /// Mark as needing paint
        #[inline]
        pub fn mark_needs_paint(&mut self) {
            self.$field.mark_needs_paint()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::BoxParentData;
    
    #[test]
    fn test_multi_child_state_new() {
        let state: MultiChildState<BoxParentData> = MultiChildState::new();
        assert!(state.element_id.is_none());
        assert_eq!(state.child_count(), 0);
        assert!(state.is_empty());
        assert!(state.needs_layout());
        assert!(state.needs_paint());
    }
    
    #[test]
    fn test_multi_child_state_with_element_id() {
        let id = ElementId::new();
        let state: MultiChildState<BoxParentData> = MultiChildState::with_element_id(id);
        assert_eq!(state.element_id, Some(id));
    }
    
    #[test]
    fn test_add_child() {
        use crate::RenderBox;
        
        let mut state: MultiChildState<BoxParentData> = MultiChildState::new();
        assert_eq!(state.child_count(), 0);
        
        state.add_child(Box::new(RenderBox::new()), BoxParentData::default());
        assert_eq!(state.child_count(), 1);
        assert!(state.needs_layout());
    }
    
    #[test]
    fn test_remove_child() {
        use crate::RenderBox;
        
        let mut state: MultiChildState<BoxParentData> = MultiChildState::new();
        state.add_child(Box::new(RenderBox::new()), BoxParentData::default());
        state.add_child(Box::new(RenderBox::new()), BoxParentData::default());
        
        assert_eq!(state.child_count(), 2);
        
        let removed = state.remove_child(0);
        assert!(removed.is_some());
        assert_eq!(state.child_count(), 1);
        assert!(state.needs_layout());
    }
    
    #[test]
    fn test_clear_children() {
        use crate::RenderBox;
        
        let mut state: MultiChildState<BoxParentData> = MultiChildState::new();
        state.add_child(Box::new(RenderBox::new()), BoxParentData::default());
        state.add_child(Box::new(RenderBox::new()), BoxParentData::default());
        
        assert_eq!(state.child_count(), 2);
        
        state.clear_children();
        assert_eq!(state.child_count(), 0);
        assert!(state.is_empty());
    }
}
