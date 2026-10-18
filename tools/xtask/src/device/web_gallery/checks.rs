//! The gallery's checks in Chrome, the Windows checks' list with the page's
//! input. Each opens its screen fresh, acts with trusted input, and reads
//! the outcome from the published tree and from pixels sampled where the
//! right and the wrong outcome differ.

use std::time::Duration;

use super::super::gallery::{
    ACCENT, ADA, ARCHIVE, BACKGROUND, BROOK, CARDS, COLLAPSING, CONTACTLESS_LATCH_IDLE, DESKTOP,
    EMPTY_DONE, FIRST_CARD, FIRST_STOP, FOCUS, FULL, GESTURES, HALF, INITIAL_FOCUS, INITIAL_MENU,
    INITIAL_SWIPE, LANE, MAP_GROUND, MENU_SCREEN, PAD_IDLE, PAGE_COLORS, PAGES, PIN, REFRESH, RING,
    Rgb, SHEET, SHEET_NODE, SURFACE, SWIPE, TINT_DOUBLE, TINT_LONG, TINT_PAN, TINT_SCALE, TINT_TAP,
    TOLERANCE, VIEWER, below_list, cell_at, done_slot, ensure_near, grid_origin, in_header,
    pager_point, scene_under, viewer_state,
};
use super::{BUTTON, Bounds, Check, EDIT, MENU, MENU_ITEM, Point, SLIDER, TEXT, Web, key};

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
        "refresh: F5 with focus in the list refreshes the list, not the page",
        REFRESH,
        refresh_f5,
    ),
    check(
        "refresh: Ctrl+R with focus in the list refreshes the list, not the page",
        REFRESH,
        refresh_ctrl_r,
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
        "viewer: a mouse double click zooms about the click",
        VIEWER,
        viewer_double_click,
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
    check(
        "viewer: Ctrl+wheel zooms about the pointer and a mouse drag pans",
        VIEWER,
        viewer_mouse_pan,
    ),
    check(
        "cards: a mouse drag moves a card to Done",
        CARDS,
        cards_drag,
    ),
    check(
        "cards: a touch drag moves a card to Done",
        CARDS,
        cards_touch_drag,
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
        "focus: an input method composes and commits into the field",
        FOCUS,
        focus_ime,
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
    run: fn(&mut Web) -> anyhow::Result<()>,
) -> (&'static str, Check) {
    (name, Check { screen, run })
}

// ---------------------------------------------------------------------------
// Swipe actions
// ---------------------------------------------------------------------------

fn swipe_touch_archive(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_SWIPE)?;
    let row = web.bounds_of(TEXT, ADA)?;
    let tray = row.right_middle(30.0);
    ensure_near("the row's right end", web.sample(tray)?, SURFACE, TOLERANCE)?;
    let from = row.right_middle(60.0);
    web.touch_drag(
        from,
        from.offset(-150.0, 0.0),
        Duration::from_millis(300),
        Duration::from_millis(150),
    )?;
    web.wait_node(BUTTON, "Archive Ada")?;
    web.pixel("the revealed Archive action", tray, ARCHIVE, TOLERANCE)?;
    let archive = web.bounds_of(BUTTON, "Archive Ada")?.centre();
    web.touch_tap(archive)?;
    web.text("3 in inbox, 0 pinned, 1 archived, 3 tasks left, 0 completed")?;
    web.gone(TEXT, ADA)
}

fn swipe_mouse_pin(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_SWIPE)?;
    let row = web.bounds_of(TEXT, BROOK)?;
    // Between the row's left edge and its avatar: where the revealed action
    // slides in.
    let tray = row.left_middle(8.0);
    ensure_near("the row's left end", web.sample(tray)?, SURFACE, TOLERANCE)?;
    let from = row.left_middle(120.0);
    web.mouse_drag(
        from,
        from.offset(150.0, 0.0),
        12,
        Duration::from_millis(150),
    )?;
    web.wait_node(BUTTON, "Pin Brook")?;
    web.pixel("the revealed Pin action", tray, PIN, TOLERANCE)?;
    web.click_node(BUTTON, "Pin Brook")?;
    web.text("4 in inbox, 1 pinned, 0 archived, 3 tasks left, 0 completed")?;
    web.text("B Brook (pinned) Your order has shipped")
}

