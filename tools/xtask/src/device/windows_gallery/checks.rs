//! The gallery's checks, several per screen. Each opens its screen fresh,
//! acts with real input, and reads the outcome from the UIA tree and from
//! pixels sampled where the right and the wrong outcome differ.

use std::time::Duration;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_APPS, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F5, VK_F10, VK_HOME, VK_OEM_MINUS, VK_OEM_PLUS,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

use super::super::gallery::{
    ACCENT, ADA, ARCHIVE, BACKGROUND, BROOK, CARDS, COLLAPSING, CONTACTLESS_LATCH_IDLE, DESKTOP,
    EMPTY_DONE, FIRST_CARD, FIRST_STOP, FOCUS, FULL, GESTURES, HALF, INITIAL_FOCUS, INITIAL_MENU,
    INITIAL_SWIPE, LANE, MAP_GROUND, MENU_SCREEN, PAD_IDLE, PAGE_COLORS, PAGES, PIN, REFRESH, RING,
    Rgb, SHEET, SHEET_NODE, SURFACE, SWIPE, TINT_DOUBLE, TINT_LONG, TINT_PAN, TINT_SCALE, TINT_TAP,
    TOLERANCE, VIEWER, below_list, cell_at, done_slot, ensure_near, grid_origin, in_header,
    left_of, pager_point, right_of, scene_under, viewer_state,
};
use super::super::uia::{BUTTON, TEXT};
use super::super::windows_input::centre;
use super::{Check, Driver, EDIT, MENU, MENU_ITEM, SLIDER};

