//! Browser adapter contracts against a controlled JS native-API boundary.
//! Runs on Node in the existing wasm lane; it does not change OS preferences.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_native::{Platform, WebPlatform};
use flui_platform::MotionPreference;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[path = "wasm32/display.rs"]
mod display;
#[path = "wasm32/page_store.rs"]
mod page_store;
#[path = "wasm32/web_page_lifecycle.rs"]
mod web_page_lifecycle;

#[wasm_bindgen(inline_js = r#"
let restore;
let reduced = false;
let motionSupported = true;
// 'more', 'less', 'no-preference', 'custom', or null when unsupported.
let contrast = 'no-preference';
// true, false, or null when unsupported.
let forced = false;
let languages = ['en-US'];
let language = 'en-US';
let queries = [];
let windowEvents;
let windowListeners;
let registrationFails = false;
let removalFails = false;

function matchesQuery(query) {
    switch (query) {
        case '(prefers-reduced-motion: reduce)': return motionSupported && reduced;
        case '(prefers-reduced-motion: no-preference)': return motionSupported && !reduced;
        case '(prefers-contrast: more)': return contrast === 'more';
        case '(prefers-contrast: less)': return contrast === 'less';
        case '(prefers-contrast: no-preference)': return contrast === 'no-preference';
        case '(forced-colors: active)': return forced === true;
        case '(forced-colors: none)': return forced === false;
        default: throw new Error(`unexpected media query ${query}`);
    }
}
// Count the listeners a target really holds for `observed`; refuse on demand.
function guard(target, listeners, observed) {
    const add = target.addEventListener.bind(target);
    const remove = target.removeEventListener.bind(target);
    target.addEventListener = (name, callback) => {
        if (name === observed) {
            if (registrationFails) throw new Error('registration refused');
            listeners.add(callback);
        }
        add(name, callback);
    };
    target.removeEventListener = (name, callback) => {
        if (name === observed) {
            if (removalFails) throw new Error('removal refused');
            listeners.delete(callback);
        }
        remove(name, callback);
    };
}
export function installPreferenceProbe() {
    const previous = Object.getPrototypeOf(globalThis);
    const names = ['Window', 'matchMedia', 'requestAnimationFrame', 'navigator'];
    const descriptors = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name));
    class ProbeWindow {}
    windowEvents = new EventTarget();
    windowListeners = new Set();
    guard(windowEvents, windowListeners, 'languagechange');
    ProbeWindow.prototype.addEventListener = (name, callback) => windowEvents.addEventListener(name, callback);
    ProbeWindow.prototype.removeEventListener = (name, callback) => windowEvents.removeEventListener(name, callback);
    globalThis.Window = ProbeWindow;
    Object.setPrototypeOf(globalThis, ProbeWindow.prototype);
    reduced = false;
    motionSupported = true;
    contrast = 'no-preference';
    forced = false;
    languages = ['en-US'];
    language = 'en-US';
    queries = [];
    registrationFails = false;
    removalFails = false;
    Object.defineProperty(globalThis, 'navigator', {
        configurable: true,
        get: () => ({ languages, language }),
    });
    globalThis.matchMedia = query => {
        matchesQuery(query);
        const target = new EventTarget();
        const listeners = new Set();
        guard(target, listeners, 'change');
        Object.defineProperty(target, 'matches', { get: () => matchesQuery(query) });
        target.listenerCount = () => listeners.size;
        queries.push(target);
        return target;
    };
    globalThis.requestAnimationFrame = () => 1;
    restore = () => {
        Object.setPrototypeOf(globalThis, previous);
        names.forEach((name, index) => {
            if (descriptors[index]) Object.defineProperty(globalThis, name, descriptors[index]);
            else delete globalThis[name];
        });
    };
}
function changeQueries() {
    for (const query of queries) query.dispatchEvent(new Event('change'));
}
export function setReducedMotion(value) { reduced = value; changeQueries(); }
export function setContrast(value, forcedColors) { contrast = value; forced = forcedColors; changeQueries(); }
export function setLanguages(list, fallback) {
    languages = list;
    language = fallback;
    windowEvents.dispatchEvent(new Event('languagechange'));
}
export function preferenceListenerCount() {
    return queries.reduce((sum, query) => sum + query.listenerCount(), 0) + windowListeners.size;
}
export function restorePreferenceProbe() { restore(); }
export function refusePreferenceRegistration(value) { registrationFails = value; }
export function refusePreferenceRemoval(value) { removalFails = value; }
export function supportMotion(value) { motionSupported = value; }
export function nextOwnerTask() { return new Promise(resolve => setImmediate(resolve)); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = installPreferenceProbe)]
    fn install_probe();
    #[wasm_bindgen(js_name = setReducedMotion)]
    fn set_reduced(value: bool);
    #[wasm_bindgen(js_name = setContrast)]
    fn set_contrast(value: JsValue, forced_colors: JsValue);
    #[wasm_bindgen(js_name = setLanguages)]
    fn set_languages(list: Vec<String>, fallback: JsValue);
    #[wasm_bindgen(js_name = preferenceListenerCount)]
    fn listener_count() -> usize;
    #[wasm_bindgen(js_name = restorePreferenceProbe)]
    fn restore_probe();
    #[wasm_bindgen(js_name = nextOwnerTask)]
    fn next_task() -> js_sys::Promise;
    #[wasm_bindgen(js_name = refusePreferenceRegistration)]
    fn refuse_registration(value: bool);
    #[wasm_bindgen(js_name = refusePreferenceRemoval)]
    fn refuse_removal(value: bool);
    #[wasm_bindgen(js_name = supportMotion)]
    fn support_motion(value: bool);
}

