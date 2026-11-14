use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/search")]
    Search {
        #[query(key = "q")]
        query: Option<String>,
    },
}

fn main() {}
