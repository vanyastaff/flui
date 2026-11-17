//! `#[view(...)]` where the derive could not act on it. `site` reads a field
//! of `self`, which only a struct has, and `inspect` is about the whole type;
//! anywhere else the attribute is refused instead of ignored, so a view never
//! silently reports no creation site.
use flui_foundation::CreationSite;
use flui_view::prelude::*;

#[derive(Clone, StatelessView)]
enum OnAVariantsField {
    Shown {
        #[view(site)]
        site: CreationSite,
    },
}

#[derive(Clone, StatefulView)]
#[view(site)]
struct OnTheType {
    site: CreationSite,
}

#[derive(Clone, StatelessView)]
struct OnAField {
    #[view(inspect)]
    site: CreationSite,
}

#[derive(Clone, StatelessView)]
struct Twice {
    #[view(site)]
    first: CreationSite,
    #[view(site)]
    second: CreationSite,
}

#[derive(Clone, StatelessView)]
struct OneFieldTwice {
    #[view(site, site)]
    site: CreationSite,
}

#[derive(Clone, StatelessView)]
#[view(inspect)]
#[view(inspect)]
struct InspectTwice;

#[derive(Clone, StatelessView)]
#[view()]
struct NamesNothing;

#[derive(Clone, StatelessView)]
enum SpelledRaw {
    #[r#view(inspect)]
    Shown,
}

fn main() {}
