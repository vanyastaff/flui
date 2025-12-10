//! ViewHandle - Typestate-based view configuration and lifecycle management
//!
//! This module implements the typestate pattern for view lifecycle,
//! separating unmounted configuration from mounted state.
//!
//! # Architecture
//!
//! ```text
//! ViewHandle<Unmounted>           ViewHandle<Mounted>
//! ├─ ViewConfig (immutable)       ├─ ViewConfig (preserved)
//! └─ PhantomData<Unmounted>       ├─ ViewObject (live state)
//!                                 ├─ depth, parent (tree position)
//!                                 └─ PhantomData<Mounted>
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::prelude::*;
//!
//! // Create unmounted view handle
//! let unmounted = ViewHandle::new(Padding::all(16.0));
//!
//! // Mount it into tree
//! let mut mounted = unmounted.mount_as_root();
//!
//! // Access view object
//! let view_obj = mounted.view_object();
//!
//! // Hot-reload: unmount preserves config
//! let unmounted = mounted.unmount();
//! let remounted = unmounted.mount_as_root();  // Recreates ViewObject
//! ```

use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::sync::Arc;

use flui_foundation::ViewId;
use flui_tree::{Depth, Mountable, Mounted, NodeState, Unmountable, Unmounted};

use crate::ViewObject;

// ============================================================================
// VIEW CONFIG - Type-erased view configuration
// ============================================================================

/// Type-erased immutable view configuration.
///
/// Stores the original view value and provides a factory to create
/// `ViewObject` instances on demand. This enables:
/// - Hot-reload (recreate ViewObject from config)
/// - Reconciliation (compare type_id for compatibility)
/// - Config preservation (ViewObject can be discarded and recreated)
///
/// # Design
///
/// Similar to Flutter's Widget concept - immutable configuration that
/// describes what to build, not the live state.
///
/// # Example
///
/// ```rust,ignore
/// // Create config from any view
/// let config = ViewConfig::new(Padding::all(16.0));
///
/// // Can create ViewObject multiple times
/// let obj1 = config.create_view_object();
/// let obj2 = config.create_view_object();  // Independent instance
/// ```
#[derive(Clone)]
pub struct ViewConfig {
    /// Unique type ID for reconciliation
    type_id: TypeId,

    /// Human-readable debug name
    debug_name: &'static str,

    /// Factory to create ViewObject from stored configuration
    ///
    /// Arc allows cheap cloning for storing in multiple places.
    /// The function takes &dyn Any (the view_data) and creates ViewObject.
    create: Arc<dyn Fn(&dyn Any) -> Box<dyn ViewObject> + Send + Sync>,

    /// Stored view value (immutable configuration)
    ///
    /// Box<dyn Any> provides type erasure while maintaining the original value.
    view_data: Arc<Box<dyn Any + Send + Sync>>,
}

impl ViewConfig {
    /// Create ViewConfig from any view that can create ViewObject.
    ///
    /// The view must be Clone + Send + 'static and provide a way to
    /// create ViewObject.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ViewConfig::new_with_factory(
    ///     Padding { padding: 16.0 },
    ///     |view: &Padding| Box::new(PaddingViewObject::new(view.clone()))
    /// );
    /// ```
    pub fn new_with_factory<V, F>(view: V, factory: F) -> Self
    where
        V: Clone + Send + Sync + 'static,
        F: Fn(&V) -> Box<dyn ViewObject> + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<V>();
        let debug_name = std::any::type_name::<V>();

        Self {
            type_id,
            debug_name,
            create: Arc::new(move |data| {
                let view = data.downcast_ref::<V>().unwrap();
                factory(view)
            }),
            view_data: Arc::new(Box::new(view)),
        }
    }

    /// Create ViewObject from stored configuration.
    ///
    /// This can be called multiple times to create independent ViewObject
    /// instances from the same configuration.
    pub fn create_view_object(&self) -> Box<dyn ViewObject> {
        (self.create)(self.view_data.as_ref().as_ref())
    }

    /// Get TypeId for reconciliation.
    ///
    /// Used to check if two ViewConfigs are compatible for updates.
    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get debug name.
    #[inline]
    pub fn debug_name(&self) -> &'static str {
        self.debug_name
    }

    /// Check if two ViewConfigs are compatible (same type).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config1 = ViewConfig::new(Padding::all(16.0));
    /// let config2 = ViewConfig::new(Padding::all(32.0));
    /// let config3 = ViewConfig::new(Text::new("Hello"));
    ///
    /// assert!(config1.can_update(&config2));  // Same type
    /// assert!(!config1.can_update(&config3)); // Different types
    /// ```
    pub fn can_update(&self, other: &ViewConfig) -> bool {
        self.type_id == other.type_id
    }
}

impl std::fmt::Debug for ViewConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewConfig")
            .field("type_id", &self.type_id)
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

// ============================================================================
// VIEW HANDLE - Typestate-based view lifecycle
// ============================================================================

