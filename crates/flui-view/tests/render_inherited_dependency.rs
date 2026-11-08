//! Render-object configuration reads inherited data during the element's build.

use std::{cell::RefCell, rc::Rc};

use flui_foundation::{ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::{
    RenderUpdateImpact,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_view::{
    BuildOwner, ElementTree, InheritedView, RebuildReason, RenderObjectContext, RenderView,
    RootRenderView, View,
};

#[derive(Clone)]
struct SizeProvider {
    width: f64,
    child: InheritedSizeView,
}

impl InheritedView for SizeProvider {
    type Data = f64;

    fn data(&self) -> &Self::Data {
        &self.width
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.width != old.width
    }
}

impl View for SizeProvider {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::inherited(self)
    }
}

#[derive(Clone)]
struct InheritedSizeView {
    key: ValueKey<u32>,
    create_reads: Rc<RefCell<Vec<Option<f64>>>>,
    update_reads: Rc<RefCell<Vec<Option<f64>>>>,
}

impl InheritedSizeView {
    fn read_width(ctx: &RenderObjectContext<'_>) -> Option<f64> {
        ctx.depend_on::<SizeProvider, _>(|provider| provider.width)
    }
}

impl RenderView for InheritedSizeView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let width = Self::read_width(ctx);
        self.create_reads.borrow_mut().push(width);
        RenderSizedBox::fixed(width.unwrap_or(0.0), 10.0)
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        let width = Self::read_width(ctx);
        self.update_reads.borrow_mut().push(width);
        render_object.set_size(Some(width.unwrap_or(0.0)), Some(10.0))
    }
}

impl View for InheritedSizeView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[test]
fn provider_only_change_reconfigures_the_same_render_view() {
    let pipeline = PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    ));
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let child = InheritedSizeView {
        key: ValueKey::new(7),
        create_reads: Rc::new(RefCell::new(Vec::new())),
        update_reads: Rc::new(RefCell::new(Vec::new())),
    };
    let provider = SizeProvider {
        width: 11.0,
        child: child.clone(),
    };
    let root = tree.mount_root_with_pipeline_owner(
        &RootRenderView::new(provider, 800.0, 600.0),
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let provider_id = tree.get(root).expect("render root is mounted").child_ids()[0];
    let child_id = tree
        .get(provider_id)
        .expect("provider is mounted")
        .child_ids()[0];
    let render_id = tree
        .get(child_id)
        .expect("render view is mounted")
        .element()
        .render_id()
        .expect("render view owns a render object");
    let width = || {
        pipeline.with(|pipeline| {
            pipeline
                .render_tree()
                .get(render_id)
                .expect("render object stays mounted")
                .downcast_render_object::<RenderSizedBox>()
                .expect("render object keeps its type")
                .width()
        })
    };

    assert_eq!(child.create_reads.borrow().as_slice(), &[None]);
    assert_eq!(child.update_reads.borrow().as_slice(), &[Some(11.0)]);
    assert_eq!(width(), Some(11.0));

    tree.update(
        provider_id,
        &SizeProvider {
            width: 22.0,
            child: child.clone(),
        },
        &mut owner.element_owner_mut(),
    );
    owner.build_scope(&mut tree);

    assert_eq!(
        tree.get(provider_id)
            .expect("provider keeps its element")
            .child_ids()[0],
        child_id,
        "a provider-only change must not replace the render-view element"
    );
    assert_eq!(child.create_reads.borrow().as_slice(), &[None]);
    assert_eq!(
        child.update_reads.borrow().as_slice(),
        &[Some(11.0), Some(22.0)]
    );
    assert_eq!(width(), Some(22.0));
}
