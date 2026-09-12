//! The two-phase reconcile's bookkeeping, at the element tier: the
//! scenarios the in-place remap could not survive (a shift of two keyed
//! residents, a swap), plus the panic boundary.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::{ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_types::geometry::px;

use super::{ReconcileOutcome, ReconcileSource, SparseChildren, build_item_or_error};
use crate::view::{RenderView, View};
use crate::{BoxedView, BuildOwner, ElementTree, RecoveredAt};

#[derive(Clone)]
struct KeyedBox {
    key: ValueKey<u32>,
}

impl KeyedBox {
    fn new(id: u32) -> Self {
        Self {
            key: ValueKey::new(id),
        }
    }
}

impl RenderView for KeyedBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(10.0)), Some(px(10.0)))
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for KeyedBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct HostBox;
impl RenderView for HostBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(100.0)), Some(px(100.0)))
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}
impl View for HostBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

struct Fixture {
    tree: ElementTree,
    owner: BuildOwner,
    pipeline: PipelineCell,
    host: flui_foundation::ElementId,
    sparse: SparseChildren,
}

fn fixture() -> Fixture {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let host = tree.mount_root_with_pipeline_owner(
        &HostBox,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    Fixture {
        tree,
        owner,
        pipeline,
        host,
        sparse: SparseChildren::new(),
    }
}

fn index_of(fx: &Fixture, id: flui_foundation::ElementId) -> Option<usize> {
    let render_id = fx.tree.get(id)?.element().render_id()?;
    fx.pipeline.with(|owner| {
        owner
            .render_tree()
            .get(render_id)?
            .parent_data()?
            .downcast_ref::<SliverMultiBoxAdaptorParentData>()
            .map(|pd| pd.index)
    })
}

fn builder_over(ids: Vec<u32>) -> Rc<dyn Fn(usize) -> Option<BoxedView>> {
    Rc::new(move |i| ids.get(i).map(|&id| BoxedView(Box::new(KeyedBox::new(id)))))
}

fn seed(fx: &mut Fixture, ids: &[(usize, u32)]) -> Vec<flui_foundation::ElementId> {
    let mut out = Vec::new();
    for &(index, id) in ids {
        let mut element_owner = fx.owner.element_owner_mut();
        out.push(fx.sparse.ensure(
            index,
            &KeyedBox::new(id),
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        ));
    }
    out
}

#[derive(Clone)]
struct PlainBox;
impl RenderView for PlainBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(10.0)), Some(px(10.0)))
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}
impl View for PlainBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// A scroll that rebuilds the host must not call the builder for the
/// keyless residents the band is about to drop: they are carried over
/// for the band eviction, untouched. A keyed resident outside the band
/// is still reconciled — here its data moved into the band, so it is
/// relocated rather than rebuilt fresh. Nothing is mounted outside the
/// band, even when the builder has a view for the index.
#[test]
fn keyless_residents_outside_the_band_are_carried_over_not_rebuilt() {
    let mut fx = fixture();
    // Keyless residents at 0 and 1, a keyed one (id 7) at 2; the band
    // moves to [4, 8) and the keyed item's data moves to index 5.
    let plain = {
        let mut element_owner = fx.owner.element_owner_mut();
        [0usize, 1].map(|index| {
            fx.sparse.ensure(
                index,
                &PlainBox,
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            )
        })
    };
    let keyed = seed(&mut fx, &[(2, 7)])[0];
    let built = Rc::new(std::cell::RefCell::new(Vec::<usize>::new()));
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = {
        let built = Rc::clone(&built);
        Rc::new(move |i| {
            built.borrow_mut().push(i);
            match i {
                5 => Some(BoxedView(Box::new(KeyedBox::new(7)))),
                _ => Some(BoxedView(Box::new(PlainBox))),
            }
        })
    };
    let find = |key: &dyn ViewKey| (key.key_eq(&ValueKey::new(7u32))).then_some(5);
    let outcome = {
        let mut element_owner = fx.owner.element_owner_mut();
        fx.sparse.reconcile(
            ReconcileSource {
                builder: &*builder,
                find_index_by_key: Some(&find),
                item_count: 100,
                retain_band: (4, 8),
            },
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        )
    };
    assert!(outcome.did_work);
    let built = built.borrow();
    assert!(
        !built.contains(&0) && !built.contains(&1),
        "the builder must not run for keyless residents outside the band; built={built:?}"
    );
    assert_eq!(
        fx.sparse.get(0),
        Some(plain[0]),
        "carried over, not evicted here"
    );
    assert_eq!(
        fx.sparse.get(1),
        Some(plain[1]),
        "carried over, not evicted here"
    );
    assert_eq!(fx.sparse.get(2), None, "the keyed resident left index 2");
    assert_eq!(
        fx.sparse.get(5),
        Some(keyed),
        "the keyed resident moved into the band"
    );
    assert_eq!(index_of(&fx, keyed), Some(5));
    assert_eq!(
        fx.sparse.len(),
        3,
        "nothing mounted fresh: index 2 was out of band"
    );
}

