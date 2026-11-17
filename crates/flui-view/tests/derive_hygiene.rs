//! The view derives name `std`, `core` and their runtime crates by absolute
//! path, so a view derives in a module that has items of those names, and
//! their generated `self` names the receiver when the derive and the fields
//! come from different `macro_rules!` contexts.

use flui_foundation::ValueKey;
use flui_view::{InheritedData, View};

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
fn a_view_derives_in_a_module_with_its_own_std_and_core() {
    let view: std::rc::Rc<dyn View> = std::rc::Rc::new(shadowed::Plain);
    assert!(view.key().is_none());

    let keyed: std::rc::Rc<dyn View> = std::rc::Rc::new(shadowed::Keyed {
        key: ValueKey::new(7),
    });
    assert!(keyed.key().is_some());
}

// The derive is written in the macro's body; the fields, and the key's type
// as plain tokens, come from its caller.
macro_rules! provider {
    ($name:ident { $($field:ident : $ty:ty),* }) => {
        #[derive(Clone, InheritedData)]
        struct $name {
            $($field: $ty),*
        }
    };
}

macro_rules! keyed_view {
    ($name:ident, $field:ident : $($key:tt)*) => {
        #[derive(Clone, flui_view::StatelessView)]
        struct $name {
            #[view(key)]
            $field: $($key)*,
        }

        impl flui_view::StatelessView for $name {
            fn build(
                &self,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl flui_view::IntoView {
                flui_view::ErrorView::new("a keyed view from a macro")
            }
        }
    };
}

provider!(Palette {
    accent: u32,
    surface: u32
});
keyed_view!(MacroRow, row_key: ValueKey<u32>);

#[test]
fn a_derive_reads_fields_a_macro_rules_caller_wrote() {
    let before = Palette {
        accent: 1,
        surface: 2,
    };
    let after = Palette {
        accent: 1,
        surface: 3,
    };
    let changed = after.field_mask_diff(&before);
    assert!(changed.intersects(Palette::FIELD_SURFACE));
    assert!(!changed.intersects(Palette::FIELD_ACCENT));

    let row: std::rc::Rc<dyn View> = std::rc::Rc::new(MacroRow {
        row_key: ValueKey::new(3),
    });
    assert!(row.key().is_some());
}
