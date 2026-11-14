//! The page's own address opens it: the web platform reports `location.href`
//! as the cold link the application launched with, and answers its
//! `location.origin` as the page's origin, against a fake `location` on top
//! of the display probe's page.
//! Runs on Node in the wasm lane; nothing here touches a real browser.

use flui_platform::{Launch, OpenTarget, Opened};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::display::{Probe, Running, install_probe, settle};
use crate::app::AppCall;

#[wasm_bindgen(inline_js = r#"
let previous;
// Give the page an address, as a browser's `location` holds it.
export function placePage(href, origin) {
    previous = Object.getOwnPropertyDescriptor(globalThis, 'location');
    Object.defineProperty(globalThis, 'location', {
        value: { href, origin }, configurable: true, writable: true,
    });
}
export function unplacePage() {
    if (previous) Object.defineProperty(globalThis, 'location', previous);
    else delete globalThis.location;
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = placePage)]
    fn place_page(href: &str, origin: &str);
    #[wasm_bindgen(js_name = unplacePage)]
    fn unplace_page();
}

/// Takes the fake address away when the test ends, whatever it asserted.
struct Placed;
impl Drop for Placed {
    fn drop(&mut self) {
        unplace_page();
    }
}

/// A page at `https://notes.example.com:8443/note/3?from=mail#top` launches
/// with that address as its cold link for the application as a whole,
/// heard once after `ready`, and answers its origin, scheme, host and port.
#[wasm_bindgen_test]
async fn web_the_page_address_is_the_link_it_launched_with() {
    install_probe("none");
    let _probe = Probe;
    let href = "https://notes.example.com:8443/note/3?from=mail#top";
    place_page(href, "https://notes.example.com:8443");
    let _placed = Placed;

    let running = Running::start();
    settle().await;

    assert_eq!(
        running.log.opens(),
        [Opened::new(
            [href.to_owned()],
            [],
            OpenTarget::Application,
            Launch::Cold
        )],
        "the page's address is reported once, as a cold link"
    );
    let calls = running.log.calls();
    let ready = calls.iter().position(|call| *call == AppCall::Ready);
    let opened = calls
        .iter()
        .position(|call| matches!(call, AppCall::Opened { .. }));
    assert!(
        matches!((ready, opened), (Some(ready), Some(opened)) if ready < opened),
        "the link waits for ready, which may take it: {calls:?}"
    );
    assert_eq!(
        running
            .owner
            .borrow()
            .as_ref()
            .expect("owner")
            .page_origin()
            .as_deref(),
        Some("https://notes.example.com:8443")
    );
    running.quit();
    settle().await;
}
