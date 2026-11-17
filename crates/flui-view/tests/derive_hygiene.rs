//! The view derives name `std` and their runtime crates by absolute path, so
//! a view derives in a module that has items of those names.

use flui_view::View;

mod shadowed {
    use flui_view::prelude::*;

    // What a relative `std::rc::Rc` or `core::option::Option` in the
    // expansion would resolve to here.
    mod std {}
    mod core {}

    #[derive(Clone, StatelessView)]
    pub(super) struct Plain;

    impl StatelessView for Plain {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            flui_view::ErrorView::new("a view beside its own `std`")
        }
    }
}

#[test]
fn a_view_derives_in_a_module_with_its_own_std() {
    let view: std::rc::Rc<dyn View> = std::rc::Rc::new(shadowed::Plain);
    assert!(view.key().is_none());
}
