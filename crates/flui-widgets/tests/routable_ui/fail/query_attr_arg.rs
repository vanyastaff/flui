// What `#[query]` takes is `rename = "key"`, once, on a field that carries
// one `#[query]` and reads a key no other field of its variant reads.
use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/search")]
    Search {
        #[query(key = "q")]
        query: Option<String>,
    },
    #[route("/list")]
    List {
        #[query]
        #[query]
        page: u32,
        #[query(rename = "s", rename = "sort")]
        sort: String,
    },
    #[route("/pair")]
    Pair {
        #[query(rename = "k")]
        first: u32,
        #[query(rename = "k")]
        second: u32,
    },
}

fn main() {}
