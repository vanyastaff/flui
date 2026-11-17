//! `#[view(...)]` where the derive could not act on it. `key` and `site` read
//! a field of `self`, which only a struct has, and `inspect` is about the
//! whole type; anywhere else the attribute is refused instead of ignored, so
//! a view never silently reports no key or no creation site.
use flui_foundation::{CreationSite, UniqueKey};
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

#[derive(Clone, StatelessView)]
struct TwoKeys {
    #[view(key)]
    first: UniqueKey,
    #[view(key)]
    second: UniqueKey,
}

#[derive(Clone, StatelessView)]
struct OnAParameter<#[view(site)] T> {
    value: T,
}

fn main() {}
