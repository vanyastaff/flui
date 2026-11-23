//! WidgetsBinding - owner-local binding for one UI ui_runtime.
//!
//! This module provides the binding that coordinates:
//! - BuildOwner for managing element rebuilds
//! - ElementTree for storing elements
//! - Root element attachment
//!
//! # Architecture
//!
//! ```text
//! WidgetsBinding (owned by UiRuntime / headless harness / plugin pipeline)
//!   ├── build_owner: BuildOwner     (manages dirty elements)
//!   ├── element_tree: ElementTree   (stores elements)
//!   ├── root_element: ElementId     (root of element tree)
//!   └── lifecycle: LifecycleSource (app lifecycle state and its subscribers)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_view::__runtime::WidgetsBinding;
//!
//! // The UiRuntime owns the binding; construct it directly.
//! let binding = WidgetsBinding::new();
//!
//! // Attach root widget
//! binding.attach_root_widget(&MyApp);
//!
//! // In frame loop
//! binding.draw_frame();
//! ```

mod tree_cell;

use std::{cell::RefCell, rc::Rc};

// The build-phase flag is a debug assertion: release builds carry no `Cell`.
#[cfg(debug_assertions)]
use std::cell::Cell;

use flui_foundation::ElementId;
use flui_foundation::inspect::{SnapshotError, SnapshotNode, SnapshotQuery};
use flui_foundation::panic::FirstPanic;
use flui_interaction::focus::FocusManager;
use flui_rendering::pipeline::PipelineCell;
use tree_cell::TreeCell;
use tree_cell::TreePhase;

use crate::{
    owner::{BuildOwner, RecoveredPanic},
    tree::ElementTree,
    view::{BoxedView, RootRenderView, View},
};

/// Application lifecycle states.
///
/// Re-exported from [`flui_scheduler::AppLifecycleState`], the one lifecycle
/// enum (`UpdateScheduler::handle_app_lifecycle_state_change`), which is tied
/// to frame-scheduling behavior (`frames_enabled`) (ADR-0035).
pub use flui_scheduler::AppLifecycleState;

/// The owner-local binding for one widgets layer.
///
/// WidgetsBinding manages:
/// - A single ElementTree rooted at `root_element`
/// - A BuildOwner that tracks dirty elements
/// - The app lifecycle state and its subscribers
/// - First frame tracking
///
/// # Ownership
///
/// Not a singleton: the binding is owned by its `UiRuntime` (one per UI
/// session, a headless host's included). Construct via
/// [`WidgetsBinding::new`]:
///
/// ```rust,ignore
/// let binding = WidgetsBinding::new();
/// binding.attach_root_widget(&my_view);
/// ```
///
/// # Threads and re-entry
///
/// The binding stays on its owner thread. Code it runs (a view's `build` or
/// `inspect`, a lifecycle hook, a `with_element_tree` closure) may reach the
/// binding again: a call that needs the tree while it is in use gets
/// [`SnapshotError::Busy`] from [`Self::inspect_elements`] and a panic naming
/// the phase from the other accessors, never a deadlock.
pub struct WidgetsBinding {
    pub(crate) lifecycle: crate::lifecycle::LifecycleSource,
    /// Inner mutable state. `Rc` so the GlobalKey registry closures can
    /// hold a `Weak` back-reference to *this* binding's tree (a dead
    /// binding's keys resolve to `None` — the weak-callback pattern);
    /// the cell names what the tree is busy with, so re-entry is answered.
    inner: Rc<TreeCell<WidgetsBindingInner>>,

    /// This binding's GlobalKey lookup handle. It is activated by the owning
    /// `UiRuntime` for the dynamic extent of each UI runtime entry.
    pub(crate) global_key_registry: crate::key::registry::GlobalKeyRegistryHandle,

    /// Callback when a frame is needed.
    ///
    /// Owner-local: the binding never crosses a thread, so the callback needs
    /// no `Send + Sync`. `Rc` so a call can clone it out of the cell and run
    /// it with the cell's borrow released.
    on_need_frame: RefCell<Option<Rc<dyn Fn()>>>,

    /// Whether a frame is building dirty elements (debug builds only).
    ///
    /// Outside `inner`: `handle_build_scheduled` runs from inside
    /// `build_scope` (the `on_build_scheduled` callback) while the frame
    /// holds `inner`, so it reads this flag without borrowing the tree.
    /// `draw_frame_impl` sets it for the frame and a guard clears it on
    /// every exit, unwinding included.
    #[cfg(debug_assertions)]
    debug_building_dirty_elements: Cell<bool>,
}

/// Which build pass of a frame [`WidgetsBinding::draw_frame_impl`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildPass {
    /// The frame's first build: expires what the previous frame left.
    FrameEntry,
    /// A repeat within the same frame: keeps what this frame recorded.
    Repeat,
}

/// Inner mutable state of WidgetsBinding
struct WidgetsBindingInner {
    /// The build owner manages dirty elements and rebuild scheduling.
    build_owner: BuildOwner,

    /// The element tree stores all elements.
    element_tree: ElementTree,

    /// The root element ID (set after attachRootWidget).
    root_element: Option<ElementId>,

    /// Owner-local handle to the render tree's `PipelineOwner`.
    /// This is set by the application binding (`flui-app`'s `UiRuntime`)
    /// and propagated to elements during mounting.
    pipeline_owner: Option<PipelineCell>,

    /// Whether a build has been scheduled.
    build_scheduled: bool,

    /// Whether we need to report the first frame.
    need_to_report_first_frame: bool,
}

