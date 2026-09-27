use std::fmt;

use flui_widgets::Routable;

/// Prints, so the only missing bound is `FromStr`.
#[derive(Clone, PartialEq)]
struct NoteId(u32);

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/note/:id")]
    Note { id: NoteId },
}

fn main() {}