/// Residents at 3 and 4 both shift to 4 and 5 (an insert at the head,
/// reported by the callback): both elements survive, at their new
/// indices, with their render parent data re-stamped — the in-place
/// remap orphaned one of them here.
#[test]
fn shifting_two_keyed_residents_keeps_both_elements() {
    let mut fx = fixture();
    let seeded = seed(&mut fx, &[(3, 30), (4, 40)]);
    // New data: 99 inserted at the head → 30 is now index 4, 40 index 5.
    let data = vec![0, 1, 2, 99, 30, 40];
    let builder = builder_over(data.clone());
    let find = move |key: &dyn ViewKey| {
        key.as_any()
            .downcast_ref::<ValueKey<u32>>()
            .and_then(|k| data.iter().position(|id| id == k.value()))
    };
    let outcome = {
        let mut element_owner = fx.owner.element_owner_mut();
        fx.sparse.reconcile(
            ReconcileSource {
                builder: &*builder,
                find_index_by_key: Some(&find),
                item_count: 6,
                retain_band: (0, usize::MAX),
            },
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        )
    };
    assert!(outcome.did_work);
    assert_eq!(outcome.end_reached_at, None);
    assert_eq!(
        fx.sparse.get(4),
        Some(seeded[0]),
        "30 moved to 4, same element"
    );
    assert_eq!(
        fx.sparse.get(5),
        Some(seeded[1]),
        "40 moved to 5, same element"
    );
    assert!(
        fx.sparse
            .get(3)
            .is_some_and(|id| id != seeded[0] && id != seeded[1]),
        "99 mounted fresh at 3"
    );
    assert_eq!(
        index_of(&fx, seeded[0]),
        Some(4),
        "render parent data re-stamped"
    );
    assert_eq!(index_of(&fx, seeded[1]), Some(5));
    assert_eq!(
        fx.tree.get(seeded[0]).map(crate::tree::ElementNode::slot),
        Some(4)
    );
}

/// A swap within the band, with no callback at all: matched by key.
#[test]
fn swapping_two_keyed_residents_needs_no_callback() {
    let mut fx = fixture();
    let seeded = seed(&mut fx, &[(1, 10), (2, 20), (3, 30)]);
    let builder = builder_over(vec![0, 30, 20, 10]);
    let outcome = {
        let mut element_owner = fx.owner.element_owner_mut();
        fx.sparse.reconcile(
            ReconcileSource {
                builder: &*builder,
                find_index_by_key: None,
                item_count: 4,
                retain_band: (0, usize::MAX),
            },
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        )
    };
    assert!(outcome.did_work);
    assert_eq!(fx.sparse.get(1), Some(seeded[2]));
    assert_eq!(fx.sparse.get(2), Some(seeded[1]));
    assert_eq!(fx.sparse.get(3), Some(seeded[0]));
    assert_eq!(index_of(&fx, seeded[0]), Some(3));
    assert_eq!(index_of(&fx, seeded[2]), Some(1));
    assert_eq!(fx.sparse.len(), 3);
}

