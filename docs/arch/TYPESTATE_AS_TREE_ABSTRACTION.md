# Typestate as Universal Tree Abstraction

## Key Insight

All three trees in FLUI have identical lifecycle and structure:

| Tree | Node type | Children | Lifecycle |
|------|-----------|----------|-----------|
| **ViewTree** | ViewHandle | Yes (Padding.child, Flex.children) | unmounted → mounted |
| **ElementTree** | Element | Yes (parent-child structure) | unmounted → mounted |
| **RenderTree** | RenderNode | Yes (RenderPadding has child) | unmounted → mounted |

**Conclusion:** Typestate should be a **universal abstraction** in `flui-tree`!

This changes everything - typestate is no longer just about Views, but about the **fundamental tree structure** of FLUI.

---

## Approach 1: Shared Typestate Markers + Concrete Handles

### Design

```rust
// ============================================================================
// flui-tree/src/state.rs - Shared typestate markers
// ============================================================================

/// Marker trait for node states
pub trait NodeState: Send + Sync + 'static {
    const IS_MOUNTED: bool;
}

/// Unmounted state - node has config but is not in tree
pub struct Unmounted;
impl NodeState for Unmounted {
    const IS_MOUNTED: bool = false;
}

/// Mounted state - node is in tree with parent/children
pub struct Mounted;
impl NodeState for Mounted {
    const IS_MOUNTED: bool = true;
}

/// Common tree information (present only when Mounted)
#[derive(Debug, Clone)]
pub struct TreeInfo {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: usize,
}

/// Trait for nodes that can be mounted
pub trait Mountable {
    type Unmounted;
    type Mounted;

    /// Mount an unmounted node, returning mounted version
    fn mount(unmounted: Self::Unmounted, parent: Option<usize>) -> Self::Mounted;

    /// Unmount a mounted node, returning unmounted version
    fn unmount(mounted: Self::Mounted) -> Self::Unmounted;
}
```

### Usage in Each Crate

```rust
// ============================================================================
// flui-view/src/handle.rs - ViewHandle with typestate
// ============================================================================

use flui_tree::{NodeState, Unmounted, Mounted, TreeInfo, Mountable};

/// View handle with typestate
pub struct ViewHandle<S: NodeState> {
    type_id: TypeId,
    debug_name: &'static str,

    // Present in both states
    config: Box<dyn Any + Send + Sync>,  // Original view config

    // Present only when Mounted
    view_object: Option<Box<dyn ViewObject>>,
    tree_info: Option<TreeInfo>,

    _state: PhantomData<S>,
}

impl ViewHandle<Unmounted> {
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        Self {
            type_id: TypeId::of::<V>(),
            debug_name: std::any::type_name::<V>(),
            config: Box::new(view),
            view_object: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    /// Access the view config (only available when unmounted)
    pub fn config<V: 'static>(&self) -> Option<&V> {
        self.config.downcast_ref::<V>()
    }

    /// Mount: Unmounted → Mounted
    pub fn mount(self, parent: Option<usize>) -> ViewHandle<Mounted> {
        let view_object = create_view_object(&self.config);

        ViewHandle {
            type_id: self.type_id,
            debug_name: self.debug_name,
            config: self.config,
            view_object: Some(view_object),
            tree_info: Some(TreeInfo {
                parent,
                children: Vec::new(),
                depth: 0,
            }),
            _state: PhantomData,
        }
    }
}

impl ViewHandle<Mounted> {
    /// Access the ViewObject (only available when mounted)
    pub fn view_object(&self) -> &dyn ViewObject {
        self.view_object.as_ref().unwrap()
    }

    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        self.view_object.as_mut().unwrap()
    }

    /// Access tree info
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()
    }

    /// Unmount: Mounted → Unmounted
    pub fn unmount(self) -> ViewHandle<Unmounted> {
        ViewHandle {
            type_id: self.type_id,
            debug_name: self.debug_name,
            config: self.config,  // Config preserved!
            view_object: None,
            tree_info: None,
            _state: PhantomData,
        }
    }
}

impl Mountable for ViewHandle<Unmounted> {
    type Unmounted = ViewHandle<Unmounted>;
    type Mounted = ViewHandle<Mounted>;

    fn mount(unmounted: Self::Unmounted, parent: Option<usize>) -> Self::Mounted {
        unmounted.mount(parent)
    }

    fn unmount(mounted: Self::Mounted) -> Self::Unmounted {
        mounted.unmount()
    }
}

// ============================================================================
// flui-element/src/handle.rs - ElementHandle with typestate
// ============================================================================

use flui_tree::{NodeState, Unmounted, Mounted, TreeInfo};

pub struct ElementHandle<S: NodeState> {
    id: ElementId,

    // Present only when Mounted
    parent: Option<ElementId>,
    children: Vec<ElementId>,

    _state: PhantomData<S>,
}

impl ElementHandle<Unmounted> {
    pub fn new(id: ElementId) -> Self {
        Self {
            id,
            parent: None,
            children: Vec::new(),
            _state: PhantomData,
        }
    }

    pub fn mount(self, parent: Option<ElementId>) -> ElementHandle<Mounted> {
        ElementHandle {
            id: self.id,
            parent,
            children: Vec::new(),
            _state: PhantomData,
        }
    }
}

impl ElementHandle<Mounted> {
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    pub fn add_child(&mut self, child: ElementId) {
        self.children.push(child);
    }
}

// ============================================================================
// flui-rendering/src/handle.rs - RenderHandle with typestate
// ============================================================================

use flui_tree::{NodeState, Unmounted, Mounted, TreeInfo};

pub struct RenderHandle<S: NodeState> {
    // Present in both states
    render_object: Box<dyn RenderObject>,

    // Present only when Mounted
    tree_info: Option<TreeInfo>,
    needs_layout: bool,
    needs_paint: bool,

    _state: PhantomData<S>,
}

impl RenderHandle<Unmounted> {
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object,
            tree_info: None,
            needs_layout: false,
            needs_paint: false,
            _state: PhantomData,
        }
    }

    pub fn mount(self, parent: Option<usize>) -> RenderHandle<Mounted> {
        RenderHandle {
            render_object: self.render_object,
            tree_info: Some(TreeInfo {
                parent,
                children: Vec::new(),
                depth: 0,
            }),
            needs_layout: true,
            needs_paint: true,
            _state: PhantomData,
        }
    }
}

impl RenderHandle<Mounted> {
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    pub fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }
}
```

