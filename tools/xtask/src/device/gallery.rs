//! What the interaction gallery's end-to-end drivers share, on any host: its
//! screens' sidebar names and the colours it paints, which a check samples
//! where the right and the wrong outcome differ.
//!
//! The colours are the gallery's own (`examples/interaction_gallery/src/
//! style.rs`, `painters.rs`, `screens/gestures.rs`).

/// An sRGB colour read off a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rgb(pub(super) u8, pub(super) u8, pub(super) u8);

impl Rgb {
    /// The largest per-channel difference to `other`.
    pub(super) fn distance(self, other: Self) -> u8 {
        let channel = |a: u8, b: u8| a.abs_diff(b);
        channel(self.0, other.0)
            .max(channel(self.1, other.1))
            .max(channel(self.2, other.2))
    }

    /// Within `tolerance` of `other` on every channel.
    pub(super) fn near(self, other: Self, tolerance: u8) -> bool {
        self.distance(other) <= tolerance
    }
}

impl std::fmt::Display for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

// The sidebar's entries, each a screen's title.
pub(super) const SWIPE: &str = "Swipe actions";
pub(super) const REFRESH: &str = "Pull to refresh";
pub(super) const COLLAPSING: &str = "Collapsing header";
pub(super) const PAGES: &str = "Pages";
pub(super) const SHEET: &str = "Bottom sheet";
pub(super) const VIEWER: &str = "Zoom and pan";
pub(super) const CARDS: &str = "Drag and drop";
pub(super) const MENU_SCREEN: &str = "Context menu";
pub(super) const GESTURES: &str = "Gestures";
pub(super) const FOCUS: &str = "Focus and keys";

// The gallery's colours.
pub(super) const BACKGROUND: Rgb = Rgb(244, 245, 248);
pub(super) const SURFACE: Rgb = Rgb(255, 255, 255);
pub(super) const ACCENT: Rgb = Rgb(46, 92, 230);
pub(super) const PIN: Rgb = Rgb(232, 160, 32);
pub(super) const ARCHIVE: Rgb = Rgb(56, 142, 92);
pub(super) const PAGE_COLORS: [Rgb; 3] = [ACCENT, ARCHIVE, Rgb(214, 69, 65)];
pub(super) const MAP_GROUND: Rgb = Rgb(229, 236, 222);
pub(super) const LANE: Rgb = Rgb(232, 235, 241);
pub(super) const PAD_IDLE: Rgb = Rgb(222, 230, 252);
pub(super) const TINT_TAP: Rgb = Rgb(204, 232, 214);
pub(super) const TINT_DOUBLE: Rgb = Rgb(252, 228, 196);
pub(super) const TINT_LONG: Rgb = Rgb(246, 214, 214);
pub(super) const TINT_PAN: Rgb = Rgb(226, 214, 246);
pub(super) const TINT_SCALE: Rgb = Rgb(204, 236, 240);
/// The focus ring's dark band.
pub(super) const RING: Rgb = Rgb(0, 0, 0);
/// What anti-aliasing and colour management may move a flat fill by.
pub(super) const TOLERANCE: u8 = 10;

/// The viewer grid's cell colour (`painters::cell_color`).
pub(super) fn cell(column: i32, row: i32) -> Rgb {
    let (column, row) = (
        u8::try_from(column).expect("BUG: a grid column"),
        u8::try_from(row).expect("BUG: a grid row"),
    );
    Rgb(30 + 9 * column, 40 + 8 * row, 200 - 4 * column.min(row))
}

/// The viewer grid's cell side, in logical pixels.
pub(super) const CELL: f64 = 40.0;

/// The cell under a point `scene` logical pixels into the unzoomed grid.
pub(super) fn cell_at(scene: (f64, f64)) -> Rgb {
    cell((scene.0 / CELL) as i32, (scene.1 / CELL) as i32)
}

/// Fails unless `seen` is within `tolerance` of `expected`: a check's
/// precondition, sampled before it acts.
pub(super) fn ensure_near(
    what: &str,
    seen: Rgb,
    expected: Rgb,
    tolerance: u8,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        seen.near(expected, tolerance),
        "{what}: {seen} before the action, expected {expected}"
    );
    Ok(())
}

// The gallery's texts a check finds or waits for.
pub(super) const INITIAL_SWIPE: &str =
    "4 in inbox, 0 pinned, 0 archived, 3 tasks left, 0 completed";
pub(super) const ADA: &str = "A Ada Design review moved to Thursday";
pub(super) const BROOK: &str = "B Brook Your order has shipped";
pub(super) const FIRST_STOP: &str = "Train to Porto";
pub(super) const SHEET_NODE: &str = "Places sheet Nearby places";
pub(super) const FIRST_CARD: &str = "Write the release notes Drag me";
pub(super) const INITIAL_MENU: &str = "4 files, last action: nothing yet";
pub(super) const INITIAL_FOCUS: &str =
    "Focused: nothing; saved name: none; settings page: Settings";
const LAST_MESSAGE: &str = "Message 1";
const FIRST_PAGE: &str = "Page 1 of 3: Plan";
const UNZOOMED: &str = "Zoom 100%, offset 0 by 0 px";
pub(super) const EMPTY_DONE: &str = "Done (0)";

/// How long a wheel burst stays one sequence after its last notch: the
/// default preset's `GestureSettings::contactless_latch_idle`
/// (`flui-interaction`, which xtask does not link).
pub(super) const CONTACTLESS_LATCH_IDLE: std::time::Duration =
    std::time::Duration::from_millis(1500);

