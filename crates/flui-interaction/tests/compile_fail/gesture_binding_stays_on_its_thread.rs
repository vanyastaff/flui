use flui_interaction::GestureBinding;

fn main() {
    let binding = GestureBinding::new();
    std::thread::spawn(move || drop(binding));
}