impl Default for WidgetsBinding {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned by [`WidgetsBinding::attach_root_widget`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AttachError {
    /// A root widget is already attached; call `detach_root_widget` first.
    #[error("Root widget already attached. Call detach_root_widget first.")]
    AlreadyAttached,
}

impl WidgetsBinding {
    /// Create a binding with a fresh, isolated focus manager.
    ///
    /// Production presentation composition should use
    /// [`Self::with_focus_manager`] so its input dispatcher and element tree
    /// receive the same manager.
    ///
    /// # GlobalKey registry ownership
    ///
    /// Constructing the binding creates (but does not globally install) a
    /// [`GlobalKey`](crate::GlobalKey) lookup handle whose closures hold a
    /// `Weak` reference to **this** binding's element tree, so
    /// `GlobalKey::current_element` / `with_current_state` resolve against
    /// the tree that actually hosts the elements. (The previous shape
    /// captured the `WidgetsBinding::instance()` singleton lazily, so
    /// production lookups resolved against an empty tree the moment a
    /// non-singleton binding drove the frames.) The owning runtime activates
    /// the handle only while entering this UI runtime.
    pub fn new() -> Self {
        Self::with_focus_manager(FocusManager::new())
    }

    /// Create a binding using the presentation's exact focus manager.
    ///
    /// The manager is passed directly into the binding's `BuildOwner`; no
    /// ambient registry or later setter participates in focus ownership.
    pub fn with_focus_manager(focus_manager: Rc<FocusManager>) -> Self {
        let lifecycle = crate::lifecycle::LifecycleSource::new();
        let mut build_owner = BuildOwner::with_focus_manager(focus_manager);
        build_owner.set_lifecycle_handle(lifecycle.handle());
        let inner = Rc::new(TreeCell::new(WidgetsBindingInner {
            build_owner,
            element_tree: ElementTree::new(),
            root_element: None,
            pipeline_owner: None,
            build_scheduled: false,
            need_to_report_first_frame: true,
        }));
        let global_key_registry = Self::make_global_key_registry(&inner);
        Self {
            lifecycle,
            inner,
            global_key_registry,
            on_need_frame: RefCell::new(None),
            #[cfg(debug_assertions)]
            debug_building_dirty_elements: Cell::new(false),
        }
    }

    /// Build the `GlobalKey` registry handle for one binding.
    ///
    /// The `lookup`/`visit` closures hold a `Weak` reference to the
    /// binding's inner state: lookups resolve against **this** binding's
    /// element tree while it lives, and become inert `None`s once it
    /// drops (the weak-callback pattern — a dead tree is a miss, never a
    /// dangle and never a panic).
    ///
    /// Neither closure panics on a busy tree. The binding borrows its tree
    /// exclusively across a frame, attach, detach and layout-builder build,
    /// and user code inside those (build, lifecycle hooks, dispose) may read
    /// a `GlobalKey`; the closure reports `RegistryBusy` then. A shared
    /// borrow nests, so `GlobalKey::with_current_state`'s callback, which
    /// runs inside the visit's borrow, may look up a second key.
    fn make_global_key_registry(
        inner: &Rc<TreeCell<WidgetsBindingInner>>,
    ) -> crate::key::registry::GlobalKeyRegistryHandle {
        use crate::key::registry::RegistryBusy;

        let lookup_inner = Rc::downgrade(inner);
        let visit_inner = Rc::downgrade(inner);
        crate::key::registry::GlobalKeyRegistryHandle::new(
            move |key| {
                let Some(inner) = lookup_inner.upgrade() else {
                    return Ok(None);
                };
                let Ok(inner) = inner.try_read() else {
                    return Err(RegistryBusy);
                };
                if inner.build_owner.owner_closed() {
                    return Ok(None);
                }
                Ok(inner.build_owner.element_for_global_key(key))
            },
            move |id, f| {
                let Some(inner) = visit_inner.upgrade() else {
                    return Ok(());
                };
                let Ok(inner) = inner.try_read() else {
                    return Err(RegistryBusy);
                };
                if inner.build_owner.owner_closed() {
                    return Ok(());
                }
                if let Some(node) = inner.element_tree.get(id) {
                    f(node.element());
                }
                Ok(())
            },
        )
    }

    /// This binding's own `GlobalKey` registry handle, for composing into a
    /// [`GlobalKeyRegistryComposite`](crate::__runtime::GlobalKeyRegistryComposite)
    /// spanning several bindings.
    pub(crate) fn global_key_registry_handle(
        &self,
    ) -> crate::key::registry::GlobalKeyRegistryHandle {
        self.global_key_registry.clone()
    }

    /// Set the [`PipelineCell`] for render tree management.
    ///
    /// This should be called by the application binding (`flui-app`'s
    /// `UiRuntime`) before attaching the root widget. The pipeline cell will
    /// be propagated to elements during mounting so they can create their
    /// RenderObjects.
    pub fn set_pipeline_owner(&self, owner: PipelineCell) {
        self.inner.write().pipeline_owner = Some(owner);
        tracing::debug!("WidgetsBinding: PipelineOwner set");
    }

    /// Get the [`PipelineCell`] if set.
    ///
    /// `PipelineCell` is a lock-free, closure-scoped handle, not a lock
    /// guard.
    pub fn pipeline_owner(&self) -> Option<PipelineCell> {
        self.inner.read().pipeline_owner.clone()
    }

    /// Set the callback for when a frame is needed.
    pub fn set_on_need_frame<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        // The replaced callback drops after the borrow ends, so its `Drop`
        // may reach this binding again.
        let _prev = self.on_need_frame.borrow_mut().replace(Rc::new(callback));
    }

    // ========================================================================
    // Build Owner Access
    // ========================================================================

    /// Execute a function with read access to the build owner.
    pub fn with_build_owner<R>(&self, f: impl FnOnce(&BuildOwner) -> R) -> R {
        f(&self.inner.read().build_owner)
    }

    /// Execute a function with write access to the build owner.
    pub fn with_build_owner_mut<R>(&self, f: impl FnOnce(&mut BuildOwner) -> R) -> R {
        f(&mut self.inner.write().build_owner)
    }

    /// Drain this presentation's recovered panics — every lifecycle-hook
    /// panic a per-child containment seam caught and recovered from since the
    /// last drain.
    ///
    /// Drain after the build segment before forwarding diagnostics. If a
    /// host omits the drain, the next
    /// [`Self::draw_frame`] discards the stale records with an aggregate
    /// warning. A test without a UI runtime in the loop reaches
    /// the same underlying `BuildOwner::take_recovered_panics` directly.
    ///
    /// Each record's [`context`](RecoveredPanic::context) path is read here
    /// from the live element tree when its seam left none: from the panicked
    /// element when it is still a node, else from the parent the record
    /// names, else from a lazy delegate's host. The site the seam recorded
    /// is kept.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_view::__runtime::WidgetsBinding;
    ///
    /// let binding = WidgetsBinding::new();
    /// let recovered = binding.take_recovered_panics();
    /// assert!(recovered.is_empty());
    /// ```
    #[must_use = "discarding the drain loses lifecycle-panic diagnostics"]
    pub fn take_recovered_panics(&self) -> Vec<RecoveredPanic> {
        let mut inner = self.inner.write();
        let mut recovered = inner.build_owner.take_recovered_panics();
        for panic in &mut recovered {
            panic.locate_in(&inner.element_tree);
        }
        recovered
    }

    /// Atomic seeded observer install (ADR-0040 §3): under ONE `inner`
    /// write guard, replay the current tree into `observer`
    /// (`ElementTree::replay_mounts`), then install it as the UI runtime's
    /// observer. Every frame drive also holds the `inner` write lock, so no
    /// tree mutation can interleave between replay and install — the
    /// consumer's baseline is exact.
    ///
    /// A panic during replay aborts the install: nothing is registered and
    /// the aborted install is logged (the observer proved untrustworthy
    /// before it was ever installed).
    pub fn install_tree_observer(
        &self,
        observer: std::sync::Arc<dyn flui_foundation::observe::TreeObserver>,
    ) {
        let mut inner = self.inner.write();
        let mut caught = FirstPanic::new();
        if caught
            .contain(|| inner.element_tree.replay_mounts(&*observer))
            .is_some()
        {
            inner.build_owner.set_tree_observer(observer);
        } else {
            // The replay's payload is destroyed under containment when
            // `caught` drops; the log is contained the same way.
            FirstPanic::new().contain(|| {
                tracing::error!(
                    "TreeObserver panicked during seeded replay; install aborted, nothing registered"
                );
            });
        }
    }

    /// Symmetric removal (install/clear symmetry); fires `detached()` on
    /// the outgoing observer. Idempotent.
    pub fn remove_tree_observer(&self) {
        self.inner.write().build_owner.clear_tree_observer();
    }

    // ========================================================================
    // Element Tree Access
    // ========================================================================

    /// Execute a function with read access to the element tree.
    ///
    /// # Panics
    ///
    /// When the tree is being built, inspected or lent for writing further up this thread's
    /// stack.
    #[track_caller]
    pub fn with_element_tree<R>(&self, f: impl FnOnce(&ElementTree) -> R) -> R {
        f(&self.inner.read().element_tree)
    }

    /// Execute a function with write access to the element tree.
    ///
    /// # Panics
    ///
    /// When the tree is in use further up this thread's stack.
    #[track_caller]
    pub fn with_element_tree_mut<R>(&self, f: impl FnOnce(&mut ElementTree) -> R) -> R {
        f(&mut self.inner.write().element_tree)
    }

    /// A typed snapshot of the element tree (ADR-0242): each node is an element named by the
    /// view that created it. A view's `inspect` that panics is recorded on its node as
    /// [`INSPECT_FAILED`](flui_foundation::inspect::INSPECT_FAILED).
    ///
    /// # Errors
    ///
    /// [`SnapshotError::Busy`] when asked from code the binding is running (a `build`, a view's
    /// `inspect`, a `with_element_tree_mut` closure), and the tree's own
    /// [`SnapshotError`]s otherwise.
    pub fn inspect_elements(&self, query: SnapshotQuery) -> Result<SnapshotNode, SnapshotError> {
        let inner = self.inner.try_read_in(TreePhase::Inspecting)?;
        flui_foundation::inspect::snapshot(&inner.element_tree, query)
    }

    /// Get the root element ID.
    pub fn root_element(&self) -> Option<ElementId> {
        self.inner.read().root_element
    }

    // ========================================================================
    // Root Widget Attachment
    // ========================================================================

    /// Attach a root widget to the binding.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// The PipelineOwner (if set via `set_pipeline_owner`) will be passed
    /// to the root element during mounting, enabling RenderObjectElements
    /// to create their RenderObjects.
    ///
    /// # Root bootstrap
    ///
    /// The user `view` is not mounted directly. It is wrapped in a
    /// [`RootRenderView`], and *that* is mounted as the element-tree
    /// root. [`RootRenderView`] / `RootRenderElement` own the root
    /// [`RenderView`](flui_rendering::view::RenderView) and bootstrap
    /// the render tree (creating the `RenderView`, setting it as the
    /// `PipelineOwner`'s root node). This is the single root-bootstrap
    /// path — there is no parallel direct-mount of the user view.
    ///
    /// # Errors
    ///
    /// Returns [`AttachError::AlreadyAttached`] if a root widget is
    /// already attached.
    pub fn attach_root_widget<V>(&self, view: &V) -> Result<(), AttachError>
    where
        V: View + Clone + 'static,
    {
        let mut inner = self.inner.write_in(TreePhase::Building);

        if inner.root_element.is_some() {
            return Err(AttachError::AlreadyAttached);
        }

        // Wrap the user view in `RootRenderView` so the render tree is
        // bootstrapped through `RootRenderElement` instead of mounting the
        // user view directly. The root holds its child as a `BoxedView`
        // (a `BoxedView` passed here is held as it is) and hands that shared
        // allocation to the reconciler, which unwraps it to update the
        // child's `Element<V>` in place.
        let root_render_view = BoxedView::new(RootRenderView::new(view.clone()));

        // Mount the `RootRenderView` as the element-tree root with the
        // PipelineOwner. This ensures `RootRenderElement` (and the
        // RenderObjectElements below it) can create their RenderObjects.
        // Split the borrow so the BuildOwner-derived ElementOwner handle
        // and the ElementTree borrow don't overlap.
        let pipeline_owner = inner.pipeline_owner.clone();
        let root_id = {
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            element_tree.mount_root_with_pipeline_owner(
                &root_render_view,
                pipeline_owner,
                &mut build_owner.element_owner_mut(),
            )
        };
        inner.root_element = Some(root_id);

        // Schedule initial build
        inner
            .build_owner
            .schedule_build_for(root_id, 0, crate::RebuildReason::InitialMount);
        inner.build_scheduled = true;

        tracing::debug!(?root_id, "Root widget attached");

        // Request a frame
        drop(inner); // Release lock before calling callback
        self.handle_build_scheduled();

        Ok(())
    }

    /// Detach the root widget.
    ///
    /// This clears the element tree — the WHOLE tree, not just the root
    /// node: `ElementTree::remove_subtree` (not the single-node `remove`)
    /// walks every descendant deepest-first, running each one's own
    /// `unmount`/`State::dispose` hook, exactly as a real permanent removal
    /// must. A plain `remove` would only unmount the root's own element
    /// (in practice the internal `RootRenderView` wrapper every attached
    /// view is mounted under — see [`Self::attach_root_widget`]'s doc) and
    /// silently orphan everything beneath it: no descendant's `dispose`
    /// would ever run, leaking subscriptions/listeners/timers for the life
    /// of this binding.
    pub fn detach_root_widget(&self) {
        let mut inner = self.inner.write_in(TreePhase::Building);

        if let Some(root_id) = inner.root_element.take() {
            // Remove the whole subtree (this clears the tree since root_id
            // is its root).
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            // `Finalize`: permanent teardown. Nothing runs `finalize_tree`
            // after this and no frame follows, so a keyed descendant left
            // deactivated would never have `dispose` called and a later
            // `attach_root_widget` could retake its stale state.
            element_tree.remove_subtree(
                root_id,
                &mut build_owner.element_owner_mut(),
                crate::tree::SubtreeRemoval::Finalize,
            );
            tracing::debug!(?root_id, "Root widget detached");
        }
    }

    /// Withdraw graph, inbox and key authority, handing the withdrawn key
    /// owners to the caller instead of dropping them, so no user code runs.
    pub(crate) fn withdraw_owner_authority(&self) -> Vec<crate::owner::ScopedKeyOwner> {
        self.inner.write().build_owner.withdraw_owner(false)
    }

    pub(crate) fn withdraw_root_owner(&self, preserving: bool) {
        let keys = {
            let mut inner = self.inner.write();
            let keys = inner.build_owner.withdraw_owner(preserving);
            if preserving {
                inner.root_element.take();
            }
            keys
        };
        let mut first = FirstPanic::new();
        for key in keys {
            if preserving {
                // The host already owns the failure this close preserves.
                crate::drop_contained(key);
            } else {
                first.contain(|| drop(key));
            }
        }
        first.resume();
    }

    // ========================================================================
    // Build Scheduling
    // ========================================================================

    /// Schedule a build if not already scheduled.
    pub fn schedule_build(&self) {
        let mut inner = self.inner.write();
        if !inner.build_scheduled {
            inner.build_scheduled = true;
            drop(inner); // Release lock before calling callback
            self.handle_build_scheduled();
        }
    }

    /// Schedule the root element and all its descendants for rebuild.
    ///
    /// This is useful for animation demos where the entire tree needs to
    /// rebuild each frame to reflect updated animation values.
    pub fn schedule_root_rebuild(&self) {
        let mut inner = self.inner.write();
        if let Some(root_id) = inner.root_element {
            // Collect all element IDs first to avoid borrow issues
            let elements_to_mark = Self::collect_all_elements(&inner.element_tree, root_id, 0);

            // Now mark all as dirty
            for (id, depth) in elements_to_mark {
                inner.element_tree.mark_needs_build(id);
                inner.build_owner.schedule_build_for(
                    id,
                    depth,
                    crate::RebuildReason::AnimationTick,
                );
            }

            if !inner.build_scheduled {
                inner.build_scheduled = true;
                drop(inner);
                self.handle_build_scheduled();
            }
        }
    }

    /// Iteratively collect every `(ElementId, depth)` pair reachable from
    /// `id`, in pre-order DFS order (parent before its children, children
    /// in `ElementNode::child_ids` slot order).
    ///
    /// The earlier recursive shape did
    /// `result.extend(recursive_call(child))` once per child, so each
    /// `extend` re-copied its child's entire subtree into the parent's
    /// vec. For a balanced tree of N elements that totals `O(N log N)`
    /// allocation+copy; for a degenerate chain (the FLUI worst case where
    /// many `StatelessView`s nest linearly) it is `O(N²)`. The recursion
    /// also burned stack proportional to tree depth.
    ///
    /// This implementation pre-sizes a single `Vec<(ElementId, usize)>`
    /// to `tree.len()` (every node in the slab is at most one entry) and
    /// drives the walk with an explicit `Vec` work-stack. Total work is
    /// `O(N)` with one heap allocation amortised across the whole walk;
    /// stack depth is the constant size of two `Vec`s.
    ///
    /// **Ordering contract.** The previous recursive shape pushed the
    /// current node first, then for each child appended that child's
    /// entire pre-order subtree before moving to the next child — i.e.
    /// classic pre-order DFS in `child_ids` slot order. To preserve that
    /// ordering on a LIFO work-stack we visit each node when popped and
    /// then push its children **in reverse `child_ids` order**, so
    /// the leftmost child is on top of the stack and popped next. The
    /// per-element pop / visit / push-children sequence is identical to
    /// the recursive function call sequence.
    fn collect_all_elements(
        tree: &ElementTree,
        root_id: ElementId,
        root_depth: usize,
    ) -> Vec<(ElementId, usize)> {
        // Tree-len upper-bounds the number of reachable nodes. We may
        // visit strictly fewer (the walk is rooted at `root_id`, not the
        // full slab), but over-reserving by a few entries is cheaper than
        // reallocating during the walk.
        let mut result: Vec<(ElementId, usize)> = Vec::with_capacity(tree.len());
        let mut stack: Vec<(ElementId, usize)> = Vec::with_capacity(16);
        let mut child_buf: Vec<ElementId> = Vec::with_capacity(8);

        stack.push((root_id, root_depth));

        while let Some((id, depth)) = stack.pop() {
            result.push((id, depth));

            let Some(node) = tree.get(id) else {
                continue;
            };

            // Children come from the slab
            // node's `child_ids` list — the single element graph. They
            // are already in forward slot order; we collect into a scratch
            // buffer so we can push them back onto the LIFO stack in
            // reverse, preserving the recursive shape's pre-order visit
            // order.
            child_buf.clear();
            child_buf.extend_from_slice(node.child_ids());
            for child_id in child_buf.iter().rev() {
                stack.push((*child_id, depth + 1));
            }
        }

        result
    }

    /// Called when a build has been scheduled.
    ///
    /// Checks that we're not currently building, then requests a frame from
    /// the scheduler.
    ///
    /// # Re-entrancy
    ///
    /// This method never borrows `self.inner`.
    ///
    /// It is called from inside `build_scope` (via the `on_build_scheduled`
    /// callback on `BuildOwner`) while `draw_frame` holds `inner.write()`,
    /// so borrowing `inner` here would be refused as re-entry. Instead the
    /// building flag is a `Cell<bool>` on `WidgetsBinding` itself (outside
    /// `inner`). The `on_need_frame` callback is cloned out of its cell and
    /// called with no borrow held, so it may call
    /// [`set_on_need_frame`](Self::set_on_need_frame) or schedule another
    /// build.
    ///
    /// # Panics (debug only)
    ///
    /// Panics if called while building dirty elements. This check catches
    /// `setState()` called from a layout or paint callback.
    pub fn handle_build_scheduled(&self) {
        #[cfg(debug_assertions)]
        {
            assert!(
                !self.debug_building_dirty_elements.get(),
                "setState() or markNeedsBuild() called during build.\n\
                 While the widget tree was being built, laid out, and painted, \
                 a new frame was scheduled to rebuild the widget tree.\n\
                 Do not call setState() from a build, layout, or paint callback."
            );
        }

        // Request a frame from the scheduler. The callback is user code: the
        // cell's borrow ends before it runs, and the clone keeps a callback
        // that replaces itself alive until it returns.
        let callback = self.on_need_frame.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    /// Check if there are pending builds.
    pub fn has_pending_builds(&self) -> bool {
        self.inner.read().build_owner.has_dirty_elements()
    }

    /// Get the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.inner.read().build_owner.dirty_count()
    }

    /// Record whether this presentation collects its semantics tree
    /// ([`BuildOwner::set_collects_semantics`]). On a change with a mounted
    /// tree, the next [`draw_frame`](Self::draw_frame) pushes every render
    /// view's configuration again.
    pub fn set_collects_semantics(&self, collects: bool) {
        let marked = {
            let mut inner = self.inner.write_in(TreePhase::Building);
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.set_collects_semantics(element_tree, collects)
        };
        if marked {
            self.handle_build_scheduled();
        }
    }

    /// Hot reload entry point.
    ///
    /// Marks every element dirty without unmounting or disposing state. The
    /// next [`draw_frame`](Self::draw_frame) re-runs all `build()` methods while
    /// preserving [`StatefulView`](crate::StatefulView) state in the element tree.
    pub fn perform_reassemble(&self) {
        {
            let mut inner = self.inner.write_in(TreePhase::Building);
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.reassemble(element_tree);
        }
        self.handle_build_scheduled();
    }

    // ========================================================================
    // Frame Drawing
    // ========================================================================

    /// Pump the build and rendering pipeline to generate a frame.
    ///
    /// This method is called by `handleDrawFrame`, which is called
    /// automatically by the engine when it is time to lay out and paint a
    /// frame.
    ///
    /// The application frame driver calls this once for every produced
    /// presentation segment, even when [`Self::has_pending_builds`] is false.
    /// The build drain below remains conditional, while the frame-entry
    /// expiry of undrained recovered-panic records and inactive-element
    /// finalization still run for pipeline-only and lazy-service-only frames.
    ///
    /// # Frame phases
    ///
    /// 1. **Build phase**: All dirty `Element`s in the widget tree are rebuilt.
    ///    See `State.setState` for details on marking a widget dirty.
    ///
    /// 2. **Layout phase**: (run by `UiRuntime` on the presentation's `PipelineOwner` after this build pass)
    ///
    /// 3. **Paint phase**: (run by `UiRuntime` on the presentation's `PipelineOwner` after this build pass)
    ///
    /// 4. **Finalization phase**: Inactive elements are unmounted. This causes
    ///    [State.dispose] to be invoked on removed widgets.
    ///
    /// # Panics
    ///
    /// In debug mode, panics if called while already building dirty elements
    /// (to catch accidental frame scheduling during build).
    pub fn draw_frame(&self) {
        self.draw_frame_impl(BuildPass::FrameEntry, || {});
    }

    /// Rebuild what owner code dirtied after this frame's build and layout
    /// settled, and finalize what that removed, inside the same frame.
    ///
    /// Unlike [`Self::draw_frame`] this does not open a frame: the recovered
    /// lifecycle panics the frame's first build left stay for the frame's
    /// drain. [`BuildOwner::last_frame_build_report`] then describes this
    /// pass.
    pub fn rebuild_within_frame(&self) {
        self.draw_frame_impl(BuildPass::Repeat, || {});
    }

    pub(crate) fn draw_frame_impl(&self, pass: BuildPass, before_finalize: impl FnOnce()) {
        let mut inner = self.inner.write_in(TreePhase::Building);

        #[cfg(debug_assertions)]
        let _building_guard = {
            assert!(
                !self.debug_building_dirty_elements.get(),
                "draw_frame called while already building dirty elements; \
                 ensure draw_frame is not called recursively or from a build callback"
            );
            // Set before build_scope so that any on_build_scheduled callback
            // fired from within build_scope sees building=true and panics with
            // the diagnostic message rather than enqueuing a second frame.
            //
            // Cleared by RAII rather than a store at the end of this
            // function: a panic that unwinds out of `build_scope` or
            // `finalize_tree` (e.g. a panicking `ViewState::dispose`) is
            // caught at the app runtime's per-presentation frame boundary
            // and the frame is retried on a later pump — a flag latched
            // `true` across that unwind would turn every retry into this
            // assert's false positive ("recursive draw_frame") instead of
            // re-running the frame.
            struct BuildingFlagReset<'a>(&'a Cell<bool>);
            impl Drop for BuildingFlagReset<'_> {
                fn drop(&mut self) {
                    self.0.set(false);
                }
            }
            self.debug_building_dirty_elements.set(true);
            BuildingFlagReset(&self.debug_building_dirty_elements)
        };