/// Typestate-based view handle for lifecycle management.
///
/// This type uses compile-time typestate to enforce correct usage:
/// - `ViewHandle<Unmounted>` - Configuration only, not in tree
/// - `ViewHandle<Mounted>` - Live ViewObject in tree with position info
///
/// # Design Philosophy
///
/// Follows the same pattern as [`NodeState`](flui_tree::NodeState):
/// - Structural state (Unmounted/Mounted) tracked at compile-time
/// - Lifecycle flags (needs_build, etc.) tracked at runtime
///
/// # Type Parameters
///
/// - `S: NodeState` - Current state (Unmounted or Mounted)
///
/// # Example
///
/// ```rust,ignore
/// // Create unmounted handle
/// let unmounted: ViewHandle<Unmounted> = ViewHandle::new(MyView { ... });
///
/// // Can only access config when unmounted
/// let config = unmounted.config();
///
/// // Mount transitions to Mounted state
/// let mounted: ViewHandle<Mounted> = unmounted.mount_as_root();
///
/// // Can now access ViewObject and tree info
/// let view_obj = mounted.view_object();
/// let parent = mounted.parent();
/// ```
pub struct ViewHandle<S: NodeState> {
    /// Immutable view configuration (always present)
    ///
    /// Preserved across mount/unmount cycles for hot-reload support.
    config: ViewConfig,

    /// Live ViewObject (Some for Mounted, None for Unmounted)
    ///
    /// Created from config during mount, discarded during unmount.
    view_object: Option<Box<dyn ViewObject>>,

    /// Tree position - depth in hierarchy (like Flutter's Element._depth)
    depth: Depth,

    /// Tree position - parent ID (like Flutter's Element._parent)
    parent: Option<ViewId>,

    /// Typestate marker (zero-sized)
    _state: PhantomData<S>,
}

// ============================================================================
// UNMOUNTED STATE
// ============================================================================