/// A builder that declines an index below the count reports it, and the
/// resident there is evicted.
#[test]
fn a_declined_index_is_reported_and_its_resident_evicted() {
    let mut fx = fixture();
    seed(&mut fx, &[(0, 10), (1, 20)]);
    let builder = builder_over(vec![10]);
    let outcome = {
        let mut element_owner = fx.owner.element_owner_mut();
        fx.sparse.reconcile(
            ReconcileSource {
                builder: &*builder,
                find_index_by_key: None,
                item_count: 2,
                retain_band: (0, usize::MAX),
            },
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        )
    };
    assert_eq!(outcome.end_reached_at, Some(1));
    assert_eq!(fx.sparse.len(), 1);
    assert!(fx.sparse.get(1).is_none());
}

/// A registered error-view factory that answers with a keyed view is
/// wrapped keyless: the recovered item may never join key matching.
#[test]
fn a_keyed_custom_error_view_is_recovered_unkeyed() {
    use crate::view::{FlutterError, clear_error_view_builder, set_error_view_builder};
    fn keyed_error_view(_error: &FlutterError) -> Box<dyn View> {
        Box::new(KeyedBox::new(7))
    }
    // The factory is process-global; this test owns it for its duration.
    set_error_view_builder(keyed_error_view);
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|_| panic!("boom"));
    let recovered = build_item_or_error(&*builder, 0).expect("an error view");
    clear_error_view_builder();
    assert!(
        recovered.0.key().is_none(),
        "the recovered item must be unkeyed"
    );
}

/// A render view whose `create_render_object` panics — the user code
/// `ElementTree::insert` reaches through `RenderBehavior::on_mount`, one
/// level past the builder boundary.
#[derive(Clone)]
struct PanicsOnCreate;

