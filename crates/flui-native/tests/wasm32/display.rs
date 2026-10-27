//! The Web display source against a controlled browser boundary: animation
//! frames with chosen timestamps, a pixel ratio the test changes, a page
//! visibility it flips and a Window Management API it grants, withholds or
//! omits. Runs on Node in the wasm lane; nothing here touches a real display.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_native::{HostWindow, OwnerPlatform, Platform, WebPlatform, WindowOpen};
use flui_platform::DisplaySnapshot;
use flui_platform::WindowOptions;
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
let windowListeners = [];
let lastCanvas;
let resizeObservers = [];

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
        this.padding = 0;
    }
    getRootNode() { return document; }
    contains(node) { return node === this; }
    setAttribute() {}
}

// The page's own `flui-canvas`, laid out as a browser lays out a replaced
// element in a `content-box`: a side with no CSS size is the backing store's
// size in CSS pixels, or the other side at the store's aspect ratio, and a box
// below `minHeight` is raised to it with the ratio kept. A size set on its
// style wins. `clientWidth`/`clientHeight` are the padding box; assigning one
// lays the canvas out at that padding box.
class PageCanvas extends FakeCanvas {
    constructor({ cssWidth, minHeight, padding }) {
        super();
        this.padding = padding;
        this.minHeight = minHeight ?? 0;
        this.declared = cssWidth === undefined ? {} : { width: `${cssWidth}px` };
        this.writes = [];
        const canvas = this;
        this.style = {
            setProperty(name, value) {
                canvas.declared[name] = value;
                if (name === 'width' || name === 'height') canvas.writes.push(`${name}: ${value}`);
            },
        };
        const side = (index, name) => ({
            get: () => this.content()[index] + 2 * this.padding,
            set: value => { this.declared[name] = `${value - 2 * this.padding}px`; },
        });
        Object.defineProperty(this, 'clientWidth', side(0, 'width'));
        Object.defineProperty(this, 'clientHeight', side(1, 'height'));
    }
    content() {
        const px = name => (this.declared[name] === undefined ? undefined : parseFloat(this.declared[name]));
        const ratio = this.width / this.height;
        let [width, height] = [px('width'), px('height')];
        if (width === undefined && height === undefined) {
            [width, height] = [this.width, this.height];
            if (height < this.minHeight) [width, height] = [this.minHeight * ratio, this.minHeight];
        } else if (width === undefined) {
            width = height * ratio;
        } else if (height === undefined) {
            height = width / ratio;
        }
        return [Math.round(width), Math.round(height)];
    }
}

