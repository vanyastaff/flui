//! `ChildViews`, what one build of an element hands the reconciler: the
//! views it holds come back in the order they were given, whether it holds
//! none, one or several.

use std::any::TypeId;

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
            fn create_element(&self) -> ElementKind {
                unreachable!("these views are only compared by type, never mounted")
            }
        }
    )*};
}

unbuilt_view!(First, Second, Third);

fn types(views: &ChildViews) -> Vec<TypeId> {
    views
        .iter()
        .map(|view| view.as_any().type_id())
        .collect::<Vec<TypeId>>()
}

#[test]
fn child_views_keep_the_order_they_were_given() {
    let cases: [(&str, ChildViews, Vec<TypeId>); 5] = [
        ("none", ChildViews::none(), Vec::new()),
        ("one", ChildViews::one(First), vec![TypeId::of::<First>()]),
        (
            "pushed onto none",
            {
                let mut views = ChildViews::none();
                views.push(First);
                views
            },
            vec![TypeId::of::<First>()],
        ),
        (
            "pushed onto one",
            {
                let mut views = ChildViews::one(First);
                views.push(Second);
                views.push(Third);
                views
            },
            vec![
                TypeId::of::<First>(),
                TypeId::of::<Second>(),
                TypeId::of::<Third>(),
            ],
        ),
        (
            "from a list",
            ChildViews::from(vec![
                Box::new(Second) as Box<dyn View>,
                Box::new(First) as Box<dyn View>,
            ]),
            vec![TypeId::of::<Second>(), TypeId::of::<First>()],
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
    let mut pushed = ChildViews::none();
    pushed.push(held.clone());
    for (case, views) in [("one", ChildViews::one(held.clone())), ("pushed", pushed)] {
        let handed = views.get(0).expect("one child view");
        let shared = handed
            .as_any()
            .downcast_ref::<BoxedView>()
            .expect("a shared child is handed on as the BoxedView itself");
        assert!(
            std::ptr::addr_eq(shared.as_view(), held.as_view()),
            "{case}: the child view was copied"
        );
    }
}
