//! The two-phase reconcile's bookkeeping, at the element tier: the
//! scenarios the in-place remap could not survive (a shift of two keyed
//! residents, a swap), plus the panic boundary.

use std::rc::Rc;

use flui_foundation::{ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

use super::{ReconcileSource, SparseChildren, build_item_or_error};
use crate::view::{RenderView, View};
use crate::{BoxedView, BuildOwner, ElementTree};

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
        RenderSizedBox::new(Some(10.0), Some(10.0))
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
        RenderSizedBox::new(Some(100.0), Some(100.0))
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
    let pipeline = PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    ));
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
        RenderSizedBox::new(Some(10.0), Some(10.0))
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

/// The type of the view an element was mounted from.
fn view_type_of(fx: &Fixture, id: flui_foundation::ElementId) -> std::any::TypeId {
    fx.tree
        .get(id)
        .expect("a live element")
        .element()
        .view_type_id()
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
    // (unlike `StatefulBehavior::on_activate`), so the staged handoff
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

#[test]
fn sparse_reconcile_containment_matrix() {
    crate::table_test::run_table(
        "sparse_reconcile_containment_matrix",
        &[
            (
                "keyless_residents_outside_the_band_are_carried_over_not_rebuilt",
                keyless_residents_outside_the_band_are_carried_over_not_rebuilt as fn(),
            ),
            (
                "a_child_panicking_in_create_render_object_is_replaced_at_that_index_only",
                a_child_panicking_in_create_render_object_is_replaced_at_that_index_only as fn(),
            ),
            (
                "a_panicking_builder_yields_the_error_view_for_that_index_only",
                a_panicking_builder_yields_the_error_view_for_that_index_only as fn(),
            ),
        ],
    );
}
