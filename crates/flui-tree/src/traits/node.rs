//! Node trait - Type-safe tree node abstraction.
//!
//! The `Node` trait defines the contract for types that can be stored in
//! FLUI's tree structures. Each node type has an associated ID type used
//! for referencing nodes when mounted in a tree.
//!
//! # FLUI Tree Architecture
//!
//! FLUI uses a multi-tree architecture similar to Flutter:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        Tree Types                           │
//! ├─────────────────┬─────────────────┬─────────────────────────┤
//! │   View Tree     │  Element Tree   │     Render Tree         │
//! │  (flui-view)    │ (flui-element)  │   (flui_rendering)      │
//! ├─────────────────┼─────────────────┼─────────────────────────┤
//! │   ViewNode      │   ElementBase   │    RenderElement        │
//! │      ↓          │       ↓         │         ↓               │
//! │   ViewId        │   ElementId     │      RenderId           │
//! └─────────────────┴─────────────────┴─────────────────────────┘
//! ```
//!
//! # Node vs Identifier
//!
//! - **`Identifier`** (flui-foundation): Trait for ID types (`ViewId`, `ElementId`, etc.)
//! - **`Node`** (this module): Trait for node types that can be stored in trees
//!
//! The relationship is: `Node::Id: Identifier`
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_tree::{Node, Children, Single, Variable};
//! use flui_foundation::ElementId;
//!
//! // Define a node type
//! struct Element {
//!     name: String,
//!     // ...
//! }
//!
//! impl Node for Element {
//!     type Id = ElementId;
//! }
//!
//! // Use with Children container
//! let mut children: Children<Element, Variable> = Children::new();
//! children.push(Element { name: "child".into() });
//! ```
//!
//! # Design Philosophy
//!
//! The `Node` trait is intentionally minimal:
//!
//! 1. **Single Responsibility**: Only defines the node ↔ ID relationship
//! 2. **No Lifecycle**: Mounting/unmounting is handled by `Children`
//! 3. **No Tree Position**: Parent/children managed externally
//! 4. **Composable**: Works with any arity (Leaf, Single, Variable)
//!
//! This separation allows:
//! - Same node type in different tree structures
//! - Generic algorithms over any node type
//! - Type-safe storage transitions (Unmounted → Mounted)

use std::fmt;

use flui_foundation::Identifier;

// ============================================================================
// NODE TRAIT
// ============================================================================

/// Trait for types that can be stored as nodes in FLUI's tree structures.
///
/// Each node type has an associated `Id` type that is used to reference
/// the node when it's mounted in a tree. The ID must implement `Identifier`
/// from flui-foundation.
///
/// # Type Parameters
///
/// - `Id`: The identifier type used to reference this node in a tree.
///   Must be `Copy + Send + Sync + Eq + Debug + 'static`.
///
/// # Requirements
///
/// Node types must be:
/// - `Sized`: Can be stored by value
/// - `Send + Sync`: Safe to use across threads
/// - `'static`: No borrowed data (owned or Arc)
///
/// # Examples
///
/// ## Basic Implementation
///
/// ```rust,ignore
/// use flui_tree::Node;
/// use flui_foundation::ElementId;
///
/// struct MyElement {
///     data: String,
/// }
///
/// impl Node for MyElement {
///     type Id = ElementId;
/// }
/// ```
///
/// ## With Generic Node
///
/// ```rust,ignore
/// use flui_tree::Node;
/// use std::marker::PhantomData;
///
/// struct GenericNode<T: Send + Sync + 'static> {
///     value: T,
/// }
///
/// // Assuming MyId implements the required traits
/// impl<T: Send + Sync + 'static> Node for GenericNode<T> {
///     type Id = MyId;
/// }
/// ```
///
/// ## Usage with Children
///
/// ```rust,ignore
/// use flui_tree::{Node, Children, Variable, Unmounted, Mounted};
///
/// fn process_children<N: Node>(
///     children: Children<N, Variable, Unmounted>,
///     parent_id: N::Id,
///     mut mount_fn: impl FnMut(N, N::Id) -> N::Id,
/// ) -> Children<N, Variable, Mounted> {
///     children.mount(parent_id, mount_fn)
/// }
/// ```
pub trait Node: Sized + Send + Sync + 'static {
    /// The identifier type for this node.
    ///
    /// This type is used to reference the node when it's mounted in a tree.
    /// Typically one of: `ViewId`, `ElementId`, `RenderId`, `LayerId`.
    type Id: Identifier;
}

