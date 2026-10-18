//! The gallery's checks on a phone, several per screen: the touch
//! counterparts of `windows-gallery`'s, and the hardware keys a phone with a
//! keyboard attached sends. Each opens its screen fresh, acts with the
//! device's own input, and reads the outcome from the agent's tree and from
//! pixels sampled where the right and the wrong outcome differ.
//!
//! The colours are the gallery's own (`examples/interaction_gallery/src/
//! style.rs`, `painters.rs`, `screens/gestures.rs`). Offsets are logical
//! pixels from an element's rectangle, so they hold at any density; on a
//! phone the content column is about 340 logical pixels wide.

use super::super::gallery::{
    ACCENT, ADA, ARCHIVE, BACKGROUND, BROOK, CARDS, COLLAPSING, EMPTY_DONE, FIRST_CARD, FIRST_STOP,
    FOCUS, FULL, GESTURES, HALF, INITIAL_FOCUS, INITIAL_MENU, INITIAL_SWIPE, MAP_GROUND,
    MENU_SCREEN, PAD_IDLE, PAGE_COLORS, PAGES, PHONE, PIN, REFRESH, RING, Rgb, SHEET, SHEET_NODE,
    SURFACE, SWIPE, TINT_DOUBLE, TINT_LONG, TINT_PAN, TINT_SCALE, TINT_TAP, TOLERANCE, VIEWER,
    below_list, cell_at, done_slot, ensure_near, grid_origin, in_header, left_of, pager_point,
    right_of, scene_under, viewer_state,
};
use super::{BUTTON, Check, Driver, EDIT, Gesture, MENU, MENU_ITEM, Point, TEXT};

/// Every check, in the order it runs.
pub(super) const CHECKS: &[(&str, Check)] = &[
    check(
        "swipe: a touch swipe reveals Archive and tapping it archives",
        SWIPE,
        swipe_archive,
    ),
    check(
        "swipe: a touch swipe the other way reveals Pin and tapping it pins",
        SWIPE,
        swipe_pin,
    ),
    check(
        "swipe: a touch fling dismisses a task",
        SWIPE,
        swipe_dismiss_task,
    ),
    check(
        "swipe: tapping the Done control completes a task",
        SWIPE,
        swipe_done_control,
    ),
    check(
        "swipe: a long press opens the row's menu and Archive archives",
        SWIPE,
        swipe_long_press_menu,
    ),
    check(
        "refresh: a touch pull past the top refreshes",
        REFRESH,
        refresh_pull,
    ),
    check(
        "refresh: tapping the Refresh control refreshes",
        REFRESH,
        refresh_control,
    ),
    check(
        "refresh: F5 with focus in the list refreshes",
        REFRESH,
        refresh_key,
    ),
    check(
        "collapsing: a touch drag collapses the header before the list, and back",
        COLLAPSING,
        collapsing_touch,
    ),
    check(
        "collapsing: a mouse wheel collapses the header",
        COLLAPSING,
        collapsing_wheel,
    ),
    check("pages: a touch swipe turns the page", PAGES, pages_swipe),
    check(
        "pages: the arrow keys and Home turn pages",
        PAGES,
        pages_keys,
    ),
    check(
        "pages: tapping Next and Previous turns the page",
        PAGES,
        pages_buttons,
    ),
    check(
        "sheet: a touch drag on the handle rests at Full",
        SHEET,
        sheet_touch_full,
    ),
    check(
        "sheet: a touch drag down on the handle rests at Peek",
        SHEET,
        sheet_touch_peek,
    ),
    check(
        "sheet: the arrow keys step between anchors",
        SHEET,
        sheet_keys,
    ),
    check(
        "viewer: a touch double tap zooms about the tap",
        VIEWER,
        viewer_double_tap,
    ),
    check(
        "viewer: a two-finger spread zooms in",
        VIEWER,
        viewer_spread,
    ),
    check(
        "viewer: a one-finger drag pans the zoomed grid",
        VIEWER,
        viewer_touch_pan,
    ),
    check(
        "viewer: + and - zoom and the arrows pan",
        VIEWER,
        viewer_keys,
    ),
    check(
        "cards: a long press and a touch drag move a card to Done",
        CARDS,
        cards_drag,
    ),
    check(
        "cards: tapping Cut and Paste moves a card",
        CARDS,
        cards_controls,
    ),
    check(
        "cards: Ctrl+X on a card and Ctrl+V on a column move it",
        CARDS,
        cards_keys,
    ),
    check(
        "menu: a touch long press opens the menu and a tap chooses",
        MENU_SCREEN,
        menu_long_press,
    ),
    check(
        "menu: Escape closes the menu and chooses nothing",
        MENU_SCREEN,
        menu_escape,
    ),
    check(
        "menu: the Menu key opens the focused row's menu, Down and Enter choose",
        MENU_SCREEN,
        menu_keys,
    ),
    check("gestures: a touch tap is a tap", GESTURES, gestures_tap),
    check(
        "gestures: a touch double tap is a double tap",
        GESTURES,
        gestures_double_tap,
    ),
    check(
        "gestures: a held touch is a long press",
        GESTURES,
        gestures_long_press,
    ),
    check("gestures: a touch drag is a pan", GESTURES, gestures_pan),
    check(
        "gestures: a two-finger pinch is a scale",
        GESTURES,
        gestures_pinch,
    ),
    check(
        "gestures: a stylus tap is a tap",
        GESTURES,
        gestures_stylus_tap,
    ),
    check(
        "gestures: a mouse click is a tap",
        GESTURES,
        gestures_mouse_click,
    ),
    check(
        "focus: Tab and Shift+Tab move the focus ring in order",
        FOCUS,
        focus_tab_order,
    ),
    check(
        "focus: the soft keyboard types into the field and its action key saves",
        FOCUS,
        focus_soft_keyboard,
    ),
    check(
        "focus: Enter opens a settings page and Space presses Back",
        FOCUS,
        focus_navigator,
    ),
];

