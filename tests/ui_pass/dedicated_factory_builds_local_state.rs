use std::rc::Rc;
use flui::view::prelude::*;

#[derive(Clone, StatelessView)]
struct LocalRoot { text: Rc<String> }
impl StatelessView for LocalRoot {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        flui::widgets::Text::new(self.text.as_str())
    }
}

fn main() {
    let text = String::from("only the factory crosses");
    let _ = flui::open_dedicated_window(move || (
        LocalRoot { text: Rc::new(text) }, flui::RuntimeWindowConfig::new(),
    ));
}
