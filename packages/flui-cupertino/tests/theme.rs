//! Integration tests for [`CupertinoTheme::of`]/brightness resolution —
//! proving the resolve chain actually reaches through a mounted
//! [`BuildContext`], not just the pure data-model unit tests in
//! `src/theme.rs`/`src/colors.rs`.
//!
//! Every test captures [`CupertinoTheme::of`]'s result **without** an extra
//! `.resolve(ctx)` call on top — asserting the captured
//! [`CupertinoColor`](flui_cupertino::CupertinoColor) is *already*
//! `Static(expected)`. Calling `.resolve(ctx)` again in the test would mask
//! two distinct mutants: `CupertinoTheme::of` returning the ancestor's data
//! without calling `resolve_from` at all (still `Dynamic(...)`, but a
//! same-context re-resolve would silently fix it up to the right color
//! anyway), and `CupertinoThemeData::resolve_from` returning `self`
//! unchanged (ditto). Asserting the *unresolved-looking* enum variant
//! (`Static`, not `Dynamic`) — not just the final color — is what makes both
//! mutants observable.

#![expect(clippy::unwrap_used)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{CupertinoColor, CupertinoTheme, CupertinoThemeData};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

/// Captures `CupertinoTheme::of(ctx).primary_color()` — with no further
/// resolution — during `build()`.
#[derive(Clone, Debug, StatelessView)]
struct PrimaryColorCapture {
    captured: Arc<Mutex<Option<CupertinoColor>>>,
}

impl StatelessView for PrimaryColorCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let primary_color = CupertinoTheme::of(ctx).primary_color();
        *self.captured.lock().unwrap() = Some(primary_color);
        SizedBox::shrink()
    }
}

fn mount_and_capture(
    root_builder: impl FnOnce(PrimaryColorCapture) -> BoxedView,
) -> CupertinoColor {
    let captured: Arc<Mutex<Option<CupertinoColor>>> = Arc::new(Mutex::new(None));
    let capture = PrimaryColorCapture {
        captured: Arc::clone(&captured),
    };
    let root = root_builder(capture);
    let _laid = lay_out(root, loose(100.0));
    captured
        .lock()
        .unwrap()
        .expect("build should have run and captured a primary color")
}

/// An explicit `CupertinoThemeData::brightness` takes precedence over a
/// conflicting ambient `MediaQuery::platform_brightness` — the oracle's
/// `brightness ?? MediaQuery...` chain short-circuits on the theme's own
/// value, never consulting `MediaQuery` at all when it is set.
pub fn explicit_theme_brightness_overrides_media_query() {
    let primary_color = mount_and_capture(|capture| {
        MediaQuery::new(
            MediaQueryData {
                platform_brightness: Brightness::Dark,
                ..MediaQueryData::default()
            },
            CupertinoTheme::new(
                CupertinoThemeData::default().with_brightness(Brightness::Light),
                capture,
            ),
        )
        .boxed()
    });
    // Light, not dark — the theme's explicit brightness won, not the
    // (conflicting) ambient MediaQuery.
    assert_eq!(
        primary_color,
        CupertinoColor::Static(Color::rgb(0, 122, 255))
    );
}
