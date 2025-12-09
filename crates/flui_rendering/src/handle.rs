//! RenderHandle - Type-safe handle for render objects with typestate lifecycle.
//!
//! This module provides the `RenderHandle` type with compile-time state tracking
//! for render object lifecycle management.
//!
//! # Overview
//!
//! `RenderHandle` wraps a `RenderObject` and uses the typestate pattern to enforce
//! valid state transitions at compile time:
//!
//! ```text
//! RenderHandle<Unmounted> ──mount()──→ RenderHandle<Mounted>
//!                         ←─unmount()──
//! ```
//!
//! # Key Features
//!
//! - **Typestate pattern**: Compile-time enforcement of mount/unmount lifecycle
//! - **Immutable config**: `RenderConfig` preserves render object configuration
//! - **Tree navigation**: Generic `NavigableHandle` trait + typed `RenderId` methods
//! - **Parent data access**: Render-specific `parent_data()` and `parent_data_mut()`
//! - **Protocol tracking**: ProtocolId for Box vs Sliver protocol distinction
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::handle::{RenderConfig, RenderHandle};
//! use flui_rendering::core::ProtocolId;
//! use flui_tree::{Mountable, Unmountable};
//!
//! // Create unmounted handle
//! let config = RenderConfig::new(ProtocolId::Box, Some("MyRender"));
//! let unmounted = RenderHandle::new(config);
//!
//! // Mount as root
//! let mut mounted = unmounted.mount(None);
//!
//! // Use typed navigation
//! if let Some(parent_id) = mounted.parent_render() {
//!     println!("Parent: {:?}", parent_id);
//! }
//!
//! // Access parent data (render-specific)
//! if let Some(parent_data) = mounted.parent_data() {
//!     // Use parent data
//! }
//! ```

use std::marker::PhantomData;

use flui_foundation::RenderId;
use flui_tree::{Mountable, TreeInfo, Unmountable};
use flui_tree::{Mounted, NodeState, Unmounted};

use crate::core::{ParentData, ProtocolId, RenderElement, RuntimeArity};

// ============================================================================
// RENDER CONFIG - Immutable render object configuration
// ============================================================================

/// Immutable configuration for render objects.
///
/// This struct stores the configuration needed to recreate or identify
/// a render object without holding the live render object itself.
///
/// # Purpose
///
/// - **Hot-reload**: Recreate render objects from configuration
/// - **Serialization**: Save render tree configuration
/// - **Comparison**: Check if render objects are compatible
///
/// # Fields
///
/// - `protocol`: The layout protocol (Box or Sliver)
/// - `debug_name`: Optional name for debugging
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::handle::RenderConfig;
/// use flui_rendering::core::ProtocolId;
///
/// let config = RenderConfig::new(ProtocolId::Box, Some("Padding"));
/// assert_eq!(config.protocol(), ProtocolId::Box);
/// ```
#[derive(Debug, Clone)]
pub struct RenderConfig {
    protocol: ProtocolId,
    debug_name: Option<&'static str>,
}

impl RenderConfig {
    /// Create a new render config.
    ///
    /// # Parameters
    ///
    /// - `protocol`: The layout protocol (Box or Sliver)
    /// - `debug_name`: Optional name for debugging
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::handle::RenderConfig;
    /// use flui_rendering::core::ProtocolId;
    ///
    /// let config = RenderConfig::new(ProtocolId::Box, Some("Container"));
    /// ```
    pub fn new(protocol: ProtocolId, debug_name: Option<&'static str>) -> Self {
        Self {
            protocol,
            debug_name,
        }
    }

    /// Get the protocol ID.
    pub fn protocol(&self) -> ProtocolId {
        self.protocol
    }

    /// Get the debug name, if any.
    pub fn debug_name(&self) -> Option<&'static str> {
        self.debug_name
    }
}

// ============================================================================
// RENDER HANDLE - Typestate-based render object handle
// ============================================================================

/// Type-safe handle for render objects with compile-time state tracking.
///
/// This struct wraps a render object and uses the typestate pattern to enforce
/// valid lifecycle transitions at compile time.
///
/// # Type Parameters
///
/// - `S`: The node state (Unmounted or Mounted)
///
/// # States
///
/// - **Unmounted**: Configuration only, no live render object or tree position
/// - **Mounted**: Has live render object and tree position information
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::handle::{RenderConfig, RenderHandle};
/// use flui_rendering::core::ProtocolId;
/// use flui_tree::Mountable;
///
/// // Create unmounted handle
/// let config = RenderConfig::new(ProtocolId::Sliver, None);
/// let unmounted = RenderHandle::new(config);
///
/// // Mount to tree
/// let mounted = unmounted.mount(Some(42));
/// ```
pub struct RenderHandle<S: NodeState> {
    config: RenderConfig,
    render_element: Option<RenderElement>,
    tree_info: Option<TreeInfo>,
    _state: PhantomData<S>,
}