const fn check(
    name: &'static str,
    screen: &'static str,
    run: fn(&Driver<'_>) -> anyhow::Result<()>,
) -> (&'static str, Check) {
    (name, Check { screen, run })
}

/// The viewer's cells differ by 8 or 9 per row or column in a channel, so
/// they are told apart more tightly than other fills.
const CELL_TOLERANCE: u8 = 4;
/// How long a held touch must last to be a long press, past the device's
/// long-press timeout.
const LONG_PRESS: u32 = 900;

// ---------------------------------------------------------------------------
// Swipe actions
// ---------------------------------------------------------------------------

const CHEN: &str = "C Chen Lunch on Friday?";

fn swipe_archive(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let row = driver.bounds_of(TEXT, ADA)?;
    let tray = right_of(&driver, row, 30.0)?;
    ensure_near(
        "the row's right end",
        driver.sample(tray)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = right_of(&driver, row, 40.0)?;
    let to = driver.offset(from, -150.0, 0.0);
    driver.drag(from, to, 300, 150)?;
    driver.wait_node(BUTTON, "Archive Ada")?;
    driver.pixel("the revealed Archive action", tray, ARCHIVE, TOLERANCE)?;
    driver.tap_node(BUTTON, "Archive Ada")?;
    driver.text("3 in inbox, 0 pinned, 1 archived, 3 tasks left, 0 completed")?;
    driver.gone(TEXT, ADA)
}

fn swipe_pin(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let row = driver.bounds_of(TEXT, BROOK)?;
    // Between the row's left edge and its avatar: where the revealed action
    // slides in.
    let tray = left_of(&driver, row, 8.0)?;
    ensure_near(
        "the row's left end",
        driver.sample(tray)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = left_of(&driver, row, 60.0)?;
    let to = driver.offset(from, 150.0, 0.0);
    driver.drag(from, to, 300, 150)?;
    driver.wait_node(BUTTON, "Pin Brook")?;
    driver.pixel("the revealed Pin action", tray, PIN, TOLERANCE)?;
    driver.tap_node(BUTTON, "Pin Brook")?;
    driver.text("4 in inbox, 1 pinned, 0 archived, 3 tasks left, 0 completed")?;
    driver.text("B Brook (pinned) Your order has shipped")
}

/// Where the last task is now: a row, and once a task is gone, the page
/// behind the shortened list.
fn task_tail(driver: &Driver<'_>) -> anyhow::Result<Point> {
    let last = driver.bounds_of(TEXT, "Call the bank")?;
    Ok(driver.offset(
        Point {
            x: last.left,
            y: last.centre().y,
        },
        200.0,
        0.0,
    ))
}

fn swipe_dismiss_task(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let first = driver.bounds_of(TEXT, "Water the plants")?;
    let tail = task_tail(driver)?;
    ensure_near(
        "the last task row",
        driver.sample(tail)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = left_of(&driver, first, 30.0)?;
    let to = driver.offset(from, 280.0, 0.0);
    driver.drag(from, to, 120, 0)?;
    driver.text("4 in inbox, 0 pinned, 0 archived, 2 tasks left, 1 completed")?;
    driver.gone(TEXT, "Water the plants")?;
    driver.pixel(
        "the page below the shortened task list",
        tail,
        BACKGROUND,
        TOLERANCE,
    )
}

fn swipe_done_control(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let tail = task_tail(driver)?;
    driver.tap_node(BUTTON, "Complete Book the train")?;
    driver.text("4 in inbox, 0 pinned, 0 archived, 2 tasks left, 1 completed")?;
    driver.gone(TEXT, "Book the train")?;
    driver.pixel(
        "the page below the shortened task list",
        tail,
        BACKGROUND,
        TOLERANCE,
    )
}

fn swipe_long_press_menu(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let row = driver.bounds_of(TEXT, CHEN)?;
    driver.gesture(Gesture::press(left_of(&driver, row, 160.0)?, LONG_PRESS))?;
    driver.wait_node(MENU_ITEM, "Archive Chen")?;
    driver.tap_node(MENU_ITEM, "Archive Chen")?;
    driver.text("3 in inbox, 0 pinned, 1 archived, 3 tasks left, 0 completed")?;
    driver.gone(TEXT, CHEN)
}

// ---------------------------------------------------------------------------
// Pull to refresh
// ---------------------------------------------------------------------------

fn refresh_pull(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &PHONE)?;
    ensure_near(
        "below the list",
        driver.sample(below)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    let top = driver.bounds_of(TEXT, "Message 6")?;
    let from = top.centre();
    let to = driver.offset(from, 0.0, 220.0);
    driver.drag(from, to, 400, 300)?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.text("Message 7")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

fn refresh_control(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &PHONE)?;
    ensure_near(
        "below the list",
        driver.sample(below)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    driver.tap_node(BUTTON, "Refresh inbox")?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

fn refresh_key(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &PHONE)?;
    driver.tab_until(16, |name| name == "Refresh inbox" || name == "Messages")?;
    driver.key("KEYCODE_F5")?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

// ---------------------------------------------------------------------------
// Collapsing header
// ---------------------------------------------------------------------------

fn collapsing_touch(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Itinerary scrolled 0 px")?;
    let header = in_header(driver, &PHONE)?;
    ensure_near(
        "the expanded header",
        driver.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let from = driver.offset(driver.bounds_of(TEXT, FIRST_STOP)?.centre(), 0.0, 250.0);
    let to = driver.offset(from, 0.0, -200.0);
    driver.drag(from, to, 500, 150)?;
    driver.wait("the header to collapse past the sample point", |driver| {
        Ok((!driver.sample(header)?.near(ACCENT, TOLERANCE)).then_some(()))
    })?;
    // Further up, the list itself moves.
    let more = driver.offset(to, 0.0, 200.0);
    driver.drag(more, to, 500, 150)?;
    let scrolled = driver.text_where("Itinerary scrolled ", |text| {
        text != "Itinerary scrolled 0 px"
    })?;
    // Back down: the list returns to its top, then the header expands.
    let down_to = driver.offset(to, 0.0, 450.0);
    driver.drag(to, down_to, 600, 150)?;
    driver.drag(to, down_to, 600, 150)?;
    driver
        .text("Itinerary scrolled 0 px")
        .map_err(|error| error.context(format!("after {scrolled:?}, dragging back down")))?;
    driver.pixel("the header expanded again", header, ACCENT, TOLERANCE)
}

fn collapsing_wheel(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Itinerary scrolled 0 px")?;
    let header = in_header(driver, &PHONE)?;
    ensure_near(
        "the expanded header",
        driver.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let list = driver.offset(driver.bounds_of(TEXT, FIRST_STOP)?.centre(), 0.0, 120.0);
    driver.input(&format!(
        "mouse scroll {} {} --axis VSCROLL,-1",
        list.x, list.y
    ))?;
    driver.wait("the header to leave the sample point", |driver| {
        Ok((!driver.sample(header)?.near(ACCENT, TOLERANCE)).then_some(()))
    })
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn pages_swipe(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &PHONE)?;
    ensure_near("page 1", driver.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    let from = driver.offset(point, 130.0, 0.0);
    let to = driver.offset(point, -130.0, 0.0);
    driver.drag(from, to, 200, 0)?;
    driver.text("Page 2 of 3: Pack")?;
    driver.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)
}

fn pages_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &PHONE)?;
    // A drag gives the pager the focus its keys need.
    let from = driver.offset(point, 100.0, 0.0);
    let to = driver.offset(point, -100.0, 0.0);
    driver.drag(from, to, 250, 0)?;
    driver.text("Page 2 of 3: Pack")?;
    driver.key("KEYCODE_DPAD_RIGHT")?;
    driver.text("Page 3 of 3: Go")?;
    driver.pixel("page 3", point, PAGE_COLORS[2], TOLERANCE)?;
    driver.key("KEYCODE_MOVE_HOME")?;
    driver.text("Page 1 of 3: Plan")?;
    driver.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

fn pages_buttons(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &PHONE)?;
    ensure_near("page 1", driver.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    driver.tap_node(BUTTON, "Next")?;
    driver.text("Page 2 of 3: Pack")?;
    driver.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)?;
    driver.tap_node(BUTTON, "Previous")?;
    driver.text("Page 1 of 3: Plan")?;
    driver.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

// ---------------------------------------------------------------------------
// Bottom sheet
// ---------------------------------------------------------------------------

/// The sheet's anchors: its top below the map's (`screens/sheet.rs`).
const PEEK: f64 = 430.0;

/// The map's top-left corner: the sheet's slider spans the drag's whole
/// area, which the map fills.
fn map_origin(driver: &Driver<'_>) -> anyhow::Result<Point> {
    let sheet = driver.bounds_of(super::SLIDER, SHEET_NODE)?;
    Ok(Point {
        x: sheet.left,
        y: sheet.top,
    })
}

/// The sheet's handle at Half.
fn handle(driver: &Driver<'_>) -> anyhow::Result<Point> {
    let sheet = driver.bounds_of(super::SLIDER, SHEET_NODE)?;
    Ok(driver.offset(
        Point {
            x: sheet.centre().x,
            y: sheet.top,
        },
        0.0,
        HALF + 13.0,
    ))
}

/// A block of the map clear of every street, above the sheet at Half and
/// under it at Full.
fn above_half(driver: &Driver<'_>) -> anyhow::Result<Point> {
    Ok(driver.offset(map_origin(driver)?, 250.0, 100.0))
}

fn sheet_touch_full(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let probe = above_half(driver)?;
    ensure_near(
        "the map above the sheet",
        driver.sample(probe)?,
        MAP_GROUND,
        TOLERANCE,
    )?;
    let from = handle(driver)?;
    let to = driver.offset(from, 0.0, FULL - HALF);
    driver.drag(from, to, 500, 200)?;
    driver.text("Sheet at Full")?;
    driver.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)
}

fn sheet_touch_peek(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let map = map_origin(driver)?;
    // Between Half and Peek: the sheet now, the map once it rests at Peek.
    let probe = driver.offset(map, 250.0, f64::midpoint(HALF, PEEK));
    ensure_near(
        "the sheet at Half",
        driver.sample(probe)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = handle(driver)?;
    let to = driver.offset(from, 0.0, PEEK - HALF);
    driver.drag(from, to, 500, 200)?;
    driver.text("Sheet at Peek")?;
    driver.wait("the map above the sheet at Peek", |driver| {
        Ok((!driver.sample(probe)?.near(SURFACE, TOLERANCE)).then_some(()))
    })
}

fn sheet_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let probe = above_half(driver)?;
    driver.tab_until(16, |name| name.starts_with("Places sheet"))?;
    driver.key("KEYCODE_DPAD_UP")?;
    driver.text("Sheet at Full")?;
    driver.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)?;
    driver.key("KEYCODE_DPAD_DOWN")?;
    driver.text("Sheet at Half")?;
    driver.pixel(
        "the map above the sheet at Half",
        probe,
        MAP_GROUND,
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Zoom and pan
// ---------------------------------------------------------------------------

/// Where the probe samples the grid, in viewer logical pixels: inside the
/// phone's narrow viewer, on cells whose colours differ from those the
/// zooms below bring under it.
const PROBE: (f64, f64) = (130.0, 100.0);

fn viewer_double_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let at = (20.0, 300.0);
    let tap = driver.offset(origin, at.0, at.1);
    let probe = driver.offset(origin, PROBE.0, PROBE.1);
    ensure_near(
        "the cell before the zoom",
        driver.sample(probe)?,
        cell_at(PROBE),
        CELL_TOLERANCE,
    )?;
    driver.gesture(Gesture::double_tap(tap, 80))?;
    let zoomed = driver.text_where("Zoom 200%", |_| true)?;
    let state = viewer_state(&zoomed).ok_or_else(|| anyhow::anyhow!("unreadable {zoomed:?}"))?;
    anyhow::ensure!(
        (state.1 + at.0).abs() <= 2.0 && (state.2 + at.1).abs() <= 2.0,
        "the zoom is not about the tap: {zoomed:?}"
    );
    // About the tap, the probe now shows the scene halfway to it.
    driver.pixel(
        "the cell after the zoom",
        probe,
        cell_at(scene_under(state, PROBE)),
        CELL_TOLERANCE,
    )
}

fn viewer_spread(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let focal = driver.offset(origin, 90.0, 250.0);
    let probe = driver.offset(origin, PROBE.0, PROBE.1);
    let before = driver.sample(probe)?;
    ensure_near(
        "the cell before the spread",
        before,
        cell_at(PROBE),
        CELL_TOLERANCE,
    )?;
    let a = (
        driver.offset(focal, 0.0, -30.0),
        driver.offset(focal, 0.0, -120.0),
    );
    let b = (
        driver.offset(focal, 0.0, 30.0),
        driver.offset(focal, 0.0, 120.0),
    );
    driver.pinch(a, b, 500)?;
    let zoom = driver.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(percent, _, _)| percent > 150.0)
    })?;
    let state = viewer_state(&zoom).expect("BUG: accepted as readable");
    driver
        .pixel(
            "the cell after the spread",
            probe,
            cell_at(scene_under(state, PROBE)),
            CELL_TOLERANCE,
        )
        .map_err(|error| error.context(format!("readout {zoom:?}")))
}

fn viewer_touch_pan(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let probe = driver.offset(origin, PROBE.0, PROBE.1);
    // The child fills the viewport, so there is room to pan only zoomed in.
    driver.tap_node(BUTTON, "Zoom in")?;
    let zoomed = driver.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(zoom, _, _)| zoom > 100.0)
    })?;
    let before = viewer_state(&zoomed).expect("BUG: accepted as readable");
    let from = driver.offset(origin, 120.0, 400.0);
    let to = driver.offset(from, -60.0, -80.0);
    driver.drag(from, to, 400, 300)?;
    let panned = driver.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(zoom, x, y)| {
            zoom == before.0
                && (x - (before.1 - 60.0)).abs() <= 3.0
                && (y - (before.2 - 80.0)).abs() <= 3.0
        })
    })?;
    let after = viewer_state(&panned).expect("BUG: accepted as readable");
    driver.pixel(
        "the cell after the pan",
        probe,
        cell_at(scene_under(after, PROBE)),
        CELL_TOLERANCE,
    )
}