### Evaluation

**✅ Pros:**
- Each crate has full control over structure
- Can add crate-specific fields (e.g., `needs_layout` only for RenderHandle)
- Shared typestate markers provide consistency
- Flexible and extensible

**⚠️ Cons:**
- Some code duplication (but not much)
- Each crate must implement state transitions

---

## Approach 2: Fully Generic TreeNode<S, Data>

### Design

```rust
// ============================================================================
// flui-tree/src/node.rs - Generic tree node with typestate
// ============================================================================

use std::marker::PhantomData;

/// Generic tree node with typestate
pub struct TreeNode<S: NodeState, Config, Live> {
    // Present in both states
    id: usize,
    config: Config,  // Immutable configuration

    // Present only when Mounted
    live_data: Option<Live>,  // Live object (ViewObject, RenderObject, etc.)
    tree_info: Option<TreeInfo>,

    _state: PhantomData<S>,
}

impl<Config, Live> TreeNode<Unmounted, Config, Live> {
    pub fn new(id: usize, config: Config) -> Self {
        Self {
            id,
            config,
            live_data: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    /// Access config (only when unmounted)
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Mount with a factory function
    pub fn mount<F>(self, parent: Option<usize>, create_live: F) -> TreeNode<Mounted, Config, Live>
    where
        F: FnOnce(&Config) -> Live,
    {
        let live_data = create_live(&self.config);

        TreeNode {
            id: self.id,
            config: self.config,
            live_data: Some(live_data),
            tree_info: Some(TreeInfo {
                parent,
                children: Vec::new(),
                depth: 0,
            }),
            _state: PhantomData,
        }
    }
}

impl<Config, Live> TreeNode<Mounted, Config, Live> {
    /// Access live data (only when mounted)
    pub fn live_data(&self) -> &Live {
        self.live_data.as_ref().unwrap()
    }

    pub fn live_data_mut(&mut self) -> &mut Live {
        self.live_data.as_mut().unwrap()
    }

    /// Access tree info
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()
    }

    /// Unmount back to config-only state
    pub fn unmount(self) -> TreeNode<Unmounted, Config, Live> {
        TreeNode {
            id: self.id,
            config: self.config,  // Config preserved!
            live_data: None,
            tree_info: None,
            _state: PhantomData,
        }
    }
}
```

### Usage in Each Crate

