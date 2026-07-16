//! Integration tests for [`CupertinoTheme::of`]/brightness resolution —
//! proving the resolve chain actually reaches through a mounted
//! [`BuildContext`], not just the pure data-model unit tests in
//! `src/theme.rs`/`src/colors.rs`.
//!
//! Mutation-honest by construction: `CupertinoTheme::of` calling
//! `CupertinoThemeData::default()` instead of reading the ancestor, or
//! `resolve_from` returning `self` unchanged instead of actually resolving,
//! would make [`primary_color_flips_with_explicit_theme_brightness`] and
//! [`explicit_theme_brightness_overrides_media_query`] observe the wrong
//! color/root.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{CupertinoColors, CupertinoTheme, CupertinoThemeData};
use flui_types::Color;
use flui_types::platform::Brightness;
use flui_view::prelude::*;
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

/// Captures the resolved (concrete, ambient-aware) primary color read via
/// [`CupertinoTheme::of`] during `build()`.
#[derive(Clone, Debug, StatelessView)]
struct PrimaryColorCapture {
    captured: Arc<Mutex<Option<Color>>>,
}

impl StatelessView for PrimaryColorCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let resolved = CupertinoTheme::of(ctx).primary_color().resolve(ctx);
        *self.captured.lock().unwrap() = Some(resolved);
        SizedBox::shrink()
    }
}

fn mount_and_capture(root_builder: impl FnOnce(PrimaryColorCapture) -> BoxedView) -> Color {
    let captured: Arc<Mutex<Option<Color>>> = Arc::new(Mutex::new(None));
    let capture = PrimaryColorCapture {
        captured: Arc::clone(&captured),
    };
    let root = root_builder(capture);
    let _laid = lay_out(root, loose(100.0));
    captured
        .lock()
        .unwrap()
        .expect("build should have run and captured a resolved color")
}

/// `CupertinoTheme::of` with no ancestor still resolves — falls back to
/// `CupertinoThemeData::default()` (systemBlue), then resolves it (light,
/// with no `MediaQuery` ancestor either).
#[test]
fn of_with_no_ancestor_resolves_the_default_theme() {
    let color = mount_and_capture(ViewExt::boxed);
    assert_eq!(color, Color::rgb(0, 122, 255));
}

/// `CupertinoTheme::of` returns the ANCESTOR's data, resolved — not a
/// default. Mutation-honest: swapping in `CupertinoThemeData::default()`
/// here would still pass `of_with_no_ancestor_resolves_the_default_theme`
/// above but fail this one, since `SYSTEM_RED` differs from the default
/// `SYSTEM_BLUE`.
#[test]
fn of_returns_the_ancestor_theme_resolved_not_a_default() {
    let provided = CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
    let color = mount_and_capture(|capture| CupertinoTheme::new(provided, capture).boxed());
    // Light mode (no brightness set anywhere): SYSTEM_RED's light variant.
    assert_eq!(color, Color::rgb(255, 59, 48));
}

/// Brightness root #1: an explicit `CupertinoThemeData::brightness` flips the
/// resolved primary color to its dark variant, with no `MediaQuery` ancestor
/// at all.
#[test]
fn primary_color_flips_with_explicit_theme_brightness() {
    let provided = CupertinoThemeData::default().with_brightness(Brightness::Dark);
    let color = mount_and_capture(|capture| CupertinoTheme::new(provided, capture).boxed());
    // systemBlue dark variant — the tag-verified (10, 132, 255), not the
    // superficially-plausible (9, 132, 255) a from-memory port would land on.
    assert_eq!(color, Color::rgb(10, 132, 255));
}

/// Brightness root #2: with no `CupertinoTheme::brightness` set, the ambient
/// `MediaQuery::platform_brightness` is the fallback root — full oracle
/// parity for `CupertinoDynamicColor.resolveFrom`'s
/// `CupertinoTheme.maybeBrightnessOf ?? MediaQuery.maybePlatformBrightnessOf`
/// chain.
#[test]
fn primary_color_flips_with_ambient_media_query_brightness_when_theme_is_silent() {
    let color = mount_and_capture(|capture| {
        MediaQuery::new(
            MediaQueryData {
                platform_brightness: Brightness::Dark,
                ..MediaQueryData::default()
            },
            CupertinoTheme::new(CupertinoThemeData::default(), capture),
        )
        .boxed()
    });
    assert_eq!(color, Color::rgb(10, 132, 255));
}

/// An explicit `CupertinoThemeData::brightness` takes precedence over a
/// conflicting ambient `MediaQuery::platform_brightness` — the oracle's
/// `brightness ?? MediaQuery...` chain short-circuits on the theme's own
/// value, never consulting `MediaQuery` at all when it is set.
#[test]
fn explicit_theme_brightness_overrides_media_query() {
    let color = mount_and_capture(|capture| {
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
    assert_eq!(color, Color::rgb(0, 122, 255));
}
