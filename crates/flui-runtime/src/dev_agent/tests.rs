//! The development-agent host's containment matrix: each hook method
//! panicking alone, a hook whose `Drop` panics too, a nested call, a refused
//! second attach, a new loop after the last one ended, and no semantics work
//! while no hook is attached, while it does not serve, or once it has let go
//! of its windows; a deferred detach whose panic payload panics on drop, and
//! a hook whose `Drop` panics when the last host clone goes, unwinding or
//! not. After every failure the UI runtime still frames and
//! later publishes do nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_protocol::ReadQuery;

use super::*;
use crate::testing::ScriptedSink;

// The attachment stays on the owner thread, so its drop (the detach) runs
// there and never races a hand-over.
static_assertions::assert_not_impl_any!(DevAgentAttachment: Send, Sync);

/// Which hook method panics.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PanicIn {
    #[default]
    Nowhere,
    Attach,
    WindowOpened,
    Detach,
    /// `detach` panics with a payload whose own `Drop` panics.
    DetachWithPanickingPayload,
    /// `attach` returns, answering that the hook does not serve.
    Inert,
}

/// A panic payload whose `Drop` panics.
struct PanickingPayload;

impl Drop for PanickingPayload {
    fn drop(&mut self) {
        panic!("the panic payload's drop fails");
    }
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

thread_local! {
    /// The loop's attachment, which `window_opened` drops, ending the loop
    /// while the hook is lent. Owner-thread state, as the attachment is.
    static RELEASE: std::cell::RefCell<Option<DevAgentAttachment>> =
        const { std::cell::RefCell::new(None) };
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
    fn attach(&mut self) -> bool {
        self.record.attaches.fetch_add(1, Ordering::SeqCst);
        assert!(self.panic_in != PanicIn::Attach, "hook attach fails");
        self.panic_in != PanicIn::Inert
    }

    fn detach(&mut self) {
        self.record.detaches.fetch_add(1, Ordering::SeqCst);
        self.record.windows.lock().clear();
        assert!(self.panic_in != PanicIn::Detach, "hook detach fails");
        if self.panic_in == PanicIn::DetachWithPanickingPayload {
            std::panic::panic_any(PanickingPayload);
        }
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
        let release = RELEASE.with_borrow_mut(Option::take);
        drop(release);
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

/// A UI runtime with a root mounted and one frame committed.
fn ui_runtime() -> (UiRuntime, ScriptedSink) {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget_to_for_test(
            ui_runtime.presentation_id(),
            &flui_widgets::Text::new("root"),
        )
        .expect("the root attaches");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, sink)
}

fn frame(ui_runtime: &UiRuntime, sink: &mut ScriptedSink) {
    let now = flui_scheduler::Instant::now();
    let _presented =
        ui_runtime.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {}, || {
            ui_runtime.render_frame(sink)
        });
}

/// Frames once more and says whether the UI runtime collected semantics.
fn collects_semantics(ui_runtime: &UiRuntime, sink: &mut ScriptedSink) -> bool {
    ui_runtime.request_redraw();
    frame(ui_runtime, sink);
    ui_runtime
        .pipeline_for_test()
        .with(|pipeline| pipeline.semantics_owner().is_some())
}

fn a_hook_panicking_in_attach_is_dropped_and_nothing_is_vended() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::Attach, false);
    assert!(
        host.attach().is_none(),
        "a panicking attach attaches nothing"
    );
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 0, 0, 1),
        "dropped, never handed a window"
    );
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "no semantics work"
    );
    assert!(
        host.attach().is_none(),
        "a dropped hook never attaches again"
    );
}

fn a_hook_that_does_not_serve_is_not_attached_and_costs_nothing() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::Inert, false);
    assert!(host.attach().is_none(), "an inert hook attaches nothing");
    assert!(!host.is_attached());
    assert!(
        host.vend(&ui_runtime, ui_runtime.presentation_id())
            .is_none()
    );
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 0, 0, 0),
        "asked once, handed nothing, never detached, kept"
    );
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "no semantics work"
    );
    assert!(
        host.attach().is_none(),
        "the next loop asks again and is answered the same"
    );
    assert_eq!(record.counts(), (2, 0, 0, 0));
}

fn a_hook_panicking_in_window_opened_is_dropped_with_the_window() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::WindowOpened, false);
    let attachment = host.attach().expect("attach succeeds");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1), "the hook is dropped");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    drop(attachment);
    assert_eq!(
        record.counts(),
        (1, 0, 1, 1),
        "a later publish and the loop's end call nothing"
    );
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "the window went with the hook, and its semantics work with it"
    );
}

fn a_hook_panicking_in_detach_is_contained() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::Detach, false);
    let attachment = host.attach().expect("attach succeeds");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    drop(attachment);
    assert_eq!(record.counts(), (1, 1, 1, 1), "detached once, then dropped");
    assert!(
        host.attach().is_none(),
        "a dropped hook never attaches again"
    );
    frame(&ui_runtime, &mut sink);
}