impl RenderView for PanicsOnCreate {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        panic!("create_render_object boom")
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PanicsOnCreate {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// A render view whose `update_render_object` panics once `armed` is set,
/// so one reconcile can mount it cleanly and the next can fail it.
#[derive(Clone)]
struct PanicsOnUpdate {
    armed: Arc<AtomicBool>,
}

impl RenderView for PanicsOnUpdate {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(10.0)), Some(px(10.0)))
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        assert!(
            !self.armed.load(Ordering::SeqCst),
            "update_render_object boom"
        );
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PanicsOnUpdate {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// The type of the view an element was mounted from.
fn view_type_of(fx: &Fixture, id: flui_foundation::ElementId) -> std::any::TypeId {
    fx.tree
        .get(id)
        .expect("a live element")
        .element()
        .view_type_id()
}

fn reconcile_with(
    fx: &mut Fixture,
    builder: &dyn Fn(usize) -> Option<BoxedView>,
    item_count: usize,
) -> ReconcileOutcome {
    let mut element_owner = fx.owner.element_owner_mut();
    fx.sparse.reconcile(
        ReconcileSource {
            builder,
            find_index_by_key: None,
            item_count,
            retain_band: (0, usize::MAX),
        },
        fx.host,
        &mut fx.tree,
        &mut element_owner,
        &fx.pipeline,
    )
}

/// The boundary one level up from the builder: a panic inside the item's
/// own `create_render_object` substitutes the error view at that index and
/// leaves every other index mounted. Without it the panic unwinds out of
/// `reconcile`, through the service pass, and takes the frame down.
/// Mount `view` at `index` the way the band walker does.
fn ensure_at(fx: &mut Fixture, index: usize, view: &dyn View) -> flui_foundation::ElementId {
    let mut element_owner = fx.owner.element_owner_mut();
    fx.sparse.ensure(
        index,
        view,
        fx.host,
        &mut fx.tree,
        &mut element_owner,
        &fx.pipeline,
    )
}

#[test]
fn a_child_panicking_in_create_render_object_is_replaced_at_that_index_only() {
    let mut fx = fixture();
    ensure_at(&mut fx, 0, &KeyedBox::new(0));
    ensure_at(&mut fx, 1, &PanicsOnCreate);
    ensure_at(&mut fx, 2, &KeyedBox::new(2));

    assert_eq!(fx.sparse.len(), 3, "every index is resident");
    let failed = fx.sparse.get(1).expect("index 1 is resident");
    assert_eq!(
        view_type_of(&fx, failed),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the failing index carries the error view"
    );
    for index in [0, 2] {
        let ok = fx.sparse.get(index).expect("neighbour is resident");
        assert_eq!(
            view_type_of(&fx, ok),
            std::any::TypeId::of::<KeyedBox>(),
            "a neighbour of the failing index is untouched"
        );
    }

    let recovered_panics = fx.owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded for the one failing mount"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Mount);
    // A window-level record, not a behavior-level one:
    // `RenderBehavior::on_mount` does not catch/record its own panic
    // (unlike `StatefulBehavior::on_activate`), so `hook_panic_recorded`
    // stays unset and `mount_or_substitute` pushes this `Substituted`
    // record itself.
    match recovered_panics[0].at {
        crate::owner::RecoveredAt::Substituted {
            element,
            substitute,
            parent,
            slot,
            ..
        } => {
            assert_eq!(parent, fx.host);
            assert_eq!(slot, 1);
            assert_eq!(
                substitute, failed,
                "the substitute named in the record is the ErrorView actually mounted at \
                     that index"
            );
            assert!(
                element.is_some(),
                "create_render_object panics after the child is minted (`InsertedChild::Minted`), \
                     so there is a stranded element to name"
            );
        }
        other => panic!(
            "expected a Substituted record for a window-level create_render_object panic, \
                 got {other:?}"
        ),
    }
}

/// The stranded node a panicking mount leaves in the slab is retired, not
/// leaked: the tree holds the host, the two good children and the one
/// error child, and nothing else.
#[test]
fn a_panicking_mount_leaves_no_stranded_element_behind() {
    let mut fx = fixture();
    ensure_at(&mut fx, 0, &KeyedBox::new(0));
    ensure_at(&mut fx, 2, &KeyedBox::new(2));
    let clean = fx.tree.len();

    ensure_at(&mut fx, 1, &PanicsOnCreate);
    let recovered = fx.tree.len();

    // The error child is one element (plus whatever its own view builds
    // on a later pass, which has not run here). The half-mounted node the
    // panic stranded would push this higher, and it is reachable through
    // nothing but the `ChildHookPanic` report `mount_or_substitute`'s
    // containment window hands back.
    assert_eq!(
        recovered,
        clean + 1,
        "the failed mount contributed exactly the error child"
    );
}

/// A panic inside `update_render_object` cannot be left in place: the
/// render object is half-configured and `apply_render_update_impact` never
/// ran, so nothing marked it dirty. The resident is removed outright and
/// the error view takes its index.
#[test]
fn a_child_panicking_in_update_render_object_is_replaced_at_that_index() {
    let mut fx = fixture();
    let armed = Arc::new(AtomicBool::new(false));
    let builder = {
        let armed = Arc::clone(&armed);
        move |i: usize| -> Option<BoxedView> {
            Some(if i == 1 {
                BoxedView(Box::new(PanicsOnUpdate {
                    armed: Arc::clone(&armed),
                }))
            } else {
                BoxedView(Box::new(KeyedBox::new(i as u32)))
            })
        }
    };

    ensure_at(&mut fx, 0, &KeyedBox::new(0));
    ensure_at(
        &mut fx,
        1,
        &PanicsOnUpdate {
            armed: Arc::clone(&armed),
        },
    );
    ensure_at(&mut fx, 2, &KeyedBox::new(2));
    let before = fx.sparse.get(1).expect("index 1 mounted cleanly");
    assert_eq!(
        view_type_of(&fx, before),
        std::any::TypeId::of::<PanicsOnUpdate>()
    );

    armed.store(true, Ordering::SeqCst);
    reconcile_with(&mut fx, &builder, 3);

    let after = fx.sparse.get(1).expect("index 1 is still resident");
    assert_ne!(after, before, "the failed resident was replaced, not kept");
    assert_eq!(
        view_type_of(&fx, after),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the failing index carries the error view"
    );
    assert!(
        fx.tree.get(before).is_none(),
        "the failed resident left the tree"
    );
    for index in [0, 2] {
        let ok = fx.sparse.get(index).expect("neighbour is resident");
        assert_eq!(view_type_of(&fx, ok), std::any::TypeId::of::<KeyedBox>());
    }

    let recovered_panics = fx.owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded for the one failing update"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Update);
}

#[test]
fn a_panicking_builder_yields_the_error_view_for_that_index_only() {
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|i| {
        assert!(i != 1, "boom");
        Some(BoxedView(Box::new(KeyedBox::new(i as u32))))
    });
    assert!(build_item_or_error(&*builder, 0).is_some());
    let recovered = build_item_or_error(&*builder, 1).expect("an error view, not None");
    assert_eq!(
        recovered.0.view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>()
    );
    assert!(recovered.0.key().is_none(), "the error view is unkeyed");
}

/// The mount-path counterpart of `a_panicking_builder_yields_the_error_view_for_that_index_only`:
/// a builder panic reaching `reconcile` through `build_item_or_report`
/// must both mount the error view at the panicking index AND record
/// exactly one `LazyDelegate` panic for it.
#[test]
fn a_panicking_item_builder_on_the_mount_path_mounts_the_error_view_and_records_one_panic() {
    let mut fx = fixture();
    {
        let mut element_owner = fx.owner.element_owner_mut();
        for index in 0..3 {
            fx.sparse.ensure(
                index,
                &PlainBox,
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            );
        }
    }

    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|i| {
        assert!(i != 1, "boom");
        Some(BoxedView(Box::new(PlainBox)))
    });
    reconcile_with(&mut fx, &*builder, 3);

