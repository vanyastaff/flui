use flui_interaction::RecognizerSet;

fn main() {
    let recognizers = RecognizerSet::default();
    std::thread::spawn(move || drop(recognizers));
}
