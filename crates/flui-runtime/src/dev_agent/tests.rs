//! The development-agent host's containment matrix: each hook method
//! panicking alone, a hook whose `Drop` panics too, a nested call, a refused
//! second attach, a new loop after the last one ended, and no semantics work
//! while no hook is attached. After every failure the realm still frames and
//! later publishes do nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_protocol::ReadQuery;

use super::*;
use crate::testing::ScriptedSink;

/// Which hook method panics.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PanicIn {
    #[default]
    Nowhere,
    Attach,
    WindowOpened,
    Detach,
}

#[derive(Default)]
struct Record {
    attaches: AtomicUsize,
    detaches: AtomicUsize,
    opened: AtomicUsize,
    drops: AtomicUsize,
    /// A window handed over, kept by a hook that does not panic.
    windows: Mutex<Vec<AgentWindow>>,
    /// The host a re-entrant hook publishes through from `window_opened`.
    reenter: Mutex<Option<DevAgentHost>>,
}

impl Record {
    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.attaches.load(Ordering::SeqCst),
            self.detaches.load(Ordering::SeqCst),
            self.opened.load(Ordering::SeqCst),
            self.drops.load(Ordering::SeqCst),
        )
    }
}

struct Hook {
    record: Arc<Record>,
    panic_in: PanicIn,
    panic_on_drop: bool,
}

impl DevAgentHook for Hook {
    fn attach(&mut self) {
        self.record.attaches.fetch_add(1, Ordering::SeqCst);
        assert!(self.panic_in != PanicIn::Attach, "hook attach fails");
    }

    fn detach(&mut self) {
        self.record.detaches.fetch_add(1, Ordering::SeqCst);
        self.record.windows.lock().clear();
        assert!(self.panic_in != PanicIn::Detach, "hook detach fails");
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.record.opened.fetch_add(1, Ordering::SeqCst);
        assert!(
            self.panic_in != PanicIn::WindowOpened,
            "hook window_opened fails"
        );
        let reenter = self.record.reenter.lock().take();
        if let Some(host) = reenter {
            host.window_opened(window.clone());
        }
        self.record.windows.lock().push(window);
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.record.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_on_drop, "hook drop fails");
    }
}

fn host(panic_in: PanicIn, panic_on_drop: bool) -> (DevAgentHost, Arc<Record>) {
    let record = Arc::new(Record::default());
    let host = DevAgentHost::new(Hook {
        record: Arc::clone(&record),
        panic_in,
        panic_on_drop,
    });
    (host, record)
}

/// A realm with a root mounted and one frame committed.
fn realm() -> (UiRealm, ScriptedSink) {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget_to_for_test(realm.presentation_id(), &flui_widgets::Text::new("root"))
        .expect("the root attaches");
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);
    (realm, sink)
}

fn frame(realm: &UiRealm, sink: &mut ScriptedSink) {
    let now = flui_scheduler::Instant::now();
    let _presented = realm.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {
        realm.render_frame(sink)
    });
}

/// Frames once more and says whether the realm collected semantics.
fn collects_semantics(realm: &UiRealm, sink: &mut ScriptedSink) -> bool {
    realm.request_redraw();
    frame(realm, sink);
    realm
        .pipeline_for_test()
        .with(|pipeline| pipeline.semantics_owner().is_some())
}

fn a_hook_panicking_in_attach_is_dropped_and_nothing_is_vended() {
    let (realm, mut sink) = realm();
    let (host, record) = host(PanicIn::Attach, false);
    assert!(
        host.attach().is_none(),
        "a panicking attach attaches nothing"
    );
    host.publish(&realm, realm.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 0, 0, 1),
        "dropped, never handed a window"
    );
    assert!(!collects_semantics(&realm, &mut sink), "no semantics work");
    assert!(
        host.attach().is_none(),
        "a dropped hook never attaches again"
    );
}

fn a_hook_panicking_in_window_opened_is_dropped_with_the_window() {
    let (realm, mut sink) = realm();
    let (host, record) = host(PanicIn::WindowOpened, false);
    let attachment = host.attach().expect("attach succeeds");
    host.publish(&realm, realm.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1), "the hook is dropped");
    host.publish(&realm, realm.presentation_id());
    drop(attachment);
    assert_eq!(
        record.counts(),
        (1, 0, 1, 1),
        "a later publish and the loop's end call nothing"
    );
    assert!(
        collects_semantics(&realm, &mut sink),
        "the vended agent is the presentation's until it closes"
    );
}