// ============================================================================
// RENDER HANDLE - Constructors and methods
// ============================================================================

impl RenderHandle<Unmounted> {
    /// Create a new unmounted render handle with the given config.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::handle::{RenderConfig, RenderHandle};
    /// use flui_rendering::core::ProtocolId;
    ///
    /// let config = RenderConfig::new(ProtocolId::Box, Some("MyRender"));
    /// let handle = RenderHandle::new(config);
    /// ```
    pub fn new(config: RenderConfig) -> Self {
        Self {
            config,
            render_element: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &RenderConfig {
        &self.config
    }
}

impl RenderHandle<Mounted> {
    /// Get a reference to the config.
    pub fn config(&self) -> &RenderConfig {
        &self.config
    }

    /// Get a reference to the render element, if mounted.
    pub fn render_element(&self) -> Option<&RenderElement> {
        self.render_element.as_ref()
    }

    /// Get a mutable reference to the render element, if mounted.
    pub fn render_element_mut(&mut self) -> Option<&mut RenderElement> {
        self.render_element.as_mut()
    }
}

// ============================================================================
// TYPED NAVIGATION METHODS (Mounted only)
// ============================================================================

impl RenderHandle<Mounted> {
    /// Get the parent RenderId.
    ///
    /// Returns `None` if this is the root render object.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(parent_id) = mounted.parent_render() {
    ///     println!("Parent: {:?}", parent_id);
    /// }
    /// ```
    pub fn parent_render(&self) -> Option<RenderId> {
        self.tree_info
            .as_ref()
            .and_then(|info| info.parent)
            .map(RenderId::new)
    }

    /// Get the children RenderIds.
    ///
    /// Returns an iterator over child IDs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for child_id in mounted.children_renders() {
    ///     println!("Child: {:?}", child_id);
    /// }
    /// ```
    pub fn children_renders(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.tree_info
            .as_ref()
            .map(|info| info.children.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|&id| RenderId::new(id))
    }
}

// ============================================================================
// PARENT DATA ACCESS (Render-specific, Mounted only)
// ============================================================================

impl RenderHandle<Mounted> {
    /// Get a reference to the parent data, if present.
    ///
    /// Parent data is set by the parent render object and contains
    /// layout-specific metadata like offsets or flex factors.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::core::BoxParentData;
    ///
    /// if let Some(parent_data) = mounted.parent_data() {
    ///     if let Some(box_data) = parent_data.as_any().downcast_ref::<BoxParentData>() {
    ///         println!("Offset: {:?}", box_data.offset());
    ///     }
    /// }
    /// ```
    pub fn parent_data(&self) -> Option<&dyn ParentData> {
        self.render_element
            .as_ref()
            .and_then(|elem| elem.parent_data())
    }

    /// Get a mutable reference to the parent data, if present.
    ///
    /// This allows the parent render object to update layout metadata.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::core::{BoxParentData, ParentDataWithOffset};
    /// use flui_types::Offset;
    ///
    /// if let Some(parent_data) = mounted.parent_data_mut() {
    ///     if let Some(box_data) = parent_data.as_any_mut().downcast_mut::<BoxParentData>() {
    ///         box_data.set_offset(Offset::new(10.0, 20.0));
    ///     }
    /// }
    /// ```
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.render_element
            .as_mut()
            .and_then(|elem| elem.parent_data_mut())
    }
}

// ============================================================================
// RENDER STATE ACCESS (Render-specific, Mounted only)
// ============================================================================

impl RenderHandle<Mounted> {
    /// Get the render size (for Box protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::Size;
    ///
    /// let size = mounted.size();
    /// println!("Render size: {:?}", size);
    /// ```
    pub fn size(&self) -> Option<flui_types::Size> {
        self.render_element.as_ref().map(|elem| elem.size())
    }

