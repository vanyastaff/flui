//! Code the binding runs (a view's `build` or `inspect`) that reaches back into its binding for
//! a snapshot is answered with `SnapshotError::Busy`, never left waiting on its own stack, and a
//! view's `inspect` that panics is recorded on its node.

use std::cell::{OnceCell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::inspect::{
    Fields, INSPECT_FAILED, SnapshotError, SnapshotNode, SnapshotQuery,
};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{BuildContext, IntoView, RenderView, StatelessView, View, WidgetsBinding};

type Answer = Result<SnapshotNode, SnapshotError>;

/// What the view does when it is built or inspected.
#[derive(Clone, Copy)]
enum Reach {
    /// Asks its binding for a snapshot and keeps the answer.
    Snapshot,
    /// Borrows its binding's tree for writing.
    Write,
}

struct Hook {
    binding: OnceCell<Weak<WidgetsBinding>>,
    reach: Reach,
    from_build: RefCell<Option<Answer>>,
    from_inspect: RefCell<Option<Answer>>,
}

impl Hook {
    fn binding(&self) -> Rc<WidgetsBinding> {
        self.binding
            .get()
            .and_then(Weak::upgrade)
            .expect("the test keeps its binding alive")
    }
}

#[derive(Clone)]
struct Reentrant(Rc<Hook>);

impl StatelessView for Reentrant {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if matches!(self.0.reach, Reach::Snapshot) {
            let answer = self.0.binding().inspect_elements(SnapshotQuery::default());
            *self.0.from_build.borrow_mut() = Some(answer);
        }
        Leaf
    }
}

impl View for Reentrant {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        out.field("before", &true);
        match self.0.reach {
            Reach::Snapshot => {
                let answer = self.0.binding().inspect_elements(SnapshotQuery::default());
                *self.0.from_inspect.borrow_mut() = Some(answer);
            }
            Reach::Write => self.0.binding().with_element_tree_mut(|_| ()),
        }
    }
}

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

fn mounted(reach: Reach) -> (Rc<WidgetsBinding>, Rc<Hook>) {
    let binding = Rc::new(WidgetsBinding::new());
    binding.set_pipeline_owner(PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    )));
    let hook = Rc::new(Hook {
        binding: OnceCell::new(),
        reach,
        from_build: RefCell::new(None),
        from_inspect: RefCell::new(None),
    });
    hook.binding.set(Rc::downgrade(&binding)).expect("set once");
    binding
        .attach_root_widget(&Reentrant(Rc::clone(&hook)))
        .expect("a fresh binding attaches");
    binding.draw_frame();
    (binding, hook)
}

fn reentrant(root: &SnapshotNode) -> &SnapshotNode {
    root.find_by_kind("Reentrant")
        .unique()
        .expect("one reentrant view is mounted")
}

/// A snapshot asked for from a `build` answers that the tree is building, and one asked for
/// from a view's `inspect` answers that it is inspecting, while the outer snapshot completes.
///
/// Fails without the phase guard: the nested snapshot from `inspect` walks the tree again from
/// inside its own walk, without end.
#[test]
fn a_snapshot_asked_for_from_build_or_inspect_answers_busy() {
    let (binding, hook) = mounted(Reach::Snapshot);
    assert_eq!(
        hook.from_build.borrow_mut().take().map(Result::err),
        Some(Some(SnapshotError::Busy("building")))
    );

    let root = binding
        .inspect_elements(SnapshotQuery::default())
        .expect("asked from outside, the snapshot is taken");
    assert_eq!(
        hook.from_inspect.borrow_mut().take().map(Result::err),
        Some(Some(SnapshotError::Busy("inspecting")))
    );
    assert_eq!(reentrant(&root).get::<bool>("before"), Ok(true));
    assert_eq!(reentrant(&root).fact(INSPECT_FAILED), None);
    assert!(
        binding.inspect_elements(SnapshotQuery::default()).is_ok(),
        "the phase ends with the snapshot"
    );
}

/// A view's `inspect` that borrows the tree for writing is refused at once, and the refusal is
/// recorded on that view's node while the rest of the snapshot is taken.
///
/// Fails without the containment: the refusal escapes and no snapshot is taken.
#[test]
fn an_inspect_that_reaches_for_the_tree_is_recorded_on_its_node() {
    let (binding, _probe) = mounted(Reach::Write);
    let root = binding
        .inspect_elements(SnapshotQuery::default())
        .expect("the snapshot is taken");
    let node = reentrant(&root);
    let failure = node
        .get_fact::<&str>(INSPECT_FAILED)
        .expect("the refusal is recorded");
    assert!(failure.contains("busy inspecting"), "{failure}");
    assert_eq!(node.get::<bool>("before"), Ok(true));
    assert_eq!(node.children.len(), 1, "the leaf below is still taken");
}
