//! A widget made by its public constructors reports the line that called the constructor, not a
//! line inside the widget's own file, whichever constructor made it.

use flui_foundation::CreationSite;
use flui_foundation::geometry::EdgeInsets;
use flui_view::View;
use flui_widgets::prelude::*;

/// Where the widget says it was made, as `(file, line)`.
fn made_at(view: &dyn View) -> Option<(&'static str, u32)> {
    let site = view.creation_site();
    Some((site.file()?, site.line()?))
}

/// Each constructor of `Padding` records its caller.
///
/// Fails when a constructor that delegates to another loses `#[track_caller]`: the site then
/// names a line in `padding.rs`.
#[test]
fn every_padding_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let cases: [(u32, Padding); 4] = [
        (line!(), Padding::new(EdgeInsets::all(1.0))),
        (line!(), Padding::all(1.0)),
        (line!(), Padding::symmetric(1.0, 2.0)),
        (line!(), Padding::only(1.0, 2.0, 3.0, 4.0)),
    ];
    for (line, padding) in cases {
        let (file, at) = made_at(&padding).expect("a constructor records a site");
        assert!(
            file.ends_with("creation_site.rs"),
            "{file}:{at} is not the caller"
        );
        assert_eq!(at, line);
    }
}
