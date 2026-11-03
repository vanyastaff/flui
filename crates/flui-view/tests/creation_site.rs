//! A view made by a `#[track_caller]` constructor reports where, the element copies it at mount,
//! and the inspector shows it as `created_at`.

use flui_foundation::CreationSite;
use flui_foundation::inspect::SnapshotQuery;
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{ElementBase, WidgetsBinding};

#[derive(Clone)]
struct Leaf;

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

/// A view with a site, filled by its constructor.
#[derive(Clone, StatelessView)]
struct Tracked {
    #[view(site)]
    site: CreationSite,
}

impl Tracked {
    #[track_caller]
    fn new() -> Self {
        Self {
            site: CreationSite::here(),
        }
    }
}

impl StatelessView for Tracked {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

/// A view built by struct literal, which has no site.
#[derive(Clone, StatelessView)]
struct Literal;

impl StatelessView for Literal {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

fn mounted<V: View + Clone>(root: &V) -> WidgetsBinding {
    let binding = WidgetsBinding::new();
    binding.set_pipeline_owner(PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    )));
    binding
        .attach_root_widget(root)
        .expect("a fresh binding attaches");
    binding.draw_frame();
    binding
}

/// The element of a tracked view shows the line its constructor was called on, and one whose
/// view has no site shows none.
///
/// Fails without the derive's `creation_site`, or without the copy at mount: the snapshot then
/// has no `created_at`.
#[test]
fn an_element_shows_where_its_view_was_created() {
    let view = Tracked::new();
    let line = line!() - 1;
    let binding = mounted(&view);
    let root = binding
        .inspect_elements(SnapshotQuery::default())
        .expect("the snapshot is taken");
    let node = root
        .find_by_kind("Tracked")
        .unique()
        .expect("one tracked view");
    if CreationSite::KEPT {
        let site = node
            .get_fact::<&str>("created_at")
            .expect("a site is shown");
        assert!(
            site.contains("creation_site.rs") && site.contains(&format!(":{line}:")),
            "{site} names the constructor call on line {line}"
        );
    }

    let binding = mounted(&Literal);
    let root = binding
        .inspect_elements(SnapshotQuery::default())
        .expect("the snapshot is taken");
    let node = root
        .find_by_kind("Literal")
        .unique()
        .expect("one literal view");
    assert!(
        node.fact("created_at").is_none(),
        "a struct literal has no site"
    );
}

/// The site is read from the view, not remembered by the type.
#[test]
fn the_view_reports_the_site_its_constructor_recorded() {
    let first = Tracked::new();
    let second = Tracked::new();
    if CreationSite::KEPT {
        assert_ne!(
            View::creation_site(&first).line(),
            View::creation_site(&second).line()
        );
    }
    assert_eq!(View::creation_site(&Literal).location(), None);
    let _ = <dyn ElementBase>::creation_site;
}
