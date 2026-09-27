use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/search?q=:q")]
    Search { q: String },
}

fn main() {}
