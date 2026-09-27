use flui_widgets::Routable;

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    // Declared before the literal sibling it loses to.
    #[route("/s/:slug")]
    Slug { slug: String },
    #[route("/s/new")]
    New,
    #[route("/:section/edit")]
    Edit { section: String },
}

fn main() {
    assert_eq!(AppRoute::parse("/s/new"), Ok(AppRoute::New));
    assert_eq!(
        AppRoute::parse("/s/old"),
        Ok(AppRoute::Slug { slug: "old".to_owned() })
    );
    // `/s/edit` matches `/s/:slug` and `/:section/edit`: the leading literal
    // ranks first.
    assert_eq!(
        AppRoute::parse("/s/edit"),
        Ok(AppRoute::Slug { slug: "edit".to_owned() })
    );
    assert_eq!(
        AppRoute::parse("/t/edit"),
        Ok(AppRoute::Edit { section: "t".to_owned() })
    );
}
