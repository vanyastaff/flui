//! The view derives name `std`, `core` and their runtime crates by absolute
//! path, so a view derives in a module that has items of those names.

use flui_foundation::ValueKey;
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

    // The generated `key` is where the expansion names `core`.
    #[derive(Clone, StatelessView)]
    pub(super) struct Keyed {
        #[view(key)]
        pub(super) key: flui_foundation::ValueKey<u32>,
    }

    impl StatelessView for Keyed {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            flui_view::ErrorView::new("a keyed view beside its own `core`")
        }
    }
}

#[test]
fn a_view_derives_in_a_module_with_its_own_std() {
    let view: std::rc::Rc<dyn View> = std::rc::Rc::new(shadowed::Plain);
    assert!(view.key().is_none());

    let keyed: std::rc::Rc<dyn View> = std::rc::Rc::new(shadowed::Keyed {
        key: ValueKey::new(7),
    });
    assert!(keyed.key().is_some());
}
