use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note(u32),
}

fn main() {}