fn a_hook_panicking_in_detach_is_contained() {
    let (realm, mut sink) = realm();
    let (host, record) = host(PanicIn::Detach, false);
    let attachment = host.attach().expect("attach succeeds");
    host.publish(&realm, realm.presentation_id());
    drop(attachment);
    assert_eq!(record.counts(), (1, 1, 1, 1), "detached once, then dropped");
    assert!(
        host.attach().is_none(),
        "a dropped hook never attaches again"
    );
    frame(&realm, &mut sink);
}

fn a_hook_whose_drop_panics_too_is_contained() {
    let (realm, mut sink) = realm();
    let (host, record) = host(PanicIn::WindowOpened, true);
    let _attachment = host.attach().expect("attach succeeds");
    host.publish(&realm, realm.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1));
    host.publish(&realm, realm.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1));
    frame(&realm, &mut sink);
}

fn a_nested_call_finds_the_hook_lent_and_does_nothing() {
    let (realm, _sink) = realm();
    let (host, record) = host(PanicIn::Nowhere, false);
    let _attachment = host.attach().expect("attach succeeds");
    *record.reenter.lock() = Some(host.clone());
    host.publish(&realm, realm.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 0, 1, 0),
        "the nested hand-over reached nothing"
    );
    assert_eq!(record.windows.lock().len(), 1);
}

fn a_second_attach_while_attached_is_refused() {
    let (host, record) = host(PanicIn::Nowhere, false);
    let first = host.attach().expect("attach succeeds");
    assert!(host.attach().is_none(), "one attach per detach");
    assert_eq!(record.counts(), (1, 0, 0, 0));
    drop(first);
    assert_eq!(record.counts(), (1, 1, 0, 0));
}

fn a_new_loop_attaches_and_publishes_again() {
    let (realm, _sink) = realm();
    let (host, record) = host(PanicIn::Nowhere, false);
    drop(host.attach().expect("the first loop attaches"));
    let _second = host.attach().expect("the next loop attaches");
    host.publish(&realm, realm.presentation_id());
    assert_eq!(record.counts(), (2, 1, 1, 0));
    let window = record.windows.lock()[0].clone();
    assert_eq!(
        window.id().get(),
        realm.presentation_id().as_u64(),
        "the handle is the presentation's"
    );
    assert!(
        window.read(ReadQuery::new()).is_ok(),
        "the window reads while open"
    );
}

fn nothing_is_vended_while_no_hook_is_attached() {
    let (realm, mut sink) = realm();
    let (host, record) = host(PanicIn::Nowhere, false);
    assert!(host.vend(&realm, realm.presentation_id()).is_none());
    host.publish(&realm, realm.presentation_id());
    assert_eq!(record.counts(), (0, 0, 0, 0));
    assert!(!collects_semantics(&realm, &mut sink), "no semantics work");

    let attachment = host.attach().expect("attach succeeds");
    drop(attachment);
    host.publish(&realm, realm.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 1, 0, 0),
        "nothing after the loop ended"
    );
    assert!(!collects_semantics(&realm, &mut sink), "no semantics work");
}

#[test]
fn dev_agent_host_contains_its_hook() {
    crate::table_test::run_table(
        "dev_agent_host_contains_its_hook",
        &[
            (
                "a_hook_panicking_in_attach_is_dropped_and_nothing_is_vended",
                a_hook_panicking_in_attach_is_dropped_and_nothing_is_vended,
            ),
            (
                "a_hook_panicking_in_window_opened_is_dropped_with_the_window",
                a_hook_panicking_in_window_opened_is_dropped_with_the_window,
            ),
            (
                "a_hook_panicking_in_detach_is_contained",
                a_hook_panicking_in_detach_is_contained,
            ),
            (
                "a_hook_whose_drop_panics_too_is_contained",
                a_hook_whose_drop_panics_too_is_contained,
            ),
            (
                "a_nested_call_finds_the_hook_lent_and_does_nothing",
                a_nested_call_finds_the_hook_lent_and_does_nothing,
            ),
            (
                "a_second_attach_while_attached_is_refused",
                a_second_attach_while_attached_is_refused,
            ),
            (
                "a_new_loop_attaches_and_publishes_again",
                a_new_loop_attaches_and_publishes_again,
            ),
            (
                "nothing_is_vended_while_no_hook_is_attached",
                nothing_is_vended_while_no_hook_is_attached,
            ),
        ],
    );
}
