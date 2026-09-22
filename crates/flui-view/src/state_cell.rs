//! [`StateCell`] and [`StateHandle`] — ergonomic, `Rc`-backed local state
//! bound to one element's [`RebuildHandle`].
//!
//! # What this is
//!
//! Every hand-written stateful view in this codebase (the CLI's generated
//! `counter` template, `examples/multi_window_demo.rs`,
//! `examples/vertical_slice_demo/tree.rs`) repeats the same four-line shape:
//!
//! ```rust,ignore
//! struct CounterState {
//!     count: Rc<Cell<usize>>,
//!     rebuild: Option<RebuildHandle>,
//! }
//!
//! impl ViewState<CounterView> for CounterState {
//!     fn init_state(&mut self, ctx: &dyn BuildContext) {
//!         self.rebuild = Some(ctx.rebuild_handle());
//!     }
//!
//!     fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
//!         let count = Rc::clone(&self.count);
//!         let rebuild = self.rebuild.clone().expect("BUG: init_state runs first");
//!         // ... count.set(count.get() + 1); rebuild.schedule(RebuildReason::StateChange);
//!     }
//! }
//! ```
//!
//! `StateCell<T>` (for `Copy` types) and `StateHandle<T>` (for everything
//! else) fold the `Rc<Cell<_>>`/`Rc<RefCell<_>>` storage and the
//! `Option<RebuildHandle>` bookkeeping into one cloneable value: `set`/
//! `update` mutate the storage AND schedule a rebuild in a single call, so a
//! callback closure captures one clone instead of two.
//!
//! # It invents nothing
//!
//! Both types are thin: a shared cell for the value plus a shared,
//! optionally-populated [`RebuildHandle`] slot. Everything about *when* a
//! scheduled rebuild actually runs, how bursts of `schedule` calls coalesce,
//! and why a call from a build already in progress joins that same drain
//! instead of waiting a frame is [`RebuildHandle::schedule`]'s contract,
//! unchanged — see `owner/rebuild_handle.rs` (its module doc and the tests
//! around lines 296-410 exercise that contract directly). These types only
//! decide *whether* to call it.
//!
//! # Binding
//!
//! A freshly constructed cell is **unbound**: it holds a value like a plain
//! `Cell`/`RefCell`, and mutating it schedules nothing. Call
//! [`StateCell::bind`] / [`StateHandle::bind`] exactly once, from
//! `ViewState::init_state`, to store the element's [`RebuildHandle`] —
//! obtained from [`BuildContext::rebuild_handle`], which documents that it
//! must **not** be called during build, layout, or paint. `bind` inherits
//! that same restriction: call it only from `init_state` (or
//! `did_change_dependencies`), never from `build`.
//!
//! # Mutating before `bind` and after unmount
//!
//! - **Unbound mutation** (before `bind` runs) changes the value and
//!   schedules nothing. This is not a bug to guard against — a cell can be
//!   constructed in `StatefulView::create_state` (or even earlier, as a
//!   `pub` field seeded by a caller before mounting) and read or written
//!   before the element exists to rebuild. Mirrors
//!   `RebuildHandle::inert`'s "no scheduler yet" window (a crate-private
//!   constructor, hence not linked).
//! - **Mutation after the element is gone** is a silent no-op for the same
//!   reason a stale [`RebuildHandle`] is: `schedule` writes to the owner's
//!   inbox, and `BuildOwner::build_scope`'s drain looks the element id up and
//!   skips it if the node is gone. Nothing here re-implements that check;
//!   it falls out of delegating straight to `RebuildHandle::schedule`.
//!
//! # Cloning and thread affinity
//!
//! `Clone` shares storage (both fields are `Rc`-backed): every clone reads
//! and writes the same value and the same rebuild slot. Binding through any
//! one clone binds all of them. Both types are `!Send`/`!Sync` by
//! construction (`Rc` is neither); the `compile_fail` doctests on each type
//! pin that down at a concrete type, so a future field change that made
//! either type shareable across threads would fail `cargo test --doc`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use flui_foundation::RebuildReason;

use crate::context::BuildContext;
use crate::owner::RebuildHandle;