impl ViewHandle<Unmounted> {
    /// Create new unmounted view handle from config.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ViewConfig::new_with_factory(
    ///     Padding { padding: 16.0 },
    ///     |p| Box::new(PaddingViewObject::new(p.clone()))
    /// );
    /// let handle = ViewHandle::from_config(config);
    /// ```
    pub fn from_config(config: ViewConfig) -> Self {
        Self {
            config,
            view_object: None,
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }

    /// Access the view configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let unmounted = ViewHandle::from_config(config);
    /// println!("View type: {}", unmounted.config().debug_name());
    /// ```
    pub fn config(&self) -> &ViewConfig {
        &self.config
    }

    /// Mount as root node.
    ///
    /// Creates ViewObject and transitions to Mounted state.
    pub fn mount_as_root(self) -> ViewHandle<Mounted> {
        let view_object = Some(self.config.create_view_object());

        ViewHandle {
            config: self.config,
            view_object,
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }

    /// Mount as child of parent.
    ///
    /// Creates ViewObject and transitions to Mounted state.
    pub fn mount_as_child(self, parent: ViewId, parent_depth: Depth) -> ViewHandle<Mounted> {
        let view_object = Some(self.config.create_view_object());

        ViewHandle {
            config: self.config,
            view_object,
            depth: parent_depth.child_depth(),
            parent: Some(parent),
            _state: PhantomData,
        }
    }

    /// Mount with explicit parent and depth.
    pub fn mount(self, parent: Option<ViewId>, depth: Depth) -> ViewHandle<Mounted> {
        let view_object = Some(self.config.create_view_object());

        ViewHandle {
            config: self.config,
            view_object,
            depth,
            parent,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// MOUNTED STATE
// ============================================================================

impl ViewHandle<Mounted> {
    /// Access the view configuration.
    ///
    /// Config is preserved when mounted to enable hot-reload.
    pub fn config(&self) -> &ViewConfig {
        &self.config
    }

    /// Access the live ViewObject.
    ///
    /// This is always safe to call for Mounted handles as ViewObject
    /// is guaranteed to be present.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mounted = unmounted.mount_as_root();
    /// let view_obj = mounted.view_object();
    /// view_obj.build(ctx);
    /// ```
    pub fn view_object(&self) -> &dyn ViewObject {
        self.view_object.as_ref().unwrap().as_ref()
    }

    /// Access the live ViewObject mutably.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut mounted = unmounted.mount_as_root();
    /// let view_obj = mounted.view_object_mut();
    /// view_obj.build(ctx);
    /// ```
    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        self.view_object.as_mut().unwrap().as_mut()
    }

    /// Unmount from tree.
    ///
    /// Discards ViewObject but preserves config for hot-reload.
    pub fn unmount(self) -> ViewHandle<Unmounted> {
        ViewHandle {
            config: self.config, // Preserve config for hot-reload
            view_object: None,   // Discard live ViewObject
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }

    /// Get parent ViewId.
    ///
    /// Returns `None` if this is the root view.
    #[inline]
    pub fn parent(&self) -> Option<ViewId> {
        self.parent
    }

    /// Check if this is the root view.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Get depth in tree.
    #[inline]
    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// Redepth from parent (Flutter's redepthChild pattern).
    pub fn redepth_from_parent(&mut self, parent_depth: Depth) {
        if self.depth <= parent_depth {
            self.depth = parent_depth.child_depth();
        }
    }
}

// ============================================================================
// TYPED NAVIGATION (ViewId instead of usize)
// ============================================================================

impl ViewHandle<Mounted> {
    /// Get the parent ViewId.
    ///
    /// Returns `None` if this is the root view.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(parent_id) = mounted.parent_view() {
    ///     println!("Parent: {:?}", parent_id);
    /// }
    /// ```
    pub fn parent_view(&self) -> Option<ViewId> {
        self.parent
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Mountable for ViewHandle<Unmounted> {
    type Id = ViewId;
    type Mounted = ViewHandle<Mounted>;

    fn mount(self, parent: Option<ViewId>, parent_depth: Depth) -> ViewHandle<Mounted> {
        let view_object = Some(self.config.create_view_object());
        let depth = if parent.is_some() {
            parent_depth.child_depth()
        } else {
            Depth::root()
        };

        ViewHandle {
            config: self.config,
            view_object,
            depth,
            parent,
            _state: PhantomData,
        }
    }
}

impl Unmountable for ViewHandle<Mounted> {
    type Id = ViewId;
    type Unmounted = ViewHandle<Unmounted>;

    fn parent(&self) -> Option<ViewId> {
        self.parent
    }

    fn depth(&self) -> Depth {
        self.depth
    }

    fn unmount(self) -> ViewHandle<Unmounted> {
        ViewHandle {
            config: self.config, // Preserve config for hot-reload
            view_object: None,   // Discard live ViewObject
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// DEBUG IMPLEMENTATIONS
// ============================================================================

impl<S: NodeState> std::fmt::Debug for ViewHandle<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewHandle")
            .field("state", &S::name())
            .field("config", &self.config)
            .field("has_view_object", &self.view_object.is_some())
            .field("depth", &self.depth)
            .field("parent", &self.parent)
            .finish()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViewMode;

    // Mock ViewObject for testing
    struct MockViewObject {
        value: i32,
    }

    impl ViewObject for MockViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn crate::BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    // Mock View for testing
    #[derive(Clone)]
    struct MockView {
        value: i32,
    }

    #[test]
    fn test_view_config_creation() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        assert_eq!(config.type_id(), TypeId::of::<MockView>());
        assert!(config.debug_name().contains("MockView"));
    }

    #[test]
    fn test_view_config_create_multiple() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        // Can create multiple ViewObjects
        let obj1 = config.create_view_object();
        let obj2 = config.create_view_object();

        assert_eq!(obj1.mode(), ViewMode::Stateless);
        assert_eq!(obj2.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_view_config_can_update() {
        let config1 = ViewConfig::new_with_factory(MockView { value: 1 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });
        let config2 = ViewConfig::new_with_factory(MockView { value: 2 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        assert!(config1.can_update(&config2));
    }

    #[test]
    fn test_view_handle_unmounted() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let unmounted = ViewHandle::from_config(config);

        assert_eq!(unmounted.config().type_id(), TypeId::of::<MockView>());
    }

    #[test]
    fn test_view_handle_mount_as_root() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let unmounted = ViewHandle::from_config(config);
        let mounted = unmounted.mount_as_root();

        // Check mounted state
        assert!(mounted.is_root());
        assert_eq!(mounted.depth(), Depth::root());
        assert_eq!(mounted.view_object().mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_view_handle_mount_as_child() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let parent_id = ViewId::new(10);
        let unmounted = ViewHandle::from_config(config);
        let mounted = unmounted.mount_as_child(parent_id, Depth::root());

        assert!(!mounted.is_root());
        assert_eq!(mounted.parent(), Some(parent_id));
        assert_eq!(mounted.depth(), Depth::new(1)); // parent depth + 1
    }

    #[test]
    fn test_view_handle_unmount() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let unmounted = ViewHandle::from_config(config);
        let mounted = unmounted.mount_as_root();
        let unmounted_again = mounted.unmount();

        // Config preserved
        assert_eq!(unmounted_again.config().type_id(), TypeId::of::<MockView>());
    }

    #[test]
    fn test_typed_navigation() {
        let config = ViewConfig::new_with_factory(MockView { value: 1 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let parent_id = ViewId::new(10);
        let unmounted = ViewHandle::from_config(config);
        let mounted = unmounted.mount_as_child(parent_id, Depth::root());

        // Typed ViewId methods
        if let Some(pid) = mounted.parent_view() {
            assert_eq!(pid.get(), 10);
        } else {
            panic!("Expected parent");
        }
    }

    #[test]
    fn test_redepth() {
        let config = ViewConfig::new_with_factory(MockView { value: 42 }, |view| {
            Box::new(MockViewObject { value: view.value })
        });

        let mut mounted = ViewHandle::from_config(config).mount_as_root();

        // Initial depth is 0 (root)
        assert_eq!(mounted.depth(), Depth::root());

        // Redepth from deeper parent
        mounted.redepth_from_parent(Depth::new(5));
        assert_eq!(mounted.depth(), Depth::new(6));
    }
}
