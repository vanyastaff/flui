//! ElementHandle - Typestate-based element configuration and lifecycle management
//!
//! This module implements the typestate pattern for element lifecycle,
//! coordinating between View and Render trees.
//!
//! # Architecture
//!
//! ```text
//! ElementHandle<Unmounted>           ElementHandle<Mounted>
//! ├─ ElementConfig (immutable)       ├─ ElementConfig (preserved)
//! │  ├─ View variant                 ├─ Element (live state)
//! │  │  └─ ViewHandle<Unmounted>     │  ├─ View(ViewElement)
//! │  └─ Render variant               │  │  └─ ViewHandle<Mounted>
//! │     └─ RenderConfig              │  └─ Render(RenderElement)
//! └─ PhantomData<Unmounted>          ├─ TreeInfo (tree position)
//!                                    └─ PhantomData<Mounted>
//! ```
//!
//! # Element Coordination
//!
//! ElementHandle coordinates between:
//! - **ViewTree**: ViewHandle<Mounted> for component views
//! - **RenderTree**: RenderObject for layout/paint
//! - **ElementTree**: Element with parent/children relationships
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_element::prelude::*;
//! use flui_tree::{Mountable, Unmountable, NavigableHandle};
//!
//! // Create unmounted element from view
//! let view_handle = ViewHandle::from_config(view_config);
//! let unmounted = ElementHandle::view(view_handle, None);
//!
//! // Mount into element tree
//! let mut mounted = unmounted.mount(None);
//!
//! // Access view if this is a view element
//! if let Some(view) = mounted.view_handle() {
//!     let view_obj = view.view_object();
//! }
//!
//! // NavigableHandle methods available
//! mounted.add_child(child_id);
//! ```

use std::marker::PhantomData;

use flui_foundation::{ElementId, Key};
#[cfg(test)]
use flui_tree::MountableExt;
use flui_tree::{Depth, Mountable, Mounted, NodeState, Unmountable, Unmounted};

use flui_rendering::ProtocolId;
use flui_view::ViewMode;

use crate::element::Element;

// ============================================================================
// ELEMENT CONFIG - Type-erased element configuration
// ============================================================================

/// Element configuration for creating View or Render elements.
///
/// This enum holds the configuration needed to create either a ViewElement
/// or RenderElement. The configuration is preserved across mount/unmount
/// cycles for hot-reload support.
///
/// # Variants
///
/// - `View`: Component element that builds children (links to ViewHandle)
/// - `Render`: Render element for layout/paint (links to RenderObject)
///
/// # Design Philosophy
///
/// ElementConfig coordinates between trees:
/// - For View: holds ViewHandle<Unmounted>
/// - For Render: holds render configuration
///
/// This enables proper hot-reload where ViewObject can be recreated from config.
#[derive(Debug)]
pub enum ElementConfig {
    /// Component element configuration (Stateless, Stateful, Provider, etc.)
    View {
        /// View handle with configuration
        ///
        /// This preserves the ViewHandle across unmount/remount cycles.
        /// When unmounted, this is ViewHandle<Unmounted>.
        /// When mounted, we'll store ViewHandle<Mounted> in Element instead.
        view_mode: ViewMode,

        /// Optional key for reconciliation
        key: Option<Key>,

        /// Debug name for diagnostics
        debug_name: Option<&'static str>,
    },

    /// Render element configuration (RenderBox, RenderSliver)
    Render {
        /// Protocol ID (Box or Sliver)
        protocol: ProtocolId,

        /// Debug name for diagnostics
        debug_name: Option<&'static str>,
    },
}

impl ElementConfig {
    /// Create View element configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ElementConfig::view(ViewMode::Stateless, None);
    /// ```
    pub fn view(mode: ViewMode, key: Option<Key>) -> Self {
        Self::View {
            view_mode: mode,
            key,
            debug_name: None,
        }
    }

    /// Create View element configuration with debug name.
    pub fn view_with_name(mode: ViewMode, key: Option<Key>, debug_name: &'static str) -> Self {
        Self::View {
            view_mode: mode,
            key,
            debug_name: Some(debug_name),
        }
    }

    /// Create Render element configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ElementConfig::render(ProtocolId::Box);
    /// ```
    pub fn render(protocol: ProtocolId) -> Self {
        Self::Render {
            protocol,
            debug_name: None,
        }
    }

    /// Create Render element configuration with debug name.
    pub fn render_with_name(protocol: ProtocolId, debug_name: &'static str) -> Self {
        Self::Render {
            protocol,
            debug_name: Some(debug_name),
        }
    }

