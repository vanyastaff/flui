//! Single-child render object base infrastructure
//!
//! Provides reusable components for RenderObjects that have exactly one child.
//! This eliminates ~80% of boilerplate code for single-child render objects.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::{Offset, Size};
use crate::RenderFlags;

/// Common state for single-child render objects
///
/// This struct contains all the common fields that every single-child
/// RenderObject needs. By using this, you eliminate field duplication across
/// 50+ render object types.
///
/// # Memory Layout
///
/// ```text
/// SingleChildState:
/// ├── element_id: Option<ElementId>           (16 bytes)
/// ├── child: Option<Box<dyn DynRenderObject>> (16 bytes)
/// ├── size: Size                               (8 bytes)
/// ├── constraints: Option<BoxConstraints>      (40 bytes)
/// └── flags: RenderFlags                       (1 byte)
/// Total: 81 bytes (padded to 88)
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_rendering::core::SingleChildState;
///
/// pub struct RenderOpacity {
///     state: SingleChildState,  // All common fields
///     opacity: f32,              // Only specific field
/// }
///
/// impl RenderOpacity {
///     pub fn new(opacity: f32) -> Self {
///         Self {
///             state: SingleChildState::new(),
///             opacity,
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct SingleChildState {
    /// Element ID for cache invalidation
    pub element_id: Option<ElementId>,
    
    /// The single child render object
    pub child: Option<Box<dyn DynRenderObject>>,
    
    /// Current size after layout
    pub size: Size,
    
    /// Current constraints
    pub constraints: Option<BoxConstraints>,
    
    /// State flags (needs_layout, needs_paint, boundaries)
    pub flags: RenderFlags,
}

impl SingleChildState {
    /// Create new state with default values
    #[inline]
    pub const fn new() -> Self {
        Self {
            element_id: None,
            child: None,
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
            child: None,
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
    // Child Management
    // ========================================================================
    
    #[inline]
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }
    
    #[inline]
    pub fn child_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
        self.child.as_deref_mut()
    }
    
