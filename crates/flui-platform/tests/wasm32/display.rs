//! The Web display source against a controlled browser boundary: animation
//! frames with chosen timestamps, a pixel ratio the test changes, a page
//! visibility it flips and a Window Management API it grants, withholds or
//! omits. Runs on Node in the wasm lane; nothing here touches a real display.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_platform::{OwnerPlatform, Platform, WebPlatform, WindowOpen, WindowOptions};
use flui_platform_api::DisplaySnapshot;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(inline_js = r#"
let restore;
let rafQueue = [];
let rafIds = 0;
let ratio = 1;
let ratioQueries = [];
let driftTo = null;
let failQueries = 0;
let hidden = false;
let document;
let screenObject;
let permission = 'granted';
let permissionStatus;
let permissionNames = [];
let screenDetailsCalls = 0;
let details;
let mainScreen;
let sideScreen;
let everScreens = [];

// An event target that counts the listeners it really holds, and removes one
// only when that very function was added.
function track(target) {
    const held = new Map();
    const add = target.addEventListener.bind(target);
    const remove = target.removeEventListener.bind(target);
    target.addEventListener = (name, callback) => {
        if (!held.has(name)) held.set(name, new Set());
        held.get(name).add(callback);
        add(name, callback);
    };
    target.removeEventListener = (name, callback) => {
        if (held.get(name)?.delete(callback)) remove(name, callback);
    };
    target.listenerCount = () => [...held.values()].reduce((sum, set) => sum + set.size, 0);
    return target;
}

function screen(label, left, dppx, primary) {
    const made = Object.assign(track(new EventTarget()), {
        label, left, top: 0, width: 1920, height: 1080,
        availLeft: left, availTop: 0, availWidth: 1920, availHeight: 1040,
        devicePixelRatio: dppx, isPrimary: primary,
    });
    everScreens.push(made);
    return made;
}

class FakeCanvas extends EventTarget {
    constructor() {
        super();
        this.style = { setProperty() {} };
        this.clientWidth = 300;
        this.clientHeight = 150;
        this.width = 300;
        this.height = 150;
        this.ownerDocument = document;
    }
    getRootNode() { return document; }
    setAttribute() {}
}

// `windowManagement`: 'none' omits the API; 'granted', 'prompt' and 'legacy'
// (only the older permission name is known) set the permission it reports.
export function installDisplayProbe(windowManagement) {
    const names = [
        'Window', 'matchMedia', 'requestAnimationFrame', 'cancelAnimationFrame',
        'devicePixelRatio', 'screen', 'document', 'navigator', 'getScreenDetails',
        'HTMLCanvasElement', 'addEventListener', 'removeEventListener',
    ];
    const descriptors = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name));
    const previous = Object.getPrototypeOf(globalThis);
    class ProbeWindow {}
    globalThis.Window = ProbeWindow;
    Object.setPrototypeOf(globalThis, ProbeWindow.prototype);
    const define = (name, value) =>
        Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
    rafQueue = [];
    ratio = 1;
    ratioQueries = [];
    driftTo = null;
    failQueries = 0;
    hidden = false;
    permissionNames = [];
    screenDetailsCalls = 0;
    everScreens = [];
    define('devicePixelRatio', ratio);
    screenObject = Object.assign(track(new EventTarget()), { width: 1280, height: 720, onchange: null });
    define('screen', screenObject);
    define('requestAnimationFrame', callback => {
        rafIds += 1;
        rafQueue.push({ id: rafIds, callback });
        return rafIds;
    });
    define('cancelAnimationFrame', id => { rafQueue = rafQueue.filter(entry => entry.id !== id); });
    define('matchMedia', query => {
        const match = /^\(resolution: ([0-9.]+)dppx\)$/.exec(query);
        if (match && failQueries > 0) { failQueries -= 1; throw new Error('matchMedia refused'); }
        const target = track(new EventTarget());
        target.dppx = match ? Number(match[1]) : null;
        Object.defineProperty(target, 'matches', { get: () => target.dppx === ratio });
        if (match) ratioQueries.push(target);
        if (match && driftTo !== null) {
            // The ratio moves between the page reading it and the query listening.
            ratio = driftTo;
            globalThis.devicePixelRatio = driftTo;
            driftTo = null;
        }
        return target;
    });
    document = track(new EventTarget());
    Object.defineProperty(document, 'hidden', { get: () => hidden });
    document.hasFocus = () => true;
    document.title = '';
    document.body = { appendChild() {} };
    document.getElementById = () => null;
    document.createElement = () => new FakeCanvas();
    define('document', document);
    define('HTMLCanvasElement', FakeCanvas);
    define('addEventListener', () => {});
    define('removeEventListener', () => {});

    mainScreen = screen('Main', 0, 2, true);
    sideScreen = screen('Main', 1920, 1.5, false);
    details = track(new EventTarget());
    details.screens = [mainScreen, sideScreen];
    details.currentScreen = mainScreen;
    permissionStatus = track(new EventTarget());
    Object.defineProperty(permissionStatus, 'state', { get: () => permission });
    permission = windowManagement === 'prompt' ? 'prompt' : 'granted';
    if (windowManagement !== 'none') {
        define('navigator', {
            permissions: {
                query: ({ name }) => {
                    permissionNames.push(name);
                    const known = windowManagement === 'legacy' ? 'window-placement' : 'window-management';
                    // An unknown name rejects, as a browser does.
                    return name === known
                        ? Promise.resolve(permissionStatus)
                        : Promise.reject(new TypeError('unknown permission'));
                },
            },
        });
        define('getScreenDetails', () => { screenDetailsCalls += 1; return Promise.resolve(details); });
    } else {
        define('navigator', {});
        delete globalThis.getScreenDetails;
    }
    restore = () => {
        Object.setPrototypeOf(globalThis, previous);
        names.forEach((name, index) => {
            if (descriptors[index]) Object.defineProperty(globalThis, name, descriptors[index]);
            else delete globalThis[name];
        });
    };
}
export function restoreDisplayProbe() { restore(); }
// Run `count` animation frames `interval` milliseconds apart from `start`; each
// runs every callback queued for it. The timestamp after the last frame.
export function runFrames(start, interval, count) {
    let now = start;
    for (let frame = 0; frame < count; frame += 1) {
        const batch = rafQueue;
        rafQueue = [];
        for (const entry of batch) entry.callback(now);
        now += interval;
    }
    return now;
}
export function setPixelRatio(next) {
    const before = ratio;
    ratio = next;
    globalThis.devicePixelRatio = next;
    for (const query of ratioQueries.slice()) {
        if (query.dppx === before && query.listenerCount() > 0) query.dispatchEvent(new Event('change'));
    }
}
export function driftRatioOnNextQuery(next) { driftTo = next; }
export function refuseNextQueries(count) { failQueries = count; }
export function pixelRatioListeners() { return ratioQueries.reduce((sum, query) => sum + query.listenerCount(), 0); }
export function setHidden(value) {
    hidden = value;
    document.dispatchEvent(new Event('visibilitychange'));
}
export function documentListeners() { return document.listenerCount(); }
export function omitScreenChange() { delete screenObject.onchange; }
export function windowScreenListeners() { return screenObject.listenerCount(); }
export function changeWindowScreen(width) {
    screenObject.width = width;
    screenObject.dispatchEvent(new Event('change'));
}
export function permissionQueries() { return permissionNames.length; }
export function screenDetailsRequests() { return screenDetailsCalls; }
export function detailsListeners() { return details.listenerCount(); }
export function permissionListeners() { return permissionStatus.listenerCount(); }
export function listedScreenListeners() { return everScreens.reduce((sum, one) => sum + one.listenerCount(), 0); }
export function grantPermission() {
    permission = 'granted';
    permissionStatus.dispatchEvent(new Event('change'));
}
export function revokePermission() {
    permission = 'denied';
    permissionStatus.dispatchEvent(new Event('change'));
}
// A screen that was not listed before: a new JS object, labelled like another.
export function plugInScreen() {
    details.screens = [...details.screens, screen('Main', 3840, 1, false)];
    details.dispatchEvent(new Event('screenschange'));
}
// The same screen objects listed again, in the opposite order.
export function reorderScreens() {
    details.screens = [...details.screens].reverse();
    details.dispatchEvent(new Event('screenschange'));
}
export function moveWindowToSideScreen() {
    details.currentScreen = sideScreen;
    details.dispatchEvent(new Event('currentscreenchange'));
}
// `currentscreenchange` for a change of the current screen's own metrics.
export function currentScreenChangesSize(width) {
    details.currentScreen.width = width;
    details.dispatchEvent(new Event('currentscreenchange'));
}
export function changeSideScreen(width) {
    sideScreen.width = width;
    sideScreen.dispatchEvent(new Event('change'));
}
export function nextOwnerTask() { return new Promise(resolve => setImmediate(resolve)); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = installDisplayProbe)]
    fn install_probe(window_management: &str);
    #[wasm_bindgen(js_name = restoreDisplayProbe)]
    fn restore_probe();
    #[wasm_bindgen(js_name = runFrames)]
    fn run_frames(start: f64, interval: f64, count: u32) -> f64;
    #[wasm_bindgen(js_name = setPixelRatio)]
    fn set_pixel_ratio(next: f64);
    #[wasm_bindgen(js_name = driftRatioOnNextQuery)]
    fn drift_ratio_on_next_query(next: f64);
    #[wasm_bindgen(js_name = refuseNextQueries)]
    fn refuse_next_queries(count: u32);
    #[wasm_bindgen(js_name = pixelRatioListeners)]
    fn pixel_ratio_listeners() -> i32;
    #[wasm_bindgen(js_name = setHidden)]
    fn set_hidden(value: bool);
    #[wasm_bindgen(js_name = documentListeners)]
    fn document_listeners() -> i32;
    #[wasm_bindgen(js_name = omitScreenChange)]
    fn omit_screen_change();
    #[wasm_bindgen(js_name = windowScreenListeners)]
    fn window_screen_listeners() -> i32;
    #[wasm_bindgen(js_name = changeWindowScreen)]
    fn change_window_screen(width: u32);
    #[wasm_bindgen(js_name = permissionQueries)]
    fn permission_queries() -> usize;
    #[wasm_bindgen(js_name = screenDetailsRequests)]
    fn screen_details_requests() -> usize;
    #[wasm_bindgen(js_name = detailsListeners)]
    fn details_listeners() -> i32;
    #[wasm_bindgen(js_name = permissionListeners)]
    fn permission_listeners() -> i32;
    #[wasm_bindgen(js_name = listedScreenListeners)]
    fn listed_screen_listeners() -> i32;
    #[wasm_bindgen(js_name = grantPermission)]
    fn grant_permission();
    #[wasm_bindgen(js_name = revokePermission)]
    fn revoke_permission();
    #[wasm_bindgen(js_name = plugInScreen)]
    fn plug_in_screen();
    #[wasm_bindgen(js_name = reorderScreens)]
    fn reorder_screens();
    #[wasm_bindgen(js_name = moveWindowToSideScreen)]
    fn move_window_to_side_screen();
    #[wasm_bindgen(js_name = currentScreenChangesSize)]
    fn current_screen_changes_size(width: u32);
    #[wasm_bindgen(js_name = changeSideScreen)]
    fn change_side_screen(width: u32);
    #[wasm_bindgen(js_name = nextOwnerTask)]
    fn next_task() -> js_sys::Promise;
}