/// Every query and the window's `languagechange` hold one listener each.
const LISTENERS: usize = 8;

struct Probe;
impl Drop for Probe {
    fn drop(&mut self) {
        restore_probe();
    }
}

#[wasm_bindgen_test]
async fn browser_motion_observations_wake_replace_and_retire_their_owner() {
    install_probe();
    let _probe = Probe;
    support_motion(false);
    {
        let unsupported = WebPlatform::new().expect("unsupported query source");
        assert!(
            unsupported
                .preferences()
                .expect("unknown observation")
                .motion()
                .is_none(),
            "an unmatched unsupported query is not NoPreference"
        );
    }
    assert_eq!(listener_count(), 0);
    support_motion(true);
    refuse_registration(true);
    assert!(
        WebPlatform::new().is_err(),
        "failed registration is not an observation"
    );
    assert_eq!(listener_count(), 0);
    refuse_registration(false);
    let platform = WebPlatform::new().expect("browser source");
    assert_eq!(
        platform
            .preferences()
            .expect("initial observation")
            .motion(),
        Some(MotionPreference::NoPreference)
    );
    let owner = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&owner);
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    Box::new(platform)
        .run(Box::new(move |capability| {
            let proxy = capability.proxy();
            capability
                .on_wake(Box::new(move || {
                    if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                        proxy.wake().expect("reentrant next turn");
                    }
                }))
                .expect("browser owner hook");
            installed.replace(Some(capability));
            Ok(())
        }))
        .expect("browser loop");
    assert_eq!(listener_count(), LISTENERS);
    set_reduced(true);
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        0,
        "DOM invalidation cannot run owner code inline"
    );
    next_task().await.expect("owner tasks");
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        2,
        "reentry is delivered on another turn"
    );
    assert_eq!(
        owner
            .borrow()
            .as_ref()
            .expect("owner")
            .preferences()
            .expect("changed observation")
            .motion(),
        Some(MotionPreference::Reduce)
    );
    let replacement = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&replacement);
    owner
        .borrow()
        .as_ref()
        .expect("owner")
        .on_wake(Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("replace hook");
    set_reduced(false);
    next_task().await.expect("replacement task");
    assert_eq!(replacement.load(Ordering::SeqCst), 1);
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        2,
        "outgoing hook stays retired"
    );
    owner
        .borrow()
        .as_ref()
        .expect("owner")
        .proxy()
        .request_quit()
        .expect("quit");
    set_reduced(true);
    next_task().await.expect("quit task");
    assert_eq!(listener_count(), 0, "quit removes the native listener");
    assert!(
        owner
            .borrow()
            .as_ref()
            .expect("owner")
            .preferences()
            .is_err()
    );
    set_reduced(false);
    next_task().await.expect("late task");
    assert_eq!(
        replacement.load(Ordering::SeqCst),
        1,
        "late invalidation is inert"
    );

    let (retiring, retiring_wakes) = counter_owner();
    refuse_removal(true);
    retiring
        .proxy()
        .request_quit()
        .expect("quit with refused removal");
    next_task().await.expect("retirement task");
    assert_eq!(
        listener_count(),
        LISTENERS,
        "refused removal retains a valid trampoline"
    );
    set_reduced(true);
    next_task().await.expect("retained callback task");
    assert_eq!(
        retiring_wakes.load(Ordering::SeqCst),
        0,
        "retained callback is inert"
    );
    assert!(retiring.preferences().is_err());
    refuse_removal(false);
    let (successor, successor_wakes) = counter_owner();
    set_reduced(false);
    next_task().await.expect("successor task");
    assert_eq!(successor_wakes.load(Ordering::SeqCst), 1);
    assert_eq!(
        retiring_wakes.load(Ordering::SeqCst),
        0,
        "old source cannot reach successor"
    );
    successor.proxy().request_quit().expect("successor quit");
    next_task().await.expect("successor retirement");
    assert_eq!(
        listener_count(),
        LISTENERS,
        "only the inert refused removal remains"
    );
}

