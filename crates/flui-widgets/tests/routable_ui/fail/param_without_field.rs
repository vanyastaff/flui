use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/note/:id")]
    Note,
}

fn main() {}