// `windowManagement`: 'none' omits the API; 'granted', 'prompt' and 'legacy'
// (only the older permission name is known) set the permission it reports.
export function installDisplayProbe(windowManagement) {
    const names = [
        'Window', 'matchMedia', 'requestAnimationFrame', 'cancelAnimationFrame',
        'devicePixelRatio', 'screen', 'document', 'navigator', 'getScreenDetails',
        'HTMLCanvasElement', 'addEventListener', 'removeEventListener', 'ResizeObserver',
        'getComputedStyle',
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
    resizeObservers = [];
    delete globalThis.ResizeObserver;
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
    document.createElement = () => (lastCanvas = new FakeCanvas());
    define('document', document);
    define('HTMLCanvasElement', FakeCanvas);
    define('getComputedStyle', element => {
        const padding = `${element.padding ?? 0}px`;
        return {
            paddingTop: padding, paddingRight: padding, paddingBottom: padding, paddingLeft: padding,
            writingMode: 'horizontal-tb',
            // The content box, as `content-box` computes `width` and `height`.
            getPropertyValue: name => ({
                width: `${element.clientWidth - 2 * (element.padding ?? 0)}px`,
                height: `${element.clientHeight - 2 * (element.padding ?? 0)}px`,
            })[name] ?? '',
        };
    });
    windowListeners = [];
    define('addEventListener', (name, callback) => { windowListeners.push({ name, callback }); });
    define('removeEventListener', (name, callback) => {
        windowListeners = windowListeners.filter(entry => entry.name !== name || entry.callback !== callback);
    });

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
export function fireWindowResize() {
    for (const entry of windowListeners.slice()) if (entry.name === 'resize') entry.callback(new Event('resize'));
}
// The backing store of the canvas the page made last: [width, height].
export function canvasBackingSize() { return [lastCanvas.width, lastCanvas.height]; }
// A `ResizeObserver` that observes the device-pixel content box, as Chrome and
// Firefox do, or rejects that box, as Safari does.
export function installResizeObserver(devicePixels) {
    globalThis.ResizeObserver = class {
        constructor(callback) {
            this.callback = callback;
            this.targets = [];
            resizeObservers.push(this);
        }
        observe(target, options) {
            const box = options?.box ?? 'content-box';
            if (box === 'device-pixel-content-box' && !devicePixels) {
                throw new TypeError(`'${box}' is not a valid ResizeObserverBoxOptions value`);
            }
            this.targets.push({ target, box });
        }
        disconnect() { this.targets = []; }
    };
    globalThis.ResizeObserver.devicePixels = devicePixels;
}
// The page lays the last canvas out at a padding box of `width`×`height`
// whose content box, `padding` CSS pixels in on every side, the browser
// snapped to `deviceWidth`×`deviceHeight` device pixels. `boxes` names the
// observed boxes that changed, so the observers of those report it; every
// entry carries the device size where the browser has it, as Chrome's do.
function layOut(width, height, deviceWidth, deviceHeight, padding, boxes) {
    lastCanvas.padding = padding;
    lastCanvas.clientWidth = width;
    lastCanvas.clientHeight = height;
    notifyObservers(width, height, deviceWidth, deviceHeight, padding, boxes);
}
function notifyObservers(width, height, deviceWidth, deviceHeight, padding, boxes) {
    const content = { width: width - 2 * padding, height: height - 2 * padding };
    const entry = {
        target: lastCanvas,
        contentRect: content,
        contentBoxSize: [{ inlineSize: content.width, blockSize: content.height }],
    };
    if (globalThis.ResizeObserver.devicePixels) {
        entry.devicePixelContentBoxSize = [{ inlineSize: deviceWidth, blockSize: deviceHeight }];
    }
    for (const observer of resizeObservers.slice()) {
        const watched = each => each.target === lastCanvas && boxes.includes(each.box);
        if (observer.targets.some(watched)) observer.callback([entry], observer);
    }
}
// Every box changed.
export function layOutCanvas(width, height, deviceWidth, deviceHeight, padding = 0) {
    layOut(width, height, deviceWidth, deviceHeight, padding, ['content-box', 'device-pixel-content-box', 'border-box']);
}
// A layout in which only the boxes `changed` names, space-separated, moved:
// the observers of the others do not report it.
export function changeCanvasBoxes(width, height, deviceWidth, deviceHeight, padding, changed) {
    layOut(width, height, deviceWidth, deviceHeight, padding, changed.split(' '));
}
// The page holds a `PageCanvas` with the CSS width and minimum height given
// (`undefined` for none) and `padding`.
export function putCanvasInPage(cssWidth, minHeight, padding) {
    lastCanvas = new PageCanvas({ cssWidth, minHeight, padding });
    document.getElementById = id => (id === 'flui-canvas' ? lastCanvas : null);
}
// The browser lays the canvas out again: every observer reports the box it has
// now, at the current ratio.
export function reportCanvasBox() {
    const [width, height, padding] = [lastCanvas.clientWidth, lastCanvas.clientHeight, lastCanvas.padding];
    notifyObservers(width, height, Math.round((width - 2 * padding) * ratio),
        Math.round((height - 2 * padding) * ratio), padding,
        ['content-box', 'device-pixel-content-box', 'border-box']);
}
// The CSS sizes set on the page canvas's style, in order, as `name: value`.
export function canvasSizeWrites() { return lastCanvas.writes; }
export function windowResizeListeners() { return windowListeners.filter(entry => entry.name === 'resize').length; }
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
export function resizeMainScreen(width, height, dppx) {
    Object.assign(mainScreen, { width, height, availWidth: width, availHeight: height, devicePixelRatio: dppx });
}
export function changeSideScreen(width) {
    sideScreen.width = width;
    sideScreen.dispatchEvent(new Event('change'));
}
export function nextOwnerTask() { return new Promise(resolve => setImmediate(resolve)); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = installDisplayProbe)]
    pub(super) fn install_probe(window_management: &str);
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
    #[wasm_bindgen(js_name = fireWindowResize)]
    fn fire_window_resize();
    #[wasm_bindgen(js_name = windowResizeListeners)]
    fn window_resize_listeners() -> usize;
    #[wasm_bindgen(js_name = canvasBackingSize)]
    fn canvas_backing_size() -> Vec<u32>;
    #[wasm_bindgen(js_name = installResizeObserver)]
    fn install_resize_observer(device_pixels: bool);
    #[wasm_bindgen(js_name = layOutCanvas)]
    fn lay_out_canvas(width: u32, height: u32, device_width: u32, device_height: u32);
    #[wasm_bindgen(js_name = changeCanvasBoxes)]
    fn change_canvas_boxes(
        width: u32,
        height: u32,
        device_width: u32,
        device_height: u32,
        padding: f64,
        changed: &str,
    );
    #[wasm_bindgen(js_name = putCanvasInPage)]
    fn put_canvas_in_page(css_width: Option<u32>, min_height: Option<u32>, padding: u32);
    #[wasm_bindgen(js_name = reportCanvasBox)]
    fn report_canvas_box();
    #[wasm_bindgen(js_name = canvasSizeWrites)]
    fn canvas_size_writes() -> Vec<String>;
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
    #[wasm_bindgen(js_name = resizeMainScreen)]
    fn resize_main_screen(width: u32, height: u32, dppx: f64);
    #[wasm_bindgen(js_name = changeSideScreen)]
    fn change_side_screen(width: u32);
    #[wasm_bindgen(js_name = nextOwnerTask)]
    fn next_task() -> js_sys::Promise;
}

