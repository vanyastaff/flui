# flui-tree-core

Core tree abstraction traits for FLUI framework.

## ADDED Requirements

### Requirement: Generic Tree Node Interface

The `TreeNode` trait MUST provide a minimal interface for any node that can exist in a tree structure. All tree node types (View, Element, Render, Layer, Semantics) SHALL implement this trait.

#### Scenario: Access parent from node

Given a tree with parent-child relationships
When I call `node.parent()` on a child node
Then I receive `Some(parent_id)`

#### Scenario: Access children from node

Given a tree with parent-child relationships
When I call `node.children()` on a parent node
Then I receive an iterator over child IDs

#### Scenario: Root node has no parent

Given a root node in a tree
When I call `node.parent()`
Then I receive `None`

### Requirement: Read-Only Tree Access

The `TreeRead` trait MUST provide immutable access to tree nodes by their ID. Implementations SHALL be `Send + Sync` for thread safety.

#### Scenario: Get node by ID

Given a tree containing a node with ID 1
When I call `tree.get(ElementId::new(1))`
Then I receive `Some(&node)`

#### Scenario: Get non-existent node

Given a tree without node ID 99
When I call `tree.get(ElementId::new(99))`
Then I receive `None`

#### Scenario: Check node existence

Given a tree containing node ID 1
When I call `tree.contains(ElementId::new(1))`
Then I receive `true`

#### Scenario: Get tree length

Given a tree with 5 nodes
When I call `tree.len()`
Then I receive `5`

### Requirement: Tree Navigation

The `TreeNav` trait MUST provide navigation capabilities for traversing tree relationships. It SHALL extend `TreeRead` to ensure node access is available.

#### Scenario: Iterate ancestors

Given a tree with path root → parent → child
When I call `tree.ancestors(child_id)` and collect
Then I receive `[parent_id, root_id]`

#### Scenario: Iterate descendants depth-first

Given a tree with root and 3 children
When I call `tree.descendants(root_id)` and collect
Then I receive all descendant IDs in depth-first order

#### Scenario: Check if node is root

Given a tree with a root node
When I call `tree.is_root(root_id)`
Then I receive `true`

#### Scenario: Check if node is leaf

Given a tree with a leaf node (no children)
When I call `tree.is_leaf(leaf_id)`
Then I receive `true`

### Requirement: Mutable Tree Operations

The `TreeWrite` trait MUST provide mutable operations for modifying tree structure. Cycle detection SHALL prevent creating circular references.

#### Scenario: Insert node with parent

Given an empty tree
When I call `tree.insert(node, Some(parent_id))`
Then the node is added as a child of parent

#### Scenario: Remove node

Given a tree with node ID 1
When I call `tree.remove(ElementId::new(1))`
Then the node is removed and returned

#### Scenario: Reparent node

Given a tree with node A under parent P1
When I call `tree.set_parent(a_id, Some(p2_id))`
Then node A becomes a child of P2

#### Scenario: Clear tree

Given a tree with 5 nodes
When I call `tree.clear()`
Then `tree.len()` returns 0

## MODIFIED Requirements

### Requirement: Simplified Iterator Types

Iterators MUST use concrete types instead of GAT with `impl Trait` in associated types. This SHALL ensure stable Rust compatibility.

#### Scenario: Ancestors iterator is concrete type

Given a `TreeNav` implementation
When I access `tree.ancestors(id)`
Then I receive `Ancestors<'_, T>` (concrete struct, not `impl Iterator`)

#### Scenario: Descendants iterator is concrete type

Given a `TreeNav` implementation
When I access `tree.descendants(id)`
Then I receive `Descendants<'_, T>` (concrete struct)

## REMOVED Requirements

### Requirement: Render-Specific Traits Removed from flui-tree

All render-specific functionality MUST be moved to `flui_rendering`. The `flui-tree` crate SHALL NOT contain render-specific logic.

#### Scenario: RenderTreeAccess not in flui-tree

Given the `flui-tree` crate
When I try to import `RenderTreeAccess`
Then the import fails (moved to `flui_rendering`)

#### Scenario: DirtyTracking not in flui-tree

Given the `flui-tree` crate
When I try to import `DirtyTracking`
Then the import fails (moved to `flui_rendering`)

### Requirement: Element-Specific Traits Removed from flui-tree

All element-specific functionality MUST be moved to element modules. The `flui-tree` crate SHALL NOT contain element lifecycle or reconciliation logic.

#### Scenario: Reconciler not in flui-tree

Given the `flui-tree` crate
When I try to import `Reconciler`
Then the import fails (moved to element module)

#### Scenario: InheritedData not in flui-tree

Given the `flui-tree` crate
When I try to import `InheritedData`
Then the import fails (moved to element module)

### Requirement: View-Specific Traits Removed from flui-tree

All view-specific functionality MUST be moved to `flui-view`. The `flui-tree` crate SHALL NOT contain view snapshot or diff logic.

#### Scenario: TreeSnapshot not in flui-tree

Given the `flui-tree` crate
When I try to import `TreeSnapshot`
Then the import fails (moved to `flui-view`)