fn viewer_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let probe = driver.offset(origin, PROBE.0, PROBE.1);
    // A drag gives the viewer the keys, as it gives a scrollable its own;
    // unzoomed, it has no room to move the grid.
    let from = driver.offset(origin, 60.0, 200.0);
    driver.drag(from, driver.offset(from, 30.0, 0.0), 200, 100)?;
    driver.key("KEYCODE_PLUS")?;
    let zoomed = driver.text_where("Zoom 200%", |_| true)?;
    let state = viewer_state(&zoomed).expect("BUG: accepted as readable");
    driver.pixel(
        "the cell after +",
        probe,
        cell_at(scene_under(state, PROBE)),
        CELL_TOLERANCE,
    )?;
    // The arrow pans one step: the scene moves left under the probe.
    driver.key("KEYCODE_DPAD_RIGHT")?;
    let panned = driver.text_where("Zoom 200%", |text| {
        viewer_state(text).is_some_and(|(_, x, _)| x < state.1 - 20.0)
    })?;
    let after = viewer_state(&panned).expect("BUG: accepted as readable");
    driver.pixel(
        "the cell after the arrow",
        probe,
        cell_at(scene_under(after, PROBE)),
        CELL_TOLERANCE,
    )?;
    driver.key("KEYCODE_MINUS")?;
    driver
        .text_where("Zoom 100%", |_| true)
        .map(drop)
        .map_err(|error| error.context(format!("after {zoomed:?} and {panned:?}")))
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