pub(super) struct Probe;
impl Drop for Probe {
    fn drop(&mut self) {
        restore_probe();
    }
}

/// A running platform and the number of owner wakes it has delivered.
pub(super) struct Running {
    pub(super) owner: Rc<RefCell<Option<OwnerPlatform>>>,
    wakes: Arc<AtomicUsize>,
}

impl Running {
    pub(super) fn start() -> Self {
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

    pub(super) fn quit(&self) {
        self.owner
            .borrow()
            .as_ref()
            .expect("owner")
            .proxy()
            .request_quit()
            .expect("quit");
    }
}

pub(super) async fn settle() {
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
        .map(flui_platform::RefreshPeriod::get)
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
        // The screen the window is on reports the window's own pixel ratio (1),
        // which includes page zoom, but its bounds are measured with the 2 its
        // ScreenDetailed reports, which does not.
        assert_eq!(main.scale_factor, 1.0);
        assert_eq!(main.bounds.size.width, 3840);
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
    assert_eq!(running.snapshot().displays()[0].bounds.size.width, 4000);

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
            .map(flui_platform::RefreshPeriod::get),
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
        match owner
            .open_window(
                WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("window")
        {
            WindowOpen::Ready(window) => window,
            WindowOpen::Pending(_) => panic!("a Web window opens at once"),
        }
    };
    let snapshot = running.snapshot();
    let main = snapshot.displays()[0].id;
    let side = snapshot.displays()[1].id;
    assert_eq!(window.state().display, Some(main), "the current screen");
    assert_eq!(
        snapshot
            .window_display(window.state().display)
            .map(|d| d.id),
        Some(main)
    );

    move_window_to_side_screen();
    settle().await;
    assert_eq!(window.state().display, Some(side), "follows currentScreen");

    // The period reaches the window through the snapshot it names.
    run_frames(0.0, 8.333, 35);
    settle().await;
    let display = running
        .snapshot()
        .window_display(window.state().display)
        .expect("window display");
    assert!(near(
        display
            .refresh_period
            .map(flui_platform::RefreshPeriod::get),
        8.333
    ));
    drop(window);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_bounds_do_not_inflate_with_page_zoom_with_screen_access() {
    install_probe("granted");
    let _probe = Probe;
    // A 3440 x 1440 monitor at an OS scale of 100%, whose page is zoomed to 125%.
    resize_main_screen(3440, 1440, 1.0);
    set_pixel_ratio(1.25);
    let running = Running::start();
    settle().await;
    let main = running.snapshot().displays()[0].clone();
    assert_eq!(main.scale_factor, 1.25, "the scale includes page zoom");
    assert_eq!(
        (main.bounds.size.width, main.bounds.size.height),
        (3440, 1440)
    );
    assert_eq!(
        (
            main.usable_bounds.size.width,
            main.usable_bounds.size.height
        ),
        (3440, 1440)
    );
    set_pixel_ratio(1.5);
    settle().await;
    let main = running.snapshot().displays()[0].clone();
    assert_eq!(main.scale_factor, 1.5);
    assert_eq!(
        (main.bounds.size.width, main.bounds.size.height),
        (3440, 1440)
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_bounds_do_not_inflate_with_page_zoom_without_screen_access() {
    install_probe("none");
    let _probe = Probe;
    // A 1280 x 720 (CSS pixel) screen on a display scaled to 200%, page at 100%.
    set_pixel_ratio(2.0);
    let running = Running::start();
    let size = |snapshot: &DisplaySnapshot| {
        let bounds = snapshot.displays()[0].bounds;
        (bounds.size.width, bounds.size.height)
    };
    assert_eq!(size(&running.snapshot()), (2560, 1440));
    // Zoom to 125%: the pixel ratio is 2.5, the screen is the same screen.
    set_pixel_ratio(2.5);
    settle().await;
    let zoomed = running.snapshot();
    assert_eq!(scale_of_primary(&zoomed), Some(2.5));
    assert_eq!(size(&zoomed), (2560, 1440));
    // The window moves to a display with another scale: `change` on the screen
    // says the screen is another one, and the new ratio is its own.
    set_pixel_ratio(1.0);
    change_window_screen(1280);
    settle().await;
    assert_eq!(size(&running.snapshot()), (1280, 720));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_granting_screen_access_keeps_the_window_display() {
    install_probe("prompt");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let window = {
        let owner = running.owner.borrow();
        match owner
            .as_ref()
            .expect("owner")
            .open_window(
                WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("window")
        {
            WindowOpen::Ready(window) => window,
            WindowOpen::Pending(_) => panic!("a Web window opens at once"),
        }
    };
    let before = running.snapshot();
    let id = window.state().display.expect("the one screen");
    assert_eq!(
        before.get(id).map(|d| d.name.as_str()),
        Some("Browser Screen")
    );

    // The same physical screen is now listed: its name is known, its id is the
    // one the window already has, and the screen next to it is a new display.
    grant_permission();
    settle().await;
    let after = running.snapshot();
    assert_eq!(
        window.state().display,
        Some(id),
        "the window saw no display change"
    );
    assert_eq!(after.get(id).map(|d| d.name.as_str()), Some("Main"));
    assert_eq!(after.displays().len(), 2);
    assert_eq!(after.displays().iter().filter(|d| d.id != id).count(), 1);

    revoke_permission();
    settle().await;
    assert_eq!(
        window.state().display,
        Some(id),
        "and none when access ends"
    );
    drop(window);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_screen_change_forgets_the_period_of_the_old_monitor() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    let now = run_frames(0.0, 6.944, 40);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 6.944));

    // Without screen access `change` on `window.screen` is the only sign of a
    // move to another monitor, and it cannot be told from a change of the same
    // screen's metrics: the period is unknown until measured again.
    change_window_screen(1280);
    settle().await;
    assert_eq!(primary_period(&running.snapshot()), None);
    run_frames(now, 10.0, 40);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 10.0));
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_pixel_ratio_watch_arms_again_after_a_long_refusal() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    refuse_next_queries(10_000);
    set_pixel_ratio(2.0);
    // Far more refused frames than any fixed number of retries.
    run_frames(0.0, 16.0, 300);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0, "still refused");

    // The browser accepts queries again: a later retry arms the watch and sees
    // the ratio it missed, then the next change.
    refuse_next_queries(0);
    run_frames(0.0, 16.0, 300);
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
async fn web_pixel_ratio_watch_arms_again_from_resize_and_screen_change() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    assert_eq!(window_resize_listeners(), 1);
    refuse_next_queries(10_000);
    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0);

