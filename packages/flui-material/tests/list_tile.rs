//! `ListTile` widget-level integration coverage — mounts a real `ListTile`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/ink_well.rs` use) and proves the whole-tile tap
//! target, the `enabled`/theme cascades, and slot presence/absence actually
//! reach a mounted tree, not just `resolve_style` computed in isolation.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{ListTile, Radio, Theme, ThemeData};
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MergeSemantics, Text};
use flui_testing::a11y::Role;

/// `ListTile::build` reads `SafeArea`, which panics without an ambient
/// `MediaQuery` (`tests/app_bar.rs`'s own tests wrap the same way) — every
/// test mounts under this default `MediaQueryData` (no system insets) so the
/// `Theme`/`ListTile` under test is the only thing varying per case.
fn themed(theme: ThemeData, child: impl IntoView) -> MediaQuery {
    MediaQuery::new(MediaQueryData::default(), Theme::new(theme, child))
}

/// A tap anywhere on the tile — including inside the content padding gutter,
/// well away from `title`'s own text glyphs — fires `on_tap`: the whole tile
/// is the tap target, not just the title: the tile wraps its ENTIRE content
/// (padding included) in a single `InkWell`, so the test probes the mounted
/// surface rather than the title's own bounds.
pub fn whole_tile_tap_fires_from_a_point_inside_the_content_padding() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            ListTile::new()
                .title(Text::new("Inbox"))
                .on_tap(move |_ctx| {
                    counted.fetch_add(1, Ordering::SeqCst);
                }),
        ),
        tight(400.0, 56.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");
    let origin = laid.absolute_offset(material);

    // `_LisTileDefaultsM3.contentPadding` starts at `left: 16.0` — a point
    // 2px from the tile's left edge sits inside that padding gutter, well
    // before any title glyph begins.
    laid.dispatch_pointer_down(origin.dx + 2.0, origin.dy + 2.0);
    laid.dispatch_pointer_up(origin.dx + 2.0, origin.dy + 2.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap inside the content padding (not on the title's own glyphs) must still fire \
         on_tap — the whole mounted tile, not just the title, is the tap target"
    );
}

/// `MergeSemantics` over the same tile is the reference's own composition —
/// `RadioListTile` wraps its `ListTile` in `MergeSemantics` (`material/
/// radio_list_tile.dart`, oracle tag `3.44.0`) so the whole tile is one
/// interactive entity — and here the tile and the radio share **one** node,
/// as `test/material/radio_list_tile_test.dart`'s `testWidgets('RadioListTile
/// semantics')` asserts of the reference. The forced merge meets the tile's
/// `Button` role and the radio's `RadioButton` role, so `more_specific_role`
/// is load-bearing for it: the checkable role must outrank `Button`.
///
/// The roles beyond the root are asserted exactly rather than with `contains`,
/// because the claim is the node *count*: a second node would mean the merge
/// did not happen, and a `Button` in its place would mean the precedence
/// regressed. The root's own role is the fallback for a role-free node and is
/// not this test's claim, so it is split off rather than pinned. What the merged
/// node still lacks against the oracle — its `'Title'` label, its `tap` action
/// — is recorded in `crates/flui-semantics/ARCHITECTURE.md`, not asserted here.
pub fn merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button() {
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            MergeSemantics::new().child(
                ListTile::new()
                    .leading(Radio::new("spring", Some("spring")).on_changed(|_ctx, _| {}))
                    .title(Text::new("Spring"))
                    .on_tap(|_ctx| {}),
            ),
        ),
        tight(400.0, 56.0),
    );
    laid.enable_semantics();
    laid.pump();

    let roles: Vec<Role> = laid
        .a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect();

    let (_root, rest) = roles
        .split_first()
        .expect("the a11y tree always carries its root");
    assert_eq!(
        rest,
        [Role::RadioButton],
        "beyond the root, exactly one merged node for the tile and its radio, resolving \
         RadioButton: a Button here means the checkable roles fell below Button again, \
         and a second node means the tile and the radio stopped merging. Roles: {roles:?}"
    );
}

/// A selectable tile is an option, the role ARIA gives a selection, whether or
/// not it is selected, so its role does not change as selection toggles; a
/// tile that is not selectable declares no selection, even while highlighted.
/// Neither drops a state its role does not carry (the harness fails a frame
/// that does).
pub fn a_selectable_tile_is_an_option_whether_or_not_selected() {
    use flui_sdk::widgets::{Semantics, SemanticsRole};

    for (selectable, selected, role, announced) in [
        (true, true, Role::ListBoxOption, Some(true)),
        (true, false, Role::ListBoxOption, Some(false)),
        (false, true, Role::Button, None),
    ] {
        let mut laid = lay_out(
            themed(
                ThemeData::light(),
                Semantics::new()
                    .container(true)
                    .role(SemanticsRole::ListBox)
                    .child(
                        ListTile::new()
                            .title(Text::new("Inbox"))
                            .selectable(selectable)
                            .selected(selected)
                            .on_tap(|_ctx| {}),
                    ),
            ),
            tight(400.0, 56.0),
        );
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let tile = tree
            .nodes()
            .find(|node| node.role() == role)
            .unwrap_or_else(|| panic!("a {role:?} for the tile. Tree:\n{}", tree.describe()));
        assert_eq!(
            tile.raw().is_selected(),
            announced,
            "selectable: {selectable}, selected: {selected}"
        );
    }
}