/// The empty Done lane ends above its first card's slot.
fn empty_done_lane(driver: &Driver<'_>) -> anyhow::Result<()> {
    let slot = done_slot(driver, &PHONE, EMPTY_DONE)?;
    ensure_near(
        "the page below the empty Done lane",
        driver.sample(slot)?,
        BACKGROUND,
        TOLERANCE,
    )
}

/// The moved card in Done, measured once the lanes settled: the To do lane
/// above shrank by the card.
fn card_in_done(driver: &Driver<'_>) -> anyhow::Result<()> {
    let slot = done_slot(driver, &PHONE, "Done (1)")?;
    driver.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_drag(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    empty_done_lane(driver)?;
    let card = driver.bounds_of(TEXT, FIRST_CARD)?;
    let from = left_of(&driver, card, 40.0)?;
    let lane = driver.bounds_of(TEXT, "Done (0)")?;
    // The lanes scroll on a phone, so a card is picked up by a long press,
    // then carried without lifting.
    driver.hold_then_drag(from, lane.centre(), LONG_PRESS, 700, 300)?;
    driver.text("To do 2, done 1, moved Write the release notes to Done")?;
    driver.text("Done (1)")?;
    card_in_done(driver)
}

fn cards_controls(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    empty_done_lane(driver)?;
    driver.tap_node(BUTTON, "Cut Review the layout fix")?;
    driver.tap_node(BUTTON, "Paste into Done")?;
    driver.text("To do 2, done 1, moved Review the layout fix to Done")?;
    card_in_done(driver)
}

fn cards_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    empty_done_lane(driver)?;
    driver.tab_until(20, |name| name.starts_with("Record the demo"))?;
    driver.chord(&["KEYCODE_CTRL_LEFT", "KEYCODE_X"])?;
    driver.tab_until(20, |name| name.starts_with("Done"))?;
    driver.chord(&["KEYCODE_CTRL_LEFT", "KEYCODE_V"])?;
    driver.text("To do 2, done 1, moved Record the demo to Done")?;
    card_in_done(driver)
}

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