    // Zoom fires `resize`: the watch arms again at once, without waiting for a
    // retry frame.
    refuse_next_queries(0);
    fire_window_resize();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 1);
    assert_eq!(scale_of_primary(&running.snapshot()), Some(2.0));

    // The same through `change` on `window.screen`.
    refuse_next_queries(10_000);
    set_pixel_ratio(3.0);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0);
    refuse_next_queries(0);
    change_window_screen(1280);
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 1);
    assert_eq!(scale_of_primary(&running.snapshot()), Some(3.0));

    running.quit();
    settle().await;
    assert_eq!(window_resize_listeners(), 0);
}

#[wasm_bindgen_test]
async fn web_screen_change_keeps_the_period_while_the_current_screen_is_known() {
    install_probe("granted");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    run_frames(0.0, 6.944, 40);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 6.944));

    // With screen access `currentScreen` says whether the window moved. A
    // `change` of `window.screen` for the same current screen's metrics is not
    // a move, and the measured period still holds.
    change_window_screen(1280);
    settle().await;
    assert!(near(primary_period(&running.snapshot()), 6.944));
    running.quit();
    settle().await;
}

/// A window opened on `running`, and the scale of every resize it reports.
fn open_recorded_window(running: &Running) -> (Rc<dyn HostWindow>, Arc<Mutex<Vec<f64>>>) {
    let handler = flui_testing::window_handler::RecordingWindowHandler::new();
    let window = {
        let owner = running.owner.borrow();
        match owner
            .as_ref()
            .expect("owner")
            .open_window(WindowOptions::default(), handler.clone())
            .expect("window")
        {
            WindowOpen::Ready(window) => window,
            WindowOpen::Pending(_) => panic!("a Web window opens at once"),
        }
    };
    let resized = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&resized);
    let observed = std::rc::Rc::downgrade(&window);
    let last = std::cell::Cell::new(window.state().metrics);
    handler.react_to_state_change(move || {
        let Some(window) = observed.upgrade() else {
            return;
        };
        let metrics = window.state().metrics;
        if metrics == last.replace(metrics) {
            return;
        }
        if let Some(metrics) = metrics {
            seen.lock().expect("resizes").push(metrics.scale().get());
        }
    });
    (window, resized)
}