    /// Check if this is a View configuration.
    #[inline]
    pub fn is_view(&self) -> bool {
        matches!(self, ElementConfig::View { .. })
    }

    /// Check if this is a Render configuration.
    #[inline]
    pub fn is_render(&self) -> bool {
        matches!(self, ElementConfig::Render { .. })
    }

    /// Get debug name if available.
    pub fn debug_name(&self) -> Option<&'static str> {
        match self {
            ElementConfig::View { debug_name, .. } => *debug_name,
            ElementConfig::Render { debug_name, .. } => *debug_name,
        }
    }
}

// ============================================================================
// ELEMENT HANDLE - Typestate-based element lifecycle
// ============================================================================

/// Typestate-based element handle for lifecycle management.
///
/// This type uses compile-time typestate to enforce correct usage:
/// - `ElementHandle<Unmounted>` - Configuration only, not in tree
/// - `ElementHandle<Mounted>` - Live Element in tree with position info
///
/// # Design Philosophy
///
/// Follows the same pattern as [`ViewHandle`](flui_view::ViewHandle):
/// - Structural state (Unmounted/Mounted) tracked at compile-time
/// - Lifecycle flags (needs_build, etc.) tracked at runtime
///
/// # Coordination
///
/// ElementHandle coordinates:
/// - ViewHandle for component views
/// - Element enum (View or Render variant)
/// - TreeInfo for element tree position
///
/// # Type Parameters
///
/// - `S: NodeState` - Current state (Unmounted or Mounted)
///
/// # Example
///
/// ```rust,ignore
/// // Create unmounted element
/// let unmounted: ElementHandle<Unmounted> =
///     ElementHandle::view(ViewMode::Stateless, None);
///
/// // Mount transitions to Mounted state
/// let mounted: ElementHandle<Mounted> = unmounted.mount_root();
///
/// // Can now access Element and tree info
/// let element = mounted.element();
/// let parent = mounted.parent();  // Unmountable method
/// ```
pub struct ElementHandle<S: NodeState> {
    /// Immutable element configuration (always present)
    ///
    /// Preserved across mount/unmount cycles for hot-reload support.
    config: ElementConfig,

    /// Live Element (Some for Mounted, None for Unmounted)
    ///
    /// Created from config during mount, discarded during unmount.
    element: Option<Element>,

    /// Tree position fields (Flutter-style, stored directly)
    depth: Depth,
    parent: Option<ElementId>,

    /// Typestate marker (zero-sized)
    _state: PhantomData<S>,
}

// ============================================================================
// UNMOUNTED STATE
// ============================================================================

impl ElementHandle<Unmounted> {
    /// Create new unmounted element handle from config.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ElementConfig::view(ViewMode::Stateless, None);
    /// let handle = ElementHandle::from_config(config);
    /// ```
    pub fn from_config(config: ElementConfig) -> Self {
        Self {
            config,
            element: None,
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }

    /// Create unmounted View element.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handle = ElementHandle::view(ViewMode::Stateless, None);
    /// ```
    pub fn view(mode: ViewMode, key: Option<Key>) -> Self {
        Self::from_config(ElementConfig::view(mode, key))
    }

    /// Create unmounted Render element.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handle = ElementHandle::render(ProtocolId::Box);
    /// ```
    pub fn render(protocol: ProtocolId) -> Self {
        Self::from_config(ElementConfig::render(protocol))
    }

    /// Access the element configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let unmounted = ElementHandle::view(ViewMode::Stateless, None);
    /// assert!(unmounted.config().is_view());
    /// ```
    pub fn config(&self) -> &ElementConfig {
        &self.config
    }
}

// ============================================================================
// MOUNTED STATE
// ============================================================================

impl ElementHandle<Mounted> {
    /// Access the element configuration.
    ///
    /// Config is preserved when mounted to enable hot-reload.
    pub fn config(&self) -> &ElementConfig {
        &self.config
    }

    /// Access the live Element.
    ///
    /// This is always safe to call for Mounted handles as Element
    /// is guaranteed to be present.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mounted = unmounted.mount(None);
    /// let element = mounted.element();
    /// match element {
    ///     Element::View(view_elem) => { /* ... */ }
    ///     Element::Render(render_elem) => { /* ... */ }
    /// }
    /// ```
    pub fn element(&self) -> &Element {
        self.element.as_ref().unwrap()
    }

    /// Access the live Element mutably.
    pub fn element_mut(&mut self) -> &mut Element {
        self.element.as_mut().unwrap()
    }

    /// Check if this is a View element.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if mounted.is_view() {
    ///     // Handle view element
    /// }
    /// ```
    #[inline]
    pub fn is_view(&self) -> bool {
        matches!(self.element.as_ref().unwrap(), Element::View(_))
    }

