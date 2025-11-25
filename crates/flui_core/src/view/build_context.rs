//! Build context for view construction.

use crate::pipeline::{ElementTree, RebuildQueue};
use crate::ElementId;
use flui_reactivity::HookContext;
use parking_lot::{Mutex, RwLock};
use std::cell::Cell;
use std::sync::Arc;

/// Context provided to views during the build phase.
///
/// Provides read-only access to the element tree, hook state, and tree queries.
/// State changes happen through hooks which manage rebuild scheduling internally.
///
/// # Thread safety
///
/// `BuildContext` is `Clone + Send + Sync`. Uses `parking_lot::Mutex` for
/// interior mutability of hook state.
///
/// # Examples
///
/// ```rust,ignore
/// impl View for MyView {
///     fn build(&self, ctx: &BuildContext) -> impl IntoElement {
///         let count = use_signal(ctx, 0);
///         let theme = ctx.depend_on::<Theme>();
///
///         Text::new(format!("Count: {}", count.get()))
///     }
/// }
/// ```
#[derive(Clone)]
pub struct BuildContext {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    hook_context: Arc<Mutex<HookContext>>,
    rebuild_queue: RebuildQueue,
}

impl std::fmt::Debug for BuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildContext")
            .field("element_id", &self.element_id)
            .finish_non_exhaustive()
    }
}

impl BuildContext {
    /// Creates a new `BuildContext`.
    pub fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self {
            tree,
            element_id,
            hook_context: Arc::new(Mutex::new(HookContext::new())),
            rebuild_queue: RebuildQueue::new(),
        }
    }

    /// Creates a `BuildContext` with an existing hook context.
    pub fn with_hook_context(
        tree: Arc<RwLock<ElementTree>>,
        element_id: ElementId,
        hook_context: Arc<Mutex<HookContext>>,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context,
            rebuild_queue: RebuildQueue::new(),
        }
    }

    /// Creates a `BuildContext` with existing hook context and rebuild queue.
    pub fn with_hook_context_and_queue(
        tree: Arc<RwLock<ElementTree>>,
        element_id: ElementId,
        hook_context: Arc<Mutex<HookContext>>,
        rebuild_queue: RebuildQueue,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context,
            rebuild_queue,
        }
    }

    /// Executes a closure with mutable access to the hook context.
    pub fn with_hook_context_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        let mut hook_ctx = self.hook_context.lock();
        f(&mut hook_ctx)
    }

    /// Returns the shared hook context.
    pub fn hook_context(&self) -> Arc<Mutex<HookContext>> {
        Arc::clone(&self.hook_context)
    }

    /// Returns the rebuild queue.
    pub fn rebuild_queue(&self) -> &RebuildQueue {
        &self.rebuild_queue
    }

    /// Returns the current element ID.
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns `true` if the element still exists in the tree.
    pub fn is_valid(&self) -> bool {
        let tree = self.tree.read();
        tree.get(self.element_id).is_some()
    }

    /// Returns the element tree.
    pub fn tree(&self) -> Arc<RwLock<ElementTree>> {
        Arc::clone(&self.tree)
    }

    /// Returns the parent element ID, if any.
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree.read();
        tree.parent(self.element_id)
    }

    /// Returns `true` if this is the root element.
    pub fn is_root(&self) -> bool {
        self.parent().is_none()
    }

    /// Returns the depth in the tree. Root has depth 0.
    pub fn depth(&self) -> usize {
        let tree = self.tree.read();
        let mut depth = 0;
        let mut current = self.element_id;
        while let Some(parent) = tree.parent(current) {
            depth += 1;
            current = parent;
        }
        depth
    }

    /// Visits ancestors with a callback. Returns `false` to stop.
    pub fn visit_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(ElementId) -> bool,
    {
        let tree = self.tree.read();
        let mut current_id = tree.parent(self.element_id);

        while let Some(id) = current_id {
            if !visitor(id) {
                break;
            }
            current_id = tree.parent(id);
        }
    }

    /// Finds the nearest render object, starting from self.
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree.read();

        if let Some(element) = tree.get(self.element_id) {
            if element.render_object().is_some() {
                return Some(self.element_id);
            }
        }

        let mut current_id = tree.parent(self.element_id);
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                if element.render_object().is_some() {
                    return Some(id);
                }
            }
            current_id = tree.parent(id);
        }

        None
    }

    /// Returns the size of this element after layout.
    pub fn size(&self) -> Option<flui_types::Size> {
        let tree = self.tree.read();

        if let Some(element) = tree.get(self.element_id) {
            if element.is_render() {
                if let Some(render_state) = element.render_state() {
                    if render_state.has_size() {
                        return Some(render_state.size());
                    }
                }
            }
        }

        None
    }

    /// Creates a minimal root build context for bootstrap purposes
    ///
    /// This creates a BuildContext with a dummy element ID and minimal setup,
    /// used primarily during application bootstrap when creating the root view.
    /// The actual element tree and proper context will be established later.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let ctx = BuildContext::root();
    /// let root_element = root_view.build(&ctx);
    /// ```
    pub fn root() -> Self {
        use crate::ElementId;
        use parking_lot::RwLock;
        use std::sync::Arc;

        // Create minimal element tree for bootstrap
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Use element ID 1 as placeholder for root context
        let element_id = ElementId::new(1);

        Self {
            tree,
            element_id,
            hook_context: Arc::new(Mutex::new(HookContext::new())),
            rebuild_queue: RebuildQueue::new(),
        }
    }
}

