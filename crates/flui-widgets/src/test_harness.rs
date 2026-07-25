//! A headless widget harness for `flui-widgets`' **in-crate** unit tests.
//!
//! `tests/common::lay_out` is an integration-test module and cannot be reached
//! from `src/`, so private modules — `overlay` and `navigator`
//! — need their own. This is the trimmed equivalent: it keeps `lay_out`'s
//! load-bearing ordering (**binding first, so the async driver is installed before
//! the mount `build_scope`**) and drops the geometry helpers.

use std::any::TypeId;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_binding::HeadlessBinding;
use flui_foundation::ElementId;
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerType, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::geometry::{Bounds, Pixels, px};
use flui_types::{Offset, Size};
use flui_view::View;
use parking_lot::RwLock;

/// A mounted, laid-out widget tree.
pub(crate) struct Harness {
    binding: HeadlessBinding,
    /// Focus owner of the exact `BuildOwner` backing this mounted tree.
    focus_manager: Rc<flui_interaction::FocusManager>,
    /// Mounted presentation wrapper, retained as the root-swap target.
    root_element: ElementId,
    /// Concrete type of the caller's logical root below presentation
    /// infrastructure. Element-structure probes resolve this node lazily.
    logical_root_type: TypeId,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    /// Every `TextInputHandle::set_cursor_area` call recorded by the
    /// installed IME capability, in delivery order. `None` when the harness
    /// was mounted with [`TextInputCapability::Absent`] — there is nothing
    /// to record.
    cursor_area_calls: Option<Arc<parking_lot::Mutex<Vec<Bounds<Pixels>>>>>,
    /// Every platform IME enable/disable transition recorded by the harness.
    ime_allowed_calls: Option<Arc<parking_lot::Mutex<Vec<bool>>>>,
    /// Owner-local state backing the installed IME capability.
    text_input_owner: Option<Rc<flui_interaction::TextInputOwner>>,
    next_pointer: Cell<u64>,
    current_pointer: Cell<u64>,
}

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
/// wraps a harness-owned `flui_interaction::TextInputOwner` directly (no
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
    let logical_root_type = root.view_type_id();
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    let mut build_owner = flui_view::BuildOwner::new();
    let focus_manager = build_owner.focus_manager();
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
    let (cursor_area_calls, ime_allowed_calls, text_input_owner) =
        if text_input == TextInputCapability::Installed {
            struct HarnessTextInput {
                cursor_areas: Arc<parking_lot::Mutex<Vec<Bounds<Pixels>>>>,
                ime_allowed: Arc<parking_lot::Mutex<Vec<bool>>>,
            }

            impl flui_platform::traits::PlatformTextInput for HarnessTextInput {
                fn set_ime_allowed(&self, allowed: bool) {
                    self.ime_allowed.lock().push(allowed);
                }

                fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
                    self.cursor_areas.lock().push(area);
                }
            }

            let recorded: Arc<parking_lot::Mutex<Vec<Bounds<Pixels>>>> =
                Arc::new(parking_lot::Mutex::new(Vec::new()));
            let ime_allowed = Arc::new(parking_lot::Mutex::new(Vec::new()));
            let platform: Arc<dyn flui_platform::traits::PlatformTextInput> = // PORT-CHECK-OK-DYN: headless harness supplies the same direct OS-capability boundary as a presentation.
            Arc::new(HarnessTextInput {
                cursor_areas: Arc::clone(&recorded),
                ime_allowed: Arc::clone(&ime_allowed),
            });
            let owner = flui_interaction::TextInputOwner::new(Some(platform));
            build_owner.set_text_input_handle(owner.handle());
            (Some(recorded), Some(ime_allowed), Some(owner))
        } else {
            (None, None, None)
        };

    let root = crate::GestureArenaScope::new(binding.arena().clone(), crate::FocusRoot::new(root));
    let root_element = binding.enter_owner_scope(|| {
        let root_element = tree.mount_root_with_pipeline_owner(
            &root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );

        build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
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
        focus_manager,
        root_element,
        logical_root_type,
        pipeline_owner,
        cursor_area_calls,
        ime_allowed_calls,
        text_input_owner,
        next_pointer: Cell::new(1),
        current_pointer: Cell::new(0),
    }
}

