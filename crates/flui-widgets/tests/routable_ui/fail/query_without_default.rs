use flui_widgets::Routable;

#[derive(Debug, Clone, PartialEq)]
struct Page(u32);

impl std::str::FromStr for Page {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Page)
    }
}

impl std::fmt::Display for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/list")]
    List {
        #[query]
        page: Page,
    },
}

fn main() {}