/// The drawable metrics the window's snapshot carries.
fn metrics(window: &Rc<dyn HostWindow>) -> flui_foundation::geometry::WindowMetrics {
    window
        .state()
        .metrics
        .expect("an open canvas has drawable metrics")
}

fn physical_size(window: &Rc<dyn HostWindow>) -> [i32; 2] {
    let size = metrics(window).physical();
    [size.width, size.height]
}

fn scale(window: &Rc<dyn HostWindow>) -> f64 {
    metrics(window).scale().get()
}

#[wasm_bindgen_test]
async fn web_window_backing_store_follows_a_pixel_ratio_change_without_a_resize() {
    install_probe("none");
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let (window, resized) = open_recorded_window(&running);
    assert_eq!(scale(&window), 1.0);
    assert_eq!(canvas_backing_size(), [300, 150], "the 300×150 box at 1×");
    assert_eq!(
        pixel_ratio_listeners(),
        2,
        "the display source's and the window's"
    );

    // The window is dragged to a display of twice the density: its CSS box
    // stays, so no `ResizeObserver` entry and no `resize` event arrive.
    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(scale(&window), 2.0, "the window follows the ratio");
    assert_eq!(
        canvas_backing_size(),
        [600, 300],
        "one texel per device pixel"
    );
    assert_eq!(*resized.lock().expect("resizes"), [2.0]);
    assert_eq!(pixel_ratio_listeners(), 2, "both queries were armed again");

    drop(window);
    running.quit();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0, "quit removes both queries");
}

