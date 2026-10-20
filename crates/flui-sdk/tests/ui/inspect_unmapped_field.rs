use flui_sdk::foundation::inspect::Inspect;

struct Handle;

#[derive(Inspect)]
struct Card {
    title: String,
    handle: Handle,
}

fn main() {}
