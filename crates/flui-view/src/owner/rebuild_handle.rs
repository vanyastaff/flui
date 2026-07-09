//! [`RebuildHandle`] — the `'static` "rebuild this element later" token
//! (ADR-0018, unit U1).
//!
//! # What this is
//!
//! An owned, cloneable, thread-safe capability to schedule **one specific
//! element** for rebuild on the next frame. It is what a `ViewState` captures in
//! `init_state` so that a callback firing later — an async completion, a stream
//! event, a worker thread — can ask for a rebuild without holding any borrow of
//! the tree, the owner, or the context.
//!
//! # It invents nothing
//!
//! `RebuildHandle` is a thin newtype over the channel `AnimatedView` has always
//! used: [`ExternalBuildScheduler`] + an [`ElementId`]. Calling
//! [`schedule`](RebuildHandle::schedule) inserts the id into
//! `BuildOwner::external_inbox` (a `HashSet`, so a burst of calls between frames
//! collapses to one entry) and, only if the id was newly queued, asks the
//! binding for a frame. `BuildOwner::build_scope` drains that inbox at frame
//! start, marks each drained element dirty, and rebuilds it — on the frame
//! thread, in the build phase.
//!
//! So `schedule()` **never touches the element tree, the render tree, or the
//! pipeline**. It writes to a mutex-guarded set and calls one `Fn()`. Everything
//! that mutates a tree happens later, synchronously, inside `build_scope`.
//!
//! # Where it may be acquired
//!
//! From `init_state` / `did_change_dependencies` — i.e. lifecycle hooks, not
//! `build`. Acquiring a handle inside `build` (or any layout/paint path) and
//! scheduling from it is how you write an unbounded rebuild loop.
//! [`FOUNDATIONS.md`](../../../../docs/FOUNDATIONS.md) permits an
//! out-of-catalog `mark_needs_build` driver only when "gated by a refusal
//! trigger barring signal subscriptions from `build`/`layout`/`paint`" — that
//! gate is `scripts/port-check.sh` trigger **#22**.
//!
//! # Stale handles are inert
//!
//! A handle can outlive its element (the subtree was reconciled away while an
//! async task was still in flight). Scheduling then queues an id whose node is
//! gone; the `build_scope` drain looks the node up (`tree.get(id)`), finds
//! nothing, and the processing loop skips it (`let Some(node) = tree.get_mut(id)
//! else { continue }`). No panic, no resurrection. A handle taken before mount
//! (no scheduler, no id yet) is inert by construction — see
//! [`RebuildHandle::is_active`].

use flui_foundation::ElementId;

use super::build_owner::ExternalBuildScheduler;

/// The live half of a [`RebuildHandle`].
#[derive(Clone)]
struct Active {
    /// Shared inbox + frame-request hook. Cloneable and `Send + Sync`.
    scheduler: ExternalBuildScheduler,
    /// The element to rebuild.
    element: ElementId,
}

/// A `Clone + Send + Sync + 'static` capability to schedule one element's
/// rebuild.
///
/// Obtain one from [`BuildContext::rebuild_handle`](crate::context::BuildContext::rebuild_handle)
/// inside `init_state`, store it in your `ViewState`, and call
/// [`schedule`](Self::schedule) from a completion callback on any thread.
///
/// The rebuild itself runs on the frame thread during the next frame's build
/// phase. Nothing is mutated at `schedule()` time beyond the shared inbox.
///
/// # Example
///
/// ```rust,ignore
/// fn init_state(&mut self, ctx: &dyn BuildContext) {
///     let handle = ctx.rebuild_handle();
///     std::thread::spawn(move || {
///         let value = expensive();
///         *shared.lock() = Some(value);
///         handle.schedule(); // rebuilds on the next frame, on the frame thread
///     });
/// }
/// ```
#[derive(Clone)]
pub struct RebuildHandle {
    /// `None` when the handle was minted before the element was mounted (no
    /// scheduler / no stamped id). Such a handle is permanently inert.
    inner: Option<Active>,
}

impl RebuildHandle {
    /// A handle bound to `element`, scheduling through `scheduler`.
    pub(crate) fn new(scheduler: ExternalBuildScheduler, element: ElementId) -> Self {
        Self {
            inner: Some(Active { scheduler, element }),
        }
    }

    /// A handle that does nothing.
    ///
    /// Minted when no scheduler or element id is available yet — an
    /// `ElementCore` before `ElementTree::insert` stamps it. Calling
    /// [`schedule`](Self::schedule) on one is a no-op, not a panic: the
    /// pre-mount window is a legitimate state, not a bug.
    pub(crate) fn inert() -> Self {
        Self { inner: None }
    }

