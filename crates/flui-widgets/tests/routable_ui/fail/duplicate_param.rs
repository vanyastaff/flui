use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/note/:id/copy/:id")]
    Note { id: u32 },
}

fn main() {}
