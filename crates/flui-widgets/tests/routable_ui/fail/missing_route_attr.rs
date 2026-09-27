use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    About,
}

fn main() {}