/// The sheet's anchors: its top below the map's (`screens/sheet.rs`).
pub(super) const HALF: f64 = 240.0;
pub(super) const FULL: f64 = 24.0;

/// Where on an element's rectangle a check points from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Corner {
    TopLeft,
    BottomLeft,
    LeftMiddle,
    RightMiddle,
}

/// How a driver finds the gallery's elements and turns a logical offset
/// from one of their corners into a point it can act at. Implemented for a
/// shared reference to the Windows and Android drivers and for a mutable one
/// to the web driver, so the helpers below take it by value.
pub(super) trait Placement {
    /// An element's rectangle, in the driver's pixels.
    type Rect: Copy;
    /// A point, in the driver's pixels.
    type Point;
    /// The rectangle of the one text element named `name`.
    fn text_bounds(&mut self, name: &str) -> anyhow::Result<Self::Rect>;
    /// `corner` of `rect`, moved by `(dx, dy)` logical pixels.
    fn at(&self, rect: Self::Rect, corner: Corner, dx: f64, dy: f64)
    -> anyhow::Result<Self::Point>;
}

/// The logical offsets the gallery's anchors take on one form factor: the
/// content column is about 340 logical pixels wide on a phone and wider on
/// a desktop window or page.
#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    below_list: (f64, f64),
    in_header: (f64, f64),
    pager: (f64, f64),
    done_slot: (f64, f64),
}

/// A desktop window or page.
pub(super) const DESKTOP: Layout = Layout {
    below_list: (200.0, 40.0),
    in_header: (400.0, -60.0),
    pager: (300.0, 200.0),
    done_slot: (60.0, 50.0),
};

/// A phone.
pub(super) const PHONE: Layout = Layout {
    below_list: (100.0, 40.0),
    in_header: (220.0, -60.0),
    pager: (150.0, 250.0),
    done_slot: (40.0, 80.0),
};

fn anchor<P: Placement>(
    mut scene: P,
    name: &str,
    corner: Corner,
    (dx, dy): (f64, f64),
) -> anyhow::Result<P::Point> {
    let rect = scene.text_bounds(name)?;
    scene.at(rect, corner, dx, dy)
}

/// The middle of `rect`'s left edge, `inset` logical pixels in.
pub(super) fn left_of<P: Placement>(
    scene: &P,
    rect: P::Rect,
    inset: f64,
) -> anyhow::Result<P::Point> {
    scene.at(rect, Corner::LeftMiddle, inset, 0.0)
}

/// The middle of `rect`'s right edge, `inset` logical pixels in.
pub(super) fn right_of<P: Placement>(
    scene: &P,
    rect: P::Rect,
    inset: f64,
) -> anyhow::Result<P::Point> {
    scene.at(rect, Corner::RightMiddle, -inset, 0.0)
}

// A point just below the refresh list's last row: the page now, a row once
/// the list has grown by one.
pub(super) fn below_list<P: Placement>(scene: P, layout: &Layout) -> anyhow::Result<P::Point> {
    anchor(scene, LAST_MESSAGE, Corner::BottomLeft, layout.below_list)
}

// A point inside the expanded header, above the first stop as it first
/// lies: the header's blue until it collapses past it.
pub(super) fn in_header<P: Placement>(scene: P, layout: &Layout) -> anyhow::Result<P::Point> {
    anchor(scene, FIRST_STOP, Corner::TopLeft, layout.in_header)
}

// The middle of the pager: the current page's colour.
pub(super) fn pager_point<P: Placement>(scene: P, layout: &Layout) -> anyhow::Result<P::Point> {
    anchor(scene, FIRST_PAGE, Corner::BottomLeft, layout.pager)
}

// The grid's top-left corner: 16 logical pixels below the readout's panel,
/// whose text is inset 12 across and 8 down.
pub(super) fn grid_origin<P: Placement>(scene: P) -> anyhow::Result<P::Point> {
    anchor(scene, UNZOOMED, Corner::BottomLeft, (-12.0, 24.0))
}

/// Where a first card lands in the Done lane whose heading reads `lane`,
/// below the heading: the empty lane until one does (on a phone, where the
/// lanes stack, the page below it).
pub(super) fn done_slot<P: Placement>(
    scene: P,
    layout: &Layout,
    lane: &str,
) -> anyhow::Result<P::Point> {
    anchor(scene, lane, Corner::TopLeft, layout.done_slot)
}

/// The zoom percentage and offset the viewer's readout states.
pub(super) fn viewer_state(text: &str) -> Option<(f64, f64, f64)> {
    let rest = text.strip_prefix("Zoom ")?;
    let (zoom, rest) = rest.split_once("%, offset ")?;
    let (x, rest) = rest.split_once(" by ")?;
    let y = rest.strip_suffix(" px")?;
    Some((zoom.parse().ok()?, x.parse().ok()?, y.parse().ok()?))
}

/// The scene point under a viewer point, for a readout's zoom and offset.
pub(super) fn scene_under(state: (f64, f64, f64), at: (f64, f64)) -> (f64, f64) {
    let (zoom, x, y) = state;
    ((at.0 - x) / (zoom / 100.0), (at.1 - y) / (zoom / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewer_readout_reads_back_and_maps_to_the_scene() {
        assert_eq!(
            viewer_state("Zoom 200%, offset -205 by -205 px"),
            Some((200.0, -205.0, -205.0))
        );
        assert_eq!(viewer_state("Zoom 200%"), None);
        assert_eq!(
            scene_under((200.0, -205.0, -205.0), (295.0, 255.0)),
            (250.0, 230.0)
        );
        assert_eq!(cell_at((250.0, 230.0)), cell(6, 5));
    }
}
