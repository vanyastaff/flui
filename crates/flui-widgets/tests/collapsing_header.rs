//! `CollapsingHeader` (ADR-0209, a participant of the nested scroll
//! channel): rows of the `nested_scroll_channel` table in `contracts.rs`,
//! each pointer row driven by touch and mouse scripts.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use flui_animation::Vsync;
use flui_interaction::RemainderHandoff;
use flui_interaction::focus::FocusNode;
use flui_interaction::{GestureProfiles, GestureSettings, GestureSettingsSource, GestureValues};
use flui_testing::a11y::{Action, ActionRequest, Role, TreeId};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    CollapsingHeader, Column, Focus, HeaderExtents, HeaderExtentsError, HeaderOrder, NestedScroll,
    PageSnap, ScrollCause, ScrollController, ScrollEndReason, Scrollable, Semantics, SemanticsRole,
    SizedBox, VsyncScope,
};

use crate::common::LaidOut;
use crate::nested_scroll::{DEVICES, Probe, Seen, at, fling, mounted, settle, slow};

/// The header's extents in every row but the degenerate one.
const COLLAPSED: f64 = 56.0;
const EXPANDED: f64 = 200.0;
/// The rows of the body, each this tall.
const ROWS: usize = 8;
const ROW: f64 = 100.0;

/// A 300×300 window: a header of `extents` with a "Compose" button near its
/// bottom, above a list of labelled, focusable rows, with `probe` above
/// both.
struct Screen {
    laid: LaidOut,
    list: ScrollController,
    /// The body's geometry: its top is the header's extent.
    body: Rc<FocusNode>,
    rows: Vec<Rc<FocusNode>>,
    compose: Rc<FocusNode>,
    probe: Rc<Probe>,
}

impl Screen {
    fn new(
        extents: HeaderExtents,
        configure: impl FnOnce(CollapsingHeader) -> CollapsingHeader,
    ) -> Self {
        Self::under(extents, configure, None)
    }

    fn under(
        extents: HeaderExtents,
        configure: impl FnOnce(CollapsingHeader) -> CollapsingHeader,
        presets: Option<GestureProfiles>,
    ) -> Self {
        let (list, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let body = FocusNode::with_debug_label("body");
        let compose = FocusNode::with_debug_label("compose");
        let rows: Vec<Rc<FocusNode>> = (0..ROWS)
            .map(|_| FocusNode::with_debug_label("row"))
            .collect();
        let header = Column::new(vec![
            SizedBox::new(300.0, 150.0).boxed(),
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .label("Compose")
                .child(Focus::new(SizedBox::new(300.0, 40.0)).focus_node(Rc::clone(&compose)))
                .boxed(),
            SizedBox::new(300.0, 10.0).boxed(),
        ]);
        let content = Column::new(
            rows.iter()
                .enumerate()
                .map(|(index, node)| {
                    Semantics::new()
                        .container(true)
                        .label(format!("Row {index}"))
                        .child(Focus::new(SizedBox::new(300.0, ROW)).focus_node(Rc::clone(node)))
                        .boxed()
                })
                .collect::<Vec<_>>(),
        );
        let scrollable = Focus::new(Scrollable::new().controller(list.clone()).child(content))
            .focus_node(Rc::clone(&body))
            .can_request_focus(false)
            .skip_traversal(true);
        let header =
            configure(CollapsingHeader::new(extents, header, scrollable)).semantic_label("Inbox");
        let tree: BoxedView = NestedScroll::new(probe.connection(), header).boxed();
        let tree = match presets {
            Some(presets) => crate::scroll::FlingProfile {
                provider: GestureSettingsSource::new(presets).provider(),
                child: tree,
            }
            .boxed(),
            None => tree,
        };
        let mut laid = mounted(VsyncScope::new(vsync.clone(), tree).boxed(), vsync);
        laid.tick();
        Self {
            laid,
            list,
            body,
            rows,
            compose,
            probe,
        }
    }

    fn extent(&self) -> f64 {
        self.body.rect().top()
    }

    fn row_top(&self, index: usize) -> f64 {
        self.rows[index].rect().top()
    }

    fn settle(&mut self) {
        settle(&mut self.laid);
    }

    /// A drag that ends at rest, then everything settles.
    fn drag_up(&mut self, device: crate::nested_scroll::Device, distance: f64) {
        self.laid
            .replay(&slow(device, at(150.0, 290.0), at(150.0, 290.0 - distance)));
        self.settle();
    }

    fn drag_down(&mut self, device: crate::nested_scroll::Device, distance: f64) {
        self.laid
            .replay(&slow(device, at(150.0, 290.0 - distance), at(150.0, 290.0)));
        self.settle();
    }

    fn act(&self, action: Action, node: flui_testing::a11y::NodeId) {
        self.laid
            .invoke_semantics_action(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: node,
                data: None,
            })
            .expect("the request resolves");
    }