/// `Rc`-backed, `Copy`-typed local state bound to one element's rebuild
/// trigger.
///
/// Neither `Send` nor `Sync` — `Rc`-backed local state belongs to the
/// element that owns it:
///
/// ```compile_fail,E0277
/// fn require_send<T: Send>(_: T) {}
/// require_send(flui_view::StateCell::new(0_u32));
/// ```
///
/// ```compile_fail,E0277
/// fn require_sync<T: Sync>(_: T) {}
/// require_sync(flui_view::StateCell::new(0_u32));
/// ```
///
/// See the [module docs](self) for the full contract: unbound mutation is
/// silent, mutation after unmount is a no-op, and clones share storage.
///
/// # Example
///
/// ```rust
/// use flui_view::StateCell;
///
/// let count = StateCell::new(0);
/// assert_eq!(count.get(), 0);
///
/// // Unbound: nothing to schedule yet, but the value still changes.
/// count.set(1);
/// assert_eq!(count.get(), 1);
///
/// count.update(|n| n + 1);
/// assert_eq!(count.get(), 2);
///
/// // Clones share the same storage.
/// let alias = count.clone();
/// alias.set(9);
/// assert_eq!(count.get(), 9);
/// ```
#[derive(Clone)]
pub struct StateCell<T: Copy> {
    value: Rc<Cell<T>>,
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
}

impl<T: Copy> StateCell<T> {
    /// A fresh, unbound cell holding `initial`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateCell;
    ///
    /// let flag = StateCell::new(false);
    /// assert!(!flag.get());
    /// ```
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self {
            value: Rc::new(Cell::new(initial)),
            rebuild: Rc::new(RefCell::new(None)),
        }
    }

    /// Bind this cell to `ctx`'s element: from now on, [`set`](Self::set) and
    /// [`update`](Self::update) schedule a [`RebuildReason::StateChange`]
    /// rebuild.
    ///
    /// Call this exactly once, from `ViewState::init_state` (or
    /// `did_change_dependencies`) — never from `build`, `perform_layout`, or
    /// `paint`, the same restriction
    /// [`BuildContext::rebuild_handle`]
    /// documents. Every clone of this cell observes the binding (the slot is
    /// shared), so binding one clone binds them all.
    ///
    /// ```rust,ignore
    /// impl ViewState<Counter> for CounterState {
    ///     fn init_state(&mut self, ctx: &dyn BuildContext) {
    ///         self.count.bind(ctx);
    ///     }
    ///
    ///     fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
    ///         let count = self.count.clone();
    ///         ElevatedButton::new(Text::new(self.count.get().to_string()))
    ///             .on_pressed(move || count.update(|n| n + 1))
    ///     }
    /// }
    /// ```
    pub fn bind(&self, ctx: &dyn BuildContext) {
        // Mint the handle before taking the borrow, and let a previously
        // stored handle drop only after the borrow has fallen: a
        // `RebuildHandle` carries `Arc`s whose destructors must never run
        // under this slot's `RefMut` (LockDiscipline/StatementDrop).
        let handle = ctx.rebuild_handle();
        let previous = self.rebuild.borrow_mut().replace(handle);
        drop(previous);
    }

    /// The current value.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateCell;
    ///
    /// let count = StateCell::new(7);
    /// assert_eq!(count.get(), 7);
    /// ```
    #[must_use]
    pub fn get(&self) -> T {
        self.value.get()
    }

    /// Store `value` and, if bound, schedule a rebuild.
    ///
    /// Unconditional: setting to the same value still schedules, exactly
    /// like calling [`RebuildHandle::schedule`] directly would.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateCell;
    ///
    /// let count = StateCell::new(0);
    /// count.set(5);
    /// assert_eq!(count.get(), 5);
    /// ```
    pub fn set(&self, value: T) {
        self.value.set(value);
        self.schedule();
    }

    /// Replace the value with `f(current)` and, if bound, schedule a
    /// rebuild.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateCell;
    ///
    /// let count = StateCell::new(1);
    /// count.update(|n| n * 10);
    /// assert_eq!(count.get(), 10);
    /// ```
    pub fn update(&self, f: impl FnOnce(T) -> T) {
        let next = f(self.value.get());
        self.set(next);
    }

    /// Schedule a [`RebuildReason::StateChange`] rebuild if bound; a no-op
    /// otherwise (unbound, or the element has since been unmounted — see the
    /// [module docs](self)).
    fn schedule(&self) {
        if let Some(handle) = self.rebuild.borrow().as_ref() {
            handle.schedule(RebuildReason::StateChange);
        }
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for StateCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateCell")
            .field("value", &self.value.get())
            .field("bound", &self.rebuild.borrow().is_some())
            .finish()
    }
}

