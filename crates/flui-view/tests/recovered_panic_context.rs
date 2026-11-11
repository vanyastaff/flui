//! A contained lifecycle panic names where in the element tree it happened: the panicked view and
//! its ancestors by kind, and where the panicked view was created.

use flui_foundation::CreationSite;
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{LifecycleHook, RecoveredPanic, WidgetsBinding};

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

/// A view whose `build` panics, made by a constructor that records its site.
#[derive(Clone, StatelessView)]
struct PanicsInBuild {
    #[view(site)]
    site: CreationSite,
}

impl PanicsInBuild {
    #[track_caller]
    fn new() -> Self {
        Self {
            site: CreationSite::here(),
        }
    }
}

impl StatelessView for PanicsInBuild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        assert!(!std::hint::black_box(true), "induced build panic");
        Leaf
    }
}

/// A view whose `init_state` panics, made by a constructor that records its site.
#[derive(Clone, StatefulView)]
struct PanicsInInitState {
    #[view(site)]
    site: CreationSite,
}

impl PanicsInInitState {
    #[track_caller]
    fn new() -> Self {
        Self {
            site: CreationSite::here(),
        }
    }
}

struct PanicsInInitStateState;

impl StatefulView for PanicsInInitState {
    type State = PanicsInInitStateState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        PanicsInInitStateState
    }
}

impl ViewState<PanicsInInitState> for PanicsInInitStateState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        panic!("induced init_state panic");
    }

    fn build(&self, _view: &PanicsInInitState, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

/// The parent of the panicking view.
#[derive(Clone, StatelessView)]
struct Middle {
    child: BoxedView,
}

impl StatelessView for Middle {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// The grandparent of the panicking view.
#[derive(Clone, StatelessView)]
struct Outer {
    child: Middle,
}

impl StatelessView for Outer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// The one record a first frame of `Outer > Middle > child` drains.
fn drained(child: BoxedView) -> RecoveredPanic {
    let binding = WidgetsBinding::new();
    binding.set_pipeline_owner(PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    )));
    binding
        .attach_root_widget(&Outer {
            child: Middle { child },
        })
        .expect("a fresh binding attaches");
    binding.draw_frame();
    let mut recovered = binding.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "one contained panic: {recovered:?}");
    recovered.remove(0)
}

fn kinds(panic: &RecoveredPanic) -> Vec<&'static str> {
    panic
        .context
        .path()
        .iter()
        .map(flui_foundation::PathEntry::kind)
        .collect()
}

fn assert_site_on(panic: &RecoveredPanic, line: u32) {
    if !CreationSite::KEPT {
        return;
    }
    let site = panic.context.site();
    assert!(
        site.file()
            .is_some_and(|file| file.ends_with("recovered_panic_context.rs"))
            && site.line() == Some(line),
        "the site names the constructor call on line {line}: {site:?}"
    );
}

/// A `build` panic leaves its element in the tree, so the drain reads the path from it: the
/// panicked view first, its parents after it.
///
/// Fails without the drain's fill: the build seam records only the site, and the path is empty.
#[test]
fn a_contained_build_panic_names_its_ancestors_and_creation_site() {
    let view = PanicsInBuild::new();
    let line = line!() - 1;
    let panic = drained(view.boxed());

    assert_eq!(panic.hook, LifecycleHook::Build);
    assert_eq!(
        kinds(&panic).get(..3),
        Some(&["PanicsInBuild", "Middle", "Outer"][..]),
        "the path runs from the panicked view up through its parents: {}",
        panic.context
    );
    assert_eq!(
        panic.context.path()[0].id(),
        flui_foundation::inspect::InspectId::from(
            panic.element().expect("a build panic names its element")
        ),
        "the first entry is the panicked element"
    );
    assert_site_on(&panic, line);
}

/// An `init_state` panic replaces its element, so the seam reads the path before the element is
/// gone.
///
/// Fails without that read: the record names no parent, and the drain finds nothing to start at.
#[test]
fn a_contained_init_state_panic_names_its_ancestors_and_creation_site() {
    let view = PanicsInInitState::new();
    let line = line!() - 1;
    let panic = drained(view.boxed());

    assert_eq!(panic.hook, LifecycleHook::InitState);
    assert_eq!(
        kinds(&panic).get(..3),
        Some(&["PanicsInInitState", "Middle", "Outer"][..]),
        "the path runs from the panicked view up through its parents: {}",
        panic.context
    );
    assert_site_on(&panic, line);
}