    fn node(&self, label: &str) -> flui_testing::a11y::NodeId {
        self.laid
            .a11y_tree()
            .expect("published semantics")
            .find_by_label(label)
            .unwrap_or_else(|error| panic!("{label}: {error:?}"))
            .id()
    }
}

fn extents() -> HeaderExtents {
    HeaderExtents::new(COLLAPSED, EXPANDED).expect("ordered, finite extents")
}

fn near(value: f64, expected: f64) -> bool {
    (value - expected).abs() <= 0.5
}

/// A scroll toward the content's end collapses the header before the
/// content moves by default, and only once the content has reached its end
/// with `AfterContent`, in a later sequence under the default latch.
pub(crate) fn collapsing_header_collapses_before_or_after_the_content() {
    for device in DEVICES {
        let mut screen = Screen::new(extents(), |header| header);
        let list = screen.list.clone();
        let body = Rc::clone(&screen.body);
        let moved_early = Cell::new(false);
        screen.laid.replay_with(
            &slow(device, at(150.0, 290.0), at(150.0, 40.0)),
            |laid, _| {
                laid.tick();
                if list.pixels() > 0.0 && body.rect().top() > COLLAPSED + 0.5 {
                    moved_early.set(true);
                }
            },
        );
        screen.settle();
        assert!(
            near(screen.extent(), COLLAPSED),
            "{device:?}: the drag collapsed the header, at {}",
            screen.extent()
        );
        assert!(
            screen.list.pixels() > 50.0,
            "{device:?}: and moved the list with the rest, at {}",
            screen.list.pixels()
        );
        assert!(
            !moved_early.get(),
            "{device:?}: the list waited until the header had collapsed"
        );

        let mut screen = Screen::new(extents(), |header| {
            header.collapse(HeaderOrder::AfterContent)
        });
        screen.drag_up(device, 200.0);
        assert!(
            screen.list.pixels() > 150.0,
            "{device:?}: after the content, the list moves first, at {}",
            screen.list.pixels()
        );
        assert_eq!(screen.extent(), EXPANDED, "{device:?}: the header waits");
        let end = ROWS as f64 * ROW - (300.0 - EXPANDED);
        screen.list.jump_to(end);
        screen.laid.tick();
        screen.drag_up(device, 100.0);
        assert!(
            screen.extent() < EXPANDED - 50.0,
            "{device:?}: at the content's end the next scroll collapses the header, at {}",
            screen.extent()
        );
    }
}

