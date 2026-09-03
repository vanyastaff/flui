//! A plain struct handed to a view slot. The error must point the author at
//! `#[derive(Clone, StatelessView)]` + `impl StatelessView`, not at the
//! `IntoView` blanket impl the compiler walked through to get there.
use flui_view::IntoView;

struct NotAView;

fn takes_view(_view: impl IntoView) {}

fn main() {
    takes_view(NotAView);
}