    let error_child = fx.sparse.get(1).expect("index 1 is still resident");
    assert_eq!(
        view_type_of(&fx, error_child),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the panicking index mounts the registered error view"
    );

    let recovered = fx.owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one panic must be recorded for the one failing builder"
    );
    assert!(matches!(
        recovered[0].at,
        RecoveredAt::LazyDelegate {
            host,
            index: Some(1),
            ..
        } if host == fx.host
    ));
    assert_eq!(recovered[0].hook, crate::LifecycleHook::Build);
}

/// A panicking `find_index_by_key` must not propagate through
/// `reconcile`: the move it would have reported is declined (as if the
/// key were simply not found), and the panic is recorded once under the
/// host, not the resident.
#[test]
fn a_panicking_find_index_by_key_declines_the_move_and_is_recorded() {
    let mut fx = fixture();
    let keyed = seed(&mut fx, &[(2, 7)])[0];
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> =
        Rc::new(|i| (i == 2).then(|| BoxedView(Box::new(KeyedBox::new(7)))));
    let find = |_key: &dyn ViewKey| -> Option<usize> { panic!("find boom") };

    {
        let mut element_owner = fx.owner.element_owner_mut();
        fx.sparse.reconcile(
            ReconcileSource {
                builder: &*builder,
                find_index_by_key: Some(&find),
                item_count: 100,
                retain_band: (0, usize::MAX),
            },
            fx.host,
            &mut fx.tree,
            &mut element_owner,
            &fx.pipeline,
        );
    }

    assert_eq!(
        fx.sparse.get(2),
        Some(keyed),
        "the resident is exactly where it was; the panicking lookup declined the move"
    );

    let recovered = fx.owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one panic must be recorded for the one failing find_index_by_key call"
    );
    assert!(matches!(
        recovered[0].at,
        RecoveredAt::LazyDelegate {
            host,
            index: None,
            ..
        } if host == fx.host
    ));
    assert_eq!(recovered[0].hook, crate::LifecycleHook::LazyIndexLookup);
}