    /// Set the render size (for Box protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::Size;
    ///
    /// mounted.set_size(Size::new(100.0, 50.0));
    /// ```
    pub fn set_size(&mut self, size: flui_types::Size) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.set_size(size);
        }
    }

    /// Get the render offset.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::Offset;
    ///
    /// let offset = mounted.offset();
    /// println!("Render offset: {:?}", offset);
    /// ```
    pub fn offset(&self) -> Option<flui_types::Offset> {
        self.render_element.as_ref().map(|elem| elem.offset())
    }

    /// Set the render offset.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::Offset;
    ///
    /// mounted.set_offset(Offset::new(10.0, 20.0));
    /// ```
    pub fn set_offset(&mut self, offset: flui_types::Offset) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.set_offset(offset);
        }
    }

    /// Get box constraints (for Box protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::BoxConstraints;
    ///
    /// if let Some(constraints) = mounted.constraints_box() {
    ///     println!("Box constraints: {:?}", constraints);
    /// }
    /// ```
    pub fn constraints_box(&self) -> Option<flui_types::BoxConstraints> {
        self.render_element
            .as_ref()
            .and_then(|elem| elem.constraints_box())
    }

    /// Set box constraints (for Box protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::{BoxConstraints, Size};
    ///
    /// mounted.set_constraints_box(BoxConstraints::tight(Size::new(100.0, 50.0)));
    /// ```
    pub fn set_constraints_box(&mut self, constraints: flui_types::BoxConstraints) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.set_constraints_box(constraints);
        }
    }

    /// Get sliver constraints (for Sliver protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::SliverConstraints;
    ///
    /// if let Some(constraints) = mounted.constraints_sliver() {
    ///     println!("Sliver constraints: {:?}", constraints);
    /// }
    /// ```
    pub fn constraints_sliver(&self) -> Option<flui_types::SliverConstraints> {
        self.render_element
            .as_ref()
            .and_then(|elem| elem.constraints_sliver())
    }

    /// Set sliver constraints (for Sliver protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::SliverConstraints;
    ///
    /// mounted.set_constraints_sliver(SliverConstraints::default());
    /// ```
    pub fn set_constraints_sliver(&mut self, constraints: flui_types::SliverConstraints) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.set_constraints_sliver(constraints);
        }
    }

    /// Get sliver geometry (for Sliver protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::SliverGeometry;
    ///
    /// if let Some(geometry) = mounted.sliver_geometry() {
    ///     println!("Sliver geometry: {:?}", geometry);
    /// }
    /// ```
    pub fn sliver_geometry(&self) -> Option<flui_types::SliverGeometry> {
        self.render_element
            .as_ref()
            .and_then(|elem| elem.sliver_geometry())
    }

    /// Set sliver geometry (for Sliver protocol).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::SliverGeometry;
    ///
    /// mounted.set_sliver_geometry(SliverGeometry::zero());
    /// ```
    pub fn set_sliver_geometry(&mut self, geometry: flui_types::SliverGeometry) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.set_sliver_geometry(geometry);
        }
    }

    /// Mark this render object as needing layout.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// mounted.mark_needs_layout();
    /// ```
    pub fn mark_needs_layout(&mut self) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.mark_needs_layout();
        }
    }

    /// Mark this render object as needing paint.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// mounted.mark_needs_paint();
    /// ```
    pub fn mark_needs_paint(&mut self) {
        if let Some(elem) = self.render_element.as_mut() {
            elem.mark_needs_paint();
        }
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Mountable for RenderHandle<Unmounted> {
    type Mounted = RenderHandle<Mounted>;

    fn mount(self, parent: Option<usize>) -> Self::Mounted {
        let tree_info = if let Some(parent_id) = parent {
            TreeInfo::with_parent(parent_id, 0)
        } else {
            TreeInfo::root()
        };

        // Create a minimal RenderElement for the mounted state
        // In a real implementation, this would create the actual render object
        let render_element = RenderElement::new(None, self.config.protocol, RuntimeArity::Variable);

        RenderHandle {
            config: self.config,
            render_element: Some(render_element),
            tree_info: Some(tree_info),
            _state: PhantomData,
        }
    }
}

impl Unmountable for RenderHandle<Mounted> {
    type Unmounted = RenderHandle<Unmounted>;

    fn unmount(self) -> Self::Unmounted {
        RenderHandle {
            config: self.config,
            render_element: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    fn tree_info(&self) -> &TreeInfo {
        self.tree_info
            .as_ref()
            .expect("Mounted RenderHandle must have TreeInfo")
    }

    fn tree_info_mut(&mut self) -> &mut TreeInfo {
        self.tree_info
            .as_mut()
            .expect("Mounted RenderHandle must have TreeInfo")
    }
}

// ============================================================================
// DEBUG IMPLEMENTATIONS
// ============================================================================

impl<S: NodeState> std::fmt::Debug for RenderHandle<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderHandle")
            .field("state", &S::state_name())
            .field("protocol", &self.config.protocol)
            .field("debug_name", &self.config.debug_name)
            .field(
                "is_mounted",
                &if S::IS_MOUNTED {
                    "true"
                } else {
                    "false"
                },
            )
            .finish()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_tree::NavigableHandle;

    #[test]
    fn test_render_config_new() {
        let config = RenderConfig::new(ProtocolId::Box, Some("TestRender"));
        assert_eq!(config.protocol(), ProtocolId::Box);
        assert_eq!(config.debug_name(), Some("TestRender"));
    }

    #[test]
    fn test_render_config_no_debug_name() {
        let config = RenderConfig::new(ProtocolId::Sliver, None);
        assert_eq!(config.protocol(), ProtocolId::Sliver);
        assert_eq!(config.debug_name(), None);
    }

    #[test]
    fn test_render_handle_unmounted() {
        let config = RenderConfig::new(ProtocolId::Box, Some("Unmounted"));
        let handle = RenderHandle::new(config);

        assert_eq!(handle.config().protocol(), ProtocolId::Box);
        assert_eq!(handle.config().debug_name(), Some("Unmounted"));
    }

    #[test]
    fn test_render_handle_mount_as_root() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);

        let mounted = unmounted.mount(None);

        // Verify tree info
        assert!(mounted.tree_info().is_root());
        assert_eq!(mounted.tree_info().depth, 0);
        assert_eq!(mounted.tree_info().child_count(), 0);

        // Verify config preserved
        assert_eq!(mounted.config().protocol(), ProtocolId::Box);
    }

    #[test]
    fn test_render_handle_mount_with_parent() {
        let config = RenderConfig::new(ProtocolId::Sliver, Some("Child"));
        let unmounted = RenderHandle::new(config);

        let mounted = unmounted.mount(Some(42));

        // Verify parent
        assert_eq!(mounted.tree_info().parent, Some(42));
        assert_eq!(mounted.tree_info().depth, 0);

        // Verify config
        assert_eq!(mounted.config().protocol(), ProtocolId::Sliver);
        assert_eq!(mounted.config().debug_name(), Some("Child"));
    }

    #[test]
    fn test_render_handle_unmount() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);
        let mounted = unmounted.mount(Some(99));

        // Verify mounted state
        assert_eq!(mounted.tree_info().parent, Some(99));

        // Unmount
        let unmounted = mounted.unmount();
        assert_eq!(unmounted.config().protocol(), ProtocolId::Box);
    }

    #[test]
    fn test_typed_navigation() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);
        let mounted = unmounted.mount(Some(100));

        // Typed parent access
        if let Some(parent_id) = mounted.parent_render() {
            assert_eq!(parent_id, RenderId::new(100));
        } else {
            panic!("Expected parent");
        }

        // Typed children access
        let children: Vec<_> = mounted.children_renders().collect();
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_navigable_handle_integration() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);
        let mut mounted = unmounted.mount(None);

        // NavigableHandle methods (auto-implemented via Unmountable)
        assert!(mounted.is_root());
        assert_eq!(mounted.depth(), 0);
        assert_eq!(mounted.child_count(), 0);

        // Add children using NavigableHandle
        mounted.add_child(10);
        mounted.add_child(20);
        assert_eq!(mounted.child_count(), 2);
        assert_eq!(mounted.children(), &[10, 20]);

        // Remove child
        assert!(mounted.remove_child(10));
        assert_eq!(mounted.child_count(), 1);
    }

    #[test]
    fn test_render_element_access() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);
        let mounted = unmounted.mount(None);

        // RenderElement should be present after mount
        assert!(mounted.render_element().is_some());
    }

    #[test]
    fn test_parent_data_access() {
        let config = RenderConfig::new(ProtocolId::Box, None);
        let unmounted = RenderHandle::new(config);
        let mounted = unmounted.mount(None);

        // Parent data access (may be None initially)
        let _parent_data = mounted.parent_data();
        // This is just testing that the method exists and compiles
    }

    #[test]
    fn test_debug_impl() {
        let config = RenderConfig::new(ProtocolId::Box, Some("Debug"));
        let unmounted = RenderHandle::new(config.clone());
        let debug_str = format!("{:?}", unmounted);
        assert!(debug_str.contains("RenderHandle"));
        assert!(debug_str.contains("Unmounted"));

        let mounted = RenderHandle::new(config).mount(None);
        let debug_str = format!("{:?}", mounted);
        assert!(debug_str.contains("RenderHandle"));
        assert!(debug_str.contains("Mounted"));
    }
}