impl Harness {
    /// Focus manager that owns this harness's mounted tree.
    pub(crate) fn focus_manager(&self) -> Rc<flui_interaction::FocusManager> {
        Rc::clone(&self.focus_manager)
    }

    /// Run an owner-side test action under the binding's full local scope.
    pub(crate) fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.binding.enter_owner_scope(callback)
    }

    fn advance_gesture_clock() {
        let t0 = Instant::now();
        while Instant::now() == t0 {
            std::hint::spin_loop();
        }
    }

    fn begin_contact(&self) -> PointerId {
        let id = self.next_pointer.get();
        self.next_pointer.set(
            id.checked_add(1)
                .expect("BUG: headless pointer id space exhausted"),
        );
        self.current_pointer.set(id);
        PointerId::new(id).expect("BUG: headless pointer ids start at one")
    }

    fn current_contact(&self) -> PointerId {
        PointerId::new(self.current_pointer.get())
            .expect("BUG: pointer Down must precede Move, Up, or Cancel")
    }

    fn hit_test_pointer(
        &self,
        position: Offset<Pixels>,
    ) -> flui_rendering::hit_testing::HitTestResult {
        use flui_rendering::hit_testing::HitTestResult;

        let mut result = HitTestResult::new();
        let owner = self.pipeline_owner.read();
        owner.hit_test(position, &mut result);
        result
    }

    pub(crate) fn dispatch_pointer_down(&self, x: f32, y: f32) {
        Self::advance_gesture_clock();
        let event = make_down_event_for_id(
            self.begin_contact(),
            Offset::new(px(x), px(y)),
            PointerType::Mouse,
        );
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    pub(crate) fn dispatch_pointer_move(&self, x: f32, y: f32) {
        Self::advance_gesture_clock();
        let event = make_move_event_for_id(
            self.current_contact(),
            Offset::new(px(x), px(y)),
            PointerType::Mouse,
        );
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    pub(crate) fn dispatch_pointer_up(&self, x: f32, y: f32) {
        let event = make_up_event_for_id(
            self.current_contact(),
            Offset::new(px(x), px(y)),
            PointerType::Mouse,
        );
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
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

    /// Platform IME enable/disable calls in delivery order.
    pub(crate) fn ime_allowed_calls(&self) -> Vec<bool> {
        self.ime_allowed_calls
            .as_ref()
            .expect("ime_allowed_calls requires mount_with_ime")
            .lock()
            .clone()
    }

    /// Deliver an IME event to this harness's active text client.
    pub(crate) fn dispatch_ime(&self, event: &flui_types::ImeEvent) {
        self.text_input_owner
            .as_ref()
            .expect("dispatch_ime requires mount_with_ime")
            .dispatch(event);
    }

    /// Number of active clients in this harness's presentation-local registry.
    pub(crate) fn active_ime_clients(&self) -> usize {
        self.text_input_owner
            .as_ref()
            .expect("active_ime_clients requires mount_with_ime")
            .active_count()
    }
    /// The root element id.
    pub(crate) fn root(&mut self) -> ElementId {
        let logical_root_type = self.logical_root_type;
        self.binding
            .tree_mut()
            .iter_nodes()
            .filter(|(_, node)| node.element().view_type_id() == logical_root_type)
            .min_by_key(|(_, node)| node.element().depth())
            .map(|(id, _)| id)
            .expect("the caller's logical root must remain mounted below presentation scopes")
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
        let root = crate::GestureArenaScope::new(
            self.binding.arena().clone(),
            crate::FocusRoot::new(new_root),
        );
        self.binding.swap_root_view(self.root_element, &root);
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
