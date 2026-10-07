//! An element-level headless harness for widget crates' tests.
//!
//! [`lay_out`](super::lay_out) is the canonical geometry harness, but route,
//! overlay and text-editing code needs element-tree probes (`children_of`,
//! `view_type_of`) and the text-input capability, which
//! [`LaidOut`](super::LaidOut) deliberately does not expose. This is the
//! trimmed element-level equivalent: the same realm host (every frame is
//! `UiRealm::pump`), an 800 × 600 surface with the root aligned top-left, no
//! geometry helpers, and the same pointer-contact identity and
//! sample-interval policy as the canonical harness via [`PointerContacts`] /
//! [`POINTER_SAMPLE_INTERVAL`].
//!
//! [`mount_with_ime`] gives the realm's window a recording input-method host
//! (pull-model, as on Windows) and [`mount_with_push_ime`] a recording push
//! text input (as with winit), so the realm's presentation owns the IME
//! session and its frames are text-store
//! transactions exactly as on screen: commits close for the frame's
//! duration, and the grants queued meanwhile run once it returns.

use std::any::TypeId;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::ElementId;
use flui_foundation::geometry::Bounds;
use flui_foundation::geometry::Offset;
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    make_up_event_for_id,
};
use flui_painting::Alignment;
use flui_platform_api::PlatformInput;
use flui_rendering::pipeline::PipelineCell;
use flui_view::{ElementNode, View, ViewExt};
use flui_widgets::Align;

use super::host::WidgetHost;
use super::{POINTER_SAMPLE_INTERVAL, PointerContacts};
use crate::host::HeadlessWindow;

/// The surface every [`Harness`] mounts into.
const SURFACE: (u32, u32) = (800, 600);

/// A mounted, laid-out widget tree.
pub struct Harness {
    host: WidgetHost,
    /// Concrete type of the caller's logical root below presentation
    /// infrastructure. Element-structure probes resolve this node lazily.
    logical_root_type: TypeId,
    pipeline_owner: PipelineCell,
    /// Per-contact pointer identity, shared with [`super::LaidOut`] so the
    /// two cannot drift.
    contacts: PointerContacts,
}

impl std::fmt::Debug for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("realm", self.host.realm())
            .finish_non_exhaustive()
    }
}

/// Mount `root` as the render-tree root and drive one frame. The realm's
/// window offers no text input, so a field attaches to a session no
/// platform input method serves.
pub fn mount(root: impl View) -> Harness {
    mount_in(root, HeadlessWindow::new(SURFACE.0, SURFACE.1))
}

/// [`mount`], with a window whose input method pulls from the focused field,
/// as the Win32 text services do (ADR-0135): the realm's presentation tells
/// a recording host which field's store takes input
/// ([`Harness::active_text_store`], [`Harness::store_host_calls`]), and
/// [`Harness::dispatch_ime`] still delivers push events to the active field.
pub fn mount_with_ime(root: impl View) -> Harness {
    mount_in(
        root,
        HeadlessWindow::new(SURFACE.0, SURFACE.1).with_text_store_host(),
    )
}

/// [`mount`], with a window that offers a recording push-model text input,
/// as winit does: the realm's presentation enables the IME and reports the
/// candidate area ([`Harness::cursor_area_calls`],
/// [`Harness::ime_allowed_calls`]), and [`Harness::dispatch_ime`] delivers
/// platform IME events to it.
pub fn mount_with_push_ime(root: impl View) -> Harness {
    mount_in(
        root,
        HeadlessWindow::new(SURFACE.0, SURFACE.1).with_text_input(),
    )
}

fn mount_in(root: impl View, window: HeadlessWindow) -> Harness {
    let logical_root_type = root.view_type_id();
    let host = WidgetHost::mount(aligned(root), window, None);
    let pipeline_owner = host.pipeline().clone();
    Harness {
        host,
        logical_root_type,
        pipeline_owner,
        contacts: PointerContacts::new(),
    }
}

/// The harness's one wrapper: the root at its own size in the top-left.
fn aligned(root: impl View) -> flui_view::BoxedView {
    Align::new(Alignment::TOP_LEFT).child(root).boxed()
}

impl Harness {
    /// Focus manager that owns this harness's mounted tree.
    pub fn focus_manager(&self) -> Rc<flui_interaction::FocusManager> {
        self.host
            .realm()
            .realm()
            .widgets()
            .with_build_owner(flui_view::BuildOwner::focus_manager)
    }

    /// The clipboard this tree's widgets reach through
    /// `LifecycleContext::clipboard_handle`.
    pub fn clipboard(&self) -> Arc<flui_platform_api::InMemoryClipboard> {
        self.host.realm().clipboard()
    }

