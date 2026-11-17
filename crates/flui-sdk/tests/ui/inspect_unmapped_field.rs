use flui_sdk::foundation::inspect::Inspect;

struct Handle;

#[derive(Inspect)]
struct Card {
    title: String,
    handle: Handle,
}

// A variant's field is read through a binding the derive declares, so the
// error stands at the derive and labels the field.
#[derive(Inspect)]
enum Slot {
    Held { handle: Handle },
    Empty,
}

fn main() {}