        if pass == BuildPass::FrameEntry {
            let discarded_recovered_panics = inner.build_owner.discard_stale_recovered_panics();
            if discarded_recovered_panics != 0 {
                tracing::warn!(
                    count = discarded_recovered_panics,
                    "discarding recovered lifecycle panics left undrained from the previous frame"
                );
            }
        }

        inner.build_scheduled = false;

        // Build phase: rebuild all dirty elements
        if inner.build_owner.has_dirty_elements() {
            tracing::debug!(
                dirty_count = inner.build_owner.dirty_count(),
                "Building dirty elements"
            );

            // Process all dirty elements.
            // We need to split the borrow to satisfy the borrow checker.
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.build_scope(element_tree);

            tracing::debug!("Build phase complete");
        } else if pass == BuildPass::FrameEntry {
            inner.build_owner.clear_frame_build_report();
        }

        // Layout and paint run after this build pass, from the owning
        // `UiRuntime`, not here.

        before_finalize();

        // Finalization phase: unmount inactive elements
        {
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.finalize_tree(element_tree);
        }

        // `debug_building_dirty_elements` resets when `_building_guard`
        // drops at the end of this function (and equally during an unwind).

        // Report first frame if needed
        if inner.need_to_report_first_frame {
            inner.need_to_report_first_frame = false;
            tracing::info!("First frame rendered");
        }
    }

    /// Service pending lazy-sliver child-build requests accumulated during the
    /// most recent layout pass.
    ///
    /// This is the **production entry point** for lazy `SliverList` /
    /// `ListView::builder` child building, on a platform runner and a headless
    /// host alike, and runs after `PipelineOwner::run_frame` has returned, so
    /// no node of the render tree is entered.
    ///
    /// `UiRuntime::draw_frame` (`flui-app`) calls it with the same
    /// `PipelineCell` that `run_frame` used. The tree is borrowed for the call
    /// and the pipeline owner is entered inside it, in the order `build_scope`
    /// already uses (tree, then pipeline).
    ///
    /// On an application without lazy lists the call only finds the two
    /// pending lists (`pending_child_requests`, `pending_retain_bands`) empty
    /// and returns.
    ///
    /// # One path for every frame
    ///
    /// A headless host runs the same `UiRuntime::draw_frame` a platform runner
    /// does, so the post-`run_frame` pipeline tail steps have this one call site.
    pub fn service_child_requests(&self, pipeline: &PipelineCell) {
        let mut inner = self.inner.write_in(TreePhase::Building);
        let WidgetsBindingInner {
            ref mut build_owner,
            ref mut element_tree,
            ..
        } = *inner;
        build_owner.service_child_requests(element_tree, pipeline);
    }

    /// Run one frame, settling every build-during-layout node before paint.
    ///
    /// Threads the `PipelineOwner` **by lock**, not by value: each layout pass
    /// takes it out under the write guard and restores it before the builders
    /// run, because `build_scope` mounts render objects through that same lock.
    ///
    /// `UiRuntime::draw_frame` reaches the shared fixpoint through here, on a
    /// platform runner and a headless host alike; no frame path hand-rolls the
    /// loop.
    ///
    /// With no `LayoutBuilder` mounted, this is exactly `PipelineOwner::run_frame`.
    pub fn run_frame_with_layout_builders(
        &self,
        pipeline: &PipelineCell,
        writer: &mut flui_layer::SceneWriter<'_>,
    ) -> Result<bool, flui_foundation::TreeError<flui_rendering::error::RenderError>> {
        let mut inner = self.inner.write_in(TreePhase::Building);
        let WidgetsBindingInner {
            ref mut build_owner,
            ref mut element_tree,
            ..
        } = *inner;
        build_owner.run_frame_with_layout_builders(element_tree, pipeline, writer)
    }

    /// [`Self::run_frame_with_layout_builders`]'s build-during-layout
    /// fixpoint alone, released before it returns, so the caller can run
    /// owner code against the settled tree before
    /// [`PipelineCell::run_frame`] paints it.
    ///
    /// # Errors
    ///
    /// The first layout error of a pass.
    pub fn settle_layout_with_builders(
        &self,
        pipeline: &PipelineCell,
    ) -> Result<(), flui_foundation::TreeError<flui_rendering::error::RenderError>> {
        let mut inner = self.inner.write_in(TreePhase::Building);
        let WidgetsBindingInner {
            ref mut build_owner,
            ref mut element_tree,
            ..
        } = *inner;
        build_owner.settle_layout_with_builders(element_tree, pipeline)
    }

    /// Whether a frame of this binding is building dirty elements; read
    /// without borrowing the tree.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.debug_building_dirty_elements.get()
    }

    /// Commit a local lifecycle observation and deliver it to the
    /// presentation's [`LifecycleHandle::subscribe`](crate::LifecycleHandle::subscribe)
    /// subscribers.
    pub fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        if self.lifecycle.commit(state).is_ok() {
            self.lifecycle.drain();
        }
    }

    /// Get the number of elements in the tree.
    pub fn element_count(&self) -> usize {
        self.inner.read().element_tree.len()
    }

    /// Give the elements of this binding the window they are presented in, so
    /// `LifecycleContext::window` answers. Call before the root is attached.
    #[doc(hidden)]
    pub fn set_window_handle(&self, handle: crate::WindowHandle) {
        self.inner.write().build_owner.set_window_handle(handle);
    }
}