// ============================================================================
// NODE EXTENSIONS
// ============================================================================

/// Extension trait providing utility methods for Node types.
///
/// This trait is automatically implemented for all types that implement `Node`.
pub trait NodeExt: Node {
    /// Returns the name of this node type (for debugging).
    fn type_name() -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns the name of the ID type (for debugging).
    fn id_type_name() -> &'static str {
        std::any::type_name::<Self::Id>()
    }
}

impl<N: Node> NodeExt for N {}

// ============================================================================
// DEBUG HELPER
// ============================================================================

/// Debug information about a node type.
///
/// Useful for runtime introspection and debugging.
#[derive(Debug, Clone, Copy)]
pub struct NodeTypeInfo {
    /// Name of the node type
    pub node_type: &'static str,
    /// Name of the ID type
    pub id_type: &'static str,
    /// Size of the node in bytes
    pub node_size: usize,
    /// Size of the ID in bytes
    pub id_size: usize,
}

impl NodeTypeInfo {
    /// Creates NodeTypeInfo for a given Node type.
    pub fn of<N: Node>() -> Self {
        Self {
            node_type: std::any::type_name::<N>(),
            id_type: std::any::type_name::<N::Id>(),
            node_size: std::mem::size_of::<N>(),
            id_size: std::mem::size_of::<N::Id>(),
        }
    }
}

impl fmt::Display for NodeTypeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Node<{}> ({}B) → Id<{}> ({}B)",
            self.node_type, self.node_size, self.id_type, self.id_size
        )
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock ID type for testing
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct TestId(std::num::NonZeroUsize);

    impl fmt::Display for TestId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "TestId({})", self.0)
        }
    }

    impl From<TestId> for usize {
        fn from(id: TestId) -> usize {
            id.0.get()
        }
    }

    impl Identifier for TestId {
        fn get(self) -> usize {
            self.0.get()
        }

        fn zip(index: usize) -> Self {
            Self(std::num::NonZeroUsize::new(index).expect("TestId cannot be 0"))
        }

        fn try_zip(index: usize) -> Option<Self> {
            std::num::NonZeroUsize::new(index).map(Self)
        }
    }

    // Mock node type
    #[derive(Debug)]
    struct TestNode {
        value: i32,
    }

    impl Node for TestNode {
        type Id = TestId;
    }

    #[test]
    fn test_node_trait() {
        let _node = TestNode { value: 42 };
        let id = TestId::zip(1);
        assert_eq!(id.get(), 1);
    }

    #[test]
    fn test_node_ext() {
        let type_name = TestNode::type_name();
        assert!(type_name.contains("TestNode"));

        let id_type_name = TestNode::id_type_name();
        assert!(id_type_name.contains("TestId"));
    }

    #[test]
    fn test_node_type_info() {
        let info = NodeTypeInfo::of::<TestNode>();

        assert!(info.node_type.contains("TestNode"));
        assert!(info.id_type.contains("TestId"));
        assert!(info.node_size > 0);
        assert!(info.id_size > 0);

        let display = format!("{}", info);
        assert!(display.contains("TestNode"));
        assert!(display.contains("TestId"));
    }

    #[test]
    fn test_identifier_checked() {
        assert!(TestId::try_zip(0).is_none());
        assert!(TestId::try_zip(1).is_some());
    }

    #[test]
    #[should_panic(expected = "TestId cannot be 0")]
    fn test_identifier_zero_panics() {
        let _ = TestId::zip(0);
    }
}
