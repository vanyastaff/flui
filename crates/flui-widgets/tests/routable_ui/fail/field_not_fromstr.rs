use flui_widgets::Routable;

#[derive(Clone, PartialEq)]
struct NoteId(u32);

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/note/:id")]
    Note { id: NoteId },
}

fn main() {}
