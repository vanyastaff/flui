use std::rc::Rc;

fn main() {
    let local = Rc::new(String::from("created on caller"));
    let _ = flui::open_dedicated_window(move || {
        let title = local.as_ref().clone();
        (flui::widgets::SizedBox::square(10.0), flui::RuntimeWindowConfig::new().with_title(title))
    });
}
