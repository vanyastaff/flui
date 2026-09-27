use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute<T> {
    #[route("/:id")]
    Item { id: T },
}

fn main() {}
