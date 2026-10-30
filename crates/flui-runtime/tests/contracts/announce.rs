//! A mounted widget announces through its presentation (ADR-0204 §4).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_runtime::presentation::PresentationWindow;
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::prelude::*;
use flui_widgets::{Announcer, Urgency};

type Slot = Arc<parking_lot::Mutex<Option<Announcer>>>;

#[derive(Clone, StatefulView)]
struct Announcing {
    slot: Slot,
}

struct AnnouncingState(Slot);

impl StatefulView for Announcing {
    type State = AnnouncingState;

    fn create_state(&self) -> Self::State {
        AnnouncingState(Arc::clone(&self.slot))
    }
}

impl ViewState<Announcing> for AnnouncingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.lock() = ctx.announcer();
    }

    fn build(&self, _: &Announcing, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// Records what a presentation publishes to assistive technology, which is
/// attached from the start.
#[derive(Default)]
struct Bridge {
    updates: std::sync::Mutex<Vec<accesskit::TreeUpdate>>,
}

impl flui_semantics::platform::PlatformAccessibility for Bridge {
    fn publish(&self, update: accesskit::TreeUpdate) {
        self.updates.lock().expect("updates").push(update);
    }
    fn is_active(&self) -> bool {
        true
    }
    fn set_activation_listener(
        &self,
        _: flui_semantics::platform::AccessibilityActivationListener,
    ) {
    }
    fn set_action_listener(&self, _: flui_semantics::platform::AccessibilityActionListener) {}

    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        flui_semantics::AssistiveFocusSupport::Unreported
    }
}

struct Sink;

impl flui_runtime::sink::FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (40, 40)
    }
    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

/// An announcement from a mounted widget in an idle presentation requests a
/// frame, and that frame publishes the message as a live region's value
/// with nothing else redrawn; a message equal to the last one is published
/// as a change; an agent hears both through the announcement journal.
#[test]
fn an_idle_presentation_speaks_an_announcement() {
    let bridge = Arc::new(Bridge::default());
    let window = flui_native::headless_platform()
        .open_window(
            flui_platform::WindowOptions::default(),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("headless window");
    let wakes = Arc::new(AtomicUsize::new(0));
    let mut runtime = UiRuntime::new(
        PresentationWindow::new(window, Some(Arc::clone(&bridge) as Arc<_>)),
        1.0,
        flui_runtime::ui_runtime::RuntimeHostServices::new(
            {
                let wakes = Arc::clone(&wakes);
                Arc::new(move || {
                    wakes.fetch_add(1, Ordering::Relaxed);
                })
            },
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime");
    let slot = Slot::default();
    runtime
        .attach_root_widget_with_size(
            &Announcing {
                slot: Arc::clone(&slot),
            },
            40.0,
            40.0,
        )
        .expect("mount");
    let pump = |runtime: &mut UiRuntime| {
        let _ = runtime.pump(
            &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
            &mut Sink,
        );
    };
    pump(&mut runtime);
    pump(&mut runtime);
    let announcer = slot
        .lock()
        .clone()
        .expect("a presentation hands its widgets an announcer");
    let polite_value = || {
        bridge
            .updates
            .lock()
            .expect("updates")
            .last()
            .and_then(|update| {
                update.nodes.iter().find_map(|(_, node)| {
                    (node.role() == accesskit::Role::Label
                        && node.live() == Some(accesskit::Live::Polite))
                    .then(|| node.value().map(str::to_owned))
                    .flatten()
                })
            })
    };
    let published_before = bridge.updates.lock().expect("updates").len();
    let wakes_before = wakes.load(Ordering::Relaxed);

    announcer
        .announce("Message sent", Urgency::Polite)
        .expect("an open presentation accepts");
    assert!(
        wakes.load(Ordering::Relaxed) > wakes_before,
        "an idle presentation is woken for the announcement"
    );
    pump(&mut runtime);
    assert_eq!(
        bridge.updates.lock().expect("updates").len(),
        published_before + 1,
        "the frame publishes the announcement"
    );
    assert_eq!(polite_value().as_deref(), Some("Message sent"));
    assert_eq!(
        bridge
            .updates
            .lock()
            .expect("updates")
            .last()
            .expect("an update")
            .nodes
            .len(),
        1,
        "only the announcer node changed"
    );

    announcer
        .announce("Message sent", Urgency::Polite)
        .expect("an open presentation accepts");
    pump(&mut runtime);
    let repeated = polite_value().expect("the repeat is published");
    assert_ne!(repeated, "Message sent", "a repeat changes the value");
    assert_eq!(repeated.trim_end_matches('\u{200B}'), "Message sent");

    // The announcer nodes stay out of an agent's reads; the journal is where
    // it hears what was said, one entry per announcement.
    let agent = runtime
        .semantics_agent(runtime.presentation_id())
        .expect("the runtime hosts its primary presentation");
    let window = flui_protocol::WindowId::from_u64(runtime.presentation_id().as_u64())
        .expect("a presentation id is non-zero");
    let mut reply = agent
        .announcements(flui_protocol::AnnouncementsParams::new(window))
        .expect("the inbox has room");
    let _ = runtime.drain_commands();
    let journal = reply
        .try_take()
        .expect("the drain answers the request")
        .expect("the presentation is open");
    let heard: Vec<_> = journal
        .entries
        .iter()
        .map(|entry| (entry.urgency, entry.message.as_str()))
        .collect();
    assert_eq!(
        heard,
        [
            (flui_protocol::AnnouncementUrgency::Polite, "Message sent"),
            (flui_protocol::AnnouncementUrgency::Polite, "Message sent"),
        ]
    );
}
