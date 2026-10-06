//! The text services as a `TextStoreHost`, against the real TSF in a hidden
//! window, with no input method typing: which store a completion reaches,
//! and that one queued behind a TSF call is never lost.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_platform_api::ImeEvent;
use flui_platform_api::text_store::{
    CommitGate, CompositionEnd, InMemoryTextStore, LockOutcome, TextStore, TextStoreHost,
    TextStoreHostError, project_ime_event,
};
use windows::Win32::Foundation::HWND;

use super::TextServices;
use crate::traits::{Platform, WindowOptions};

/// "ab" with "かな" composing after it, as the store sees it.
fn composing_store() -> Rc<InMemoryTextStore> {
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(CommitGate::new());
    let applied = project_ime_event(
        &*store,
        &ImeEvent::Preedit {
            text: "かな".to_owned(),
            cursor: Some((0, 0)),
        },
    );
    assert_eq!(applied, Ok(LockOutcome::Granted), "preedit applies");
    store
}

fn erased(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    store.clone()
}

/// A completion asked for inside a TSF call is queued with its store and
/// answered `Deferred`; when TSF has shut down by the time the call
/// returns, the composition is committed in place.
fn a_queued_completion_commits_in_place_after_a_shutdown(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.shutdown();
    services.leave();
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
}

/// The same when the store's document is gone by the time the call
/// returns (it was poisoned, or reopening it failed).
fn a_queued_completion_commits_in_place_without_a_document(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.close_document();
    services.leave();
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
    services.shutdown();
}

/// A store the host does not serve, queued focus changes included, is
/// refused; so is any store once TSF has shut down.
fn a_completion_reaches_only_the_focused_store(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let (a, b) = (composing_store(), composing_store());
    services.focus_store(Some(erased(&a)));
    assert_eq!(
        services.complete_composition(&erased(&b)),
        Err(TextStoreHostError::NotFocused)
    );
    services.enter();
    services.focus_store(Some(erased(&b)));
    assert_eq!(
        services.complete_composition(&erased(&a)),
        Err(TextStoreHostError::NotFocused),
        "the queued focus change counts"
    );
    services.leave();
    assert!(b.composition().is_some() && a.composition().is_some());
    services.shutdown();
    assert_eq!(
        services.complete_composition(&erased(&b)),
        Err(TextStoreHostError::Unavailable)
    );
}

/// One row: its name, and the case run against the shared window.
type Row = (&'static str, fn(HWND));

#[test]
fn the_text_services_answer_a_completion_for_its_store() {
    let platform = super::super::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(WindowOptions {
            title: "flui text services".into(),
            size: Size::new(200.0, 80.0),
            visible: false,
            ..Default::default()
        })
        .expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<super::super::WindowsWindow>()
        .expect("Win32 window")
        .hwnd();
    let cases: &[Row] = &[
        (
            "shut down",
            a_queued_completion_commits_in_place_after_a_shutdown,
        ),
        (
            "no document",
            a_queued_completion_commits_in_place_without_a_document,
        ),
        (
            "focused store only",
            a_completion_reaches_only_the_focused_store,
        ),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if catch_unwind(AssertUnwindSafe(|| case(hwnd))).is_err() {
            failed.push(name);
        }
    }
    window.close();
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}