    /// Check if this is a Render element.
    #[inline]
    pub fn is_render(&self) -> bool {
        matches!(self.element.as_ref().unwrap(), Element::Render(_))
    }

    /// Check if this is the root element.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Mountable for ElementHandle<Unmounted> {
    type Id = ElementId;
    type Mounted = ElementHandle<Mounted>;

    fn mount(self, parent: Option<ElementId>, parent_depth: Depth) -> Self::Mounted {
        let depth = if parent.is_some() {
            parent_depth.child_depth()
        } else {
            Depth::root()
        };

        // Create Element from config
        let element = match &self.config {
            ElementConfig::View { view_mode, .. } => {
                // Create ViewElement
                // In a real implementation, this would coordinate with ViewTree
                Element::view(None, *view_mode)
            }
            ElementConfig::Render { protocol, .. } => {
                // Create RenderElement
                // In a real implementation, this would coordinate with RenderTree
                Element::render(None, *protocol)
            }
        };

        ElementHandle {
            config: self.config,
            element: Some(element),
            depth,
            parent,
            _state: PhantomData,
        }
    }
}

impl Unmountable for ElementHandle<Mounted> {
    type Id = ElementId;
    type Unmounted = ElementHandle<Unmounted>;

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn depth(&self) -> Depth {
        self.depth
    }

    fn unmount(self) -> Self::Unmounted {
        ElementHandle {
            config: self.config, // Preserve config for hot-reload
            element: None,       // Discard live Element
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// DEBUG IMPLEMENTATIONS
// ============================================================================

impl<S: NodeState> std::fmt::Debug for ElementHandle<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementHandle")
            .field("state", &S::name())
            .field("config", &self.config)
            .field("has_element", &self.element.is_some())
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

    #[test]
    fn test_element_config_view() {
        let config = ElementConfig::view(ViewMode::Stateless, None);
        assert!(config.is_view());
        assert!(!config.is_render());
    }

    #[test]
    fn test_element_config_render() {
        let config = ElementConfig::render(ProtocolId::Box);
        assert!(!config.is_view());
        assert!(config.is_render());
    }

    #[test]
    fn test_element_handle_unmounted_view() {
        let unmounted = ElementHandle::view(ViewMode::Stateless, None);
        assert!(unmounted.config().is_view());
    }

    #[test]
    fn test_element_handle_unmounted_render() {
        let unmounted = ElementHandle::render(ProtocolId::Box);
        assert!(unmounted.config().is_render());
    }

    #[test]
    fn test_element_handle_mount_view() {
        let unmounted = ElementHandle::view(ViewMode::Stateless, None);
        let mounted = unmounted.mount_root();

        // Check mounted state
        assert!(mounted.is_root());
        assert_eq!(mounted.depth(), Depth::root());
        assert!(mounted.is_view());
    }

    #[test]
    fn test_element_handle_mount_render() {
        let unmounted = ElementHandle::render(ProtocolId::Box);
        let mounted = unmounted.mount_root();

        assert!(mounted.is_render());
    }

    #[test]
    fn test_element_handle_unmount() {
        let unmounted = ElementHandle::view(ViewMode::Stateless, None);
        let mounted = unmounted.mount_root();
        let unmounted_again = mounted.unmount();

        // Config preserved
        assert!(unmounted_again.config().is_view());
    }

    #[test]
    fn test_mount_as_child() {
        let parent_id = ElementId::new(10);
        let unmounted = ElementHandle::view(ViewMode::Stateless, None);
        let mounted = unmounted.mount_child(parent_id, Depth::new(1));

        // Unmountable methods
        assert_eq!(mounted.parent(), Some(parent_id));
        assert!(!mounted.is_root());
        assert_eq!(mounted.depth(), Depth::new(2)); // parent + 1
    }

    #[test]
    fn test_element_config_with_key() {
        let key = Key::new();
        let config = ElementConfig::view(ViewMode::Stateful, Some(key));

        if let ElementConfig::View {
            key: config_key, ..
        } = config
        {
            assert_eq!(config_key.unwrap(), key);
        } else {
            panic!("Expected View config");
        }
    }

    #[test]
    fn test_roundtrip() {
        let unmounted = ElementHandle::view(ViewMode::Stateless, None);
        let mounted = unmounted.mount_child(ElementId::new(5), Depth::new(2));

        assert_eq!(mounted.depth(), Depth::new(3));

        let unmounted = mounted.unmount();
        let remounted = unmounted.mount_root();

        assert!(remounted.is_root());
        assert_eq!(remounted.depth(), Depth::root());
    }
}