```rust
// ============================================================================
// flui-view/src/handle.rs - Type alias approach
// ============================================================================

use flui_tree::{TreeNode, Unmounted, Mounted};

// View config (immutable)
pub struct ViewConfig {
    type_id: TypeId,
    debug_name: &'static str,
    view_data: Box<dyn Any + Send + Sync>,
}

// View live data (mutable)
pub struct ViewLiveData {
    view_object: Box<dyn ViewObject>,
}

/// ViewHandle is just a type alias!
pub type ViewHandle<S> = TreeNode<S, ViewConfig, ViewLiveData>;

// Helper functions
impl ViewHandle<Unmounted> {
    pub fn from_view<V: IntoView + Clone + 'static>(view: V) -> Self {
        let config = ViewConfig {
            type_id: TypeId::of::<V>(),
            debug_name: std::any::type_name::<V>(),
            view_data: Box::new(view),
        };

        TreeNode::new(0, config)
    }
}

impl ViewHandle<Mounted> {
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.live_data().view_object
    }
}

// ============================================================================
// flui-element/src/handle.rs - Type alias approach
// ============================================================================

pub struct ElementConfig {
    element_id: ElementId,
}

pub struct ElementLiveData {
    // Element-specific live data
}

pub type ElementHandle<S> = TreeNode<S, ElementConfig, ElementLiveData>;

// ============================================================================
// flui-rendering/src/handle.rs - Type alias approach
// ============================================================================

pub struct RenderConfig {
    // RenderObject creation params
}

pub struct RenderLiveData {
    render_object: Box<dyn RenderObject>,
    needs_layout: bool,
    needs_paint: bool,
}

pub type RenderHandle<S> = TreeNode<S, RenderConfig, RenderLiveData>;
```

### Evaluation

**✅ Pros:**
- Zero code duplication - all logic in `TreeNode`
- Extremely DRY
- Consistent API across all trees
- Easy to add new node types

**❌ Cons:**
- Less flexible - must fit into `TreeNode` structure
- Requires defining Config/Live types for each node
- Cannot add node-specific methods easily
- Type aliases hide the actual structure

---

## Comparison

| Aspect | Approach 1: Markers + Handles | Approach 2: Generic TreeNode |
|--------|-------------------------------|------------------------------|
| **Code duplication** | Some | None |
| **Flexibility** | ✅ High | ⚠️ Limited |
| **Type safety** | ✅ Full | ✅ Full |
| **Extensibility** | ✅ Easy | ⚠️ Harder |
| **Clarity** | ✅ Clear types | ⚠️ Type aliases |
| **Node-specific methods** | ✅ Easy | ❌ Difficult |
| **Learning curve** | ✅ Low | ⚠️ Higher |

---

## Hybrid Approach (Recommended)

Combine both approaches for maximum benefit:

```rust
// ============================================================================
// flui-tree - Provide both markers AND base struct
// ============================================================================

// Markers (always available)
pub trait NodeState: Send + Sync + 'static {
    const IS_MOUNTED: bool;
}

pub struct Unmounted;
pub struct Mounted;

pub struct TreeInfo {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: usize,
}

// Optional generic base (use if it fits your needs)
pub struct TreeNode<S: NodeState, Config, Live> {
    id: usize,
    config: Config,
    live_data: Option<Live>,
    tree_info: Option<TreeInfo>,
    _state: PhantomData<S>,
}

// Helper trait for common operations
pub trait MountableNode<S: NodeState> {
    fn tree_info(&self) -> Option<&TreeInfo>;
    fn is_mounted(&self) -> bool {
        S::IS_MOUNTED
    }
}
```

**Benefits:**
- Crates can choose to use `TreeNode` or implement their own
- Typestate markers always available
- Maximum flexibility

---

## Impact on Child/Children Type Erasure Problem

Even with universal typestate, we still need type erasure:

```rust
// flui-view/src/children.rs
pub struct Child {
    // Still need trait object for heterogeneous types!
    inner: Option<Box<dyn AnyUnmountedView>>,
}

pub trait AnyUnmountedView {
    fn mount(self: Box<Self>, parent: Option<usize>) -> Box<dyn ViewObject>;
}

// But now ViewHandle implements this automatically!
impl<V: IntoView + Clone + 'static> AnyUnmountedView for ViewHandle<Unmounted> {
    fn mount(self: Box<Self>, parent: Option<usize>) -> Box<dyn ViewObject> {
        let mounted = (*self).mount(parent);
        mounted.view_object  // Extract ViewObject
    }
}
```

**Key Insight:** Typestate is still valuable even with type erasure, because:
1. ✅ Provides compile-time safety for non-erased code paths
2. ✅ Documents the intended lifecycle in the type system
3. ✅ Makes state transitions explicit
4. ✅ Enables better API design (methods only available in correct state)
5. ✅ Universal pattern across all three trees

---

## Recommendation

**Use Approach 1: Shared Markers + Concrete Handles**

Reasons:
1. Each tree has unique requirements (RenderHandle needs layout flags, ElementHandle needs lifecycle)
2. Flexibility to evolve each tree independently
3. Clear, explicit types (not hidden behind type aliases)
4. Easy to add tree-specific methods
5. Still gets all benefits of shared typestate markers

**Next Steps:**
1. Implement typestate markers in `flui-tree`
2. Update `ViewHandle` to use typestate
3. Update `ElementHandle` to use typestate
4. Update `RenderHandle` to use typestate
5. Update `Child`/`Children` to work with typestate views

This makes typestate a **fundamental architectural pattern** in FLUI, not just a detail of one tree.