    #[inline]
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.flags.mark_needs_layout();
    }
    
    #[inline]
    pub fn take_child(&mut self) -> Option<Box<dyn DynRenderObject>> {
        let child = self.child.take();
        if child.is_some() {
            self.flags.mark_needs_layout();
        }
        child
    }
    
    #[inline]
    pub fn has_child(&self) -> bool {
        self.child.is_some()
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
    // Common Layout Patterns
    // ========================================================================
    
    /// Passthrough layout - simply pass constraints to child
    ///
    /// This is the most common layout pattern for wrapper RenderObjects
    /// like Opacity, ClipRect, Transform, etc.
    ///
    /// # Performance
    ///
    /// This method is #[inline] and will be optimized away by the compiler.
    pub fn layout_passthrough(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            self.size = child.layout(constraints);
        } else {
            self.size = constraints.smallest();
        }
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    /// Layout with modified constraints
    ///
    /// Useful for RenderObjects that need to modify constraints before
    /// passing them to the child (like Padding, ConstrainedBox, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // RenderPadding modifies constraints by deflating
    /// state.layout_with_modified_constraints(constraints, |c| {
    ///     c.deflate(padding)
    /// })
    /// ```
    pub fn layout_with_modified_constraints<F>(
        &mut self,
        constraints: BoxConstraints,
        modify: F,
    ) -> Size
    where
        F: FnOnce(BoxConstraints) -> BoxConstraints,
    {
        let modified = modify(constraints);
        
        if let Some(child) = &mut self.child {
            self.size = child.layout(modified);
        } else {
            self.size = modified.smallest();
        }
        
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    /// Layout with post-processing of child size
    ///
    /// Useful when you need to adjust the size after the child has been laid out
    /// (like Padding adding padding back to the size)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // RenderPadding adds padding to child size
    /// state.layout_with_post_process(constraints, |child_size| {
    ///     Size::new(
    ///         child_size.width + padding.horizontal(),
    ///         child_size.height + padding.vertical(),
    ///     )
    /// })
    /// ```
    pub fn layout_with_post_process<F>(
        &mut self,
        constraints: BoxConstraints,
        post_process: F,
    ) -> Size
    where
        F: FnOnce(Size) -> Size,
    {
        let child_size = if let Some(child) = &mut self.child {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };
        
        self.size = post_process(child_size);
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    // ========================================================================
    // Common Visitor Patterns
    // ========================================================================
    
    /// Visit child (immutable)
    #[inline]
    pub fn visit_child(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }
    
    /// Visit child (mutable)
    #[inline]
    pub fn visit_child_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
    
    // ========================================================================
    // Common Hit Test Patterns
    // ========================================================================
    
    /// Delegate hit test to child
    #[inline]
    pub fn hit_test_child(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }
    
    /// Default hit test with bounds checking
    ///
    /// This is the most common hit test pattern: check bounds, then delegate to child
    pub fn hit_test_default(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
        hit_self: bool,
    ) -> bool {
        // Bounds check
        if position.dx < 0.0
            || position.dx >= self.size.width
            || position.dy < 0.0
            || position.dy >= self.size.height
        {
            return false;
        }
        
        // Check children first (front-to-back)
        let hit_child = self.hit_test_child(result, position);
        
        // Add to result if we hit child or self
        if hit_child || hit_self {
            result.add(flui_types::events::HitTestEntry::new(position, self.size));
            return true;
        }
        
        false
    }
    
    // ========================================================================
    // Common Paint Patterns
    // ========================================================================
    
    /// Paint child at offset
    #[inline]
    pub fn paint_child(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }
    
    /// Paint child with modified offset
    #[inline]
    pub fn paint_child_with_offset<F>(
        &self,
        painter: &egui::Painter,
        offset: Offset,
        modify_offset: F,
    ) where
        F: FnOnce(Offset) -> Offset,
    {
        if let Some(child) = &self.child {
            let modified = modify_offset(offset);
            child.paint(painter, modified);
        }
    }
}

impl Default for SingleChildState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Macros
// ============================================================================

/// Macro to delegate common methods to SingleChildState
///
/// This eliminates the need to write boilerplate delegation code in every
/// single-child RenderObject.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderOpacity {
///     state: SingleChildState,
///     opacity: f32,
/// }
///
/// impl RenderOpacity {
///     delegate_to_single_child_state!(state);
///     
///     // Now you only need to implement specific methods
///     pub fn set_opacity(&mut self, opacity: f32) { ... }
/// }
/// ```
#[macro_export]
macro_rules! delegate_to_single_child_state {
    ($field:ident) => {
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
        
        /// Get child reference
        #[inline]
        pub fn child(&self) -> Option<&dyn flui_core::DynRenderObject> {
            self.$field.child()
        }
        
        /// Get mutable child reference
        #[inline]
        pub fn child_mut(&mut self) -> Option<&mut dyn flui_core::DynRenderObject> {
            self.$field.child_mut()
        }
        
        /// Set child
        #[inline]
        pub fn set_child(&mut self, child: Option<Box<dyn flui_core::DynRenderObject>>) {
            self.$field.set_child(child)
        }
        
        /// Take child ownership
        #[inline]
        pub fn take_child(&mut self) -> Option<Box<dyn flui_core::DynRenderObject>> {
            self.$field.take_child()
        }
        
        /// Check if has child
        #[inline]
        pub fn has_child(&self) -> bool {
            self.$field.has_child()
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
    
    #[test]
    fn test_single_child_state_new() {
        let state = SingleChildState::new();
        assert!(state.element_id.is_none());
        assert!(state.child.is_none());
        assert_eq!(state.size, Size::ZERO);
        assert!(state.needs_layout());
        assert!(state.needs_paint());
    }
    
    #[test]
    fn test_single_child_state_with_element_id() {
        let id = ElementId::new();
        let state = SingleChildState::with_element_id(id);
        assert_eq!(state.element_id, Some(id));
    }
    
    #[test]
    fn test_layout_passthrough() {
        use crate::RenderBox;
        
        let mut state = SingleChildState::new();
        state.set_child(Some(Box::new(RenderBox::new())));
        
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = state.layout_passthrough(constraints);
        
        assert_eq!(size, Size::new(100.0, 50.0));
        assert!(!state.needs_layout());
    }
}