/// Contrast and the preferred languages are sampled again on every wake, from
/// the media queries and `navigator.languages`; a feature the browser lacks
/// stays unobserved, and retirement removes the window's listener too.
#[wasm_bindgen_test]
async fn browser_contrast_and_languages_reach_their_owner() {
    use flui_platform::{Locale, SystemPreferences};

    install_probe();
    let _probe = Probe;
    let locales = |preferences: &SystemPreferences| {
        preferences.locales().map(|locales| {
            locales
                .iter()
                .map(Locale::to_language_tag)
                .collect::<Vec<_>>()
        })
    };
    set_contrast(JsValue::NULL, JsValue::NULL);
    set_languages(Vec::new(), JsValue::UNDEFINED);
    {
        let unknown = WebPlatform::new()
            .expect("unsupported features")
            .preferences()
            .expect("unknown observation");
        assert_eq!(unknown.high_contrast(), None, "neither feature is known");
        assert_eq!(locales(&unknown), None, "no language is not an empty list");
    }
    set_languages(Vec::new(), "de-DE".into());
    {
        let fallback = WebPlatform::new()
            .expect("fallback source")
            .preferences()
            .expect("fallback observation");
        assert_eq!(
            locales(&fallback),
            Some(vec!["de-DE".to_owned()]),
            "navigator.language stands in for an empty list"
        );
    }
    set_contrast("no-preference".into(), false.into());
    set_languages(vec!["en-US".to_owned()], "en-US".into());

    let owner = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&owner);
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    Box::new(WebPlatform::new().expect("browser source"))
        .run(Box::new(move |capability| {
            capability
                .on_wake(Box::new(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                }))
                .expect("browser owner hook");
            installed.replace(Some(capability));
            Ok(())
        }))
        .expect("browser loop");
    let sample = || {
        owner
            .borrow()
            .as_ref()
            .expect("owner")
            .preferences()
            .expect("observation")
    };
    assert_eq!(sample().high_contrast(), Some(false));
    for (contrast, forced, expected) in [
        ("more", Some(false), Some(true)),
        ("no-preference", Some(true), Some(true)),
        ("less", Some(false), Some(false)),
        ("custom", None, None),
    ] {
        let before = wakes.load(Ordering::SeqCst);
        set_contrast(contrast.into(), forced.map_or(JsValue::NULL, JsValue::from));
        next_task().await.expect("owner task");
        assert!(
            wakes.load(Ordering::SeqCst) > before,
            "{contrast}: a change wakes the owner"
        );
        assert_eq!(
            sample().high_contrast(),
            expected,
            "{contrast}, forced colors {forced:?}"
        );
    }
    let before = wakes.load(Ordering::SeqCst);
    set_languages(
        vec!["fr-CA".to_owned(), "en--US".to_owned(), "en-US".to_owned()],
        "fr-CA".into(),
    );
    next_task().await.expect("owner task");
    assert!(
        wakes.load(Ordering::SeqCst) > before,
        "languagechange wakes the owner"
    );
    assert_eq!(
        locales(&sample()),
        Some(vec!["fr-CA".to_owned(), "en-US".to_owned()])
    );

    owner
        .borrow()
        .as_ref()
        .expect("owner")
        .proxy()
        .request_quit()
        .expect("quit");
    next_task().await.expect("quit task");
    assert_eq!(listener_count(), 0, "quit removes every listener");
    let retired = wakes.load(Ordering::SeqCst);
    set_languages(vec!["ja-JP".to_owned()], "ja-JP".into());
    next_task().await.expect("late task");
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        retired,
        "a late languagechange is inert"
    );
}

fn counter_owner() -> (flui_native::OwnerPlatform, Arc<AtomicUsize>) {
    let retained = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&retained);
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    Box::new(WebPlatform::new().expect("successor source"))
        .run(Box::new(move |owner| {
            owner
                .on_wake(Box::new(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                }))
                .expect("successor hook");
            installed.replace(Some(owner));
            Ok(())
        }))
        .expect("successor loop");
    let owner = retained.borrow_mut().take().expect("successor owner");
    (owner, wakes)
}
