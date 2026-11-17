use flui_widgets::Routable;

#[derive(Routable, Clone, PartialEq)]
enum AppRoute<T> {
    #[route("/:id")]
    Item { id: T },
}

// A clause without a predicate has only its keyword to point at.
#[derive(Routable, Clone, PartialEq)]
enum Bounded where {
    #[route("/")]
    Home,
}

fn main() {}