/// The last file's middle, and a point over the page below it where a
/// menu opened there draws its panel.
fn under_last_row(driver: &Driver<'_>) -> anyhow::Result<(Point, Point)> {
    let row = driver.bounds_of(TEXT, "N Notes.txt")?;
    let at = left_of(&driver, row, 160.0)?;
    Ok((at, driver.offset(at, 30.0, 70.0)))
}

fn menu_long_press(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(driver)?;
    ensure_near(
        "the page below the files",
        driver.sample(panel)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    driver.gesture(Gesture::press(at, LONG_PRESS))?;
    driver.wait_node(MENU_ITEM, "Duplicate Notes.txt")?;
    anyhow::ensure!(
        driver.find(MENU, |_| true)?.is_some(),
        "no menu node is published"
    );
    driver.wait("the menu's panel over the page", |driver| {
        Ok((!driver.sample(panel)?.near(BACKGROUND, TOLERANCE)).then_some(()))
    })?;
    driver.tap_node(MENU_ITEM, "Duplicate Notes.txt")?;
    driver.text("5 files, last action: made Copy of Notes.txt")?;
    driver.gone(MENU_ITEM, "Duplicate Notes.txt")
}

fn menu_escape(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(driver)?;
    driver.gesture(Gesture::press(at, LONG_PRESS))?;
    driver.wait_node(MENU_ITEM, "Delete Notes.txt")?;
    driver.wait("the menu's panel over the page", |driver| {
        Ok((!driver.sample(panel)?.near(BACKGROUND, TOLERANCE)).then_some(()))
    })?;
    driver.key("KEYCODE_ESCAPE")?;
    driver.gone(MENU_ITEM, "Delete Notes.txt")?;
    driver.pixel(
        "the page once the menu closed",
        panel,
        BACKGROUND,
        TOLERANCE,
    )?;
    driver.stays("the files, after Escape", |driver| {
        Ok(driver.node(TEXT, INITIAL_MENU)?.is_some())
    })
}

fn menu_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let row = driver.tab_until(16, |name| name.contains("Holiday photos"))?;
    driver.key("KEYCODE_MENU")?;
    driver.focused(MENU_ITEM, "Open Holiday photos")?;
    driver.key("KEYCODE_ESCAPE")?;
    driver.gone(MENU_ITEM, "Open Holiday photos")?;
    // Escape gives the focus back to the row the menu opened from.
    driver
        .wait("the focus back on the row", |driver| {
            Ok((driver.focus_name() == row).then_some(()))
        })
        .map_err(|error| error.context(format!("focus is on {:?}", driver.focus_name())))?;
    driver.key("KEYCODE_MENU")?;
    driver.focused(MENU_ITEM, "Open Holiday photos")?;
    driver.key("KEYCODE_DPAD_DOWN")?;
    driver.focused(MENU_ITEM, "Duplicate Holiday photos")?;
    driver.key("KEYCODE_ENTER")?;
    driver.text("5 files, last action: made Copy of Holiday photos")?;
    driver.gone(MENU_ITEM, "Open Holiday photos")
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

/// The pad's middle, and a point of its fill clear of its text.
fn pad(driver: &Driver<'_>) -> anyhow::Result<(Point, Point)> {
    let label = driver.wait("the gesture pad", |driver| {
        Ok(driver
            .find(TEXT, |name| name.starts_with("Gesture pad"))?
            .and_then(|element| element.rect))
    })?;
    let middle = label.centre();
    Ok((
        driver.offset(middle, 0.0, 80.0),
        driver.offset(middle, -120.0, -150.0),
    ))
}

fn gesture(
    driver: &Driver<'_>,
    act: impl FnOnce(Point) -> anyhow::Result<()>,
    readout: &str,
    tint: Rgb,
) -> anyhow::Result<()> {
    let (middle, fill) = pad(driver)?;
    ensure_near("the idle pad", driver.sample(fill)?, PAD_IDLE, TOLERANCE)?;
    act(middle)?;
    driver.text(readout)?;
    driver.pixel("the pad's tint", fill, tint, TOLERANCE)
}

const TAPPED: &str = "Taps 1, double taps 0, long presses 0, pans 0, scales 0; last: tap";

fn gestures_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(driver, |at| driver.tap(at), TAPPED, TINT_TAP)
}

