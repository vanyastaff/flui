// `route` gives a variant its pattern and `query` marks a variant's field.
// Anywhere else the derive could not act on them, so each one is refused where
// it stands, and every refusal of the type arrives in one build.
use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
#[route("/")]
#[r#query]
enum AppRoute {
    #[route("/search")]
    #[query(rename = "q")]
    Search { q: String },
    #[route("/note/:id")]
    Note {
        #[route("/ignored")]
        id: u32,
    },
    #[route("/call")]
    Call {
        #[query]
        then: fn(#[query] u32),
    },
    #[route("/tag")]
    Tag {
        #[query()]
        name: String,
    },
}

fn main() {}
