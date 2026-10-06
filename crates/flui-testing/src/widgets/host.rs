//! The realm both widget harnesses mount into.
//!
//! [`WidgetHost`] attaches one root, [`HarnessRoot`], to a [`HeadlessHost`]
//! and never replaces it: the tree under test lives in a slot the root reads
//! in `build`, so a root swap is a rebuild of that one element and the
//! realm's own root scopes (`GestureArenaScope`, `VsyncScope`, `FocusRoot`,
//! `MediaQuery`) stay mounted across it. Every frame is the realm's
//! `UiRealm::pump`.

use std::any::TypeId;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::{ElementId, ManualClock};
use flui_interaction::events::PointerEvent;
use flui_platform_api::PlatformInput;
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::FrameTiming;
use flui_view::prelude::*;
use flui_view::{BoxedView, ElementTree};
use parking_lot::Mutex;

use crate::host::{HeadlessHost, HeadlessWindow};

/// The one root a [`WidgetHost`] attaches: it builds whatever its slot holds.
#[derive(Clone, StatelessView)]
pub(super) struct HarnessRoot {
    slot: Rc<RefCell<BoxedView>>,
}

impl StatelessView for HarnessRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.slot.borrow().clone()
    }
}

/// A [`HeadlessHost`] hosting one swappable widget tree.
pub(super) struct WidgetHost {
    realm: HeadlessHost,
    slot: Rc<RefCell<BoxedView>>,
    pipeline: PipelineCell,
    /// A registry the caller built its own `VsyncScope` over, ticked at each
    /// frame's time alongside the realm's.
    adopted_vsync: Arc<Mutex<Option<Vsync>>>,
    /// Scenes the sink held before the last pump, to tell whether it painted.
    submits_before_last_pump: u64,
}

impl WidgetHost {
    /// Attach `tree` to a realm over `window` and run the first frame.
    pub(super) fn mount(
        tree: BoxedView,
        window: HeadlessWindow,
        storage: Option<Arc<dyn flui_platform_api::Storage>>,
    ) -> Self {
        let realm = HeadlessHost::with_storage(window, storage);
        let slot = Rc::new(RefCell::new(tree));
        realm
            .attach(&HarnessRoot {
                slot: Rc::clone(&slot),
            })
            .expect("BUG: a fresh realm has no root attached");
        let pipeline = realm
            .realm()
            .widgets()
            .pipeline_owner()
            .expect("BUG: a realm's presentation installs its pipeline");
        let adopted_vsync = Arc::new(Mutex::new(None::<Vsync>));
        tick_adopted_vsync(&realm, &adopted_vsync);
        let mut host = Self {
            realm,
            slot,
            pipeline,
            adopted_vsync,
            submits_before_last_pump: 0,
        };
        host.pump(Duration::ZERO);
        host
    }

    /// Replace the tree under test and run a frame.
    pub(super) fn swap(&mut self, tree: BoxedView) {
        *self.slot.borrow_mut() = tree;
        let root = self
            .shallowest(TypeId::of::<HarnessRoot>())
            .expect("BUG: the harness root stays mounted");
        self.schedule_rebuild(root);
        self.pump(Duration::ZERO);
    }

    /// Mark `(element, depth)` dirty, as a `setState` does.
    pub(super) fn schedule_rebuild(&self, (element, depth): (ElementId, usize)) {
        let widgets = self.realm.realm().widgets();
        widgets.with_element_tree_mut(|tree| {
            if let Some(node) = tree.get_mut(element) {
                node.element_mut().mark_needs_build();
            }
        });
        widgets.with_build_owner_mut(|owner| {
            owner.schedule_build_for(element, depth, flui_view::RebuildReason::StateChange);
        });
    }

    /// Run one frame `dt` after the last.
    pub(super) fn pump(&mut self, dt: Duration) {
        self.submits_before_last_pump = self.realm.sink().submits();
        let _outcome = self.realm.pump(dt);
    }