fn gestures_double_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.gesture(Gesture::double_tap(at, 80)),
        "Taps 0, double taps 1, long presses 0, pans 0, scales 0; last: double tap",
        TINT_DOUBLE,
    )
}

fn gestures_long_press(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.gesture(Gesture::press(at, LONG_PRESS)),
        "Taps 0, double taps 0, long presses 1, pans 0, scales 0; last: long press",
        TINT_LONG,
    )
}

fn gestures_pan(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.drag(at, driver.offset(at, 0.0, 200.0), 300, 0),
        "Taps 0, double taps 0, long presses 0, pans 1, scales 0; last: pan",
        TINT_PAN,
    )
}

fn gestures_pinch(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| {
            // Both fingers on the pad, whose middle `at` lies 80 below.
            let a = (
                driver.offset(at, 0.0, -260.0),
                driver.offset(at, 0.0, -140.0),
            );
            let b = (driver.offset(at, 0.0, 100.0), driver.offset(at, 0.0, -20.0));
            driver.pinch(a, b, 500)
        },
        "Taps 0, double taps 0, long presses 0, pans 0, scales 1; last: scale",
        TINT_SCALE,
    )
}

fn gestures_stylus_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.input(&format!("stylus tap {} {}", at.x, at.y)),
        TAPPED,
        TINT_TAP,
    )
}