#[wasm_bindgen_test]
async fn web_window_backing_store_is_the_device_pixel_size_the_browser_reports() {
    install_probe("none");
    install_resize_observer(true);
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let (window, resized) = open_recorded_window(&running);
    assert_eq!(
        pixel_ratio_listeners(),
        2,
        "the display source's and the window's"
    );

    // A ratio change that leaves the rounded device-pixel box as it was (a
    // small canvas) brings no observer entry; the window's own watch sees it.
    set_pixel_ratio(1.5);
    settle().await;
    assert_eq!(scale(&window), 1.5, "the watch synced the window");
    assert_eq!(canvas_backing_size(), [450, 225]);

    // At 1.5× the browser snaps a 301×150 box to 451×225 device pixels,
    // where `round(301 × 1.5)` would be 452.
    lay_out_canvas(301, 150, 451, 225);
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [451, 225],
        "the reported size, not the estimate"
    );
    assert_eq!(
        physical_size(&window),
        [451, 225],
        "the surface is the store"
    );
    assert_eq!(*resized.lock().expect("resizes"), [1.5, 1.5]);

    // A `resize` event in the same layout keeps the reported size.
    fire_window_resize();
    settle().await;
    assert_eq!(canvas_backing_size(), [451, 225]);
    assert_eq!(resized.lock().expect("resizes").len(), 2, "nothing changed");

    // A ratio change the device-pixel observer does not report left the
    // device box as the browser last reported it, where the estimate at
    // 1.501× would be 452.
    set_pixel_ratio(1.501);
    settle().await;
    assert_eq!(scale(&window), 1.501);
    assert_eq!(
        canvas_backing_size(),
        [451, 225],
        "the reported box holds across the ratio"
    );
    set_pixel_ratio(1.5);
    settle().await;

    // A reflow to 302 CSS pixels that leaves the rounded device-pixel box as
    // it was is reported only by the CSS-box observers, whose entries carry
    // the device size too.
    change_canvas_boxes(302, 150, 452, 225, 0.0, "content-box border-box");
    settle().await;
    assert_eq!(
        window.state().content_size.width,
        302.0,
        "the window follows the reflow"
    );
    assert_eq!(
        canvas_backing_size(),
        [452, 225],
        "the reported size, not round(302 × 1.5)"
    );

    // 0.2px of padding inside a `border-box` canvas moves only its content
    // box: the window is the padding box, which the device box does not
    // measure.
    change_canvas_boxes(302, 150, 452, 225, 0.2, "content-box");
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [453, 225],
        "the padding box's estimate, not the content box"
    );
    assert_eq!(physical_size(&window), [453, 225]);

    // 10px of padding around a `content-box` canvas grows its padding box,
    // the window, to 322×170 and moves neither content box.
    change_canvas_boxes(322, 170, 452, 225, 10.0, "border-box");
    settle().await;
    assert_eq!(
        window.state().content_size.width,
        322.0,
        "the window follows the padding"
    );
    assert_eq!(canvas_backing_size(), [483, 255]);

    drop(window);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_backing_store_keeps_to_the_ratio_a_device_size_contradicts() {
    install_probe("none");
    install_resize_observer(true);
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let (window, resized) = open_recorded_window(&running);
    lay_out_canvas(300, 150, 300, 150);
    settle().await;
    assert_eq!(canvas_backing_size(), [300, 150]);

    // The window moves to a display of twice the density. The ratio watch
    // runs before the observer reports the box at the new ratio, and the
    // 300×150 report is the box at 1×.
    set_pixel_ratio(2.0);
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [600, 300],
        "the estimate at 2×, not the 1× report"
    );
    assert_eq!(physical_size(&window), [600, 300]);
    assert_eq!(*resized.lock().expect("resizes"), [2.0]);

    // Chrome's device emulation reports the device-pixel box in the screen's
    // own pixels while `devicePixelRatio` is the emulated 2×.
    lay_out_canvas(300, 150, 300, 150);
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [600, 300],
        "a device size 2× contradicts is not the store"
    );
    assert_eq!(physical_size(&window), [600, 300]);
    assert_eq!(resized.lock().expect("resizes").len(), 1, "nothing changed");

    // The box the browser lays out at 2× is the store again.
    lay_out_canvas(300, 150, 601, 300);
    settle().await;
    assert_eq!(canvas_backing_size(), [601, 300], "the reported size");

    // `clientWidth` reads a box laid out 300.99 pixels wide from x = 0.5 as
    // 300, and 2× snaps it to 602 device pixels: two off the estimate.
    lay_out_canvas(300, 150, 602, 300);
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [602, 300],
        "a CSS pixel of snapping at 2× is two device pixels"
    );

    drop(window);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_backing_store_leaves_a_page_canvas_box_where_it_was() {
    /// How the page styles the 300×150 canvas, the padding box it lays it out
    /// at, and the CSS sizes the window sets on it.
    struct Page {
        css_width: Option<u32>,
        min_height: Option<u32>,
        padding: u32,
        laid_out: [u32; 2],
        pinned: &'static [&'static str],
    }
    let unstyled = Page {
        css_width: None,
        min_height: None,
        padding: 0,
        laid_out: [300, 150],
        pinned: &["width: 300px", "height: 150px"],
    };
    let cases = [
        // No CSS size: the box is the store's size, so the window pins it.
        Page { ..unstyled },
        // Padding: the pin is the content box, not the padding box it reads.
        Page {
            padding: 10,
            laid_out: [320, 170],
            ..unstyled
        },
        // A minimum height the box sits on: a store past it moves the box.
        Page {
            min_height: Some(300),
            laid_out: [600, 300],
            pinned: &["width: 600px", "height: 300px"],
            ..unstyled
        },
        // A CSS width: the height is the width at the store's aspect ratio,
        // which a store at the box's ratio keeps, so the page's CSS stays.
        Page {
            css_width: Some(400),
            laid_out: [400, 200],
            pinned: &[],
            ..unstyled
        },
    ];
    for Page {
        css_width,
        min_height,
        padding,
        laid_out: [width, height],
        pinned,
    } in cases
    {
        install_probe("none");
        install_resize_observer(true);
        let probe = Probe;
        set_pixel_ratio(2.0);
        put_canvas_in_page(css_width, min_height, padding);
        let running = Running::start();
        settle().await;
        let (window, _resized) = open_recorded_window(&running);

        // Each report is the box the browser laid out after the last store.
        for _ in 0..3 {
            report_canvas_box();
            settle().await;
        }
        let page = format!("{css_width:?} {min_height:?} {padding}");
        assert_eq!(
            canvas_backing_size(),
            [width * 2, height * 2],
            "{page}: the store at 2× did not move the box"
        );
        assert_eq!(
            window.state().content_size.width,
            f64::from(width),
            "{page}"
        );
        assert_eq!(
            window.state().content_size.height,
            f64::from(height),
            "{page}"
        );
        assert_eq!(canvas_size_writes(), pinned, "{page}");

        drop(window);
        running.quit();
        settle().await;
        drop(probe);
    }
}