struct Probe;
impl Drop for Probe {
    fn drop(&mut self) {
        restore_probe();
    }
}

/// A running platform and the number of owner wakes it has delivered.
struct Running {
    owner: Rc<RefCell<Option<OwnerPlatform>>>,
    wakes: Arc<AtomicUsize>,
}

impl Running {
    fn start() -> Self {
        let owner = Rc::new(RefCell::new(None));
        let installed = Rc::clone(&owner);
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&wakes);
        Box::new(WebPlatform::new().expect("platform"))
            .run(Box::new(move |capability| {
                capability
                    .on_wake(Box::new(move || {
                        counted.fetch_add(1, Ordering::SeqCst);
                    }))
                    .expect("wake hook");
                installed.replace(Some(capability));
                Ok(())
            }))
            .expect("run");
        Self { owner, wakes }
    }

    fn snapshot(&self) -> DisplaySnapshot {
        self.owner
            .borrow()
            .as_ref()
            .expect("owner")
            .display_snapshot()
            .expect("snapshot")
    }

    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }

    fn quit(&self) {
        self.owner
            .borrow()
            .as_ref()
            .expect("owner")
            .proxy()
            .request_quit()
            .expect("quit");
    }
}

async fn settle() {
    wasm_bindgen_futures::JsFuture::from(next_task())
        .await
        .expect("owner tasks");
}

