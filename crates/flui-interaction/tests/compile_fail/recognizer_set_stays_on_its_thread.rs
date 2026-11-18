use flui_interaction::gesture::RecognizerSet;

fn main() {
    let recognizers = RecognizerSet::default();
    std::thread::spawn(move || drop(recognizers));
}