fn gestures_mouse_click(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.input(&format!("mouse tap {} {}", at.x, at.y)),
        TAPPED,
        TINT_TAP,
    )
}

// ---------------------------------------------------------------------------
// Focus and keys
// ---------------------------------------------------------------------------

/// The focus ring's dark outer band runs along the inside of the focused
/// control's edge (`FocusRingStyle::BUILT_IN`): its left edge, halfway down.
fn ring_point(driver: &Driver<'_>, button: &str) -> anyhow::Result<Point> {
    let rect = driver.bounds_of(BUTTON, button)?;
    Ok(Point {
        x: rect.left + 1,
        y: rect.centre().y,
    })
}

fn focus_tab_order(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    let ring = ring_point(driver, "Save")?;
    ensure_near("Save's edge", driver.sample(ring)?, ACCENT, TOLERANCE)?;
    driver.tap_node(EDIT, "")?;
    driver.text("Focused: Name field; saved name: none; settings page: Settings")?;
    driver.key("KEYCODE_ESCAPE")?;
    driver.key("KEYCODE_TAB")?;
    driver.focused(BUTTON, "Save")?;
    driver.text("Focused: Save; saved name: none; settings page: Settings")?;
    driver.pixel("the focus ring on Save", ring, RING, 40)?;
    driver.key("KEYCODE_TAB")?;
    driver.focused(BUTTON, "Clear")?;
    driver.pixel("Save's edge once focus moved on", ring, ACCENT, TOLERANCE)?;
    // One Tab from the last control of the form enters the settings
    // navigator at its first button: no unnamed stop in between.
    driver.key("KEYCODE_TAB")?;
    driver
        .focused(BUTTON, "Open Privacy")
        .map_err(|error| error.context(format!("Tab went to {:?}", driver.focus_name())))?;
    driver.chord(&["KEYCODE_SHIFT_LEFT", "KEYCODE_TAB"])?;
    driver.focused(BUTTON, "Clear")?;
    driver.text("Focused: Clear; saved name: none; settings page: Settings")
}

