use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/", label = "Home")]
    Home,
}

fn main() {}
