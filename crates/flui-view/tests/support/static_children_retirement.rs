//! Independent static children and the mapper retire without competing unwinds.
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use flui_view::element::{SliverList, StaticChildren};
use flui_view::{BuildContext, IntoView, StatelessView, View, ViewExt};

struct Retirement {
    name: &'static str,
    fail: bool,
    seen: Rc<RefCell<Vec<&'static str>>>,
}

impl Drop for Retirement {
    fn drop(&mut self) {
        self.seen.borrow_mut().push(self.name);
        assert!(!self.fail, "{} retirement failed", self.name);
    }
}

#[derive(Clone)]
struct Child(Rc<Retirement>);
impl StatelessView for Child {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        // The read keeps the owning field visible to ordinary dead-code checks.
        let _ = self.0.name;
        super::dense_reconcile_containment::DenseHealthyLeaf { marker: 0 }
    }
}
impl View for Child {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn child_view(
    name: &'static str,
    fail: bool,
    seen: &Rc<RefCell<Vec<&'static str>>>,
) -> flui_view::BoxedView {
    Child(Rc::new(Retirement {
        name,
        fail,
        seen: Rc::clone(seen),
    }))
    .boxed()
}

pub(crate) fn dispatch_child(kind: &str) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let children = vec![
        child_view("first", matches!(kind, "first" | "competition"), &seen),
        child_view("second", matches!(kind, "second" | "competition"), &seen),
    ];
    let mapper = Retirement {
        name: "mapper",
        fail: matches!(kind, "mapper" | "competition"),
        seen: Rc::clone(&seen),
    };
    let delegate = StaticChildren::mapped(children, move |child| {
        let _ = &mapper;
        child
    });
    // Populate the weak callback cache through the actual adaptor constructor.
    drop(SliverList::over(20.0, &delegate));
    assert_eq!(Rc::strong_count(&delegate), 1);
    let result = catch_unwind(AssertUnwindSafe(|| {
        if kind == "incoming" {
            let _delegate = delegate;
            panic!("incoming failure");
        }
        drop(delegate);
    }));
    let expected: &[&str] = match kind {
        "healthy" | "mapper" => &["first", "second", "mapper"],
        "first" | "competition" => &["first"],
        "second" => &["first", "second"],
        "incoming" => &[],
        _ => panic!("unknown static retirement child {kind}"),
    };
    assert_eq!(seen.borrow().as_slice(), expected);
    if kind == "healthy" {
        assert!(result.is_ok());
    } else {
        let payload = result.expect_err("first failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some(match kind {
                "incoming" => "incoming failure",
                "mapper" => "mapper retirement failed",
                "second" => "second retirement failed",
                _ => "first retirement failed",
            })
        );
    }
    // An unrelated delegate still builds and releases every ordinary owner.
    let next = StaticChildren::new(vec![child_view("next", false, &seen)]);
    let built = next.build(0).expect("next child remains buildable");
    assert!(next.build(1).is_none());
    drop(next);
    assert!(!seen.borrow().contains(&"next"));
    drop(built);
    assert_eq!(seen.borrow().last(), Some(&"next"));
}

fn run(kind: &str) {
    super::child_payload_recovery::run_child("FLUI_STATIC_CHILDREN_RETIREMENT_CHILD", kind);
}

pub(crate) fn healthy_static_children_release_in_order() {
    run("healthy");
}
pub(crate) fn first_static_child_failure_retains_successors() {
    run("first");
}
pub(crate) fn later_static_child_failure_retains_mapper() {
    run("second");
}
pub(crate) fn static_mapper_failure_keeps_ordinary_child_retirement() {
    run("mapper");
}
pub(crate) fn competing_static_children_and_mapper_keep_first_failure() {
    run("competition");
}
pub(crate) fn incoming_unwind_retains_static_children_and_mapper() {
    run("incoming");
}