    /// Run an owner-side test action inside the realm's owner scope.
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.host.realm().enter(|_| callback())
    }

    /// Turn semantics on, as a platform adapter does when assistive
    /// technology attaches; the next frame assembles the tree.
    pub fn enable_semantics(&mut self) {
        self.host.realm().enable_semantics();
    }

    /// The accessibility tree as a platform adapter would receive it, or
    /// `None` until [`enable_semantics`](Self::enable_semantics) has been
    /// called and a frame has run since.
    #[must_use]
    pub fn a11y_tree(&self) -> Option<crate::A11yTree> {
        super::a11y_tree(&self.pipeline_owner)
    }

    /// Deliver an accessibility action inside this tree's realm; see
    /// [`LaidOut::invoke_semantics_action`](super::LaidOut::invoke_semantics_action).
    ///
    /// # Errors
    ///
    /// See [`InvokeActionError`](crate::InvokeActionError).
    pub fn invoke_semantics_action(
        &self,
        request: crate::ActionRequest,
    ) -> Result<(), crate::InvokeActionError> {
        self.host
            .realm()
            .enter(|_| crate::a11y::invoke_semantics_action(&self.pipeline_owner, request))
    }

    /// Advance the realm's virtual clock by the shared
    /// [`POINTER_SAMPLE_INTERVAL`] before a synthetic Move that records a new
    /// velocity sample — the same mechanism (and same 8ms rationale) as
    /// [`super::LaidOut`]. `DragGestureRecognizer` timestamps its velocity
    /// samples from `RecognizerBase::now()`, which reads the realm's
    /// clock-bound `GestureArena`, so a spin-wait on the real clock (which
    /// made sample spacing depend on however much wall-clock time the test
    /// process happened to be scheduled between dispatch calls) is neither
    /// necessary nor correct.
    ///
    /// Only a Move calls this: `DragGestureRecognizer::handle_down` always
    /// resets the velocity tracker before recording its own sample, so a Down
    /// has no predecessor sample to space apart from, and advancing the clock
    /// there would only cost deadline-timing tests virtual time they did not
    /// ask to spend.
    fn advance_pointer_clock(&self) {
        self.host.clock().advance(POINTER_SAMPLE_INTERVAL);
    }

    /// Begin a new mouse contact at logical `(x, y)`, hit-testing the mounted
    /// render tree.
    pub fn dispatch_pointer_down(&self, x: f64, y: f64) {
        let event =
            make_down_event_for_id(self.contacts.begin(), Offset::new(x, y), PointerType::Mouse);
        self.host.dispatch_pointer(&event);
    }

    /// Move the in-flight contact to logical `(x, y)`, one sample interval
    /// after the previous event.
    pub fn dispatch_pointer_move(&self, x: f64, y: f64) {
        self.advance_pointer_clock();
        let event = make_move_event_for_id(
            self.current_contact(),
            Offset::new(x, y),
            PointerType::Mouse,
        );
        self.host.dispatch_pointer(&event);
    }

    /// Lift the in-flight contact at logical `(x, y)`.
    pub fn dispatch_pointer_up(&self, x: f64, y: f64) {
        let event = make_up_event_for_id(
            self.current_contact(),
            Offset::new(x, y),
            PointerType::Mouse,
        );
        self.host.dispatch_pointer(&event);
    }

    /// Cancel the in-flight contact — the platform withdrawing a gesture
    /// (a system gesture taking over, a window losing the pointer). Carries no
    /// position, matching `make_cancel_event_for_id`.
    pub fn dispatch_pointer_cancel(&self) {
        let event = make_cancel_event_for_id(self.current_contact(), PointerType::Mouse);
        self.host.dispatch_pointer(&event);
    }

    fn current_contact(&self) -> PointerId {
        self.contacts.current()
    }

    /// Every IME cursor area the realm reported to the window, in delivery
    /// order.
    ///
    /// # Panics
    ///
    /// Panics unless the harness was mounted with [`mount_with_push_ime`]:
    /// no other window offers a push text input, so there is
    /// nothing to record, and a test reading this without it is testing the
    /// wrong harness.
    pub fn cursor_area_calls(&self) -> Vec<Bounds<f64>> {
        self.host
            .realm()
            .window()
            .ime_cursor_areas()
            .expect("cursor_area_calls requires a window with a text input (mount_with_push_ime)")
    }

    /// Platform IME enable/disable calls in delivery order.
    ///
    /// # Panics
    ///
    /// As [`Self::cursor_area_calls`].
    pub fn ime_allowed_calls(&self) -> Vec<bool> {
        self.host
            .realm()
            .window()
            .ime_allowed_calls()
            .expect("ime_allowed_calls requires mount_with_push_ime")
    }

    /// Deliver an IME event to the realm's primary presentation, as the
    /// platform delivers one between frames.
    pub fn dispatch_ime(&self, event: &flui_platform_api::ImeEvent) {
        self.host
            .realm()
            .dispatch(PlatformInput::Ime(event.clone()));
    }

    /// Number of fields the realm's input-method host serves (zero or one).
    ///
    /// # Panics
    ///
    /// As [`Self::store_host_calls`].
    pub fn active_ime_clients(&self) -> usize {
        usize::from(self.active_text_store().is_some())
    }

    /// The text store the realm's presentation last told its host to serve:
    /// the surface a platform input method pulls from (ADR-0090).
    ///
    /// # Panics
    ///
    /// As [`Self::store_host_calls`].
    pub fn active_text_store(&self) -> Option<Rc<dyn flui_platform_api::TextStore>> {
        self.store_host().focused_store()
    }

    /// Every call the realm's presentation made on its input-method host, in
    /// order.
    ///
    /// # Panics
    ///
    /// Panics unless the harness was mounted with [`mount_with_ime`]: no
    /// other window offers a host.
    pub fn store_host_calls(&self) -> Vec<crate::StoreHostCall> {
        self.store_host().calls()
    }

    fn store_host(&self) -> &Rc<crate::RecordingTextStoreHost> {
        self.host
            .realm()
            .text_store_host()
            .expect("the input-method host requires mount_with_ime")
    }

    /// The root element id.
    pub fn root(&mut self) -> ElementId {
        self.host
            .shallowest(self.logical_root_type)
            .map(|(id, _)| id)
            .expect("the caller's logical root must remain mounted below presentation scopes")
    }

    /// Drive a frame **without** dirtying the root, so only what an
    /// `OverlayHandle` / `OverlayEntry` scheduled through its `RebuildHandle`
    /// rebuilds. Every rebuild assertion depends on this: a root-dirtying pump
    /// would rebuild the whole tree and prove nothing.
    ///
    /// The frame is the realm's `UiRealm::pump`, so it is a transaction for
    /// text stores as on screen: commits close for its duration, and the
    /// grants queued meanwhile run once it returns.
    pub fn tick(&mut self) {
        self.host.pump(Duration::ZERO);
    }

    /// Replace the root view and settle.
    ///
    /// The harness root rebuilds with `new_root` under the same `Align`, so a
    /// root of the same type updates in place; toggling a field on one root
    /// type is how a subtree gets unmounted.
    pub fn swap_root(&mut self, new_root: impl View) {
        self.logical_root_type = new_root.view_type_id();
        self.host.swap(aligned(new_root));
    }

    /// The ordered children of `parent`, read through the public `ElementNode`
    /// surface (`parent()` + `slot()`); `child_ids()` is crate-private.
    pub fn children_of(&mut self, parent: ElementId) -> Vec<ElementId> {
        let mut kids: Vec<(usize, ElementId)> = self.host.with_tree(|tree| {
            tree.iter_nodes()
                .filter(|(_, node)| node.parent() == Some(parent))
                .map(|(id, node)| (node.slot(), id))
                .collect()
        });
        kids.sort_unstable();
        kids.into_iter().map(|(_, id)| id).collect()
    }

    /// The realm's **own** scheduler — never `UpdateScheduler::instance()`.
    ///
    /// A post-frame callback registered here is drained by the realm's
    /// frame, after the pipeline commits layout.
    pub fn scheduler(&self) -> &flui_scheduler::UpdateScheduler {
        self.host.realm().realm().scheduler()
    }

    /// The shared pipeline owner, so a post-frame callback can read committed
    /// geometry from inside the frame.
    pub fn pipeline_owner(&self) -> PipelineCell {
        self.pipeline_owner.clone()
    }

    /// The owner-local post-frame handle the realm installed on this tree's
    /// `BuildOwner`, so a test can `schedule_local` a callback that captures
    /// the (`!Send`) [`PipelineCell`] — `add_post_frame_callback`'s `Send`
    /// bound cannot carry it.
    pub fn local_post_frame_handle(&mut self) -> flui_scheduler::LocalPostFrameHandle {
        self.host
            .realm()
            .realm()
            .widgets()
            .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
            .expect("the realm installs an owner-local post-frame handle")
    }

    /// The `debug_name()` of every render object currently in the tree.
    ///
    /// The one structural probe a widget-level test has: it says *which* render
    /// objects a view built, without duplicating the render-layer harness in
    /// `flui-objects`, which is where their behavior is pinned.
    pub fn render_debug_names(&self) -> Vec<&'static str> {
        self.pipeline_owner.with(|owner| {
            owner
                .render_tree()
                .iter()
                .map(|(_, node)| node.debug_name())
                .collect()
        })
    }

    /// The concrete view type behind element `id`.
    pub fn view_type_of(&mut self, id: ElementId) -> TypeId {
        self.host.with_tree(|tree| {
            tree.get(id)
                .map(|node| node.element().view_type_id())
                .expect("the probed element must be mounted")
        })
    }

    /// The parent of element `id`, if any.
    pub fn parent_of(&mut self, id: ElementId) -> Option<ElementId> {
        self.host
            .with_tree(|tree| tree.get(id).and_then(ElementNode::parent))
    }

    /// Every mounted element whose view is of type `ty`, in arbitrary order.
    pub fn elements_of_type(&mut self, ty: TypeId) -> Vec<ElementId> {
        self.host.with_tree(|tree| {
            tree.iter_nodes()
                .filter(|(_, node)| node.element().view_type_id() == ty)
                .map(|(id, _)| id)
                .collect()
        })
    }

    /// The only child of `parent`.
    pub fn only_child(&mut self, parent: ElementId) -> ElementId {
        let kids = self.children_of(parent);
        assert_eq!(kids.len(), 1, "expected exactly one child of {parent:?}");
        kids[0]
    }
}
