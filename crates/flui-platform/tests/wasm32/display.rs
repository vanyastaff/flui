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

use flui_platform::{OwnerPlatform, Platform, WebPlatform};
use flui_platform_api::DisplaySnapshot;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(inline_js = r#"
let restore;
let rafCallback = null;
let ratio = 1;
let ratioQueries = [];
let hidden = false;
let document;
let permission = 'granted';
let permissionStatus;
let permissionNames = [];
let screenDetailsCalls = 0;
let details;
let mainScreen;
let sideScreen;

function screen(label, left, dppx, primary) {
    return {
        label, left, top: 0, width: 1920, height: 1080,
        availLeft: left, availTop: 0, availWidth: 1920, availHeight: 1040,
        devicePixelRatio: dppx, isPrimary: primary,
    };
}

// `windowManagement`: 'none' omits the API, 'granted' and 'prompt' set the
// permission it reports.
export function installDisplayProbe(windowManagement) {
    const names = [
        'Window', 'matchMedia', 'requestAnimationFrame', 'devicePixelRatio', 'screen',
        'document', 'navigator', 'getScreenDetails',
    ];
    const descriptors = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name));
    const previous = Object.getPrototypeOf(globalThis);
    class ProbeWindow {}
    globalThis.Window = ProbeWindow;
    Object.setPrototypeOf(globalThis, ProbeWindow.prototype);
    const define = (name, value) =>
        Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
    rafCallback = null;
    ratio = 1;
    ratioQueries = [];
    hidden = false;
    permissionNames = [];
    screenDetailsCalls = 0;
    define('devicePixelRatio', ratio);
    define('screen', { width: 1280, height: 720 });
    define('requestAnimationFrame', callback => { rafCallback = callback; return 1; });
    define('matchMedia', query => {
        const target = new EventTarget();
        const match = /^\(resolution: ([0-9.]+)dppx\)$/.exec(query);
        target.dppx = match ? Number(match[1]) : null;
        target.listeners = 0;
        Object.defineProperty(target, 'matches', { get: () => target.dppx === ratio });
        const add = target.addEventListener.bind(target);
        const remove = target.removeEventListener.bind(target);
        target.addEventListener = (name, callback) => { target.listeners += 1; add(name, callback); };
        target.removeEventListener = (name, callback) => { target.listeners -= 1; remove(name, callback); };
        if (match) ratioQueries.push(target);
        return target;
    });
    document = new EventTarget();
    Object.defineProperty(document, 'hidden', { get: () => hidden });
    define('document', document);

    mainScreen = screen('Main', 0, 2, true);
    sideScreen = screen('Main', 1920, 1, false);
    details = new EventTarget();
    details.screens = [mainScreen, sideScreen];
    details.currentScreen = mainScreen;
    permissionStatus = new EventTarget();
    Object.defineProperty(permissionStatus, 'state', { get: () => permission });
    permission = windowManagement === 'prompt' ? 'prompt' : 'granted';
    if (windowManagement !== 'none') {
        define('navigator', {
            permissions: {
                query: ({ name }) => {
                    permissionNames.push(name);
                    // The current name only: an unknown one rejects, as a browser does.
                    return name === 'window-management'
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
// Run `count` animation frames `interval` milliseconds apart from `start`;
// the timestamp after the last one.
export function runFrames(start, interval, count) {
    let now = start;
    for (let frame = 0; frame < count; frame += 1) {
        const callback = rafCallback;
        rafCallback = null;
        if (!callback) break;
        callback(now);
        now += interval;
    }
    return now;
}
export function setPixelRatio(next) {
    const before = ratio;
    ratio = next;
    globalThis.devicePixelRatio = next;
    for (const query of ratioQueries.slice()) {
        if (query.dppx === before && query.listeners > 0) query.dispatchEvent(new Event('change'));
    }
}
export function pixelRatioListeners() { return ratioQueries.reduce((sum, query) => sum + query.listeners, 0); }
export function setHidden(value) {
    hidden = value;
    document.dispatchEvent(new Event('visibilitychange'));
}
export function permissionQueries() { return permissionNames.length; }
export function screenDetailsRequests() { return screenDetailsCalls; }
export function listenerCountOnDetails() { return details.listenerCount?.() ?? -1; }
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
    #[wasm_bindgen(js_name = pixelRatioListeners)]
    fn pixel_ratio_listeners() -> i32;
    #[wasm_bindgen(js_name = setHidden)]
    fn set_hidden(value: bool);
    #[wasm_bindgen(js_name = permissionQueries)]
    fn permission_queries() -> usize;
    #[wasm_bindgen(js_name = screenDetailsRequests)]
    fn screen_details_requests() -> usize;
    #[wasm_bindgen(js_name = plugInScreen)]
    fn plug_in_screen();
    #[wasm_bindgen(js_name = reorderScreens)]
    fn reorder_screens();
    #[wasm_bindgen(js_name = moveWindowToSideScreen)]
    fn move_window_to_side_screen();
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

fn near(actual: Option<Duration>, expected_ms: f64) -> bool {
    actual.is_some_and(|actual| actual.abs_diff(millis(expected_ms)) <= Duration::from_micros(5))
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

    // A display that really runs at 120 Hz is published once the median moves.
    run_frames(3000.0, 8.333, 60);
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
    assert_eq!(
        running
            .snapshot()
            .primary()
            .map(|display| display.scale_factor),
        Some(1.0)
    );
    assert_eq!(pixel_ratio_listeners(), 1, "one query is armed");

    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(running.wakes(), 1, "a scale change wakes the owner");
    assert_eq!(
        running
            .snapshot()
            .primary()
            .map(|display| display.scale_factor),
        Some(2.0)
    );
    assert_eq!(pixel_ratio_listeners(), 1, "the spent query was replaced");

    // The re-armed query fires for the next change as well: page zoom steps.
    set_pixel_ratio(2.5);
    settle().await;
    assert_eq!(running.wakes(), 2);
    assert_eq!(
        running
            .snapshot()
            .primary()
            .map(|display| display.scale_factor),
        Some(2.5)
    );

    running.quit();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0, "quit removes the query");
    set_pixel_ratio(1.0);
    settle().await;
    assert_eq!(running.wakes(), 2, "a late change is inert");
}

#[wasm_bindgen_test]
async fn web_scale_change_measures_the_period_again() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    let now = run_frames(0.0, 16.667, 30);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 16.667));

    // The window moves to a 120 Hz display with another scale: the old
    // intervals describe the previous display and are forgotten.
    set_pixel_ratio(2.0);
    run_frames(now + 5.0, 8.333, 14);
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
    let now = run_frames(0.0, 16.667, 30);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 16.667));

    set_hidden(true);
    set_hidden(false);
    // Few enough frames that only a restart can have forgotten the old ones.
    run_frames(now + 60_000.0, 8.333, 14);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 8.333));
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
        assert_eq!(main.scale_factor, 2.0);
        assert_eq!(
            main.bounds.size.width, 3840,
            "CSS width times that screen's ratio"
        );
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
async fn web_period_belongs_to_the_screen_the_window_is_on() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    run_frames(0.0, 16.667, 30);
    settle().await;
    let snapshot = running.snapshot();
    let periods: Vec<_> = snapshot
        .displays()
        .iter()
        .map(|display| display.refresh_period.is_some())
        .collect();
    assert_eq!(
        periods,
        vec![true, false],
        "only the current screen is measured"
    );

    // The window moves to the other screen: what was measured on the first
    // describes neither, and the measurement starts over on the second.
    move_window_to_side_screen();
    settle().await;
    let has_period = |snapshot: &DisplaySnapshot| -> Vec<bool> {
        snapshot
            .displays()
            .iter()
            .map(|display| display.refresh_period.is_some())
            .collect()
    };
    assert_eq!(has_period(&running.snapshot()), vec![false, false]);
    run_frames(10_000.0, 8.333, 30);
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