// Thread-local BuildContext storage

thread_local! {
    static CURRENT_BUILD_CONTEXT: Cell<Option<*const BuildContext>> = const { Cell::new(None) };
}

/// RAII guard that sets the thread-local build context.
#[derive(Debug)]
pub struct BuildContextGuard {
    _private: (),
}

impl BuildContextGuard {
    /// Sets the current build context for this thread.
    ///
    /// # Panics
    ///
    /// Panics if a context is already set (nested builds not supported).
    pub fn new(context: &BuildContext) -> Self {
        CURRENT_BUILD_CONTEXT.with(|cell| {
            if cell.get().is_some() {
                panic!("BuildContext already set - nested builds not supported");
            }
            cell.set(Some(context as *const BuildContext));
        });
        Self { _private: () }
    }
}

impl Drop for BuildContextGuard {
    fn drop(&mut self) {
        CURRENT_BUILD_CONTEXT.with(|cell| {
            cell.set(None);
        });
    }
}

// Thread-local storage for auto-created test context
thread_local! {
    static AUTO_TEST_CONTEXT: std::cell::RefCell<Option<BuildContext>> = const { std::cell::RefCell::new(None) };
}

/// Returns the current thread-local build context.
///
/// # Panics
///
/// Panics if called outside of a build phase.
///
/// Note: When the `testing` feature is enabled, automatically creates a test
/// context if none exists. This is useful for unit tests.
pub fn current_build_context() -> &'static BuildContext {
    CURRENT_BUILD_CONTEXT.with(|cell| {
        if let Some(ptr) = cell.get() {
            unsafe { &*ptr }
        } else {
            #[cfg(feature = "testing")]
            {
                // Auto-create test context for convenience in tests
                AUTO_TEST_CONTEXT.with(|refcell| {
                    let mut borrow = refcell.borrow_mut();
                    if borrow.is_none() {
                        let tree = Arc::new(RwLock::new(crate::pipeline::ElementTree::new()));
                        *borrow = Some(BuildContext::new(tree, ElementId::new(1)));
                    }
                    let ctx = borrow.as_ref().unwrap();
                    let ptr = ctx as *const BuildContext;
                    cell.set(Some(ptr));
                    unsafe { &*ptr }
                })
            }
            #[cfg(not(feature = "testing"))]
            {
                panic!("No BuildContext - must be called during build phase")
            }
        }
    })
}

/// Executes a closure with the given build context set.
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()
}