/// `RefCell`-backed local state bound to one element's rebuild trigger, for
/// `T` that is not `Copy`.
///
/// See the [module docs](self) for the full contract shared with
/// [`StateCell`]: unbound mutation is silent, mutation after unmount is a
/// no-op, and clones share storage.
///
/// # Example
///
/// ```rust
/// use flui_view::StateHandle;
///
/// let name = StateHandle::new(String::from("alice"));
/// assert_eq!(name.with(String::len), 5);
///
/// name.update(|n| n.push_str("-updated"));
/// assert_eq!(name.with(Clone::clone), "alice-updated");
/// ```
///
/// Neither `Send` nor `Sync`, for the same reason as [`StateCell`]:
///
/// ```compile_fail,E0277
/// fn require_send<T: Send>(_: T) {}
/// require_send(flui_view::StateHandle::new(String::new()));
/// ```
///
/// ```compile_fail,E0277
/// fn require_sync<T: Sync>(_: T) {}
/// require_sync(flui_view::StateHandle::new(String::new()));
/// ```
#[derive(Clone)]
pub struct StateHandle<T> {
    value: Rc<RefCell<T>>,
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
}

impl<T> StateHandle<T> {
    /// A fresh, unbound handle holding `initial`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateHandle;
    ///
    /// let items: StateHandle<Vec<i32>> = StateHandle::new(Vec::new());
    /// assert_eq!(items.with(Vec::len), 0);
    /// ```
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self {
            value: Rc::new(RefCell::new(initial)),
            rebuild: Rc::new(RefCell::new(None)),
        }
    }

    /// Bind this handle to `ctx`'s element: from now on,
    /// [`update`](Self::update) schedules a [`RebuildReason::StateChange`]
    /// rebuild.
    ///
    /// Same rule as [`StateCell::bind`]: call it exactly once, from
    /// `ViewState::init_state` (or `did_change_dependencies`), never from
    /// `build`/`perform_layout`/`paint`.
    ///
    /// ```rust,ignore
    /// impl ViewState<Notes> for NotesState {
    ///     fn init_state(&mut self, ctx: &dyn BuildContext) {
    ///         self.text.bind(ctx);
    ///     }
    ///
    ///     fn build(&self, _view: &Notes, _ctx: &dyn BuildContext) -> impl IntoView {
    ///         let text = self.text.clone();
    ///         TextField::new().value(self.text.with(Clone::clone)).on_changed(move |next| {
    ///             text.update(|t| *t = next);
    ///         })
    ///     }
    /// }
    /// ```
    pub fn bind(&self, ctx: &dyn BuildContext) {
        // Mint the handle before taking the borrow, and let a previously
        // stored handle drop only after the borrow has fallen: a
        // `RebuildHandle` carries `Arc`s whose destructors must never run
        // under this slot's `RefMut` (LockDiscipline/StatementDrop).
        let handle = ctx.rebuild_handle();
        let previous = self.rebuild.borrow_mut().replace(handle);
        drop(previous);
    }

    /// Borrow the current value for the duration of `f` and return its
    /// result.
    ///
    /// # Panics
    ///
    /// `f` runs while the value is borrowed, so it must not reach back into
    /// this handle (or a clone of it) with [`Self::update`]: that is a
    /// re-entrant mutable borrow of the same `RefCell`, and it panics, as
    /// [`Self::update`] documents. Nested [`Self::with`] calls are fine —
    /// shared borrows stack.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateHandle;
    ///
    /// let name = StateHandle::new(String::from("bob"));
    /// let len = name.with(String::len);
    /// assert_eq!(len, 3);
    /// ```
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.value.borrow())
    }

    /// Mutate the value in place through `f` and, if bound, schedule a
    /// rebuild.
    ///
    /// Schedules unconditionally when bound: `f` mutates in place, so there
    /// is no cheap way to tell whether it actually changed anything, and
    /// [`StateCell::set`] makes the same unconditional choice.
    ///
    /// # Panics
    ///
    /// `f` runs under the value's mutable borrow, so it must not touch this
    /// handle (or a clone of it) at all — neither [`Self::with`] nor another
    /// `update` — for the duration; doing so is a re-entrant `RefCell`
    /// borrow and panics with `already borrowed`. Read what you need into
    /// locals before the call, or compute the new value outside and assign
    /// it inside `f`. A `Copy` value in a [`StateCell`] has no such hazard:
    /// its [`StateCell::update`] takes and returns the value.
    ///
    /// ```should_panic
    /// use flui_view::StateHandle;
    ///
    /// let items = StateHandle::new(vec![1]);
    /// let peek = items.clone();
    /// // Re-entrant: `with` on the same storage while `update` holds it.
    /// items.update(|v| v.push(peek.with(Vec::len)));
    /// ```
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_view::StateHandle;
    ///
    /// let items = StateHandle::new(vec![1, 2]);
    /// items.update(|v| v.push(3));
    /// assert_eq!(items.with(|v| v.clone()), vec![1, 2, 3]);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.value.borrow_mut());
        self.schedule();
    }

    /// Schedule a [`RebuildReason::StateChange`] rebuild if bound; a no-op
    /// otherwise (unbound, or the element has since been unmounted — see the
    /// [module docs](self)).
    fn schedule(&self) {
        if let Some(handle) = self.rebuild.borrow().as_ref() {
            handle.schedule(RebuildReason::StateChange);
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for StateHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateHandle")
            .field("value", &*self.value.borrow())
            .field("bound", &self.rebuild.borrow().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_objects::RenderSizedBox;
    use flui_rendering::{
        pipeline::{PipelineCell, PipelineOwner},
        protocol::BoxProtocol,
    };
    use flui_types::geometry::px;

    use super::{StateCell, StateHandle};
    use crate::{
        BuildOwner, RebuildReason,
        context::BuildContext,
        tree::ElementTree,
        view::{IntoView, RenderView, RootRenderView, StatefulView, View, ViewState},
    };

    // ── fixture: mirrors `owner/rebuild_handle.rs`'s `mount()` harness ──────
    //
    // A stateful view whose state binds a `StateCell<i32>` and a
    // `StateHandle<String>` in `init_state`, then reads both in `build` so a
    // build counter proves whether a schedule actually rebuilt the element.

    #[derive(Clone, Debug)]
    struct Bound {
        count: StateCell<i32>,
        text: StateHandle<String>,
        builds: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct BoundState {
        count: StateCell<i32>,
        text: StateHandle<String>,
        builds: Arc<AtomicUsize>,
    }

    impl StatefulView for Bound {
        type State = BoundState;

        fn create_state(&self) -> Self::State {
            BoundState {
                count: self.count.clone(),
                text: self.text.clone(),
                builds: Arc::clone(&self.builds),
            }
        }
    }

    impl ViewState<Bound> for BoundState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            self.count.bind(ctx);
            self.text.bind(ctx);
        }

        fn build(&self, _view: &Bound, _ctx: &dyn BuildContext) -> impl IntoView {
            self.builds.fetch_add(1, Ordering::Relaxed);
            Leaf
        }
    }

    impl View for Bound {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }
    }

    /// A render leaf so the stateful view has something to build.
    #[derive(Clone, Debug)]
    struct Leaf;

    impl RenderView for Leaf {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(1.0)), Some(px(1.0)))
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for Leaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// Mount `Bound` under a render root and return the owner, tree, the
    /// bound `StateCell`/`StateHandle`, the build counter, and the
    /// *stateful* element's id.
    fn mount() -> (
        BuildOwner,
        ElementTree,
        StateCell<i32>,
        StateHandle<String>,
        Arc<AtomicUsize>,
        flui_foundation::ElementId,
    ) {
        let count = StateCell::new(0);
        let text = StateHandle::new(String::from("initial"));
        let builds = Arc::new(AtomicUsize::new(0));
        let view = Bound {
            count: count.clone(),
            text: text.clone(),
            builds: Arc::clone(&builds),
        };

        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        // Same production shape `rebuild_handle.rs`'s fixture uses: a render
        // root carrying the stateful view, so `Leaf` mounts with a render
        // parent instead of being orphaned.
        let render_root = RootRenderView::new(view, 800.0, 600.0);
        let render_root_element = tree.mount_root_with_pipeline_owner(
            &render_root,
            Some(PipelineCell::new(PipelineOwner::new())),
            &mut owner.element_owner_mut(),
        );

        owner.schedule_build_for(render_root_element, 0, RebuildReason::InitialMount);
        owner.build_scope(&mut tree);

        let root = tree
            .get(render_root_element)
            .map(|node| node.child_ids()[0])
            .expect("the stateful element must sit under the render root after the mount build");
        (owner, tree, count, text, builds, root)
    }

    // ── 1. set/update after bind schedules StateChange ─────────────────────

    #[test]
    fn set_after_bind_schedules_a_state_change_rebuild_for_the_element() {
        let (mut owner, mut tree, count, _text, builds, _root) = mount();
        let builds_after_mount = builds.load(Ordering::Relaxed);
        assert_eq!(owner.pending_external_builds(), 0);

        count.set(42);
        assert_eq!(count.get(), 42);
        assert_eq!(owner.pending_external_builds(), 1, "queued, not yet built");
        assert_eq!(
            builds.load(Ordering::Relaxed),
            builds_after_mount,
            "set() must not build inline"
        );

        owner.build_scope(&mut tree);
        assert_eq!(owner.pending_external_builds(), 0, "inbox drained");
        assert_eq!(
            builds.load(Ordering::Relaxed),
            builds_after_mount + 1,
            "the next frame's build_scope must rebuild the element"
        );
    }

    #[test]
    fn update_after_bind_schedules_a_state_change_rebuild_for_the_element() {
        let (mut owner, mut tree, count, _text, builds, _root) = mount();
        let builds_after_mount = builds.load(Ordering::Relaxed);

        count.update(|n| n + 1);
        assert_eq!(count.get(), 1);
        assert_eq!(owner.pending_external_builds(), 1);

        owner.build_scope(&mut tree);
        assert_eq!(builds.load(Ordering::Relaxed), builds_after_mount + 1);
    }

    #[test]
    fn state_handle_update_after_bind_schedules_a_rebuild() {
        let (mut owner, mut tree, _count, text, builds, _root) = mount();
        let builds_after_mount = builds.load(Ordering::Relaxed);

        text.update(|s| s.push_str("-changed"));
        assert_eq!(text.with(Clone::clone), "initial-changed");
        assert_eq!(owner.pending_external_builds(), 1);

        owner.build_scope(&mut tree);
        assert_eq!(builds.load(Ordering::Relaxed), builds_after_mount + 1);
    }

    // ── 2. unbound mutation changes the value, schedules nothing ───────────

    #[test]
    fn unbound_state_cell_mutation_changes_value_and_schedules_nothing() {
        let cell = StateCell::new(0);
        cell.set(5);
        assert_eq!(cell.get(), 5);
        cell.update(|n| n + 1);
        assert_eq!(cell.get(), 6); // changed, and nothing to schedule against
    }

    #[test]
    fn unbound_state_handle_mutation_changes_value_and_schedules_nothing() {
        let handle = StateHandle::new(String::from("a"));
        handle.update(|s| s.push('b'));
        assert_eq!(handle.with(Clone::clone), "ab");
    }

    // ── 3. mutation after element removal is a silent no-op ────────────────

    #[test]
    fn mutation_after_the_element_is_removed_is_a_silent_no_op() {
        let (mut owner, mut tree, count, text, builds, root) = mount();
        let before = builds.load(Ordering::Relaxed);

        tree.remove(root, &mut owner.element_owner_mut());
        assert!(tree.get(root).is_none(), "element is gone");

        count.set(7);
        text.update(|s| s.push_str("-late"));
        owner.build_scope(&mut tree);

        assert_eq!(
            builds.load(Ordering::Relaxed),
            before,
            "a dead element must not rebuild"
        );
        assert_eq!(owner.pending_external_builds(), 0, "inbox still drained");
        // The value itself still mutated — only scheduling is inert.
        assert_eq!(count.get(), 7);
        assert_eq!(text.with(Clone::clone), "initial-late");
    }

    // ── 4. clones share storage ──────────────────────────────────────────

    #[test]
    fn state_cell_clones_share_storage() {
        let cell = StateCell::new(1);
        let alias = cell.clone();
        alias.set(2);
        assert_eq!(cell.get(), 2);
    }

    #[test]
    fn state_handle_clones_share_storage() {
        let handle = StateHandle::new(vec![1]);
        let alias = handle.clone();
        alias.update(|v| v.push(2));
        assert_eq!(handle.with(Clone::clone), vec![1, 2]);
    }

    // ── 5. StateHandle::update on a non-Copy type (String) ──────────────────

    #[test]
    fn state_handle_update_mutates_a_string_in_place() {
        let name = StateHandle::new(String::from("alice"));
        name.update(|n| *n = format!("{n}-bob"));
        assert_eq!(name.with(Clone::clone), "alice-bob");
    }

    // ── 6. Debug never panics or deadlocks ──────────────────────────────────

    #[test]
    fn debug_impls_are_safe() {
        let cell = StateCell::new(3);
        let _ = format!("{cell:?}");
        let handle = StateHandle::new(String::from("x"));
        let _ = format!("{handle:?}");
    }

    // 7. `!Send`/`!Sync` is pinned by the `compile_fail` doctests on each
    // type (see the struct docs): a negative trait bound cannot be asserted
    // from inside a generic fn — the overlap trick only fires at a concrete
    // type, which a doctest with a concrete call site gives.
}
