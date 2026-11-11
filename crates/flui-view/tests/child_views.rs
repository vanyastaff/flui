//! `ChildViews`, what one build of an element hands the reconciler: the
//! views it holds come back in the order they were given, whether it holds
//! none, one or several.

use std::any::TypeId;

use flui_view::{ChildViews, View, element::ElementKind};

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
        .as_slice()
        .iter()
        .map(|view| view.as_any().type_id())
        .collect::<Vec<TypeId>>()
}

#[test]
fn child_views_keep_the_order_they_were_given() {
    let cases: [(&str, ChildViews, Vec<TypeId>); 5] = [
        ("none", ChildViews::none(), Vec::new()),
        ("one", ChildViews::one(Box::new(First)), vec![TypeId::of::<First>()]),
        (
            "pushed onto none",
            {
                let mut views = ChildViews::none();
                views.push(Box::new(First));
                views
            },
            vec![TypeId::of::<First>()],
        ),
        (
            "pushed onto one",
            {
                let mut views = ChildViews::one(Box::new(First));
                views.push(Box::new(Second));
                views.push(Box::new(Third));
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
