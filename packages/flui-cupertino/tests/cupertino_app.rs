//! Integration tests for [`CupertinoApp`] — theme publication and Apple's
//! brightness model (an optional theme override, ambient `MediaQuery`
//! otherwise; deliberately **no** `ThemeMode` — ADR-0042 §2), through
//! mounted trees.
//!
//! The live-republish test drives brightness through the same mechanism the
//! UI runtime's root `MediaQuery` uses in production (`flui-app`'s
//! `media_query_root.rs`): an owner-local shared cell re-published by a
//! stateful wrapper through the `RebuildHandle` it captured at mount
//! (ADR-0018).

#![expect(
    clippy::unwrap_used,
    reason = "a panic from `unwrap` is the test failure report (docs/PANIC-POLICY.md)"
)]

use crate::common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{
    CupertinoApp, CupertinoColor, CupertinoColors, CupertinoTheme, CupertinoThemeData,
};
use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, RebuildHandle};
use flui_sdk::widgets::SizedBox;

/// What a descendant of the shell observes: the effective brightness, the
/// resolved theme's primary color and text roles, and a dynamic color
/// resolved at the DESCENDANT's own altitude — the observation points
/// Apple's model distinguishes.
#[derive(Clone, Debug, PartialEq)]
struct Observed {
    brightness: Brightness,
    published_primary: Color,
    label_resolved_here: Color,
    text_color: Option<Color>,
    action_color: Option<Color>,
}

#[derive(Clone, Debug, StatelessView)]
struct Probe {
    captured: Arc<Mutex<Option<Observed>>>,
}

impl StatelessView for Probe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = CupertinoTheme::of(ctx);
        *self.captured.lock().unwrap() = Some(Observed {
            brightness: CupertinoTheme::brightness_of(ctx),
            published_primary: theme.primary_color,
            label_resolved_here: CupertinoColor::Dynamic(CupertinoColors::LABEL).resolve(ctx),
            text_color: theme.text_theme.text_style.color,
            action_color: theme.text_theme.action_text_style.color,
        });
        SizedBox::shrink()
    }
}

fn probe() -> (Probe, Arc<Mutex<Option<Observed>>>) {
    let captured = Arc::new(Mutex::new(None));
    (
        Probe {
            captured: Arc::clone(&captured),
        },
        captured,
    )
}

fn observed(cell: &Arc<Mutex<Option<Observed>>>) -> Observed {
    cell.lock()
        .unwrap()
        .clone()
        .expect("the probe below CupertinoApp must have built")
}

/// systemBlue light variant — tag-verified in `tests/colors.rs`.
const SYSTEM_BLUE_LIGHT: Color = Color::rgb(0, 122, 255);
/// systemBlue dark variant.
const SYSTEM_BLUE_DARK: Color = Color::rgb(10, 132, 255);
/// systemRed light variant.
const SYSTEM_RED_LIGHT: Color = Color::rgb(255, 59, 48);

pub fn publishes_the_resolved_theme_to_descendants() {
    let (probe, captured) = probe();
    let mut theme = CupertinoThemeData::default();
    theme.primary_color = Some(CupertinoColors::SYSTEM_RED.into());
    let _tree = lay_out(CupertinoApp::new(probe).theme(theme), loose(800.0));
    let seen = observed(&captured);
    assert_eq!(
        seen.published_primary, SYSTEM_RED_LIGHT,
        "the caller's theme — not a default — must reach descendants, already resolved"
    );
}

// ============================================================================
// Live brightness republish — the ui_runtime-source pattern
// ============================================================================

/// Test-side replica of the UI runtime's `MediaQuerySource` (`flui-app`'s
/// `media_query_root.rs`) — see `flui-material`'s `material_app.rs` tests
/// for the same pattern on the Material side.
#[derive(Default)]
struct BrightnessSource {
    data: RefCell<MediaQueryData>,
    rebuild: Cell<Option<RebuildHandle>>,
}

impl BrightnessSource {
    fn set_brightness(&self, brightness: Brightness) {
        self.data.borrow_mut().platform_brightness = brightness;
        if let Some(handle) = self.rebuild.take() {
            handle.schedule(flui_sdk::view::RebuildReason::StateChange);
            self.rebuild.set(Some(handle));
        }
    }
}

#[derive(Clone)]
struct BrightnessRoot {
    source: Rc<BrightnessSource>,
    child: BoxedView,
}

