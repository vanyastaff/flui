use flui_widgets::{Routable, RouteParseError};

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    // Fields in another order than their parameters.
    #[route("/user/:uid/post/:pid")]
    UserPost { pid: i64, uid: String },
}

fn main() {
    let route = AppRoute::UserPost {
        pid: -4,
        uid: "ada".to_owned(),
    };
    assert_eq!(route.to_path().as_str(), "/user/ada/post/-4");
    assert_eq!(AppRoute::from_path(&route.to_path()), Ok(route));
    assert!(matches!(
        AppRoute::parse("/user/ada/post/x"),
        Err(RouteParseError::Param { field: "pid", .. })
    ));
}