fn focus_soft_keyboard(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    driver.tap_node(EDIT, "")?;
    driver.focused(EDIT, "")?;
    let (package, keys) = driver.keyboard()?;
    let key = |label: &str| {
        keys.iter()
            .find(|key| key.label.eq_ignore_ascii_case(label))
            .map(|key| Point {
                x: key.center.0,
                y: key.center.1,
            })
    };
    // Four letter keys of whatever layout the keyboard shows, in reading
    // order: the word is the device's, not the check's.
    let letters: Vec<&super::super::android::ImeKey> = keys
        .iter()
        .filter(|key| {
            let mut chars = key.label.chars();
            chars.next().is_some_and(char::is_alphabetic) && chars.next().is_none()
        })
        .take(4)
        .collect();
    anyhow::ensure!(
        letters.len() == 4,
        "the keyboard of {package} shows no four letter keys: {:?}",
        keys.iter()
            .map(|key| key.label.as_str())
            .collect::<Vec<_>>()
    );
    let word: String = letters.iter().map(|key| key.label.as_str()).collect();
    for letter in &letters {
        driver.tap(Point {
            x: letter.center.0,
            y: letter.center.1,
        })?;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let action = ["Done", "Enter", "Go", "Next"]
        .into_iter()
        .find_map(key)
        .ok_or_else(|| anyhow::anyhow!("the keyboard of {package} names no action key"))?;
    driver.tap(action)?;
    driver
        .text_where("Focused: ", |text| {
            text.contains("saved name: ") && !text.contains("saved name: none")
        })
        .and_then(|text| {
            anyhow::ensure!(
                text.contains(&format!("saved name: {word};")),
                "typed {word:?}, but the field saved something else: {text:?}"
            );
            Ok(())
        })
}

fn focus_navigator(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    driver.tab_until(8, |name| name == "Open Privacy")?;
    driver.key("KEYCODE_ENTER")?;
    driver.text("Privacy options would live here.")?;
    let back = driver.bounds_of(BUTTON, "Back")?;
    let fill = left_of(&driver, back, 8.0)?;
    driver.pixel("the Back button", fill, ACCENT, TOLERANCE)?;
    driver.tab_until(6, |name| name == "Back")?;
    driver.key("KEYCODE_SPACE")?;
    driver.wait_node(BUTTON, "Open Privacy")?;
    driver.gone(BUTTON, "Back")?;
    driver
        .text_where("Focused: ", |text| {
            text.ends_with("settings page: Settings")
        })
        .map(drop)
}