fn a_hook_whose_drop_panics_too_is_contained() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::WindowOpened, true);
    let _attachment = host.attach().expect("attach succeeds");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1));
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(record.counts(), (1, 0, 1, 1));
    frame(&ui_runtime, &mut sink);
}

fn a_deferred_detach_panicking_with_a_panicking_payload_is_contained() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::DetachWithPanickingPayload, false);
    let attachment = host.attach().expect("attach succeeds");
    RELEASE.set(Some(attachment));
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 1, 1, 1),
        "the owed detach ran once when the hand-over returned, then the hook was dropped"
    );
    assert!(
        host.attach().is_none(),
        "a dropped hook never attaches again"
    );
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 1, 1, 1),
        "a later publish calls nothing"
    );
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "no semantics work"
    );
}

fn a_hook_whose_drop_panics_is_contained_when_the_last_host_goes() {
    let (first, record) = host(PanicIn::Nowhere, true);
    let clone = first.clone();
    drop(first);
    assert_eq!(
        record.counts(),
        (0, 0, 0, 0),
        "a clone still holds the hook"
    );
    drop(clone);
    assert_eq!(record.counts(), (0, 0, 0, 1), "the last clone dropped it");

    let (second, record) = host(PanicIn::Nowhere, true);
    let attachment = second.attach().expect("attach succeeds");
    drop(attachment);
    let unwound = std::panic::catch_unwind(AssertUnwindSafe(move || {
        let _host = second;
        panic!("the host's owner unwinds");
    }));
    assert!(unwound.is_err(), "the owner's own panic still unwinds");
    assert_eq!(
        record.counts(),
        (1, 1, 0, 1),
        "the hook was dropped during the unwind without aborting it"
    );
}

fn a_nested_call_finds_the_hook_lent_and_does_nothing() {
    let (ui_runtime, _sink) = ui_runtime();
    let (host, record) = host(PanicIn::Nowhere, false);
    let _attachment = host.attach().expect("attach succeeds");
    *record.reenter.lock() = Some(host.clone());
    host.publish(&ui_runtime, ui_runtime.presentation_id());
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
    let (ui_runtime, _sink) = ui_runtime();
    let (host, record) = host(PanicIn::Nowhere, false);
    drop(host.attach().expect("the first loop attaches"));
    let _second = host.attach().expect("the next loop attaches");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(record.counts(), (2, 1, 1, 0));
    let window = record.windows.lock()[0].clone();
    assert_eq!(
        window.id().get(),
        ui_runtime.presentation_id().as_u64(),
        "the handle is the presentation's"
    );
    assert!(
        window.read(ReadQuery::new()).is_ok(),
        "the window reads while open"
    );
}

fn detaching_the_hook_ends_its_windows_semantics_work() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::Nowhere, false);
    let attachment = host.attach().expect("attach succeeds");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert!(
        collects_semantics(&ui_runtime, &mut sink),
        "a handed-over window is read"
    );
    drop(attachment);
    assert_eq!(record.counts(), (1, 1, 1, 0));
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "the hook let go of its windows at detach"
    );
    let second = host.attach().expect("the next loop attaches");
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert!(
        collects_semantics(&ui_runtime, &mut sink),
        "the next loop's window is read again"
    );
    drop(second);
}

fn nothing_is_vended_while_no_hook_is_attached() {
    let (ui_runtime, mut sink) = ui_runtime();
    let (host, record) = host(PanicIn::Nowhere, false);
    assert!(
        host.vend(&ui_runtime, ui_runtime.presentation_id())
            .is_none()
    );
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(record.counts(), (0, 0, 0, 0));
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "no semantics work"
    );

    let attachment = host.attach().expect("attach succeeds");
    drop(attachment);
    host.publish(&ui_runtime, ui_runtime.presentation_id());
    assert_eq!(
        record.counts(),
        (1, 1, 0, 0),
        "nothing after the loop ended"
    );
    assert!(
        !collects_semantics(&ui_runtime, &mut sink),
        "no semantics work"
    );
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
                "a_hook_that_does_not_serve_is_not_attached_and_costs_nothing",
                a_hook_that_does_not_serve_is_not_attached_and_costs_nothing,
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
                "a_deferred_detach_panicking_with_a_panicking_payload_is_contained",
                a_deferred_detach_panicking_with_a_panicking_payload_is_contained,
            ),
            (
                "a_hook_whose_drop_panics_is_contained_when_the_last_host_goes",
                a_hook_whose_drop_panics_is_contained_when_the_last_host_goes,
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
                "detaching_the_hook_ends_its_windows_semantics_work",
                detaching_the_hook_ends_its_windows_semantics_work,
            ),
            (
                "nothing_is_vended_while_no_hook_is_attached",
                nothing_is_vended_while_no_hook_is_attached,
            ),
        ],
    );
}
