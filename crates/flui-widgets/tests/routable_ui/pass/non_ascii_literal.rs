use flui_widgets::{Routable, RoutePath};

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/café/:dish")]
    Dish { dish: String },
    #[route("/a b")]
    Spaced,
}

fn main() {
    let dish = AppRoute::Dish {
        dish: "crème brûlée".to_owned(),
    };
    assert_eq!(
        dish.to_path(),
        RoutePath::root().join("café").join("crème brûlée")
    );
    assert!(dish.to_path().as_str().contains("%C3%A9/"));
    assert_eq!(AppRoute::from_path(&dish.to_path()), Ok(dish));
    assert_eq!(AppRoute::parse("/a%20b"), Ok(AppRoute::Spaced));
}
