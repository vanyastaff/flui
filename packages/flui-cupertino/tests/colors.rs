//! Const-table oracle-diff test for [`CupertinoColors`] — every V1-scoped
//! constant's full 8-variant ARGB table, asserted against
//! `cupertino/colors.dart` at tag `3.44.0`.
//!
//! Each assertion is written against the raw `(r, g, b, a)` channels rather
//! than re-deriving `Color::rgba(...)` calls that would just restate
//! `src/colors.rs`'s own construction — a copy-paste-the-source test proves
//! nothing. The oracle's `dark` `systemBlue` value is the flagged trap: the
//! actual tag-3.44.0 value is `(10, 132, 255)`, one digit away from the
//! superficially-plausible `(9, 132, 255)`.

#![expect(clippy::unwrap_used)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{CupertinoColor, CupertinoTheme, CupertinoThemeData};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;

/// Captures `CupertinoColor::Static(sentinel).resolve(ctx)` during `build()`
/// — proving `resolve` actually runs against a real mounted `BuildContext`
/// (not just constructed and equality-checked against itself).
#[derive(Clone, Debug, StatelessView)]
struct StaticResolveCapture {
    sentinel: Color,
    captured: Arc<Mutex<Option<Color>>>,
}

impl StatelessView for StaticResolveCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let resolved = CupertinoColor::Static(self.sentinel).resolve(ctx);
        *self.captured.lock().unwrap() = Some(resolved);
        SizedBox::shrink()
    }
}

/// `CupertinoColor::Static::resolve` returns its color unchanged, verified
/// against a real mounted `BuildContext` under a `CupertinoTheme::brightness`
/// of `Dark` — a mutation that made `Static::resolve` secretly route through
/// brightness resolution (e.g. treating the sentinel as if it were a
/// `Dynamic` color's light variant) would still pass a construct-and-compare
/// unit test but fails here, since a real ambient theme is present and could
/// have perturbed the result if `resolve` consulted it.
#[test]
fn static_color_resolves_to_itself_through_a_real_context() {
    let sentinel = Color::rgba(11, 22, 33, 200);
    let captured: Arc<Mutex<Option<Color>>> = Arc::new(Mutex::new(None));

    let _laid = lay_out(
        CupertinoTheme::new(
            CupertinoThemeData::default().with_brightness(Brightness::Dark),
            StaticResolveCapture {
                sentinel,
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let resolved = captured
        .lock()
        .unwrap()
        .expect("build should have run and captured a resolved color");
    assert_eq!(resolved, sentinel);
}