/// Where the last task is now: a row, and once a task is gone, the page
/// behind the shortened list.
fn task_tail(web: &mut Web) -> anyhow::Result<Point> {
    let last = web.bounds_of(TEXT, "Call the bank")?;
    Ok(last.left_middle(300.0))
}

fn swipe_dismiss_task(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_SWIPE)?;
    let first = web.bounds_of(TEXT, "Water the plants")?;
    let tail = task_tail(web)?;
    ensure_near("the last task row", web.sample(tail)?, SURFACE, TOLERANCE)?;
    let from = first.left_middle(40.0);
    web.touch_drag(
        from,
        from.offset(420.0, 0.0),
        Duration::from_millis(120),
        Duration::ZERO,
    )?;
    web.text("4 in inbox, 0 pinned, 0 archived, 2 tasks left, 1 completed")?;
    web.gone(TEXT, "Water the plants")?;
    web.pixel(
        "the page below the shortened task list",
        tail,
        BACKGROUND,
        TOLERANCE,
    )
}

fn swipe_done_control(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_SWIPE)?;
    let tail = task_tail(web)?;
    web.click_node(BUTTON, "Complete Book the train")?;
    web.text("4 in inbox, 0 pinned, 0 archived, 2 tasks left, 1 completed")?;
    web.gone(TEXT, "Book the train")?;
    web.pixel(
        "the page below the shortened task list",
        tail,
        BACKGROUND,
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Pull to refresh
// ---------------------------------------------------------------------------

fn refresh_pull(web: &mut Web) -> anyhow::Result<()> {
    web.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(&mut *web, &DESKTOP)?;
    ensure_near("below the list", web.sample(below)?, BACKGROUND, TOLERANCE)?;
    let from = web
        .bounds_of(TEXT, "Message 6")?
        .centre()
        .offset(100.0, 0.0);
    web.touch_drag(
        from,
        from.offset(0.0, 220.0),
        Duration::from_millis(400),
        Duration::from_millis(300),
    )?;
    web.text("7 messages, 1 refreshes, idle")?;
    web.text("Message 7")?;
    web.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

fn refresh_control(web: &mut Web) -> anyhow::Result<()> {
    web.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(&mut *web, &DESKTOP)?;
    ensure_near("below the list", web.sample(below)?, BACKGROUND, TOLERANCE)?;
    web.click_node(BUTTON, "Refresh inbox")?;
    web.text("7 messages, 1 refreshes, idle")?;
    web.pixel("the grown list's last row", below, SURFACE, TOLERANCE)
}

/// The page's own load, which a browser refresh replaces.
fn page_load(web: &mut Web) -> anyhow::Result<f64> {
    web.script("performance.timeOrigin")?
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("the page has no time origin"))
}

/// A refresh key with the focus in the list: the list refreshes, and the
/// browser's own reload, which the key also means, does not happen.
fn refresh_by_key(web: &mut Web, chord: &[&str]) -> anyhow::Result<()> {
    web.text("6 messages, 0 refreshes, idle")?;
    let below = below_list(&mut *web, &DESKTOP)?;
    let load = page_load(web)?;
    web.tab_until(16, |name| name == "Refresh inbox" || name == "Messages")?;
    web.chord(chord)?;
    web.text("7 messages, 1 refreshes, idle")?;
    web.pixel("the grown list's last row", below, SURFACE, TOLERANCE)?;
    let after = page_load(web)?;
    anyhow::ensure!(
        (after - load).abs() < f64::EPSILON,
        "the browser reloaded the page"
    );
    anyhow::ensure!(
        web.last_prevented("keydown")?,
        "the page left the key's default to the browser, which reloads"
    );
    Ok(())
}

fn refresh_f5(web: &mut Web) -> anyhow::Result<()> {
    refresh_by_key(web, &[key::F5])
}

fn refresh_ctrl_r(web: &mut Web) -> anyhow::Result<()> {
    refresh_by_key(web, &[key::CONTROL, "r"])
}

// ---------------------------------------------------------------------------
// Collapsing header
// ---------------------------------------------------------------------------

fn collapsing_wheel(web: &mut Web) -> anyhow::Result<()> {
    web.text("Itinerary scrolled 0 px")?;
    let header = in_header(&mut *web, &DESKTOP)?;
    ensure_near(
        "the expanded header",
        web.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let list = web.bounds_of(TEXT, FIRST_STOP)?.centre().offset(0.0, 120.0);
    // The header goes first: a notch collapses it, and only what it leaves
    // over reaches the list.
    web.wheel(list, -1, false)?;
    web.pixel_leaves(
        "the header to leave the sample point",
        header,
        ACCENT,
        TOLERANCE,
    )?;
    let scrolled = web.text_where("Itinerary scrolled ", |_| true)?;
    web.wheel(list, -4, false)?;
    web.text_where("Itinerary scrolled ", |text| text != scrolled)?;
    // Back up: the list returns to its top before the header expands.
    web.wheel(list, 12, false)?;
    web.text("Itinerary scrolled 0 px")
        .map_err(|error| error.context(format!("after {scrolled:?}, scrolling back up")))?;
    // At its top, the list leaves the next wheel burst to the header, once
    // the burst that brought it back has ended (ADR-0209 §5).
    std::thread::sleep(CONTACTLESS_LATCH_IDLE + Duration::from_millis(300));
    web.wheel(list, 3, false)?;
    web.pixel("the header expanded again", header, ACCENT, TOLERANCE)
}

fn collapsing_touch(web: &mut Web) -> anyhow::Result<()> {
    web.text("Itinerary scrolled 0 px")?;
    let header = in_header(&mut *web, &DESKTOP)?;
    ensure_near(
        "the expanded header",
        web.sample(header)?,
        ACCENT,
        TOLERANCE,
    )?;
    let from = web.bounds_of(TEXT, FIRST_STOP)?.centre().offset(0.0, 250.0);
    web.touch_drag(
        from,
        from.offset(0.0, -200.0),
        Duration::from_millis(500),
        Duration::from_millis(150),
    )?;
    web.pixel_leaves(
        "the header to collapse past the sample point",
        header,
        ACCENT,
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn pages_swipe(web: &mut Web) -> anyhow::Result<()> {
    let point = pager_point(&mut *web, &DESKTOP)?;
    ensure_near("page 1", web.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    web.touch_drag(
        point.offset(250.0, 0.0),
        point.offset(-250.0, 0.0),
        Duration::from_millis(200),
        Duration::ZERO,
    )?;
    web.text("Page 2 of 3: Pack")?;
    web.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)
}

fn pages_keys(web: &mut Web) -> anyhow::Result<()> {
    let point = pager_point(&mut *web, &DESKTOP)?;
    // A drag gives the pager the focus its keys need.
    web.mouse_drag(
        point.offset(120.0, 0.0),
        point.offset(-120.0, 0.0),
        10,
        Duration::ZERO,
    )?;
    web.text("Page 2 of 3: Pack")?;
    web.chord(&[key::RIGHT])?;
    web.text("Page 3 of 3: Go")?;
    web.pixel("page 3", point, PAGE_COLORS[2], TOLERANCE)?;
    web.chord(&[key::HOME])?;
    web.text("Page 1 of 3: Plan")?;
    web.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

fn pages_button(web: &mut Web) -> anyhow::Result<()> {
    let point = pager_point(&mut *web, &DESKTOP)?;
    ensure_near("page 1", web.sample(point)?, PAGE_COLORS[0], TOLERANCE)?;
    web.click_node(BUTTON, "Next")?;
    web.text("Page 2 of 3: Pack")?;
    web.pixel("page 2", point, PAGE_COLORS[1], TOLERANCE)?;
    web.click_node(BUTTON, "Previous")?;
    web.text("Page 1 of 3: Plan")?;
    web.pixel("page 1", point, PAGE_COLORS[0], TOLERANCE)
}

// ---------------------------------------------------------------------------
// Bottom sheet
// ---------------------------------------------------------------------------

/// The map's top-left corner: the slider's node spans the drag's whole
/// area, which the map fills.
fn sheet_area(web: &mut Web) -> anyhow::Result<Bounds> {
    web.bounds_of(SLIDER, SHEET_NODE)
}

/// The sheet's handle at Half.
fn handle(sheet: Bounds) -> Point {
    Point {
        x: sheet.centre().x,
        y: sheet.top + HALF + 13.0,
    }
}

fn sheet_touch_full(web: &mut Web) -> anyhow::Result<()> {
    web.text("Sheet at Half")?;
    let sheet = sheet_area(web)?;
    // A block of the map, clear of every street, above the sheet at Half
    // and under it at Full.
    let probe = sheet.top_left().offset(250.0, 100.0);
    ensure_near(
        "the map above the sheet",
        web.sample(probe)?,
        MAP_GROUND,
        TOLERANCE,
    )?;
    let from = handle(sheet);
    web.touch_drag(
        from,
        from.offset(0.0, FULL - HALF),
        Duration::from_millis(500),
        Duration::from_millis(200),
    )?;
    web.text("Sheet at Full")?;
    web.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)
}

fn sheet_mouse_peek(web: &mut Web) -> anyhow::Result<()> {
    web.text("Sheet at Half")?;
    let sheet = sheet_area(web)?;
    // Between Half and Peek: the sheet now, the map once it rests at Peek.
    let probe = sheet.top_left().offset(250.0, 320.0);
    ensure_near("the sheet at Half", web.sample(probe)?, SURFACE, TOLERANCE)?;
    let from = handle(sheet);
    web.mouse_drag(
        from,
        from.offset(0.0, 190.0),
        16,
        Duration::from_millis(200),
    )?;
    web.text("Sheet at Peek")?;
    web.pixel(
        "the map above the sheet at Peek",
        probe,
        MAP_GROUND,
        TOLERANCE,
    )
}

fn sheet_keys(web: &mut Web) -> anyhow::Result<()> {
    web.text("Sheet at Half")?;
    let probe = sheet_area(web)?.top_left().offset(250.0, 100.0);
    web.tab_until(16, |name| name.starts_with("Places sheet"))?;
    web.chord(&[key::UP])?;
    web.text("Sheet at Full")?;
    web.pixel("the sheet at Full", probe, SURFACE, TOLERANCE)?;
    web.chord(&[key::DOWN])?;
    web.text("Sheet at Half")?;
    web.pixel(
        "the map above the sheet at Half",
        probe,
        MAP_GROUND,
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Zoom and pan
// ---------------------------------------------------------------------------

/// Waits for the readout to state `zoom` and an offset within two pixels
/// of `offset`.
fn viewer_reads(web: &mut Web, zoom: f64, offset: (f64, f64)) -> anyhow::Result<String> {
    web.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(z, x, y)| {
            (z - zoom).abs() < f64::EPSILON
                && (x - offset.0).abs() <= 2.0
                && (y - offset.1).abs() <= 2.0
        })
    })
}

/// A double tap or click at a grid point zooms to 200 % about it: the probe
/// then shows the scene halfway to it.
fn viewer_zoom_about(
    web: &mut Web,
    act: fn(&mut Web, Point) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let origin = grid_origin(&mut *web)?;
    let tap = origin.offset(205.0, 205.0);
    let probe = origin.offset(295.0, 255.0);
    ensure_near(
        "the cell before the zoom",
        web.sample(probe)?,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    act(web, tap)?;
    viewer_reads(web, 200.0, (-205.0, -205.0))?;
    web.pixel(
        "the cell after the zoom",
        probe,
        cell_at((250.0, 230.0)),
        TOLERANCE,
    )
}

fn viewer_double_tap(web: &mut Web) -> anyhow::Result<()> {
    viewer_zoom_about(web, Web::touch_double_tap)
}

fn viewer_double_click(web: &mut Web) -> anyhow::Result<()> {
    viewer_zoom_about(web, Web::double_click)
}

fn viewer_spread(web: &mut Web) -> anyhow::Result<()> {
    let origin = grid_origin(&mut *web)?;
    let focal = origin.offset(200.0, 200.0);
    let probe = origin.offset(295.0, 255.0);
    let before = web.sample(probe)?;
    ensure_near(
        "the cell before the spread",
        before,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    web.touch_pinch(
        (focal.offset(-40.0, 0.0), focal.offset(-120.0, 0.0)),
        (focal.offset(40.0, 0.0), focal.offset(120.0, 0.0)),
        Duration::from_millis(500),
    )?;
    let zoom = web.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(percent, _, _)| percent > 150.0)
    })?;
    web.pixel_leaves("the probe to show another cell", probe, before, TOLERANCE)
        .map_err(|error| error.context(format!("readout {zoom:?}")))
}

fn viewer_keys(web: &mut Web) -> anyhow::Result<()> {
    let origin = grid_origin(&mut *web)?;
    // A drag gives the viewer the keys, as it gives a scrollable its own;
    // unzoomed, it has no room to move the grid.
    let from = origin.offset(100.0, 100.0);
    web.mouse_drag(from, from.offset(40.0, 0.0), 6, Duration::from_millis(100))?;
    web.wait("the viewer's focus", |web| {
        Ok(web.focus_on_role(SLIDER)?.then_some(()))
    })?;
    let probe = origin.offset(295.0, 255.0);
    web.chord(&["+"])?;
    let zoomed = web.text_where("Zoom 200%", |_| true)?;
    let (_, x, y) =
        viewer_state(&zoomed).ok_or_else(|| anyhow::anyhow!("unreadable readout {zoomed:?}"))?;
    // At 200 % the probe shows the scene point (probe - offset) / 2.
    let scene = |x: f64, y: f64| ((295.0 - x) / 2.0, (255.0 - y) / 2.0);
    web.pixel("the cell after +", probe, cell_at(scene(x, y)), TOLERANCE)?;
    // The arrow pans one step: the scene moves left under the probe.
    web.chord(&[key::RIGHT])?;
    let panned = web.text_where("Zoom 200%", |text| {
        viewer_state(text).is_some_and(|(_, panned, _)| panned < x - 20.0)
    })?;
    let (_, px, py) =
        viewer_state(&panned).ok_or_else(|| anyhow::anyhow!("unreadable readout {panned:?}"))?;
    web.pixel(
        "the cell after the arrow",
        probe,
        cell_at(scene(px, py)),
        TOLERANCE,
    )?;
    web.chord(&["-"])?;
    web.text_where("Zoom 100%", |_| true)
        .map(drop)
        .map_err(|error| error.context(format!("after {zoomed:?} and {panned:?}")))
}

fn viewer_mouse_pan(web: &mut Web) -> anyhow::Result<()> {
    let origin = grid_origin(&mut *web)?;
    let probe = origin.offset(295.0, 255.0);
    ensure_near(
        "the cell before the zoom",
        web.sample(probe)?,
        cell_at((295.0, 255.0)),
        TOLERANCE,
    )?;
    // The child fills the viewport, so there is room to pan only zoomed in:
    // Ctrl+wheel zooms about the cursor.
    let load = web.script("[performance.timeOrigin, visualViewport.scale]")?;
    web.wheel(origin.offset(200.0, 200.0), 2, true)?;
    let zoomed = web.text_where("Zoom ", |text| {
        viewer_state(text).is_some_and(|(zoom, _, _)| zoom > 100.0)
    })?;
    let after = web.script("[performance.timeOrigin, visualViewport.scale]")?;
    anyhow::ensure!(
        load == after,
        "Ctrl+wheel also zoomed or reloaded the page: {load} then {after}"
    );
    anyhow::ensure!(
        web.last_prevented("wheel")?,
        "the page left Ctrl+wheel's default to the browser, which zooms the page"
    );
    let before = viewer_state(&zoomed).expect("BUG: accepted as readable");
    web.pixel(
        "the cell after Ctrl+wheel",
        probe,
        cell_at(scene_under(before, (295.0, 255.0))),
        TOLERANCE,
    )?;
    let from = origin.offset(300.0, 300.0);
    web.mouse_drag(
        from,
        from.offset(-80.0, -60.0),
        12,
        Duration::from_millis(300),
    )?;
    let panned = viewer_reads(web, before.0, (before.1 - 80.0, before.2 - 60.0))?;
    let after = viewer_state(&panned).expect("BUG: accepted as readable");
    web.pixel(
        "the cell after the pan",
        probe,
        cell_at(scene_under(after, (295.0, 255.0))),
        TOLERANCE,
    )
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

fn card_grip(web: &mut Web) -> anyhow::Result<Point> {
    Ok(web.bounds_of(TEXT, FIRST_CARD)?.left_middle(60.0))
}

fn cards_drag(web: &mut Web) -> anyhow::Result<()> {
    web.text("To do 3, done 0")?;
    let slot = done_slot(&mut *web, &DESKTOP, EMPTY_DONE)?;
    ensure_near("the empty Done lane", web.sample(slot)?, LANE, TOLERANCE)?;
    let from = card_grip(web)?;
    web.mouse_drag(
        from,
        slot.offset(40.0, 60.0),
        30,
        Duration::from_millis(250),
    )?;
    web.text("To do 2, done 1, moved Write the release notes to Done")?;
    web.text("Done (1)")?;
    web.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_touch_drag(web: &mut Web) -> anyhow::Result<()> {
    web.text("To do 3, done 0")?;
    let slot = done_slot(&mut *web, &DESKTOP, EMPTY_DONE)?;
    ensure_near("the empty Done lane", web.sample(slot)?, LANE, TOLERANCE)?;
    let from = card_grip(web)?;
    web.touch_drag(
        from,
        slot.offset(40.0, 60.0),
        Duration::from_millis(500),
        Duration::from_millis(250),
    )?;
    web.text("To do 2, done 1, moved Write the release notes to Done")?;
    web.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_controls(web: &mut Web) -> anyhow::Result<()> {
    web.text("To do 3, done 0")?;
    let slot = done_slot(&mut *web, &DESKTOP, EMPTY_DONE)?;
    web.click_node(BUTTON, "Cut Review the layout fix")?;
    web.click_node(BUTTON, "Paste into Done")?;
    web.text("To do 2, done 1, moved Review the layout fix to Done")?;
    web.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

fn cards_keys(web: &mut Web) -> anyhow::Result<()> {
    web.text("To do 3, done 0")?;
    let slot = done_slot(&mut *web, &DESKTOP, EMPTY_DONE)?;
    web.tab_until(20, |name| name.starts_with("Record the demo"))?;
    web.chord(&[key::CONTROL, "x"])?;
    web.tab_until(20, |name| name.starts_with("Done"))?;
    web.chord(&[key::CONTROL, "v"])?;
    web.text("To do 2, done 1, moved Record the demo to Done")?;
    web.pixel("the card in Done", slot, SURFACE, TOLERANCE)
}

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

/// A point on the last file's row, and one over the page below it, where a
/// menu opened at the row's middle draws its panel.
fn under_last_row(web: &mut Web) -> anyhow::Result<(Point, Point)> {
    let at = web.bounds_of(TEXT, "N Notes.txt")?.left_middle(160.0);
    Ok((at, at.offset(40.0, 70.0)))
}

fn menu_right_click(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(web)?;
    ensure_near(
        "the page below the files",
        web.sample(panel)?,
        BACKGROUND,
        TOLERANCE,
    )?;
    web.right_click(at)?;
    web.wait_node(MENU_ITEM, "Duplicate Notes.txt")?;
    anyhow::ensure!(
        web.find(MENU, |_| true)?.is_some(),
        "no menu node is published"
    );
    web.pixel_leaves(
        "the menu's panel over the page",
        panel,
        BACKGROUND,
        TOLERANCE,
    )?;
    web.focused(MENU_ITEM, "Open Notes.txt")?;
    web.chord(&[key::DOWN])?;
    web.focused(MENU_ITEM, "Duplicate Notes.txt")?;
    web.chord(&[key::ENTER])?;
    web.text("5 files, last action: made Copy of Notes.txt")?;
    web.gone(MENU_ITEM, "Duplicate Notes.txt")
}

fn menu_escape(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(web)?;
    web.right_click(at)?;
    web.wait_node(MENU_ITEM, "Delete Notes.txt")?;
    web.pixel_leaves(
        "the menu's panel over the page",
        panel,
        BACKGROUND,
        TOLERANCE,
    )?;
    web.chord(&[key::ESCAPE])?;
    web.gone(MENU_ITEM, "Delete Notes.txt")?;
    web.pixel(
        "the page once the menu closed",
        panel,
        BACKGROUND,
        TOLERANCE,
    )?;
    web.stays("the files, after Escape", |web| {
        Ok(web.node(TEXT, INITIAL_MENU)?.is_some())
    })
}

fn menu_keys(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_MENU)?;
    let row = web.tab_until(16, |name| name.contains("Holiday photos"))?;
    web.chord(&[key::SHIFT, key::F10])?;
    web.wait_node(MENU_ITEM, "Open Holiday photos")?;
    web.chord(&[key::ESCAPE])?;
    web.gone(MENU_ITEM, "Open Holiday photos")?;
    // Escape gives the focus back to the row the menu opened from.
    web.wait("the focus back on the row", |web| {
        Ok((web.focus_name() == row).then_some(()))
    })
    .map_err(|error| {
        let now = web.focus_name();
        error.context(format!("focus is on {now:?}"))
    })?;
    web.menu_key()?;
    web.focused(MENU_ITEM, "Open Holiday photos")?;
    web.chord(&[key::ENTER])?;
    web.text("4 files, last action: opened Holiday photos")?;
    web.gone(MENU_ITEM, "Open Holiday photos")
}

fn menu_long_press(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_MENU)?;
    let (at, panel) = under_last_row(web)?;
    web.touch_drag(
        at,
        at,
        Duration::from_millis(16),
        Duration::from_millis(900),
    )?;
    web.wait_node(MENU_ITEM, "Delete Notes.txt")?;
    web.pixel_leaves(
        "the menu's panel over the page",
        panel,
        BACKGROUND,
        TOLERANCE,
    )?;
    let delete = web.bounds_of(MENU_ITEM, "Delete Notes.txt")?.centre();
    web.touch_tap(delete)?;
    web.text("3 files, last action: deleted Notes.txt")
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

/// The pad's middle, and a point of its fill clear of its text.
fn pad(web: &mut Web) -> anyhow::Result<(Point, Point)> {
    let label = web.wait("the gesture pad", |web| {
        Ok(web
            .find(TEXT, |name| name.starts_with("Gesture pad"))?
            .and_then(|node| node.bounds))
    })?;
    let middle = label.centre();
    Ok((middle.offset(0.0, 80.0), middle.offset(-200.0, -120.0)))
}

fn gesture(
    web: &mut Web,
    act: impl FnOnce(&mut Web, Point) -> anyhow::Result<()>,
    readout: &str,
    tint: Rgb,
) -> anyhow::Result<()> {
    let (middle, fill) = pad(web)?;
    ensure_near("the idle pad", web.sample(fill)?, PAD_IDLE, TOLERANCE)?;
    act(web, middle)?;
    web.text(readout)?;
    web.pixel("the pad's tint", fill, tint, TOLERANCE)
}

fn gestures_tap(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        Web::click,
        "Taps 1, double taps 0, long presses 0, pans 0, scales 0; last: tap",
        TINT_TAP,
    )
}

fn gestures_double_tap(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        Web::double_click,
        "Taps 0, double taps 1, long presses 0, pans 0, scales 0; last: double tap",
        TINT_DOUBLE,
    )
}

fn gestures_long_press(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        |web, at| web.press_and_hold(at, Duration::from_millis(900)),
        "Taps 0, double taps 0, long presses 1, pans 0, scales 0; last: long press",
        TINT_LONG,
    )
}

fn gestures_pan(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        |web, at| web.mouse_drag(at, at.offset(200.0, 0.0), 12, Duration::ZERO),
        "Taps 0, double taps 0, long presses 0, pans 1, scales 0; last: pan",
        TINT_PAN,
    )
}

fn gestures_touch_tap(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        Web::touch_tap,
        "Taps 1, double taps 0, long presses 0, pans 0, scales 0; last: tap",
        TINT_TAP,
    )
}

