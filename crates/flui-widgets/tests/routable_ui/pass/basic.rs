use flui_widgets::{Routable, RouteParseError};

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/about")]
    About,
    #[route("/note/:id")]
    Note { id: u32 },
}

fn main() {
    for route in [AppRoute::Home, AppRoute::About, AppRoute::Note { id: 7 }] {
        assert_eq!(AppRoute::from_path(&route.to_path()), Ok(route));
    }
    assert_eq!(AppRoute::Note { id: 7 }.to_path().as_str(), "/note/7");
    assert!(matches!(
        AppRoute::parse("/missing"),
        Err(RouteParseError::NoMatch { .. })
    ));
}
