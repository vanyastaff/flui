use flui_widgets::Routable;

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/kind/:type")]
    Kind { r#type: String },
}

fn main() {
    let route = AppRoute::Kind {
        r#type: "fruit".to_owned(),
    };
    assert_eq!(route.to_path().as_str(), "/kind/fruit");
    assert_eq!(AppRoute::from_path(&route.to_path()), Ok(route));
}
