//! `ChildViews`, what one build of an element hands the reconciler: the
//! views it holds come back in the order they were given, whether it holds
//! none, one or several, each shared as a `BoxedView`.

use std::any::{Any, TypeId};

use flui_view::__runtime::{BuildOwner, ElementTree};
use flui_view::{BoxedView, ChildViews, View, ViewExt, element::ElementKind};

#[derive(Clone)]
struct First;

#[derive(Clone)]
struct Second;

#[derive(Clone)]
struct Third;

macro_rules! unbuilt_view {
    ($($view:ty),*) => {$(
        impl View for $view {
            fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
                unreachable!("these views are only compared by type, never mounted")
            }
        }
    )*};
}

unbuilt_view!(First, Second, Third);

/// The concrete type of the view each held `BoxedView` erases.
fn types(views: &ChildViews) -> Vec<TypeId> {
    (0..views.len())
        .filter_map(|index| views.get(index))
        .map(|view| {
            let erased: &dyn Any = view.as_view();
            erased.type_id()
        })
        .collect::<Vec<TypeId>>()
}

#[test]
fn child_views_keep_the_order_they_were_given() {
    let cases: [(&str, ChildViews, Vec<TypeId>); 3] = [
        ("none", ChildViews::none(), Vec::new()),
        ("one", ChildViews::one(First), vec![TypeId::of::<First>()]),
        (
            "from a list",
            ChildViews::from(vec![Second.boxed(), First.boxed(), Third.boxed()]),
            vec![
                TypeId::of::<Second>(),
                TypeId::of::<First>(),
                TypeId::of::<Third>(),
            ],
        ),
    ];
    for (case, views, expected) in cases {
        assert_eq!(types(&views), expected, "{case}");
    }
}

/// A `BoxedView` handed on shared is the very view the parent holds, not a
/// copy of it: the reconciler sees the same shared allocation, which is the
/// child's identity.
#[test]
fn a_shared_child_view_is_the_parent_s_own() {
    let held = First.boxed();
    let views = ChildViews::one(held.clone());
    let handed = views.get(0).expect("one child view");
    assert!(
        std::ptr::addr_eq(handed.as_view(), held.as_view()),
        "the child view was copied"
    );
}

/// The upcast of a `BoxedView` names the wrapper, and the upcast of the view
/// it erases names the view: a `BoxedView` holding a `First` is not a
/// `First`, so a downcast has to unwrap it first.
#[test]
fn an_erased_view_is_told_apart_from_the_view_it_holds() {
    let boxed = First.boxed();
    let offered: &dyn View = &boxed;
    let wrapper: &dyn Any = offered;
    let inner: &dyn Any = boxed.as_view();
    assert_eq!(wrapper.type_id(), TypeId::of::<BoxedView>());
    assert!(wrapper.downcast_ref::<First>().is_none());
    assert_eq!(inner.type_id(), TypeId::of::<First>());
}

#[derive(Clone)]
struct Label(&'static str);

impl flui_view::StatelessView for Label {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        Label("unbuilt")
    }
}

impl View for Label {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// The text of the `Label` the element at `root` holds.
fn held_label(tree: &ElementTree, root: flui_foundation::ElementId) -> &'static str {
    let view = tree
        .get(root)
        .expect("the root is mounted")
        .element()
        .view_as_any()
        .expect("a stateless element holds its view");
    view.downcast_ref::<Label>()
        .expect("the root element holds a Label")
        .0
}

/// An element mounted for a `Label` takes a new `Label` in place whether it
/// is offered erased in a `BoxedView` or bare: the update unwraps the
/// `BoxedView` and downcasts the view inside, so the wrapper is neither
/// mistaken for another type nor for the view it holds.
#[test]
fn an_offered_view_updates_its_element_in_place_however_it_is_erased() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&Label("mounted").boxed(), &mut owner.element_owner_mut());

    let cases: [(&str, &dyn View, &str); 2] = [
        ("boxed", &Label("boxed").boxed(), "boxed"),
        ("bare", &Label("bare"), "bare"),
    ];
    for (case, offered, expected) in cases {
        tree.update(root, offered, &mut owner.element_owner_mut());
        assert_eq!(held_label(&tree, root), expected, "{case}");
    }
}