/// A tile is disabled only when it says so, and operable only with a
/// handler as well: an enabled tile without one is not disabled, offers no
/// click and takes no focus. A tile is one node of its own, so its state
/// stays on it and never reaches the list it sits in.
///
/// Red-check: take the well's switch from its handler alone (the
/// `ListTile`'s `enabled` ignored), and the enabled tile without a handler
/// is disabled.
pub fn a_tile_is_disabled_only_when_it_says_so_and_operable_with_a_handler() {
    use flui_sdk::widgets::{Semantics, SemanticsRole};
    use flui_testing::a11y::Action;

    for (enabled, handler) in [(true, true), (true, false), (false, true), (false, false)] {
        let taps = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&taps);
        let mut tile = ListTile::new().title(Text::new("Inbox")).enabled(enabled);
        if handler {
            tile = tile.on_tap(move |_ctx| {
                counted.fetch_add(1, Ordering::SeqCst);
            });
        }
        let mut laid = lay_out(
            themed(
                ThemeData::light(),
                Semantics::new()
                    .container(true)
                    .role(SemanticsRole::ListBox)
                    .child(tile),
            ),
            tight(400.0, 56.0),
        );
        laid.enable_semantics();
        laid.pump();
        let case = format!("enabled: {enabled}, handler: {handler}");
        {
            let tree = laid
                .a11y_tree()
                .expect("semantics enabled before the frame");
            let node = tree
                .find_by_label("Inbox")
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            assert_ne!(
                node.role(),
                Role::ListBox,
                "{case}: the tile is its own node"
            );
            assert_eq!(node.is_disabled(), !enabled, "{case}: disabled");
            let operable = enabled && handler;
            assert_eq!(
                node.supports_action(Action::Click),
                operable,
                "{case}: click"
            );
            assert_eq!(
                node.supports_action(Action::Focus),
                operable,
                "{case}: focus"
            );
            let list = tree.find(Role::ListBox).expect("the list box");
            assert!(!list.is_disabled(), "{case}: the list is never disabled");
        }
        laid.dispatch_pointer_down(2.0, 2.0);
        laid.dispatch_pointer_up(2.0, 2.0);
        assert_eq!(
            taps.load(Ordering::SeqCst),
            usize::from(enabled && handler),
            "{case}: taps"
        );
    }
}

/// A tile turned off and on with its handler in place: off, it is disabled
/// and does not run the handler; on again, it does. No element builds twice
/// in the frame that turns it: the well seeds `Disabled` before its build, so
/// the node's write of the same value during the build notifies nobody, and
/// the well is not built again for it.
///
/// Red-check: drop the seed in `InkWell::did_update_view` (the switch
/// changed, the controller kept), and the frame runs one build more than the
/// elements it builds.
pub fn turning_a_tile_off_and_on_keeps_its_handler_and_builds_the_well_once() {
    let enabled = flui_testing::widgets::SharedSignal::new(true);
    let taps = Arc::new(AtomicUsize::new(0));
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            SwitchedTile {
                enabled: enabled.clone(),
                taps: Arc::clone(&taps),
            },
        ),
        tight(400.0, 56.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tap = |laid: &mut flui_testing::widgets::LaidOut| {
        laid.dispatch_pointer_down(2.0, 2.0);
        laid.dispatch_pointer_up(2.0, 2.0);
        laid.pump();
    };
    tap(&mut laid);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "an enabled tile runs its handler"
    );

    for (on, taps_after) in [(false, 1), (true, 2)] {
        enabled.set(on);
        let report = laid.pump_report(std::time::Duration::from_millis(16)).build;
        assert_eq!(
            report.builds_run, report.elements_built,
            "on: {on}: no element builds twice in the frame that turns the tile: {report:?}"
        );
        let disabled = laid
            .a11y_tree()
            .expect("semantics enabled before the frame")
            .find_by_label("Inbox")
            .expect("the tile")
            .is_disabled();
        assert_eq!(disabled, !on, "on: {on}: disabled");
        tap(&mut laid);
        assert_eq!(taps.load(Ordering::SeqCst), taps_after, "on: {on}: taps");
    }
}

/// A `ListTile` whose switch a test turns, its handler kept.
#[derive(Clone, StatefulView)]
struct SwitchedTile {
    enabled: flui_testing::widgets::SharedSignal<bool>,
    taps: Arc<AtomicUsize>,
}

struct SwitchedTileState {
    enabled: flui_testing::widgets::SharedSignal<bool>,
}

impl StatefulView for SwitchedTile {
    type State = SwitchedTileState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SwitchedTileState {
            enabled: self.enabled.clone(),
        }
    }
}

impl ViewState<SwitchedTile> for SwitchedTileState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.enabled.bind(ctx);
    }

    fn build(&self, view: &SwitchedTile, ctx: &dyn BuildContext) -> impl IntoView {
        let taps = Arc::clone(&view.taps);
        ListTile::new()
            .title(Text::new("Inbox"))
            .enabled(self.enabled.get(ctx))
            .on_tap(move |_ctx| {
                taps.fetch_add(1, Ordering::SeqCst);
            })
    }
}