/// A scroll toward the start expands the header only once the content is
/// back at its start by default, and before the content moves with
/// `BeforeContent`; under the mid-drag hand-off preset the header expands
/// within the same contact once the content reaches its start.
pub(crate) fn collapsing_header_expands_first_or_last() {
    for device in DEVICES {
        for order in [HeaderOrder::AfterContent, HeaderOrder::BeforeContent] {
            let mut screen = Screen::new(extents(), |header| header.expand(order));
            screen.drag_up(device, 250.0);
            assert!(
                near(screen.extent(), COLLAPSED),
                "{device:?}/{order:?}: collapsed"
            );
            let scrolled = screen.list.pixels();
            assert!(
                scrolled > 50.0,
                "{device:?}/{order:?}: the list moved, at {scrolled}"
            );
            screen.drag_down(device, 40.0);
            match order {
                HeaderOrder::AfterContent => {
                    assert!(
                        screen.list.pixels() < scrolled - 10.0,
                        "{device:?}: the list moves back first, at {}",
                        screen.list.pixels()
                    );
                    assert!(
                        near(screen.extent(), COLLAPSED),
                        "{device:?}: while the header stays collapsed, at {}",
                        screen.extent()
                    );
                    screen.list.jump_to(0.0);
                    screen.laid.tick();
                    screen.drag_down(device, 100.0);
                    assert!(
                        screen.extent() > COLLAPSED + 50.0,
                        "{device:?}: at the content's start the header expands, at {}",
                        screen.extent()
                    );
                }
                HeaderOrder::BeforeContent => {
                    assert_eq!(
                        screen.list.pixels(),
                        scrolled,
                        "{device:?}: the list waits while the header expands"
                    );
                    assert!(
                        screen.extent() > COLLAPSED + 10.0,
                        "{device:?}: the header expands first, at {}",
                        screen.extent()
                    );
                }
            }
        }

        let preset = GestureProfiles::uniform(
            GestureSettings::try_from_values(GestureValues {
                remainder_handoff: RemainderHandoff::AtEdgeWithinSequence,
                ..GestureSettings::default().values()
            })
            .expect("the default values with another remainder rule"),
        );
        let mut screen = Screen::under(extents(), |header| header, Some(preset));
        screen.drag_up(device, 200.0);
        let scrolled = screen.list.pixels();
        assert!(near(screen.extent(), COLLAPSED) && scrolled > 20.0);
        screen.drag_down(device, scrolled + 100.0);
        assert_eq!(
            screen.list.pixels(),
            0.0,
            "{device:?}: the list reached its start"
        );
        assert!(
            screen.extent() > COLLAPSED + 50.0,
            "{device:?}: and the same contact went on to expand the header, at {}",
            screen.extent()
        );
    }
}

/// A fling collapses the header with its own steps before it moves the
/// list, all under the sequence of the contact that released it; a fling
/// toward the start that the list cannot carry past its edge carries on
/// into a header that expands after the content, under the same sequence.
pub(crate) fn collapsing_header_inherits_the_fling_across_its_collapse() {
    for device in DEVICES {
        let mut screen = Screen::new(extents(), |header| header);
        screen
            .laid
            .replay(&fling(device, at(150.0, 290.0), at(150.0, 230.0)));
        let after_contact = screen.extent();
        screen.settle();
        assert!(
            after_contact > COLLAPSED + 0.5,
            "{device:?}: the contact alone left the header partly expanded, at {after_contact}"
        );
        assert!(
            near(screen.extent(), COLLAPSED),
            "{device:?}: the fling went on to collapse the header, at {}",
            screen.extent()
        );
        assert!(
            screen.list.pixels() > 0.0,
            "{device:?}: and then moved the list, at {}",
            screen.list.pixels()
        );
        // The fling coasts on for a while.
        screen.settle();
        let started = screen.probe.started();
        assert_eq!(
            started.len(),
            1,
            "{device:?}: one sequence: {:?}",
            screen.probe.seen()
        );
        let steps = screen.probe.steps();
        assert!(
            steps
                .iter()
                .any(|step| step.cause == ScrollCause::SideEffect)
                && steps.iter().all(|step| step.sequence == started[0]),
            "{device:?}: the fling's steps carry the releasing sequence: {steps:?}"
        );
        assert_eq!(
            screen.probe.ended(),
            vec![(started[0], ScrollEndReason::Released)],
            "{device:?}: which ends once, after the fling"
        );

        // A residual toward the start.
        let mut screen = Screen::new(extents(), |header| header);
        screen.drag_up(device, 250.0);
        assert!(near(screen.extent(), COLLAPSED) && screen.list.pixels() > 50.0);
        screen.probe.take();
        screen
            .laid
            .replay(&fling(device, at(150.0, 150.0), at(150.0, 250.0)));
        screen.settle();
        assert_eq!(
            screen.list.pixels(),
            0.0,
            "{device:?}: the fling reached the start"
        );
        assert!(
            screen.extent() > COLLAPSED + 50.0,
            "{device:?}: and its residual expanded the header, at {}",
            screen.extent()
        );
        let started = screen.probe.started();
        assert!(
            !screen
                .probe
                .seen()
                .iter()
                .any(|seen| matches!(seen, Seen::Fling(_))),
            "{device:?}: the header took the residual before anything above it"
        );
        assert_eq!(
            screen.probe.ended(),
            vec![(started[0], ScrollEndReason::Released)],
            "{device:?}: under the one sequence"
        );
    }
}