    /// Schedule the owning element for rebuild on the next frame, and request a
    /// frame if one is not already pending.
    ///
    /// Callable from **any thread**. Idempotent between frames: repeated calls
    /// collapse to a single queued rebuild and a single frame request, because
    /// the inbox is a set.
    ///
    /// Inert when the handle is inert, and harmless when the element has since
    /// been unmounted — `build_scope` skips ids whose node is gone.
    pub fn schedule(&self) {
        if let Some(active) = &self.inner {
            active.scheduler.schedule(active.element);
        }
    }

    /// The element this handle rebuilds, or `None` if inert.
    #[must_use]
    pub fn element_id(&self) -> Option<ElementId> {
        self.inner.as_ref().map(|active| active.element)
    }

    /// Whether [`schedule`](Self::schedule) can do anything at all.
    ///
    /// `false` only for a handle minted before mount. It does **not** report
    /// whether the element is still in the tree — a handle cannot know that, and
    /// scheduling a dead element is already a safe no-op.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.is_some()
    }
}

impl std::fmt::Debug for RebuildHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            Some(active) => f
                .debug_struct("RebuildHandle")
                .field("element", &active.element)
                .finish_non_exhaustive(),
            None => f
                .debug_struct("RebuildHandle")
                .field("inert", &true)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_objects::RenderSizedBox;
    use flui_rendering::protocol::BoxProtocol;
    use flui_types::geometry::px;

    use crate::{
        BuildOwner, RebuildHandle,
        context::BuildContext,
        tree::ElementTree,
        view::{IntoView, RenderView, StatefulView, View, ViewState},
    };

    /// A stateful view whose state captures a `RebuildHandle` in `init_state` —
    /// the exact shape ADR-0018's `FutureBuilder` will use.
    #[derive(Clone, Debug)]
    struct Capturing {
        /// Where the state parks the handle it captured.
        captured: Arc<parking_lot::Mutex<Option<RebuildHandle>>>,
        /// Counts `build` calls, to observe that a schedule caused a rebuild.
        builds: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CapturingState {
        captured: Arc<parking_lot::Mutex<Option<RebuildHandle>>>,
        builds: Arc<AtomicUsize>,
    }

    impl StatefulView for Capturing {
        type State = CapturingState;

        fn create_state(&self) -> Self::State {
            CapturingState {
                captured: Arc::clone(&self.captured),
                builds: Arc::clone(&self.builds),
            }
        }
    }

    impl ViewState<Capturing> for CapturingState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            // The capability outlives the borrow of `ctx`.
            *self.captured.lock() = Some(ctx.rebuild_handle());
        }

        fn build(&self, _view: &Capturing, _ctx: &dyn BuildContext) -> impl IntoView {
            self.builds.fetch_add(1, Ordering::Relaxed);
            Leaf
        }
    }

    impl View for Capturing {
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

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(1.0)), Some(px(1.0)))
        }

        fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
    }

    impl View for Leaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// Mount `Capturing` as root; return the owner, tree, and the captured handle.
    fn mount() -> (
        BuildOwner,
        ElementTree,
        RebuildHandle,
        Arc<AtomicUsize>,
        flui_foundation::ElementId,
    ) {
        let captured = Arc::new(parking_lot::Mutex::new(None));
        let builds = Arc::new(AtomicUsize::new(0));
        let view = Capturing {
            captured: Arc::clone(&captured),
            builds: Arc::clone(&builds),
        };

        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());

        // `init_state` runs during the first build.
        owner.schedule_build_for(root, 0);
        owner.build_scope(&mut tree);

        let handle = captured
            .lock()
            .clone()
            .expect("init_state must have captured a handle");
        (owner, tree, handle, builds, root)
    }

    // ── 1. captured in init_state, usable afterwards ────────────────────────

    #[test]
    fn rebuild_handle_captured_in_init_state_schedules_after_init_returns() {
        let (mut owner, mut tree, handle, builds, root) = mount();
        assert!(handle.is_active());
        assert_eq!(handle.element_id(), Some(root));

        let builds_after_mount = builds.load(Ordering::Relaxed);
        assert_eq!(owner.pending_external_builds(), 0);

        handle.schedule();
        assert_eq!(owner.pending_external_builds(), 1, "queued, not yet built");
        assert_eq!(
            builds.load(Ordering::Relaxed),
            builds_after_mount,
            "schedule() must not build inline"
        );

        owner.build_scope(&mut tree);

        assert_eq!(owner.pending_external_builds(), 0, "inbox drained");
        assert_eq!(
            builds.load(Ordering::Relaxed),
            builds_after_mount + 1,
            "the next frame's build_scope must rebuild the element"
        );
    }

    // ── 2. coalescing ───────────────────────────────────────────────────────

    /// The inbox is a set: a burst between frames costs one queued rebuild and
    /// one frame request.
    #[test]
    fn rebuild_handle_repeated_schedules_coalesce_to_one_rebuild() {
        let (mut owner, mut tree, handle, builds, _root) = mount();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        owner.set_on_build_scheduled(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });
        // Re-capture through the owner so the handle carries the new hook.
        let handle = owner.rebuild_handle(handle.element_id().expect("active"));

        let before = builds.load(Ordering::Relaxed);
        for _ in 0..5 {
            handle.schedule();
        }

        assert_eq!(owner.pending_external_builds(), 1, "one inbox slot");
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "only the newly-queued schedule requests a frame"
        );

        owner.build_scope(&mut tree);
        assert_eq!(builds.load(Ordering::Relaxed), before + 1, "one rebuild");
    }

    // ── 3. cross-thread ─────────────────────────────────────────────────────

    /// `schedule()` is callable from another thread and rebuilds on the frame
    /// thread. Nothing is built off-thread: the build counter cannot move until
    /// `build_scope` runs here.
    #[test]
    fn rebuild_handle_schedules_from_another_thread_and_builds_on_the_frame_thread() {
        let (mut owner, mut tree, handle, builds, _root) = mount();
        let before = builds.load(Ordering::Relaxed);

        let worker = std::thread::spawn(move || {
            handle.schedule();
            std::thread::current().id()
        });
        let worker_thread = worker.join().expect("worker must not panic");
        assert_ne!(worker_thread, std::thread::current().id());

        assert_eq!(owner.pending_external_builds(), 1);
        assert_eq!(
            builds.load(Ordering::Relaxed),
            before,
            "no build may happen off the frame thread"
        );

        owner.build_scope(&mut tree);
        assert_eq!(builds.load(Ordering::Relaxed), before + 1);
    }

    // ── 4. stale handle ─────────────────────────────────────────────────────

    /// A handle outliving its element is inert: scheduling queues an id whose
    /// node is gone, and the drain skips it. No panic, no resurrection.
    #[test]
    fn rebuild_handle_outliving_its_element_is_inert() {
        let (mut owner, mut tree, handle, builds, root) = mount();
        let before = builds.load(Ordering::Relaxed);

        tree.remove(root, &mut owner.element_owner_mut());
        assert!(tree.get(root).is_none(), "element is gone");

        handle.schedule();
        owner.build_scope(&mut tree);

        assert_eq!(
            builds.load(Ordering::Relaxed),
            before,
            "a dead element must not rebuild"
        );
        assert_eq!(owner.pending_external_builds(), 0, "inbox still drained");
    }

    /// A handle minted before mount schedules nothing at all.
    #[test]
    fn rebuild_handle_inert_before_mount() {
        let handle = RebuildHandle::inert();
        assert!(!handle.is_active());
        assert_eq!(handle.element_id(), None);
        handle.schedule(); // must not panic
    }

    // ── 5. frame request ────────────────────────────────────────────────────

    /// Scheduling asks the binding for a frame through the existing
    /// `on_build_scheduled` hook — the same path `schedule_build_for` uses.
    #[test]
    fn rebuild_handle_requests_a_frame_through_the_existing_hook() {
        let (mut owner, _tree, handle, _builds, root) = mount();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        owner.set_on_build_scheduled(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        // The handle must be minted after the hook is installed — it captures
        // the frame-request `Arc` by value, as `ExternalBuildScheduler` does.
        let handle_with_hook = owner.rebuild_handle(root);
        assert_eq!(frames.load(Ordering::Relaxed), 0);

        handle_with_hook.schedule();
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a newly-queued element must request a frame"
        );

        // The pre-hook handle still schedules (same inbox), but the id is
        // already queued, so no second frame request.
        handle.schedule();
        assert_eq!(frames.load(Ordering::Relaxed), 1);
    }

    /// Debug never deadlocks and never leaks the lock.
    #[test]
    fn rebuild_handle_debug_is_safe() {
        let (owner, _tree, handle, _builds, root) = mount();
        let _ = format!("{handle:?}");
        let _ = format!("{:?}", RebuildHandle::inert());
        let _ = owner.rebuild_handle(root);
    }

    /// Send + Sync + 'static, by construction.
    #[test]
    fn rebuild_handle_is_send_sync_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<RebuildHandle>();
    }
}
