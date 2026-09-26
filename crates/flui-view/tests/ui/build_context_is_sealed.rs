//! `BuildContext` is sealed: a downstream context cannot forward the untyped
//! field set to a different provider `TypeId`. `Mine` implements the
//! `ReadScope` supertrait, so the only errors left are the seal and the
//! missing methods, not a list of `ReadScope` implementors.
struct Mine;

impl flui_view::ReadScope for Mine {
    fn scope(&self) -> flui_view::ScopeRef<'_> {
        unimplemented!()
    }
}

impl flui_view::BuildContext for Mine {}

fn main() {}
