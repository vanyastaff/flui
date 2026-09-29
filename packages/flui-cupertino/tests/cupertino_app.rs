//! Integration tests for [`CupertinoApp`] — theme publication and Apple's
//! brightness model (an optional theme override, ambient `MediaQuery`
//! otherwise; deliberately **no** `ThemeMode` — ADR-0042 §2), through
//! mounted trees.
//!
//! Flutter parity oracle: `cupertino/app.dart` `_CupertinoAppState.build`
//! (oracle tag `3.44.0`).
//!
//! The live-republish test drives brightness through the same mechanism the
//! realm's root `MediaQuery` uses in production (`flui-app`'s
//! `media_query_root.rs`): an owner-local shared cell re-published by a
//! stateful wrapper through the `RebuildHandle` it captured at mount
//! (ADR-0018).

// `unwrap` in test code: a panic IS the failure report (docs/PANIC-POLICY.md).
#![expect(clippy::unwrap_used)]

use crate::common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{
    CupertinoApp, CupertinoColor, CupertinoColors, CupertinoTheme, CupertinoThemeData,
};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, RebuildHandle};
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

/// What a descendant of the shell observes: the effective brightness, the
/// published theme's (already materialized) primary color, and a dynamic
/// color resolved at the DESCENDANT's own altitude — the three observation
/// points Apple's model distinguishes.
#[derive(Clone, Debug, PartialEq)]
struct Observed {
    brightness: Brightness,
    published_primary: CupertinoColor,
    label_resolved_here: Color,
}

#[derive(Clone, Debug, StatelessView)]
struct Probe {
    captured: Arc<Mutex<Option<Observed>>>,
}

impl StatelessView for Probe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Observed {
            brightness: CupertinoTheme::brightness_of(ctx),
            published_primary: CupertinoTheme::of(ctx).primary_color(),
            label_resolved_here: CupertinoColor::Dynamic(CupertinoColors::LABEL).resolve(ctx),
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
    let theme = CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
    let _tree = lay_out(CupertinoApp::new(probe).theme(theme), loose(800.0));
    let seen = observed(&captured);
    assert_eq!(
        seen.published_primary,
        CupertinoColor::Static(SYSTEM_RED_LIGHT),
        "the caller's theme — not a default — must reach descendants, already resolved"
    );
}

// ============================================================================
// Live brightness republish — the realm-source pattern
// ============================================================================

/// Test-side replica of the realm's `MediaQuerySource` (`flui-app`'s
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
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

struct BrightnessRootState {
    source: Rc<BrightnessSource>,
}

impl flui_sdk::view::StatefulView for BrightnessRoot {
    type State = BrightnessRootState;

    fn create_state(&self) -> Self::State {
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
    assert_eq!(
        observed(&captured).published_primary,
        CupertinoColor::Static(SYSTEM_BLUE_LIGHT)
    );

    // The appearance flip: mutate the source, let the scheduled republish
    // rebuild — no root-dirtying pump.
    source.set_brightness(Brightness::Dark);
    tree.tick();
    let seen = observed(&captured);
    assert_eq!(seen.brightness, Brightness::Dark);
    assert_eq!(
        seen.published_primary,
        CupertinoColor::Static(SYSTEM_BLUE_DARK),
        "a live platform-brightness change must re-materialize the published theme"
    );
}