/// With a snap policy the header rests fully expanded or fully collapsed
/// once the scroll that moved it ends: the nearest at rest.
pub(crate) fn collapsing_header_snaps_to_an_extent() {
    for device in DEVICES {
        let snap = || Arc::new(PageSnap::new(1.0)) as flui_widgets::SharedSnapPolicy;
        let mut screen = Screen::new(extents(), |header| header.snap_policy(snap()));
        screen.drag_up(device, 50.0);
        assert_eq!(
            screen.extent(),
            EXPANDED,
            "{device:?}: a short pull springs back open"
        );
        screen.drag_up(device, 120.0);
        assert!(
            near(screen.extent(), COLLAPSED),
            "{device:?}: a long one settles collapsed, at {}",
            screen.extent()
        );
        assert_eq!(
            screen.list.pixels(),
            0.0,
            "{device:?}: the settle moves the header alone"
        );

        let mut screen = Screen::new(extents(), |header| header);
        screen.drag_up(device, 50.0);
        let extent = screen.extent();
        assert!(
            extent > COLLAPSED + 1.0 && extent < EXPANDED - 1.0,
            "{device:?}: without a policy the header rests where it was left, at {extent}"
        );
    }
}

/// Tab onto a row scrolled under the header's edge brings it into the body,
/// Tab onto content the collapsed header clips expands it, and assistive
/// technology's reveal and focus requests do the same (WCAG 2.4.11).
pub(crate) fn collapsing_header_reveals_focused_row_from_tab_and_assistive_request() {
    let mut screen = Screen::new(extents(), |header| header);
    let manager = screen.laid.focus_manager();
    screen.list.jump_to(300.0);
    screen.laid.tick();
    let _ = screen.rows[3].request_focus();
    assert!(screen.row_top(2) < EXPANDED, "row 2 starts above the body");
    assert!(manager.focus_previous(), "Tab back moves focus");
    assert!(screen.rows[2].has_primary_focus());
    screen.settle();
    assert!(
        screen.row_top(2) >= screen.extent() - 0.5,
        "the focused row is brought out from under the header: row at {}, header to {}",
        screen.row_top(2),
        screen.extent()
    );
    assert_eq!(
        screen.extent(),
        EXPANDED,
        "a reveal moves the content, not the header"
    );

    screen.drag_up(crate::nested_scroll::Device::Touch, 250.0);
    assert!(near(screen.extent(), COLLAPSED), "collapsed");
    let first_visible = (0..ROWS)
        .find(|&index| screen.row_top(index) >= screen.extent())
        .expect("a row shows");
    let _ = screen.rows[first_visible].request_focus();
    while !screen.compose.has_primary_focus() {
        assert!(manager.focus_previous(), "Tab back reaches the header");
    }
    screen.settle();
    assert_eq!(
        screen.extent(),
        EXPANDED,
        "focus on the content the collapsed header clipped expands it"
    );

    // Assistive technology.
    screen.laid.enable_semantics();
    screen.laid.tick();
    // Content the header clips entirely is not published; content it clips
    // in part is, and a reveal request for it expands the header.
    screen.list.jump_to(0.0);
    screen.laid.tick();
    screen.drag_up(crate::nested_scroll::Device::Touch, 40.0);
    assert!(
        screen.extent() > 150.0 && screen.extent() < 190.0,
        "the header clips part of the button, at {}",
        screen.extent()
    );
    let compose = screen.node("Compose");
    screen.act(Action::ScrollIntoView, compose);
    screen.settle();
    assert_eq!(
        screen.extent(),
        EXPANDED,
        "a reveal request expands the header"
    );

    // Assistive focus on a row scrolled under the header's edge.
    screen.drag_up(crate::nested_scroll::Device::Touch, 250.0);
    assert!(near(screen.extent(), COLLAPSED), "collapsed again");
    assert!(
        screen.row_top(0) < screen.extent(),
        "row 0 is under the header's edge"
    );
    manager.unfocus();
    let row = screen.node("Row 0");
    screen.act(Action::Focus, row);
    screen.settle();
    assert!(screen.rows[0].has_primary_focus(), "assistive focus landed");
    assert!(
        screen.row_top(0) >= screen.extent() - 0.5,
        "and brought the row out, at {}",
        screen.row_top(0)
    );

    screen.list.jump_to(300.0);
    screen.laid.tick();
    let hidden = screen.list.pixels();
    let row = screen.node("Row 0");
    screen.act(Action::ScrollIntoView, row);
    screen.settle();
    assert!(
        screen.list.pixels() < hidden && screen.row_top(0) >= screen.extent() - 0.5,
        "a reveal request brings a row out from under the header, row at {}",
        screen.row_top(0)
    );
}