fn millis(ms: f64) -> Duration {
    Duration::from_secs_f64(ms / 1000.0)
}

fn primary_period(snapshot: &DisplaySnapshot) -> Option<Duration> {
    snapshot
        .primary()
        .and_then(|display| display.refresh_period)
        .map(flui_platform_api::RefreshPeriod::get)
}

/// Within 3% of `expected_ms`: the published estimate is not exact, it is
/// inside the band a replacement must leave.
fn near(actual: Option<Duration>, expected_ms: f64) -> bool {
    actual.is_some_and(|actual| {
        actual.abs_diff(millis(expected_ms)) <= millis(expected_ms).mul_f64(0.03)
    })
}

fn scale_of_primary(snapshot: &DisplaySnapshot) -> Option<f64> {
    snapshot.primary().map(|display| display.scale_factor)
}

fn has_period(snapshot: &DisplaySnapshot) -> Vec<bool> {
    snapshot
        .displays()
        .iter()
        .map(|display| display.refresh_period.is_some())
        .collect()
}

#[wasm_bindgen_test]
async fn web_period_is_estimated_from_animation_frames() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    let before = running.snapshot();
    assert_eq!(primary_period(&before), None, "unknown before any frame");

    run_frames(100.0, 16.667, 5);
    settle().await;
    assert_eq!(running.wakes(), 0, "too few frames publish nothing");

    run_frames(200.0, 16.667, 30);
    settle().await;
    assert_eq!(running.wakes(), 1, "one wake for the first estimate");
    let measured = running.snapshot();
    assert!(near(primary_period(&measured), 16.667));
    assert!(measured.revision() > before.revision());

    // Jitter around the same rate neither wakes the owner nor moves the value.
    run_frames(1000.0, 16.9, 60);
    settle().await;
    assert_eq!(running.wakes(), 1, "jitter inside the band stays quiet");
    assert_eq!(running.snapshot().revision(), measured.revision());

    // A display that really runs at 120 Hz is published once the estimate moves.
    run_frames(3000.0, 8.333, 80);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 8.333));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_page_scale_change_is_observed_and_rearmed() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    assert_eq!(scale_of_primary(&running.snapshot()), Some(1.0));
    assert_eq!(pixel_ratio_listeners(), 1, "one query is armed");

    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(running.wakes(), 1, "a scale change wakes the owner");
    assert_eq!(scale_of_primary(&running.snapshot()), Some(2.0));
    assert_eq!(pixel_ratio_listeners(), 1, "the spent query was replaced");

    // The re-armed query fires for the next change as well: page zoom steps.
    set_pixel_ratio(2.5);
    settle().await;
    assert_eq!(running.wakes(), 2);
    assert_eq!(scale_of_primary(&running.snapshot()), Some(2.5));

    running.quit();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0, "quit removes the query");
    set_pixel_ratio(1.0);
    settle().await;
    assert_eq!(running.wakes(), 2, "a late change is inert");
}