fn gestures_pinch(web: &mut Web) -> anyhow::Result<()> {
    gesture(
        web,
        |web, at| {
            web.touch_pinch(
                (at.offset(-150.0, 0.0), at.offset(-50.0, 0.0)),
                (at.offset(150.0, 0.0), at.offset(50.0, 0.0)),
                Duration::from_millis(500),
            )
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
fn ring_point(web: &mut Web, button: &str) -> anyhow::Result<Point> {
    Ok(web.bounds_of(BUTTON, button)?.left_middle(0.5))
}

fn focus_tab_order(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_FOCUS)?;
    let ring = ring_point(web, "Save")?;
    ensure_near("Save's edge", web.sample(ring)?, ACCENT, TOLERANCE)?;
    web.click_node(EDIT, "")?;
    web.text("Focused: Name field; saved name: none; settings page: Settings")?;
    web.chord(&[key::TAB])?;
    web.focused(BUTTON, "Save")?;
    web.text("Focused: Save; saved name: none; settings page: Settings")?;
    web.pixel("the focus ring on Save", ring, RING, 40)?;
    web.chord(&[key::TAB])?;
    web.focused(BUTTON, "Clear")?;
    web.pixel("Save's edge once focus moved on", ring, ACCENT, TOLERANCE)?;
    // One Tab from the last control of the form enters the settings
    // navigator at its first button: no unnamed stop in between.
    web.chord(&[key::TAB])?;
    web.focused(BUTTON, "Open Privacy")?;
    web.text("Focused: Open Privacy; saved name: none; settings page: Settings")?;
    web.chord(&[key::SHIFT, key::TAB])?;
    web.focused(BUTTON, "Clear")?;
    web.text("Focused: Clear; saved name: none; settings page: Settings")
}

fn focus_field(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_FOCUS)?;
    web.click_node(EDIT, "")?;
    web.focused(EDIT, "")?;
    web.type_text("Grace")?;
    web.chord(&[key::ENTER])?;
    web.text("Focused: Name field; saved name: Grace; settings page: Settings")?;
    // Space on the focused Clear presses it.
    web.tab_until(3, |name| name == "Clear")?;
    web.chord(&[key::SPACE])?;
    web.text("Focused: Clear; saved name: none; settings page: Settings")
}

/// Chrome's input method into the focused field: a composition edits the
/// field as composing, Escape during it belongs to the input method and
/// keeps the field focused, and the commit saves with Enter.
fn focus_ime(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_FOCUS)?;
    let field = web.bounds_of(EDIT, "")?;
    web.click(field.centre())?;
    web.focused(EDIT, "")?;
    web.compose("に")?;
    web.wait("the composition in the field", |web| {
        Ok(web
            .find(EDIT, |_| true)?
            .filter(|node| node.value() == Some("に"))
            .map(drop))
    })?;
    web.compose("にほん")?;
    web.commit("日本")?;
    web.wait("the committed text in the field", |web| {
        Ok(web
            .find(EDIT, |_| true)?
            .filter(|node| node.value() == Some("日本"))
            .map(drop))
    })?;
    web.chord(&[key::ENTER])?;
    web.text("Focused: Name field; saved name: 日本; settings page: Settings")
}

fn focus_navigator(web: &mut Web) -> anyhow::Result<()> {
    web.text(INITIAL_FOCUS)?;
    web.click_node(EDIT, "")?;
    web.tab_until(6, |name| name == "Open Privacy")?;
    web.chord(&[key::ENTER])?;
    web.text("Privacy options would live here.")?;
    let fill = web.bounds_of(BUTTON, "Back")?.left_middle(8.0);
    web.pixel("the Back button", fill, ACCENT, TOLERANCE)?;
    web.tab_until(6, |name| name == "Back")?;
    web.chord(&[key::SPACE])?;
    web.wait_node(BUTTON, "Open Privacy")?;
    web.gone(BUTTON, "Back")?;
    web.text_where("Focused: ", |text| {
        text.ends_with("settings page: Settings")
    })
    .map(drop)
}
