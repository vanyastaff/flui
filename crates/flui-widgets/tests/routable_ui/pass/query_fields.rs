use flui_widgets::{Routable, RoutePath};

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/search/:scope")]
    Search {
        scope: String,
        #[query]
        q: Option<String>,
        #[query(rename = "tag")]
        tags: Vec<String>,
        #[query]
        page: u32,
    },
}

fn main() {
    let route = AppRoute::Search {
        scope: "notes".to_owned(),
        q: Some("a b".to_owned()),
        tags: vec!["x".to_owned(), "y".to_owned()],
        page: 2,
    };
    let path = route.to_path();
    assert_eq!(path.as_str(), "/search/notes?q=a%20b&tag=x&tag=y&page=2");
    assert_eq!(AppRoute::from_path(&path), Ok(route));
    assert_eq!(
        AppRoute::from_path(&RoutePath::parse("/search/all").expect("a location")),
        Ok(AppRoute::Search {
            scope: "all".to_owned(),
            q: None,
            tags: Vec::new(),
            page: 0,
        })
    );
}
