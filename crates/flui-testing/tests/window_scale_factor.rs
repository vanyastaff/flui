//! A window's scale-factor change reaches that window's own presentation and
//! no other: two windows on monitors of different scales each publish
//! accessibility bounds in their own physical pixels.

use std::time::Duration;

use flui_testing::a11y::A11yRect;
use flui_testing::{HeadlessHost, HeadlessWindow, HeadlessWindowId};
use flui_view::ViewExt as _;
use flui_widgets::{Semantics, SizedBox};

/// A labelled box filling its 40 × 24 window, so its semantics node carries
/// bounds.
fn probe() -> flui_view::BoxedView {
    Semantics::new()
        .container(true)
        .label("probe")
        .child(SizedBox::new(40.0, 24.0))
        .boxed()
}

/// Two windows at scale 1, each showing the probe with assistive technology
/// attached, after a first frame.
fn two_windows() -> (HeadlessHost, HeadlessWindowId, HeadlessWindowId) {
    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 24));
    let first = host.primary_window();
    let second = host.open_window(HeadlessWindow::new(40, 24));
    for window in [first, second] {
        host.attach_to(window, &probe())
            .expect("a freshly opened window has no root yet");
        host.enable_semantics_on(window);
    }
    let _ = host.pump(Duration::from_millis(16));
    (host, first, second)
}

/// Where an AccessKit adapter places the probe in `window`: its bounds under
/// its own transform and every ancestor's, which AccessKit defines as
/// physical pixels relative to the window's client area.
fn probe_bounds(host: &HeadlessHost, window: HeadlessWindowId) -> A11yRect {
    let tree = host
        .published_a11y_tree(window)
        .expect("the window published a tree");
    let update = tree.raw();
    let (mut current, node) = update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("probe"))
        .map(|(id, node)| (*id, node))
        .expect("the probe is in the window's tree");
    let mut rect = node.bounds().expect("the probe's node carries bounds");
    if let Some(transform) = node.transform() {
        rect = transform.transform_rect_bbox(rect);
    }
    while let Some((parent, parent_node)) = update
        .nodes
        .iter()
        .find(|(_, node)| node.children().contains(&current))
    {
        if let Some(transform) = parent_node.transform() {
            rect = transform.transform_rect_bbox(rect);
        }
        current = *parent;
    }
    rect
}

/// Moves `moved` to a scale-2 monitor and checks that it, and only it, now
/// publishes the probe at twice its logical size.
fn rescaling_one_window_leaves_the_other(
    pick: fn(HeadlessWindowId, HeadlessWindowId) -> (HeadlessWindowId, HeadlessWindowId),
) {
    let (mut host, first, second) = two_windows();
    let (moved, kept) = pick(first, second);
    let logical = A11yRect::new(0.0, 0.0, 40.0, 24.0);
    assert_eq!(
        probe_bounds(&host, moved),
        logical,
        "scale 1 publishes logical bounds"
    );
    assert_eq!(
        probe_bounds(&host, kept),
        logical,
        "scale 1 publishes logical bounds"
    );

    host.set_scale_factor(moved, 2.0);
    let _ = host.pump(Duration::from_millis(16));

    assert_eq!(
        probe_bounds(&host, moved),
        A11yRect::new(0.0, 0.0, 80.0, 48.0),
        "the rescaled window publishes bounds in its new physical pixels"
    );
    assert_eq!(
        probe_bounds(&host, kept),
        logical,
        "the other window keeps its own scale"
    );
}

pub(crate) fn rescaling_a_secondary_window_updates_only_its_semantics_bounds() {
    rescaling_one_window_leaves_the_other(|primary, secondary| (secondary, primary));
}

pub(crate) fn rescaling_the_primary_window_updates_only_its_semantics_bounds() {
    rescaling_one_window_leaves_the_other(|primary, secondary| (primary, secondary));
}

/// Records the `MediaQuery` each of its builds reads.
#[derive(Clone, flui_view::prelude::StatelessView)]
struct MediaQueryReader {
    seen: std::rc::Rc<std::cell::RefCell<Vec<flui_widgets::MediaQueryData>>>,
}

impl flui_view::prelude::StatelessView for MediaQueryReader {
    fn build(
        &self,
        ctx: &dyn flui_view::prelude::BuildContext,
    ) -> impl flui_view::prelude::IntoView {
        self.seen
            .borrow_mut()
            .push(flui_widgets::MediaQuery::of(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

/// A widget under a secondary window reads that window's own `MediaQuery`
/// (its size and scale, not the primary's), and rebuilds with the new ratio
/// when the window moves to another monitor.
pub(crate) fn a_secondary_window_publishes_its_own_media_query() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 24));
    let second = host.open_window(HeadlessWindow::new(60, 30));
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    host.attach_to(
        second,
        &MediaQueryReader {
            seen: std::rc::Rc::clone(&seen),
        },
    )
    .expect("a freshly opened window has no root yet");
    let _ = host.pump(Duration::from_millis(16));
    let first = seen.borrow().last().cloned().expect("the reader built");
    assert_eq!(
        first.size,
        flui_foundation::geometry::Size::new(60.0, 30.0),
        "the secondary window publishes its own size"
    );
    assert!((first.device_pixel_ratio - 1.0).abs() < f64::EPSILON);

    host.set_scale_factor(second, 2.0);
    let _ = host.pump(Duration::from_millis(16));
    let rescaled = seen.borrow().last().cloned().expect("the reader built");
    assert!(
        (rescaled.device_pixel_ratio - 2.0).abs() < f64::EPSILON,
        "the reader rebuilds with the window's new ratio, read {}",
        rescaled.device_pixel_ratio
    );
}