/// The header is a labelled banner landmark whose scroll actions move it:
/// up expands it, down collapses it, each offered only where it moves the
/// header, and neither moves the content.
pub(crate) fn collapsing_header_is_a_banner_assistive_scrolls_expand_and_collapse() {
    let mut screen = Screen::new(extents(), |header| header);
    screen.laid.enable_semantics();
    screen.laid.tick();
    let banner = |screen: &Screen| {
        let tree = screen.laid.a11y_tree().expect("published semantics");
        let node = tree.find(Role::Banner).expect("the header is a banner");
        (
            node.id(),
            node.label().map(str::to_owned),
            node.supports_action(Action::ScrollUp),
            node.supports_action(Action::ScrollDown),
        )
    };
    let (id, label, can_expand, can_collapse) = banner(&screen);
    assert_eq!(label.as_deref(), Some("Inbox"));
    assert!(
        can_collapse && !can_expand,
        "an expanded header scrolls down only"
    );
    screen.act(Action::ScrollDown, id);
    screen.settle();
    assert!(
        near(screen.extent(), COLLAPSED),
        "collapsed, at {}",
        screen.extent()
    );
    let (id, _, can_expand, can_collapse) = banner(&screen);
    assert!(
        can_expand && !can_collapse,
        "a collapsed header scrolls up only"
    );
    screen.act(Action::ScrollUp, id);
    screen.settle();
    assert_eq!(screen.extent(), EXPANDED);
    assert_eq!(
        screen.list.pixels(),
        0.0,
        "neither action moves the content"
    );
}

/// Equal extents make a header that takes no scroll and offers no scroll
/// actions; extents that are not finite, negative, too large or inverted are
/// refused.
pub(crate) fn collapsing_header_with_equal_extents_takes_no_scroll() {
    for device in DEVICES {
        let fixed = HeaderExtents::new(EXPANDED, EXPANDED).expect("equal extents");
        let mut screen = Screen::new(fixed, |header| {
            header
                .expand(HeaderOrder::BeforeContent)
                .snap_policy(Arc::new(PageSnap::new(1.0)))
        });
        screen.drag_up(device, 120.0);
        assert_eq!(
            screen.extent(),
            EXPANDED,
            "{device:?}: the header never moves"
        );
        assert!(
            screen.list.pixels() > 80.0,
            "{device:?}: the list takes it all"
        );
        screen.drag_down(device, 60.0);
        assert_eq!(screen.extent(), EXPANDED, "{device:?}: either way");
        screen.laid.enable_semantics();
        screen.laid.tick();
        let tree = screen.laid.a11y_tree().expect("published semantics");
        let banner = tree.find(Role::Banner).expect("still a banner");
        assert!(
            !banner.supports_action(Action::ScrollUp)
                && !banner.supports_action(Action::ScrollDown),
            "{device:?}: and no action moves it"
        );
    }
    for (collapsed, expanded, error) in [
        (f64::NAN, 10.0, HeaderExtentsError::NonFinite),
        (0.0, f64::INFINITY, HeaderExtentsError::NonFinite),
        (-1.0, 10.0, HeaderExtentsError::Negative),
        (0.0, HeaderExtents::MAX * 2.0, HeaderExtentsError::TooLarge),
        (20.0, 10.0, HeaderExtentsError::Inverted),
    ] {
        assert_eq!(
            HeaderExtents::new(collapsed, expanded),
            Err(error),
            "{collapsed}..{expanded}"
        );
    }
    assert!(
        HeaderExtents::new(0.0, 0.0).is_ok(),
        "a header of no extent is admitted"
    );
}
