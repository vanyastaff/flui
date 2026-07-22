//! A headless widget harness for `flui-widgets`' **in-crate** unit tests.
//!
//! `tests/common::lay_out` is an integration-test module and cannot be reached
//! from `src/`, so private modules — `overlay` and `navigator`
//! — need their own. This is the trimmed equivalent: it keeps `lay_out`'s
//! load-bearing ordering (**binding first, so the async driver is installed before
//! the mount `build_scope`**) and drops the geometry helpers.

use std::sync::Arc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::ElementId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::{Bounds, Pixels, px};
use flui_view::View;
use parking_lot::RwLock;

/// A mounted, laid-out widget tree.
pub(crate) struct Harness {
    binding: HeadlessBinding,
    root_element: ElementId,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    /// Every `TextInputHandle::set_cursor_area` call recorded by the
    /// installed IME capability, in delivery order. `None` when the harness
    /// was mounted with [`TextInputCapability::Absent`] — there is nothing
    /// to record.
    cursor_area_calls: Option<Arc<parking_lot::Mutex<Vec<Bounds<Pixels>>>>>,
    /// Held for the harness's whole lifetime as conservative focus-fixture
    /// serialization across test owners. Each owner thread resolves independent
    /// TLS focus state; the guard prevents overlapping fixtures rather than
    /// cross-owner state clobbering. Reentrant, so explicit locking composes.
    _focus_guard: parking_lot::ReentrantMutexGuard<'static, ()>,
}

/// Conservatively serializes mounted focus fixtures across test owners.
///
/// **Reentrant**: [`mount`] takes it for the returned [`Harness`]'s lifetime — so a
/// test never has to remember to — and a focus test that *also* locks it explicitly
/// (for pre-mount manager setup) nests on the same thread without deadlock. nextest
/// isolates test *binaries*, not threads inside one. Each owner thread has
/// independent TLS focus state; the guard is fixture isolation rather than
/// protection against cross-owner state clobbering.
pub(crate) static FOCUS_TEST_LOCK: parking_lot::ReentrantMutex<()> =
    parking_lot::ReentrantMutex::new(());

/// Mount `root` as the render-tree root and drive one frame.
pub(crate) fn mount(root: impl View) -> Harness {
    mount_with_capabilities(
        root,
        PostFrameCapability::Installed,
        TextInputCapability::Absent,
    )
}

/// [`mount`], but with a working `BuildContext::text_input_handle()` — the
/// only capability [`mount`] withholds by default. The installed handle
/// wraps `flui_interaction::TextInputRegistry::global()` directly (no
/// `flui-app`/`PlatformWindow` involved, so `set_ime_allowed` toggling is
/// out of reach here — that half is covered at the `flui-app` layer); it
/// exists so `EditableText`'s own attach/detach/dispatch wiring is testable
/// from this crate without standing up a full binding.
pub(crate) fn mount_with_ime(root: impl View) -> Harness {
    mount_with_capabilities(
        root,
        PostFrameCapability::Installed,
        TextInputCapability::Installed,
    )
}

/// Whether the binding hands `BuildContext` a [`TextInputHandle`] at all.
///
/// [`TextInputHandle`]: flui_interaction::TextInputHandle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextInputCapability {
    Installed,
    Absent,
}

/// Whether the binding hands `BuildContext` a [`PostFrameHandle`] at all.
///
/// `BuildContext::post_frame_handle()` returns an `Option`, so "no post-frame
/// capability" is a real, reachable configuration — an embedder that drives frames
/// itself, or any binding that simply never calls `install_build_capabilities`.
/// Code that acquires the handle must behave when it is absent, and the only way to
/// test that is to mount without one.
///
/// [`PostFrameHandle`]: flui_scheduler::PostFrameHandle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PostFrameCapability {
    Installed,
    Absent,
}