    /// Deliver a pointer event through the realm's input path.
    pub(super) fn dispatch_pointer(&self, event: &PointerEvent) {
        self.realm.dispatch(PlatformInput::Pointer(event.clone()));
    }

    /// The shallowest mounted element of `view_type` below the harness root
    /// (the realm's root scopes above it are never a caller's view), with its
    /// depth.
    pub(super) fn shallowest(&self, view_type: TypeId) -> Option<(ElementId, usize)> {
        let harness_root = TypeId::of::<HarnessRoot>();
        self.with_tree(|tree| {
            let root = if view_type == harness_root {
                None
            } else {
                Some(
                    shallowest_of(tree, harness_root, None)
                        .expect("BUG: the harness root stays mounted")
                        .0,
                )
            };
            shallowest_of(tree, view_type, root)
        })
    }

    /// Run `f` over the realm's element tree.
    pub(super) fn with_tree<R>(&self, f: impl FnOnce(&mut ElementTree) -> R) -> R {
        self.realm.realm().widgets().with_element_tree_mut(f)
    }

    /// Run `f` over the realm's element tree with a predicate that says
    /// whether an element belongs to the tree under test (below the harness
    /// root) rather than to the realm's root scopes.
    pub(super) fn with_tree_under_test<R>(
        &self,
        f: impl FnOnce(&ElementTree, &dyn Fn(ElementId) -> bool) -> R,
    ) -> R {
        self.with_tree(|tree| {
            let tree: &ElementTree = tree;
            let (root, _) = shallowest_of(tree, TypeId::of::<HarnessRoot>(), None)
                .expect("BUG: the harness root stays mounted");
            let under_test = |id: ElementId| is_descendant(tree, id, root);
            f(tree, &under_test)
        })
    }

    /// Replace the adopted registry.
    pub(super) fn adopt_vsync(&self, vsync: Vsync) {
        *self.adopted_vsync.lock() = Some(vsync);
    }

    pub(super) fn realm(&self) -> &HeadlessHost {
        &self.realm
    }

    pub(super) fn pipeline(&self) -> &PipelineCell {
        &self.pipeline
    }

    pub(super) fn clock(&self) -> &ManualClock {
        self.realm.clock()
    }

    /// Whether the last pump submitted a scene.
    pub(super) fn did_paint_last_frame(&self) -> bool {
        self.realm.sink().submits() > self.submits_before_last_pump
    }
}

/// Tick `adopted` at every frame's time, relative to the realm's start, in
/// the persistent phase the realm ticks its own registry in.
fn tick_adopted_vsync(realm: &HeadlessHost, adopted: &Arc<Mutex<Option<Vsync>>>) {
    let clock = realm.clock().clone();
    let start = flui_foundation::MonotonicClock::now(&clock);
    let adopted = Arc::clone(adopted);
    realm
        .realm()
        .scheduler()
        .add_persistent_frame_callback(Arc::new(move |_timing: &FrameTiming| {
            let vsync = adopted.lock().clone();
            if let Some(vsync) = vsync {
                let now = flui_foundation::MonotonicClock::now(&clock);
                vsync.tick_all(now.saturating_duration_since(start).as_secs_f64());
            }
        }));
}

/// The shallowest element of `view_type`, within `under`'s subtree when given.
fn shallowest_of(
    tree: &ElementTree,
    view_type: TypeId,
    under: Option<ElementId>,
) -> Option<(ElementId, usize)> {
    tree.iter_nodes()
        .filter(|(_, node)| node.element().view_type_id() == view_type)
        .filter(|(id, _)| under.is_none_or(|ancestor| is_descendant(tree, *id, ancestor)))
        .min_by_key(|(_, node)| node.depth())
        .map(|(id, node)| (id, node.depth()))
}

fn is_descendant(tree: &ElementTree, mut id: ElementId, ancestor: ElementId) -> bool {
    while let Some(parent) = tree.get(id).and_then(flui_view::ElementNode::parent) {
        if parent == ancestor {
            return true;
        }
        id = parent;
    }
    false
}