#[wasm_bindgen_test]
async fn web_scale_change_that_lands_while_arming_is_not_lost() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    // The ratio moves to 2, and again to 3 between the page reading it and the
    // new query listening: a query armed for 2 would never match.
    drift_ratio_on_next_query(3.0);
    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(scale_of_primary(&running.snapshot()), Some(3.0));
    assert_eq!(pixel_ratio_listeners(), 1, "armed for the ratio in effect");
    set_pixel_ratio(4.0);
    settle().await;
    assert_eq!(scale_of_primary(&running.snapshot()), Some(4.0));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_pixel_ratio_watch_retries_after_the_browser_refuses_a_query() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    refuse_next_queries(1);
    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0, "nothing could be armed yet");
    // The next frame arms again, and the change that happened meanwhile shows.
    run_frames(0.0, 16.0, 2);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 1);
    assert_eq!(scale_of_primary(&running.snapshot()), Some(2.0));
    set_pixel_ratio(3.0);
    settle().await;
    assert_eq!(scale_of_primary(&running.snapshot()), Some(3.0));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_scale_change_measures_the_period_again() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    let now = run_frames(0.0, 16.667, 35);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 16.667));

    // The window moves to a 120 Hz display with another scale: the old
    // intervals describe the previous display and are forgotten.
    set_pixel_ratio(2.0);
    run_frames(now + 5.0, 8.333, 26);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 8.333));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_visibility_restarts_the_estimate() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    let now = run_frames(0.0, 16.667, 35);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 16.667));

    set_hidden(true);
    set_hidden(false);
    // Few enough frames that only a restart can have forgotten the old ones.
    run_frames(now + 60_000.0, 8.333, 26);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 8.333));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_screen_change_event_refreshes_the_snapshot() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    assert_eq!(window_screen_listeners(), 1, "screen.onchange exists");
    assert_eq!(
        running.snapshot().primary().map(|d| d.bounds.size.width),
        Some(1280)
    );
    // The window moved to a screen of the same pixel ratio and another size:
    // only `change` on `window.screen` says so.
    change_window_screen(1600);
    settle().await;
    assert_eq!(running.wakes(), 1);
    assert_eq!(
        running.snapshot().primary().map(|d| d.bounds.size.width),
        Some(1600)
    );
    running.quit();
    settle().await;
    assert_eq!(window_screen_listeners(), 0);
}