impl std::fmt::Debug for WidgetsBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read();
        f.debug_struct("WidgetsBinding")
            .field("root_element", &inner.root_element)
            .field("build_scheduled", &inner.build_scheduled)
            .field("dirty_count", &inner.build_owner.dirty_count())
            .field("element_count", &inner.element_tree.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::view::IntoView;

    use std::rc::Rc;

    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::__runtime::BindingRuntime as _;
    use crate::view::RootRenderElement;
    use std::cell::Cell;

    // Carries a `PipelineCell` (via `CoreState::pipeline_owner`), so it must
    // stay pinned `!Send + !Sync` -- a stray `unsafe impl` here would let a
    // `WidgetsBinding` cross threads while its `PipelineCell` still aliases
    // the owner-thread-confined `PipelineOwner`/`RenderTree` underneath.
    assert_not_impl_any!(WidgetsBinding: Send, Sync);

    /// A render-family leaf view with no child views.
    #[derive(Clone)]
    struct LeafView;

    impl crate::RenderView for LeafView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for LeafView {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[derive(Clone, crate::StatefulView)]
    struct RegistryStateView {
        #[view(key)]
        key: crate::GlobalKey<RegistryState>,
        value: i32,
    }

    struct RegistryState(i32);

    impl crate::StatefulView for RegistryStateView {
        type State = RegistryState;

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
            RegistryState(self.value)
        }
    }

    impl crate::ViewState<RegistryStateView> for RegistryState {
        fn build(
            &self,
            _view: &RegistryStateView,
            _ctx: &dyn crate::BuildContext,
        ) -> impl IntoView {
            LeafView
        }
    }

    fn binding_is_not_a_singleton_two_instances_are_independent() {
        // The binding is ui_runtime-owned; two bindings are two
        // independent trees (the "many can exist" contract a headless test
        // relies on).
        let binding1 = WidgetsBinding::new();
        let binding2 = WidgetsBinding::new();
        assert!(!Rc::ptr_eq(&binding1.inner, &binding2.inner));
    }

    /// Run `f` on a fresh thread and fail the test if it has not returned
    /// within ten seconds.
    ///
    /// The binding is `!Send`, so `f` builds it on that thread. A lookup that
    /// re-enters the binding's frame lock parks the thread forever; the
    /// timeout turns that hang into a test failure, and the parked thread
    /// does not keep the test process alive.
    fn within_deadline<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("deadlock: GlobalKey lookup re-entered the binding's frame lock")
    }

    /// Every result a probe read, in order.
    type Seen<T> = Rc<std::cell::RefCell<Vec<Option<T>>>>;

    /// Reads `key`'s state from its own `build` and records what it saw.
    #[derive(Clone)]
    struct LookupInBuild {
        key: crate::GlobalKey<RegistryState>,
        seen: Seen<i32>,
    }

    impl crate::StatelessView for LookupInBuild {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            self.seen
                .borrow_mut()
                .push(self.key.with_current_state(|state| state.0));
            LeafView
        }
    }

    impl View for LookupInBuild {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    /// A keyed stateful parent whose child is a [`LookupInBuild`].
    #[derive(Clone, crate::StatefulView)]
    struct KeyedLookupParent {
        #[view(key)]
        key: crate::GlobalKey<RegistryState>,
        value: i32,
        child: LookupInBuild,
    }

    impl crate::StatefulView for KeyedLookupParent {
        type State = RegistryState;

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
            RegistryState(self.value)
        }
    }

    impl crate::ViewState<KeyedLookupParent> for RegistryState {
        fn build(&self, view: &KeyedLookupParent, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            view.child.clone()
        }
    }

    fn global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking() {
        let (during, after) = within_deadline(|| {
            let key = crate::GlobalKey::<RegistryState>::new();
            let seen = Seen::default();
            let binding = WidgetsBinding::new();
            binding
                .attach_root_widget(&KeyedLookupParent {
                    key: key.clone(),
                    value: 10,
                    child: LookupInBuild {
                        key: key.clone(),
                        seen: Rc::clone(&seen),
                    },
                })
                .expect("attach succeeds");
            binding.with_global_key_registry(|| binding.draw_frame());
            let after =
                binding.with_global_key_registry(|| key.with_current_state(|state| state.0));
            (seen.take(), after)
        });
        assert_eq!(
            during,
            vec![None],
            "a read inside the binding's own frame resolves to nothing"
        );
        assert_eq!(after, Some(10), "the same read after the frame resolves");
    }

    /// The mounted root element produces a working render-tree root
    /// when a `PipelineOwner` is wired — `RootRenderElement` inserts the
    /// `RenderView` and sets it as the pipeline owner's root node.
    fn test_attach_root_widget_bootstraps_render_tree() {
        let binding = WidgetsBinding::new();
        let pipeline_owner = PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ));
        binding.set_pipeline_owner(pipeline_owner.clone());

        binding
            .attach_root_widget(&LeafView)
            .expect("attach succeeds");

        let root_id = binding.root_element().expect("root element is set");

        // The RootRenderElement created a RenderView and registered it
        // as the PipelineOwner's root node.
        binding.with_element_tree(|tree| {
            let element: &dyn std::any::Any = tree.get(root_id).expect("root node").element();
            let root_render_element = element
                .downcast_ref::<RootRenderElement>()
                .expect("root is RootRenderElement");
            assert!(
                root_render_element.render_id().is_some(),
                "RootRenderElement bootstrapped a RenderView (render_id set)"
            );
        });
        assert!(
            pipeline_owner.with(|owner| owner.root_id().is_some()),
            "PipelineOwner's root node is wired to the RenderView"
        );
    }

    #[derive(Clone)]
    struct DisposeMarkerView {
        disposed: Rc<Cell<bool>>,
    }

    struct DisposeMarkerState {
        disposed: Rc<Cell<bool>>,
    }

    impl crate::StatefulView for DisposeMarkerView {
        type State = DisposeMarkerState;

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
            DisposeMarkerState {
                disposed: Rc::clone(&self.disposed),
            }
        }
    }

    impl crate::ViewState<DisposeMarkerView> for DisposeMarkerState {
        fn build(
            &self,
            _view: &DisposeMarkerView,
            _ctx: &dyn crate::BuildContext,
        ) -> impl IntoView {
            LeafView
        }

        fn dispose(&mut self) {
            self.disposed.set(true);
        }
    }

    impl View for DisposeMarkerView {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }
    }

    /// `detach_root_widget` must cascade through the WHOLE tree, not just
    /// unmount the root's own node: the attached root is always wrapped in
    /// an internal `RootRenderView` (see this method's own doc), so even a
    /// single-widget tree is already two elements deep, and a real tree is
    /// deeper still (this one plants the dispose-observing element as a
    /// stateful GRANDCHILD of the reconciled `ParentView`/`LeafView` shape
    /// used elsewhere in this file). A single-node `remove` would only ever
    /// unmount `RootRenderView`'s own element and silently leak every
    /// descendant's `State::dispose` — this test is the regression guard for
    /// that exact class of bug.
    fn detach_root_widget_disposes_every_descendant_not_just_the_root() {
        #[derive(Clone)]
        struct ParentOfDisposeMarker {
            child: DisposeMarkerView,
        }

        impl crate::StatelessView for ParentOfDisposeMarker {
            fn build(&self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
                self.child.clone()
            }
        }

        impl View for ParentOfDisposeMarker {
            fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
                crate::element::ElementKind::stateless(self)
            }
        }

        let binding = WidgetsBinding::new();
        let disposed = Rc::new(Cell::new(false));
        let view = ParentOfDisposeMarker {
            child: DisposeMarkerView {
                disposed: Rc::clone(&disposed),
            },
        };

        binding.attach_root_widget(&view).expect("attach succeeds");
        binding.draw_frame();
        assert!(!disposed.get(), "precondition: nothing has torn down yet");

        binding.detach_root_widget();

        assert!(
            disposed.get(),
            "detach_root_widget must run State::dispose for every descendant, \
             not just unmount the (internal RootRenderView-wrapped) root node"
        );
    }

    #[test]
    fn binding_lifecycle_matrix() {
        crate::table_test::run_table(
            "binding_lifecycle_matrix",
            &[
                (
                    "binding_is_not_a_singleton_two_instances_are_independent",
                    binding_is_not_a_singleton_two_instances_are_independent as fn(),
                ),
                (
                    "global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking",
                    global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking
                        as fn(),
                ),
                (
                    "test_attach_root_widget_bootstraps_render_tree",
                    test_attach_root_widget_bootstraps_render_tree as fn(),
                ),
                (
                    "detach_root_widget_disposes_every_descendant_not_just_the_root",
                    detach_root_widget_disposes_every_descendant_not_just_the_root as fn(),
                ),
            ],
        );
    }

    // ========================================================================
    // collect_all_elements — iterative O(N) walk
    // ========================================================================
    //
    // These tests pin the contract of
    // `WidgetsBinding::collect_all_elements`:
    //
    // 1. It returns every `(ElementId, depth)` pair reachable from the
    //    starting `root_id`, with depths offset by the supplied
    //    `root_depth` (i.e. the root's recorded depth equals
    //    `root_depth`, each child recorded depth equals `root_depth + 1`,
    //    and so on).
    // 2. The traversal is pre-order DFS in each element's `child_ids`
    //    slot order — parent first, then each child's entire subtree
    //    before moving on to the next sibling.
    // 3. Order is deterministic across runs.
    // 4. Deep linear chains do not exhaust the stack (the walk is
    //    iterative).

    // ========================================================================
    // did_change_app_lifecycle_state dispatch
    // ========================================================================
}