/// Every check, in the order it runs.
pub(super) const CHECKS: &[(&str, Check)] = &[
    check(
        "swipe: a touch swipe reveals Archive and tapping it archives",
        SWIPE,
        swipe_touch_archive,
    ),
    check(
        "swipe: a mouse drag reveals Pin and a click pins",
        SWIPE,
        swipe_mouse_pin,
    ),
    check(
        "swipe: a touch fling dismisses a task",
        SWIPE,
        swipe_dismiss_task,
    ),
    check(
        "swipe: the Done control completes a task",
        SWIPE,
        swipe_done_control,
    ),
    check(
        "refresh: a touch pull past the top refreshes",
        REFRESH,
        refresh_pull,
    ),
    check(
        "refresh: the Refresh control refreshes",
        REFRESH,
        refresh_control,
    ),
    check(
        "refresh: F5 with focus in the list refreshes",
        REFRESH,
        refresh_key,
    ),
    check(
        "collapsing: the wheel collapses the header before the list",
        COLLAPSING,
        collapsing_wheel,
    ),
    check(
        "collapsing: a touch drag collapses the header",
        COLLAPSING,
        collapsing_touch,
    ),
    check("pages: a touch swipe turns the page", PAGES, pages_swipe),
    check(
        "pages: the arrow keys and Home turn pages",
        PAGES,
        pages_keys,
    ),
    check("pages: the Next button turns the page", PAGES, pages_button),
    check(
        "sheet: a touch drag on the handle rests at Full",
        SHEET,
        sheet_touch_full,
    ),
    check(
        "sheet: a mouse drag on the handle rests at Peek",
        SHEET,
        sheet_mouse_peek,
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
        "viewer: + and - zoom and the arrows pan",
        VIEWER,
        viewer_keys,
    ),
    check("viewer: a mouse drag pans", VIEWER, viewer_mouse_pan),
    check(
        "cards: a mouse drag moves a card to Done",
        CARDS,
        cards_drag,
    ),
    check(
        "cards: the Cut and Paste controls move a card",
        CARDS,
        cards_controls,
    ),
    check(
        "cards: Ctrl+X on a card and Ctrl+V on a column move it",
        CARDS,
        cards_keys,
    ),
    check(
        "menu: a right-click menu, Down and Enter duplicate",
        MENU_SCREEN,
        menu_right_click,
    ),
    check(
        "menu: Escape closes the menu and chooses nothing",
        MENU_SCREEN,
        menu_escape,
    ),
    check(
        "menu: Shift+F10 and the menu key open the focused row's menu",
        MENU_SCREEN,
        menu_keys,
    ),
    check(
        "menu: a touch long press opens the menu",
        MENU_SCREEN,
        menu_long_press,
    ),
    check("gestures: a click is a tap", GESTURES, gestures_tap),
    check(
        "gestures: a double click is a double tap",
        GESTURES,
        gestures_double_tap,
    ),
    check(
        "gestures: a held press is a long press",
        GESTURES,
        gestures_long_press,
    ),
    check("gestures: a mouse drag is a pan", GESTURES, gestures_pan),
    check(
        "gestures: a touch tap is a tap",
        GESTURES,
        gestures_touch_tap,
    ),
    check(
        "gestures: a two-finger pinch is a scale",
        GESTURES,
        gestures_pinch,
    ),
    check(
        "focus: Tab and Shift+Tab move the focus ring in order",
        FOCUS,
        focus_tab_order,
    ),
    check(
        "focus: typing and Enter in the field save it",
        FOCUS,
        focus_field,
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

// ---------------------------------------------------------------------------
// Swipe actions
// ---------------------------------------------------------------------------

/// The message row's full width: its merged label spans the content, which
/// fills the row.
fn row_rect(driver: &Driver<'_>, label: &str) -> anyhow::Result<RECT> {
    driver.bounds_of(TEXT, label)
}

fn swipe_touch_archive(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let row = row_rect(driver, ADA)?;
    let tray = right_of(&driver, row, 30.0)?;
    ensure_near(
        "the row's right end",
        driver.sample(tray)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = right_of(&driver, row, 60.0)?;
    let to = driver.offset(from, -150.0, 0.0)?;
    driver.touch_drag(
        from,
        to,
        Duration::from_millis(300),
        Duration::from_millis(150),
    )?;
    driver.wait_node(BUTTON, "Archive Ada")?;
    driver.pixel("the revealed Archive action", tray, ARCHIVE, TOLERANCE)?;
    let archive = centre(driver.bounds_of(BUTTON, "Archive Ada")?);
    driver.touch_tap(archive)?;
    driver.text("3 in inbox, 0 pinned, 1 archived, 3 tasks left, 0 completed")?;
    driver.gone(TEXT, ADA)
}

fn swipe_mouse_pin(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let row = row_rect(driver, BROOK)?;
    // Between the row's left edge and its avatar: where the revealed action
    // slides in.
    let tray = left_of(&driver, row, 8.0)?;
    ensure_near(
        "the row's left end",
        driver.sample(tray)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = left_of(&driver, row, 120.0)?;
    let to = driver.offset(from, 150.0, 0.0)?;
    driver.mouse_drag(from, to, 12, Duration::from_millis(150))?;
    driver.wait_node(BUTTON, "Pin Brook")?;
    driver.pixel("the revealed Pin action", tray, PIN, TOLERANCE)?;
    driver.click_node(BUTTON, "Pin Brook")?;
    driver.text("4 in inbox, 1 pinned, 0 archived, 3 tasks left, 0 completed")?;
    driver.text("B Brook (pinned) Your order has shipped")
}

fn swipe_dismiss_task(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_SWIPE)?;
    let first = driver.bounds_of(TEXT, "Water the plants")?;
    let last = driver.bounds_of(TEXT, "Call the bank")?;
    // Where the last task is now: a row, and once a task is gone, the page
    // behind the shortened list.
    let tail = driver.offset(
        POINT {
            x: last.left,
            y: centre(last).y,
        },
        300.0,
        0.0,
    )?;
    ensure_near(
        "the last task row",
        driver.sample(tail)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = driver.offset(
        POINT {
            x: first.left,
            y: centre(first).y,
        },
        40.0,
        0.0,
    )?;
    let to = driver.offset(from, 420.0, 0.0)?;
    driver.touch_drag(from, to, Duration::from_millis(120), Duration::ZERO)?;
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
    let last = driver.bounds_of(TEXT, "Call the bank")?;
    let tail = driver.offset(
        POINT {
            x: last.left,
            y: centre(last).y,
        },
        300.0,
        0.0,
    )?;
    driver.click_node(BUTTON, "Complete Book the train")?;
    driver.text("4 in inbox, 0 pinned, 0 archived, 2 tasks left, 1 completed")?;
    driver.gone(TEXT, "Book the train")?;
    driver.pixel(
        "the page below the shortened task list",
        tail,
        BACKGROUND,
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Pull to refresh
// ---------------------------------------------------------------------------

fn refresh_pull(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &DESKTOP)?;
    ensure_near(
        "below the list",
        driver.sample(below)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    let top = driver.bounds_of(TEXT, "Message 6")?;
    let from = driver.offset(centre(top), 100.0, 0.0)?;
    let to = driver.offset(from, 0.0, 220.0)?;
    driver.touch_drag(
        from,
        to,
        Duration::from_millis(400),
        Duration::from_millis(300),
    )?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.text("Message 7")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

fn refresh_control(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &DESKTOP)?;
    ensure_near(
        "below the list",
        driver.sample(below)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    driver.click_node(BUTTON, "Refresh inbox")?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

fn refresh_key(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(driver, &DESKTOP)?;
    driver.tab_until(16, |name| name == "Refresh inbox" || name == "Messages")?;
    driver.chord(&[VK_F5])?;
    driver.text("7 messages, 1 refreshes, idle")?;
    driver.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

// ---------------------------------------------------------------------------
// Collapsing header
// ---------------------------------------------------------------------------

fn collapsing_wheel(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Itinerary scrolled 0 px")?;
    let header = in_header(driver, &DESKTOP)?;
    ensure_near(
        "the expanded header",
        driver.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let list = driver.offset(centre(driver.bounds_of(TEXT, FIRST_STOP)?), 0.0, 120.0)?;
    // The header goes first: a notch collapses it, and only what it leaves
    // over reaches the list.
    driver.wheel(list, -1, false)?;
    driver.wait("the header to leave the sample point", |driver| {
        Ok((!driver.sample(header)?.near(ACCENT, TOLERANCE)).then_some(()))
    })?;
    let scrolled = driver.text_where("Itinerary scrolled ", |_| true)?;
    driver.wheel(list, -4, false)?;
    driver.text_where("Itinerary scrolled ", |text| text != scrolled)?;
    // Back up: the list returns to its top before the header expands.
    driver.wheel(list, 12, false)?;
    driver
        .text("Itinerary scrolled 0 px")
        .map_err(|error| error.context(format!("after {scrolled:?}, scrolling back up")))?;
    // At its top, the list leaves the next wheel burst to the header. The
    // burst that brought the list back is latched to the list for as long
    // as its notches keep coming (ADR-0209 §5, the preset's contactless
    // idle time), as a drag is for as long as its contact lasts.
    std::thread::sleep(CONTACTLESS_LATCH_IDLE + Duration::from_millis(300));
    driver.wheel(list, 3, false)?;
    driver.pixel("the header expanded again", header, ACCENT, TOLERANCE)
}

fn collapsing_touch(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Itinerary scrolled 0 px")?;
    let header = in_header(driver, &DESKTOP)?;
    ensure_near(
        "the expanded header",
        driver.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let from = driver.offset(centre(driver.bounds_of(TEXT, FIRST_STOP)?), 0.0, 250.0)?;
    let to = driver.offset(from, 0.0, -200.0)?;
    driver.touch_drag(
        from,
        to,
        Duration::from_millis(500),
        Duration::from_millis(150),
    )?;
    driver.wait("the header to collapse past the sample point", |driver| {
        Ok((!driver.sample(header)?.near(ACCENT, TOLERANCE)).then_some(()))
    })
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn pages_swipe(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &DESKTOP)?;
    ensure_near("page 1", driver.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    let from = driver.offset(point, 250.0, 0.0)?;
    let to = driver.offset(point, -250.0, 0.0)?;
    driver.touch_drag(from, to, Duration::from_millis(200), Duration::ZERO)?;
    driver.text("Page 2 of 3: Pack")?;
    driver.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)
}

fn pages_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &DESKTOP)?;
    // A drag gives the pager the focus its keys need.
    let from = driver.offset(point, 120.0, 0.0)?;
    let to = driver.offset(point, -120.0, 0.0)?;
    driver.mouse_drag(from, to, 10, Duration::ZERO)?;
    driver.text("Page 2 of 3: Pack")?;
    driver.chord(&[VK_RIGHT])?;
    driver.text("Page 3 of 3: Go")?;
    driver.pixel("page 3", point, PAGE_COLORS[2], TOLERANCE)?;
    driver.chord(&[VK_HOME])?;
    driver.text("Page 1 of 3: Plan")?;
    driver.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

fn pages_button(driver: &Driver<'_>) -> anyhow::Result<()> {
    let point = pager_point(driver, &DESKTOP)?;
    ensure_near("page 1", driver.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    driver.click_node(BUTTON, "Next")?;
    driver.text("Page 2 of 3: Pack")?;
    driver.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)?;
    driver.click_node(BUTTON, "Previous")?;
    driver.text("Page 1 of 3: Plan")?;
    driver.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

// ---------------------------------------------------------------------------
// Bottom sheet
// ---------------------------------------------------------------------------

/// The map's top-left corner: the slider's node spans the drag's whole
/// area, which the map fills.
fn map_origin(driver: &Driver<'_>) -> anyhow::Result<POINT> {
    let sheet = driver.bounds_of(SLIDER, SHEET_NODE)?;
    Ok(POINT {
        x: sheet.left,
        y: sheet.top,
    })
}

/// The sheet's handle at Half.
fn handle(driver: &Driver<'_>) -> anyhow::Result<POINT> {
    let sheet = driver.bounds_of(SLIDER, SHEET_NODE)?;
    driver.offset(
        POINT {
            x: centre(sheet).x,
            y: sheet.top,
        },
        0.0,
        HALF + 13.0,
    )
}

fn sheet_touch_full(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let map = map_origin(driver)?;
    // A block of the map, clear of every street, above the sheet at Half
    // and under it at Full.
    let probe = driver.offset(map, 250.0, 100.0)?;
    ensure_near(
        "the map above the sheet",
        driver.sample(probe)?,
        MAP_GROUND,
        TOLERANCE,
    )?;
    let from = handle(driver)?;
    let to = driver.offset(from, 0.0, FULL - HALF)?;
    driver.touch_drag(
        from,
        to,
        Duration::from_millis(500),
        Duration::from_millis(200),
    )?;
    driver.text("Sheet at Full")?;
    driver.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)
}

fn sheet_mouse_peek(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let map = map_origin(driver)?;
    // Between Half and Peek: the sheet now, the map once it rests at Peek.
    let probe = driver.offset(map, 250.0, 320.0)?;
    ensure_near(
        "the sheet at Half",
        driver.sample(probe)?,
        SURFACE,
        TOLERANCE,
    )?;
    let from = handle(driver)?;
    let to = driver.offset(from, 0.0, 190.0)?;
    driver.mouse_drag(from, to, 16, Duration::from_millis(200))?;
    driver.text("Sheet at Peek")?;
    driver.pixel(
        "the map above the sheet at Peek",
        probe,
        MAP_GROUND,
        TOLERANCE,
    )
}

fn sheet_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("Sheet at Half")?;
    let map = map_origin(driver)?;
    let probe = driver.offset(map, 250.0, 100.0)?;
    driver.tab_until(16, |name| name.starts_with("Places sheet"))?;
    driver.chord(&[VK_UP])?;
    driver.text("Sheet at Full")?;
    driver.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)?;
    driver.chord(&[VK_DOWN])?;
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

/// Waits for the readout to state `zoom` and an offset within two logical
/// pixels of `offset` (the readout rounds, the origin is read in physical
/// pixels).
fn viewer_reads(driver: &Driver<'_>, zoom: f64, offset: (f64, f64)) -> anyhow::Result<String> {
    driver.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(z, x, y)| {
            z == zoom && (x - offset.0).abs() <= 2.0 && (y - offset.1).abs() <= 2.0
        })
    })
}

fn viewer_double_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let tap = driver.offset(origin, 205.0, 205.0)?;
    let probe = driver.offset(origin, 295.0, 255.0)?;
    ensure_near(
        "the cell before the zoom",
        driver.sample(probe)?,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    driver.touch_tap(tap)?;
    std::thread::sleep(Duration::from_millis(80));
    driver.touch_tap(tap)?;
    viewer_reads(driver, 200.0, (-205.0, -205.0))?;
    // About the tap, the probe now shows the scene halfway to it.
    driver.pixel(
        "the cell after the zoom",
        probe,
        cell_at((250.0, 230.0)),
        TOLERANCE,
    )
}

fn viewer_spread(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let focal = driver.offset(origin, 200.0, 200.0)?;
    let probe = driver.offset(origin, 295.0, 255.0)?;
    let before = driver.sample(probe)?;
    ensure_near(
        "the cell before the spread",
        before,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    let a = (
        driver.offset(focal, -40.0, 0.0)?,
        driver.offset(focal, -120.0, 0.0)?,
    );
    let b = (
        driver.offset(focal, 40.0, 0.0)?,
        driver.offset(focal, 120.0, 0.0)?,
    );
    driver.touch_pinch(a, b, Duration::from_millis(500))?;
    let zoom = driver.text_where("Zoom ", |text| {
        text.strip_prefix("Zoom ")
            .and_then(|rest| rest.split('%').next())
            .and_then(|percent| percent.parse::<f64>().ok())
            .is_some_and(|percent| percent > 150.0)
    })?;
    driver
        .wait("the probe to show another cell", |driver| {
            Ok((!driver.sample(probe)?.near(before, TOLERANCE)).then_some(()))
        })
        .map_err(|error| error.context(format!("readout {zoom:?}")))?;
    Ok(())
}

fn viewer_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    // A drag gives the viewer the keys, as it gives a scrollable its own;
    // unzoomed, it has no room to move the grid.
    let from = driver.offset(origin, 100.0, 100.0)?;
    driver.mouse_drag(
        from,
        driver.offset(from, 40.0, 0.0)?,
        6,
        Duration::from_millis(100),
    )?;
    driver.wait("the viewer's focus", |driver| {
        Ok(driver
            .find(SLIDER, |_| true)?
            .filter(super::super::uia::has_keyboard_focus)
            .map(drop))
    })?;
    let probe = driver.offset(origin, 295.0, 255.0)?;
    driver.chord(&[VK_OEM_PLUS])?;
    let zoomed = driver.text_where("Zoom 200%", |_| true)?;
    let (_, x, y) =
        viewer_state(&zoomed).ok_or_else(|| anyhow::anyhow!("unreadable readout {zoomed:?}"))?;
    // At 200 % the probe shows the scene point (probe - offset) / 2.
    let scene = |x: f64, y: f64| ((295.0 - x) / 2.0, (255.0 - y) / 2.0);
    driver.pixel("the cell after +", probe, cell_at(scene(x, y)), TOLERANCE)?;
    // The arrow pans one step: the scene moves left under the probe.
    driver.chord(&[VK_RIGHT])?;
    let panned = driver.text_where("Zoom 200%", |text| {
        viewer_state(text).is_some_and(|(_, panned, _)| panned < x - 20.0)
    })?;
    let (_, px, py) =
        viewer_state(&panned).ok_or_else(|| anyhow::anyhow!("unreadable readout {panned:?}"))?;
    driver.pixel(
        "the cell after the arrow",
        probe,
        cell_at(scene(px, py)),
        TOLERANCE,
    )?;
    driver.chord(&[VK_OEM_MINUS])?;
    driver
        .text_where("Zoom 100%", |_| true)
        .map(drop)
        .map_err(|error| error.context(format!("after {zoomed:?} and {panned:?}")))
}

fn viewer_mouse_pan(driver: &Driver<'_>) -> anyhow::Result<()> {
    let origin = grid_origin(driver)?;
    let probe = driver.offset(origin, 295.0, 255.0)?;
    ensure_near(
        "the cell before the zoom",
        driver.sample(probe)?,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    // The child fills the viewport, so there is room to pan only zoomed in:
    // Ctrl+wheel zooms about the cursor.
    driver.wheel(driver.offset(origin, 200.0, 200.0)?, 2, true)?;
    let zoomed = driver.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(zoom, _, _)| zoom > 100.0)
    })?;
    let before = viewer_state(&zoomed).expect("BUG: accepted as readable");
    driver.pixel(
        "the cell after Ctrl+wheel",
        probe,
        cell_at(scene_under(before, (295.0, 255.0))),
        TOLERANCE,
    )?;
    let from = driver.offset(origin, 300.0, 300.0)?;
    let to = driver.offset(from, -80.0, -60.0)?;
    driver.mouse_drag(from, to, 12, Duration::from_millis(300))?;
    let panned = viewer_reads(driver, before.0, (before.1 - 80.0, before.2 - 60.0))?;
    let after = viewer_state(&panned).expect("BUG: accepted as readable");
    driver.pixel(
        "the cell after the pan",
        probe,
        cell_at(scene_under(after, (295.0, 255.0))),
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

fn cards_drag(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    let slot = done_slot(driver, &DESKTOP, EMPTY_DONE)?;
    ensure_near("the empty Done lane", driver.sample(slot)?, LANE, TOLERANCE)?;
    let card = driver.bounds_of(TEXT, FIRST_CARD)?;
    let from = driver.offset(
        POINT {
            x: card.left,
            y: centre(card).y,
        },
        60.0,
        0.0,
    )?;
    driver.mouse_drag(
        from,
        driver.offset(slot, 40.0, 60.0)?,
        30,
        Duration::from_millis(250),
    )?;
    driver.text("To do 2, done 1, moved Write the release notes to Done")?;
    driver.text("Done (1)")?;
    driver.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_controls(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    let slot = done_slot(driver, &DESKTOP, EMPTY_DONE)?;
    driver.click_node(BUTTON, "Cut Review the layout fix")?;
    driver.click_node(BUTTON, "Paste into Done")?;
    driver.text("To do 2, done 1, moved Review the layout fix to Done")?;
    driver.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_keys(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text("To do 3, done 0")?;
    let slot = done_slot(driver, &DESKTOP, EMPTY_DONE)?;
    driver.tab_until(20, |name| name.starts_with("Record the demo"))?;
    driver.chord(&[VK_CONTROL, VIRTUAL_X])?;
    driver.tab_until(20, |name| name.starts_with("Done"))?;
    driver.chord(&[VK_CONTROL, VIRTUAL_V])?;
    driver.text("To do 2, done 1, moved Record the demo to Done")?;
    driver.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

const VIRTUAL_X: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY =
    windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0x58);
const VIRTUAL_V: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY =
    windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0x56);

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

/// A point over the page below the last file, where a menu opened at the
/// last row's middle draws its panel.
fn under_last_row(driver: &Driver<'_>) -> anyhow::Result<(POINT, POINT)> {
    let row = driver.bounds_of(TEXT, "N Notes.txt")?;
    let at = driver.offset(
        POINT {
            x: row.left,
            y: centre(row).y,
        },
        160.0,
        0.0,
    )?;
    Ok((at, driver.offset(at, 40.0, 70.0)?))
}

fn menu_right_click(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(driver)?;
    ensure_near(
        "the page below the files",
        driver.sample(panel)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    driver.right_click(at)?;
    driver.wait_node(MENU_ITEM, "Duplicate Notes.txt")?;
    anyhow::ensure!(
        driver.find(MENU, |_| true)?.is_some(),
        "no menu node is published"
    );
    driver.wait("the menu's panel over the page", |driver| {
        Ok((!driver.sample(panel)?.near(BACKGROUND, TOLERANCE)).then_some(()))
    })?;
    driver.focused(MENU_ITEM, "Open Notes.txt")?;
    driver.chord(&[VK_DOWN])?;
    driver.focused(MENU_ITEM, "Duplicate Notes.txt")?;
    driver.chord(&[VK_RETURN])?;
    driver.text("5 files, last action: made Copy of Notes.txt")?;
    driver.gone(MENU_ITEM, "Duplicate Notes.txt")
}

fn menu_escape(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(driver)?;
    driver.right_click(at)?;
    driver.wait_node(MENU_ITEM, "Delete Notes.txt")?;
    driver.wait("the menu's panel over the page", |driver| {
        Ok((!driver.sample(panel)?.near(BACKGROUND, TOLERANCE)).then_some(()))
    })?;
    driver.chord(&[VK_ESCAPE])?;
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
    driver.chord(&[VK_SHIFT, VK_F10])?;
    driver.wait_node(MENU_ITEM, "Open Holiday photos")?;
    driver.chord(&[VK_ESCAPE])?;
    driver.gone(MENU_ITEM, "Open Holiday photos")?;
    // Escape gives the focus back to the row the menu opened from.
    driver
        .wait("the focus back on the row", |driver| {
            Ok((driver.focus_name() == row).then_some(()))
        })
        .map_err(|error| error.context(format!("focus is on {:?}", driver.focus_name())))?;
    driver.chord(&[VK_APPS])?;
    driver.focused(MENU_ITEM, "Open Holiday photos")?;
    driver.chord(&[VK_RETURN])?;
    driver.text("4 files, last action: opened Holiday photos")?;
    driver.gone(MENU_ITEM, "Open Holiday photos")
}

fn menu_long_press(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(driver)?;
    driver.touch_drag(
        at,
        at,
        Duration::from_millis(16),
        Duration::from_millis(900),
    )?;
    driver.wait_node(MENU_ITEM, "Delete Notes.txt")?;
    driver.wait("the menu's panel over the page", |driver| {
        Ok((!driver.sample(panel)?.near(BACKGROUND, TOLERANCE)).then_some(()))
    })?;
    driver.touch_tap(centre(driver.bounds_of(MENU_ITEM, "Delete Notes.txt")?))?;
    driver.text("3 files, last action: deleted Notes.txt")
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

/// The pad's middle, and a point of its fill clear of its text.
fn pad(driver: &Driver<'_>) -> anyhow::Result<(POINT, POINT)> {
    let label = driver.wait("the gesture pad", |driver| {
        driver
            .find(TEXT, |name| name.starts_with("Gesture pad"))?
            .map(|node| super::super::uia::bounds(&node))
            .transpose()
    })?;
    let middle = centre(label);
    Ok((
        driver.offset(middle, 0.0, 80.0)?,
        driver.offset(middle, -200.0, -120.0)?,
    ))
}

fn gesture(
    driver: &Driver<'_>,
    act: impl FnOnce(POINT) -> anyhow::Result<()>,
    readout: &str,
    tint: Rgb,
) -> anyhow::Result<()> {
    let (middle, fill) = pad(driver)?;
    ensure_near("the idle pad", driver.sample(fill)?, PAD_IDLE, TOLERANCE)?;
    act(middle)?;
    driver.text(readout)?;
    driver.pixel("the pad's tint", fill, tint, TOLERANCE)
}

fn gestures_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.click(at),
        "Taps 1, double taps 0, long presses 0, pans 0, scales 0; last: tap",
        TINT_TAP,
    )
}

fn gestures_double_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.double_click(at),
        "Taps 0, double taps 1, long presses 0, pans 0, scales 0; last: double tap",
        TINT_DOUBLE,
    )
}

fn gestures_long_press(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.press_and_hold(at, Duration::from_millis(900)),
        "Taps 0, double taps 0, long presses 1, pans 0, scales 0; last: long press",
        TINT_LONG,
    )
}

fn gestures_pan(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.mouse_drag(at, driver.offset(at, 200.0, 0.0)?, 12, Duration::ZERO),
        "Taps 0, double taps 0, long presses 0, pans 1, scales 0; last: pan",
        TINT_PAN,
    )
}

fn gestures_touch_tap(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| driver.touch_tap(at),
        "Taps 1, double taps 0, long presses 0, pans 0, scales 0; last: tap",
        TINT_TAP,
    )
}

fn gestures_pinch(driver: &Driver<'_>) -> anyhow::Result<()> {
    gesture(
        driver,
        |at| {
            let a = (
                driver.offset(at, -150.0, 0.0)?,
                driver.offset(at, -50.0, 0.0)?,
            );
            let b = (
                driver.offset(at, 150.0, 0.0)?,
                driver.offset(at, 50.0, 0.0)?,
            );
            driver.touch_pinch(a, b, Duration::from_millis(500))
        },
        "Taps 0, double taps 0, long presses 0, pans 0, scales 1; last: scale",
        TINT_SCALE,
    )
}

// ---------------------------------------------------------------------------
// Focus and keys
// ---------------------------------------------------------------------------

/// The focus ring's dark outer band runs along the inside of the focused
/// control's edge (`FocusRingStyle::BUILT_IN`): its left edge, halfway down.
fn ring_point(driver: &Driver<'_>, button: &str) -> anyhow::Result<POINT> {
    let rect = driver.bounds_of(BUTTON, button)?;
    Ok(POINT {
        x: rect.left + 1,
        y: centre(rect).y,
    })
}

fn focus_tab_order(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    let ring = ring_point(driver, "Save")?;
    ensure_near("Save's edge", driver.sample(ring)?, ACCENT, TOLERANCE)?;
    driver.click_node(EDIT, "")?;
    driver.text("Focused: Name field; saved name: none; settings page: Settings")?;
    driver.chord(&[VK_TAB])?;
    driver.focused(BUTTON, "Save")?;
    driver.text("Focused: Save; saved name: none; settings page: Settings")?;
    driver.pixel("the focus ring on Save", ring, RING, 40)?;
    driver.chord(&[VK_TAB])?;
    driver.focused(BUTTON, "Clear")?;
    driver.pixel("Save's edge once focus moved on", ring, ACCENT, TOLERANCE)?;
    // One Tab from the last control of the form enters the settings
    // navigator at its first button: no unnamed stop in between.
    driver.chord(&[VK_TAB])?;
    driver
        .focused(BUTTON, "Open Privacy")
        .map_err(|error| error.context(format!("Tab went to {:?}", driver.focus_name())))?;
    driver.text("Focused: Open Privacy; saved name: none; settings page: Settings")?;
    driver.chord(&[VK_SHIFT, VK_TAB])?;
    driver.focused(BUTTON, "Clear")?;
    driver.text("Focused: Clear; saved name: none; settings page: Settings")
}

fn focus_field(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    driver.click_node(EDIT, "")?;
    driver.focused(EDIT, "")?;
    driver.type_text("Grace")?;
    driver.chord(&[VK_RETURN])?;
    driver.text("Focused: Name field; saved name: Grace; settings page: Settings")?;
    // Space on the focused Clear presses it.
    driver.tab_until(3, |name| name == "Clear")?;
    driver.chord(&[VK_SPACE])?;
    driver.text("Focused: Clear; saved name: none; settings page: Settings")
}

fn focus_navigator(driver: &Driver<'_>) -> anyhow::Result<()> {
    driver.text(INITIAL_FOCUS)?;
    driver.click_node(EDIT, "")?;
    driver.tab_until(6, |name| name == "Open Privacy")?;
    driver.chord(&[VK_RETURN])?;
    driver.text("Privacy options would live here.")?;
    let back = driver.bounds_of(BUTTON, "Back")?;
    let fill = driver.offset(
        POINT {
            x: back.left,
            y: centre(back).y,
        },
        8.0,
        0.0,
    )?;
    driver.pixel("the Back button", fill, ACCENT, TOLERANCE)?;
    driver.tab_until(6, |name| name == "Back")?;
    driver.chord(&[VK_SPACE])?;
    driver.wait_node(BUTTON, "Open Privacy")?;
    driver.gone(BUTTON, "Back")?;
    driver
        .text_where("Focused: ", |text| {
            text.ends_with("settings page: Settings")
        })
        .map(drop)
}
