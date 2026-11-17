//! `#[view(key)]` on a field that holds no key: the derive reads the field
//! through `AsViewKey`, so the error is at the field's type and names the
//! shapes a key field takes.
use flui_view::prelude::*;

#[derive(Clone, StatelessView)]
struct Row {
    #[view(key)]
    key: String,
}

impl StatelessView for Row {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_view::ErrorView::new("row")
    }
}

fn main() {}