impl std::fmt::Debug for BrightnessRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrightnessRoot").finish_non_exhaustive()
    }
}

impl View for BrightnessRoot {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

struct BrightnessRootState {
    source: Rc<BrightnessSource>,
}

impl flui_sdk::view::StatefulView for BrightnessRoot {
    type State = BrightnessRootState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        BrightnessRootState {
            source: Rc::clone(&self.source),
        }
    }
}

impl flui_sdk::view::ViewState<BrightnessRoot> for BrightnessRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.source.rebuild.set(Some(ctx.rebuild_handle()));
    }

    fn build(&self, view: &BrightnessRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(self.source.data.borrow().clone(), view.child.clone())
    }
}

pub fn a_live_brightness_republish_re_resolves_the_theme() {
    let source = Rc::new(BrightnessSource::default());
    source.data.borrow_mut().platform_brightness = Brightness::Light;
    let (probe, captured) = probe();
    let mut tree = lay_out(
        BrightnessRoot {
            source: Rc::clone(&source),
            child: CupertinoApp::new(probe).into_view().boxed(),
        },
        loose(800.0),
    );
    assert_eq!(observed(&captured).published_primary, SYSTEM_BLUE_LIGHT);

    // The appearance flip: mutate the source, let the scheduled republish
    // rebuild — no root-dirtying pump.
    source.set_brightness(Brightness::Dark);
    tree.tick();
    let seen = observed(&captured);
    assert_eq!(seen.brightness, Brightness::Dark);
    assert_eq!(
        seen.published_primary, SYSTEM_BLUE_DARK,
        "a live platform-brightness change must re-materialize the published theme"
    );
}

pub fn explicit_app_brightness_applies_to_live_contrast_colors() {
    let source = Rc::new(BrightnessSource::default());
    let (probe, captured) = probe();
    let mut theme = CupertinoThemeData::default();
    theme.brightness = Some(Brightness::Dark);
    let mut tree = lay_out(
        BrightnessRoot {
            source: Rc::clone(&source),
            child: CupertinoApp::new(probe).theme(theme).boxed(),
        },
        loose(800.0),
    );
    assert_eq!(observed(&captured).published_primary, SYSTEM_BLUE_DARK);
    for contrast in [true, false] {
        source.data.borrow_mut().high_contrast = contrast;
        source.set_brightness(Brightness::Light);
        tree.tick();
        let seen = observed(&captured);
        assert_eq!(seen.brightness, Brightness::Dark);
        assert_eq!(
            seen.text_color,
            Some(Color::rgb(255, 255, 255)),
            "text roles use explicit dark brightness"
        );
        assert_eq!(
            seen.action_color,
            Some(if contrast {
                Color::rgb(64, 156, 255)
            } else {
                SYSTEM_BLUE_DARK
            })
        );
        assert_eq!(
            seen.published_primary,
            if contrast {
                Color::rgb(64, 156, 255)
            } else {
                SYSTEM_BLUE_DARK
            }
        );
    }
}

/// The app publishes its authored theme, so a nested `MediaQuery` that turns
/// the platform dark resolves the theme dark below it while the subtree
/// beside it, under the outer light media, stays light. Red-check: publish
/// the theme the app resolved in its own context.
pub fn a_nested_media_query_resolves_the_app_theme_below_it() {
    use flui_sdk::widgets::Column;

    let (outer, outer_seen) = probe();
    let (inner, inner_seen) = probe();
    let home = Column::new(vec![
        outer.boxed(),
        MediaQuery::new(
            MediaQueryData {
                platform_brightness: Brightness::Dark,
                ..MediaQueryData::default()
            },
            inner,
        )
        .boxed(),
    ]);
    let _tree = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoApp::new(home)),
        loose(800.0),
    );
    let outer = observed(&outer_seen);
    assert_eq!(
        (outer.brightness, outer.published_primary, outer.text_color),
        (Brightness::Light, SYSTEM_BLUE_LIGHT, Some(Color::BLACK)),
        "the outer subtree follows the outer light media"
    );
    let inner = observed(&inner_seen);
    assert_eq!(
        (inner.brightness, inner.published_primary, inner.text_color),
        (Brightness::Dark, SYSTEM_BLUE_DARK, Some(Color::WHITE)),
        "the theme resolves for the nested dark media below it"
    );
}
