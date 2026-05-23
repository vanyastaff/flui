use flui_view::prelude::*;
#[derive(Clone, StatelessView)]
struct Greeting(String);
#[rustfmt::skip]
impl StatelessView for Greeting {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView { self.clone() }
}