#[wasm_bindgen_test]
async fn web_without_a_screen_change_event_listens_to_nothing_there() {
    install_probe("none");
    let _probe = Probe;
    omit_screen_change();
    let running = Running::start();
    assert_eq!(window_screen_listeners(), 0);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_lists_screens_only_with_granted_permission() {
    install_probe("granted");
    {
        let _probe = Probe;
        let running = Running::start();
        settle().await;
        assert_eq!(
            permission_queries(),
            1,
            "the current permission name answers"
        );
        assert_eq!(screen_details_requests(), 1);
        let snapshot = running.snapshot();
        assert_eq!(snapshot.displays().len(), 2);
        let main = snapshot.primary().expect("primary");
        // The screen the window is on is scaled by the window's own pixel ratio
        // (1), not by the 2 its ScreenDetailed reports: page zoom is in the former.
        assert_eq!(main.scale_factor, 1.0);
        assert_eq!(main.bounds.size.width, 1920);
        running.quit();
        settle().await;
    }

    // Withheld: the API exists, the permission was never granted. It is read,
    // never requested, and the snapshot holds the one screen.
    install_probe("prompt");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(
        screen_details_requests(),
        0,
        "getScreenDetails would prompt"
    );
    assert_eq!(running.snapshot().displays().len(), 1);
    assert_eq!(
        running.snapshot().displays()[0].bounds.size.width,
        1280,
        "the one screen is window.screen"
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_falls_back_to_the_older_permission_name() {
    install_probe("legacy");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(
        permission_queries(),
        2,
        "the current name rejected, then the older"
    );
    assert_eq!(running.snapshot().displays().len(), 2);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_notices_a_permission_granted_after_startup() {
    install_probe("prompt");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(running.snapshot().displays().len(), 1);
    assert_eq!(permission_listeners(), 1, "the permission is watched");
    let wakes = running.wakes();

    // The application asked for access itself and the user granted it.
    grant_permission();
    settle().await;
    assert_eq!(screen_details_requests(), 1);
    assert_eq!(running.snapshot().displays().len(), 2);
    assert!(running.wakes() > wakes);

    revoke_permission();
    settle().await;
    assert_eq!(running.snapshot().displays().len(), 1, "access ended");
    assert_eq!(details_listeners(), 0);
    assert_eq!(listed_screen_listeners(), 0);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_removes_every_display_listener_on_quit() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 1);
    assert_eq!(document_listeners(), 1, "visibilitychange");
    assert_eq!(window_screen_listeners(), 1);
    assert_eq!(permission_listeners(), 1);
    assert_eq!(
        details_listeners(),
        2,
        "screenschange and currentscreenchange"
    );
    assert_eq!(listed_screen_listeners(), 2, "one per listed screen");

    plug_in_screen();
    settle().await;
    assert_eq!(
        listed_screen_listeners(),
        3,
        "the new screen is followed too"
    );

    running.quit();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0);
    assert_eq!(document_listeners(), 0);
    assert_eq!(window_screen_listeners(), 0);
    assert_eq!(permission_listeners(), 0);
    assert_eq!(details_listeners(), 0);
    assert_eq!(listed_screen_listeners(), 0);
}

#[wasm_bindgen_test]
async fn web_without_window_management_reports_one_display() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(permission_queries(), 0);
    assert_eq!(screen_details_requests(), 0);
    assert_eq!(running.snapshot().displays().len(), 1);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_screen_identity_follows_the_js_object() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let first = running.snapshot();
    let ids: Vec<_> = first.displays().iter().map(|display| display.id).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(
        ids[0], ids[1],
        "two screens with one label are two displays"
    );

    // The browser lists the same objects again, reordered: same ids.
    reorder_screens();
    settle().await;
    let reordered = running.snapshot();
    let after: Vec<_> = reordered
        .displays()
        .iter()
        .map(|display| display.id)
        .collect();
    assert_eq!(after, vec![ids[1], ids[0]]);

    // A third monitor is a new object: a fresh id, the others unchanged.
    plug_in_screen();
    settle().await;
    let plugged = running.snapshot();
    assert_eq!(plugged.displays().len(), 3);
    assert_eq!(plugged.displays()[0].id, ids[1]);
    assert_eq!(plugged.displays()[1].id, ids[0]);
    assert!(!ids.contains(&plugged.displays()[2].id));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_listed_screen_that_changes_size_is_seen() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    assert_eq!(running.snapshot().displays()[1].bounds.size.width, 2880);
    // A screen other than the window's changes resolution; the list is the same.
    change_side_screen(2560);
    settle().await;
    assert_eq!(running.snapshot().displays()[1].bounds.size.width, 3840);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_page_zoom_scales_the_current_screen_with_screen_access() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let before = running.snapshot();
    assert_eq!(scale_of_primary(&before), Some(1.0));
    // The window's pixel ratio includes page zoom; the screen's own would not.
    set_pixel_ratio(3.0);
    settle().await;
    let zoomed = running.snapshot();
    assert_eq!(scale_of_primary(&zoomed), Some(3.0));
    assert_eq!(
        zoomed.displays()[1].scale_factor,
        1.5,
        "another screen keeps its own"
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_period_belongs_to_the_screen_the_window_is_on() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    run_frames(0.0, 16.667, 35);
    settle().await;
    assert_eq!(
        has_period(&running.snapshot()),
        vec![true, false],
        "only the current screen is measured"
    );

    // The current screen's own metrics change: it is the same screen, and what
    // was measured still holds.
    current_screen_changes_size(2000);
    settle().await;
    assert_eq!(has_period(&running.snapshot()), vec![true, false]);
    assert_eq!(running.snapshot().displays()[0].bounds.size.width, 2000);

    // The window moves to the other screen: what was measured on the first
    // describes neither, and the measurement starts over on the second.
    move_window_to_side_screen();
    settle().await;
    assert_eq!(has_period(&running.snapshot()), vec![false, false]);
    run_frames(10_000.0, 8.333, 35);
    settle().await;
    let moved = running.snapshot();
    assert_eq!(has_period(&moved), vec![false, true]);
    assert!(near(
        moved.displays()[1]
            .refresh_period
            .map(flui_platform_api::RefreshPeriod::get),
        8.333
    ));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_names_the_display_the_snapshot_describes() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let window = {
        let owner = running.owner.borrow();
        let owner = owner.as_ref().expect("owner");
        match owner.open_window(WindowOptions::default()).expect("window") {
            WindowOpen::Ready(window) => window,
            WindowOpen::Pending(_) => panic!("a Web window opens at once"),
        }
    };
    let snapshot = running.snapshot();
    let main = snapshot.displays()[0].id;
    let side = snapshot.displays()[1].id;
    assert_eq!(window.display_id(), Some(main), "the current screen");
    assert_eq!(
        snapshot.window_display(window.display_id()).map(|d| d.id),
        Some(main)
    );

    move_window_to_side_screen();
    settle().await;
    assert_eq!(window.display_id(), Some(side), "follows currentScreen");

    // The period reaches the window through the snapshot it names.
    run_frames(0.0, 8.333, 35);
    settle().await;
    let display = running
        .snapshot()
        .window_display(window.display_id())
        .expect("window display");
    assert!(near(display.refresh_period.map(|p| p.get()), 8.333));
    drop(window);
    running.quit();
    settle().await;
}