#[wasm_bindgen_test]
async fn web_window_backing_store_is_estimated_where_the_browser_reports_css_pixels() {
    install_probe("none");
    install_resize_observer(false);
    let _probe = Probe;
    let running = Running::start();
    settle().await;
    let (window, resized) = open_recorded_window(&running);
    assert_eq!(
        pixel_ratio_listeners(),
        2,
        "a CSS-box observer misses a ratio change, so the window watches the ratio"
    );

    set_pixel_ratio(1.5);
    settle().await;
    assert_eq!(
        canvas_backing_size(),
        [450, 225],
        "the watch resized the store"
    );

    lay_out_canvas(301, 150, 451, 225);
    settle().await;
    assert_eq!(scale(&window), 1.5);
    assert_eq!(
        canvas_backing_size(),
        [452, 225],
        "round(301 × 1.5) × round(150 × 1.5)"
    );
    assert_eq!(physical_size(&window), [452, 225]);
    assert_eq!(*resized.lock().expect("resizes"), [1.5, 1.5]);

    // Padding around a `content-box` canvas moves only its border box.
    change_canvas_boxes(321, 170, 0, 0, 10.0, "border-box");
    settle().await;
    assert_eq!(
        window.state().content_size.width,
        321.0,
        "the window follows the padding"
    );
    assert_eq!(canvas_backing_size(), [482, 255]);

    drop(window);
    running.quit();
    settle().await;
    assert_eq!(pixel_ratio_listeners(), 0);
}