/// [`mount`], but able to withhold the post-frame and/or text-input
/// capability.
pub(crate) fn mount_with_capabilities(
    root: impl View,
    post_frame: PostFrameCapability,
    text_input: TextInputCapability,
) -> Harness {
    // Conservatively serialize mounted focus fixtures before touching the tree;
    // held until the Harness drops. Each owner has independent TLS focus state,
    // so this is fixture isolation, not cross-owner clobber protection.
    let focus_guard = FOCUS_TEST_LOCK.lock();
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    let mut build_owner = flui_view::BuildOwner::new();
    let mut tree = flui_view::ElementTree::new();

    let mut binding = HeadlessBinding::new();
    match post_frame {
        PostFrameCapability::Installed => binding.install_build_capabilities(&mut build_owner),
        // The async driver still goes in: withholding it too would change *which*
        // capability the test is about — the mount `build_scope` depends on it.
        PostFrameCapability::Absent => {
            build_owner.set_async_driver(binding.scheduler().async_driver().clone());
        }
    }
    let cursor_area_calls = if text_input == TextInputCapability::Installed {
        // Attach/detach are zero-capture closures: automatically `Send +
        // Sync` regardless of `TextInputRegistry`'s own `Rc`-based,
        // non-`Send` internals — the same reasoning `flui-app`'s production
        // `AppBinding::instance()` closures rely on (see `TextInputHandle`'s
        // doc). `set_cursor_area` has no platform window to forward to in
        // this harness (no `flui-app`/`PlatformWindow` involved — see this
        // module's doc), so it records into an `Arc<Mutex<_>>` a test can
        // read back through `Harness::cursor_area_calls` instead.
        let recorded: Arc<parking_lot::Mutex<Vec<Bounds<Pixels>>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));
        let recorded_for_closure = Arc::clone(&recorded);
        build_owner.set_text_input_handle(flui_interaction::TextInputHandle::new(
            |callback| {
                Some(
                    flui_interaction::TextInputRegistry::global()
                        .attach(flui_interaction::OpaqueWindowHandle::new(()), callback),
                )
            },
            |token| {
                flui_interaction::TextInputRegistry::global().detach(token);
            },
            move |area| recorded_for_closure.lock().push(area),
        ));
        Some(recorded)
    } else {
        None
    };

    let root_element = binding.enter_owner_scope(|| {
        let root_element = tree.mount_root_with_pipeline_owner(
            &root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );

        build_owner.schedule_build_for(root_element, 0);
        build_owner.build_scope(&mut tree);
        root_element
    });

    let root_render = {
        let owner = pipeline_owner.read();
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("the mounted subtree should have a render root")
    };
    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(root_render));
        guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(800.0), px(600.0)))));
    }
    binding.enter_owner_scope(|| {
        build_owner
            .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
            .expect("headless frame should succeed");
    });

    binding.bind_tree(build_owner, tree, Arc::clone(&pipeline_owner));

    Harness {
        binding,
        root_element,
        pipeline_owner,
        cursor_area_calls,
        _focus_guard: focus_guard,
    }
}

impl Harness {
    /// Run an owner-side test action under the binding's full local scope.
    pub(crate) fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.binding.enter_owner_scope(callback)
    }

    /// Every `TextInputHandle::set_cursor_area` call recorded so far, in
    /// delivery order.
    ///
    /// # Panics
    ///
    /// Panics if the harness was mounted with [`TextInputCapability::Absent`]
    /// (via [`mount`] rather than [`mount_with_ime`]) — that configuration
    /// installs no `TextInputHandle` at all, so there is nothing to record,
    /// and a test reading this without IME installed is testing the wrong
    /// harness.
    pub(crate) fn cursor_area_calls(&self) -> Vec<Bounds<Pixels>> {
        self.cursor_area_calls
            .as_ref()
            .expect(
                "cursor_area_calls requires mounting with TextInputCapability::Installed \
                 (mount_with_ime, not mount)",
            )
            .lock()
            .clone()
    }
    /// The root element id.
    pub(crate) fn root(&self) -> ElementId {
        self.root_element
    }

    /// Drive a frame **without** dirtying the root, so only what an
    /// `OverlayHandle` / `OverlayEntry` scheduled through its `RebuildHandle`
    /// rebuilds. Every rebuild assertion depends on this: a root-dirtying pump
    /// would rebuild the whole tree and prove nothing.
    pub(crate) fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Replace the root view and settle.
    ///
    /// Goes through `ElementTree::update`, whose dispatch is keyed by `TypeId`, so
    /// the root's *type* must not change between frames. Toggling a field on one
    /// root type is how a subtree gets unmounted.
    pub(crate) fn swap_root(&mut self, new_root: impl View) {
        self.binding.swap_root_view(self.root_element, &new_root);
        self.binding.pump_frame(Duration::ZERO);
    }

    /// The ordered children of `parent`, read through the public `ElementNode`
    /// surface (`parent()` + `slot()`); `child_ids()` is crate-private.
    pub(crate) fn children_of(&mut self, parent: ElementId) -> Vec<ElementId> {
        let mut kids: Vec<(usize, ElementId)> = self
            .binding
            .tree_mut()
            .iter_nodes()
            .filter(|(_, node)| node.parent() == Some(parent))
            .map(|(id, node)| (node.slot(), id))
            .collect();
        kids.sort_unstable();
        kids.into_iter().map(|(_, id)| id).collect()
    }

    /// The binding's **own** scheduler — never `Scheduler::instance()`.
    ///
    /// A post-frame callback registered here is drained by `pump_frame`'s
    /// `Scheduler::drive_frame`, after the pipeline commits layout.
    pub(crate) fn scheduler(&self) -> &flui_scheduler::Scheduler {
        self.binding.scheduler()
    }

    /// The shared pipeline owner, so a post-frame callback can read committed
    /// geometry from inside the frame.
    pub(crate) fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        Arc::clone(&self.pipeline_owner)
    }

    /// The `debug_name()` of every render object currently in the tree.
    ///
    /// The one structural probe a widget-level test has: it says *which* render
    /// objects a view built, without duplicating the render-layer harness in
    /// `flui-objects`, which is where their behavior is pinned.
    pub(crate) fn render_debug_names(&self) -> Vec<&'static str> {
        let owner = self.pipeline_owner.read();
        owner
            .render_tree()
            .iter()
            .map(|(_, node)| node.debug_name())
            .collect()
    }

    /// The only child of `parent`.
    pub(crate) fn only_child(&mut self, parent: ElementId) -> ElementId {
        let kids = self.children_of(parent);
        assert_eq!(kids.len(), 1, "expected exactly one child of {parent:?}");
        kids[0]
    }
}
