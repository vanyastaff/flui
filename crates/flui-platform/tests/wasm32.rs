//! Browser adapter contracts against a controlled JS native-API boundary.
//! Runs on Node in the existing wasm lane; it does not change OS preferences.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_platform::{Platform, WebPlatform};
use flui_platform_api::MotionPreference;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(inline_js = r#"
let restore;
let reduced = false;
let queries = [];
let registrationFails = false;
let removalFails = false;
let motionSupported = true;
export function installMotionProbe() {
    const previous = Object.getPrototypeOf(globalThis);
    const names = ['Window', 'matchMedia', 'requestAnimationFrame'];
    const descriptors = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name));
    class ProbeWindow {}
    globalThis.Window = ProbeWindow;
    Object.setPrototypeOf(globalThis, ProbeWindow.prototype);
    reduced = false;
    queries = [];
    registrationFails = false;
    removalFails = false;
    motionSupported = true;
    globalThis.matchMedia = query => {
        if (query !== '(prefers-reduced-motion: reduce)' && query !== '(prefers-reduced-motion: no-preference)')
            throw new Error('unexpected media query');
        const target = new EventTarget();
        const listeners = new Set();
        const add = target.addEventListener.bind(target);
        const remove = target.removeEventListener.bind(target);
        target.addEventListener = (name, callback) => {
            if (registrationFails) throw new Error('registration refused');
            listeners.add(callback); add(name, callback);
        };
        target.removeEventListener = (name, callback) => {
            if (removalFails) throw new Error('removal refused');
            listeners.delete(callback); remove(name, callback);
        };
        Object.defineProperty(target, 'matches', {
            get: () => motionSupported && (query === '(prefers-reduced-motion: reduce)' ? reduced : !reduced)
        });
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
export function setReducedMotion(value) {
    reduced = value;
    for (const query of queries) query.dispatchEvent(new Event('change'));
}
export function motionListenerCount() { return queries.reduce((sum, query) => sum + query.listenerCount(), 0); }
export function restoreMotionProbe() { restore(); }
export function refuseMotionRegistration(value) { registrationFails = value; }
export function refuseMotionRemoval(value) { removalFails = value; }
export function supportMotion(value) { motionSupported = value; }
export function nextOwnerTask() { return new Promise(resolve => setImmediate(resolve)); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = installMotionProbe)]
    fn install_probe();
    #[wasm_bindgen(js_name = setReducedMotion)]
    fn set_reduced(value: bool);
    #[wasm_bindgen(js_name = motionListenerCount)]
    fn listener_count() -> usize;
    #[wasm_bindgen(js_name = restoreMotionProbe)]
    fn restore_probe();
    #[wasm_bindgen(js_name = nextOwnerTask)]
    fn next_task() -> js_sys::Promise;
    #[wasm_bindgen(js_name = refuseMotionRegistration)]
    fn refuse_registration(value: bool);
    #[wasm_bindgen(js_name = refuseMotionRemoval)]
    fn refuse_removal(value: bool);
    #[wasm_bindgen(js_name = supportMotion)]
    fn support_motion(value: bool);
}

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
    assert_eq!(listener_count(), 1);
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
        1,
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
        1,
        "only the inert refused removal remains"
    );
}

fn counter_owner() -> (flui_platform::OwnerPlatform, Arc<AtomicUsize>) {
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
