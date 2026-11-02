//! The seats' text inputs as the dispatch reaches them: what a seat's text
//! input reported reaches its window in order with the seat's going.

use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::{Arc, Mutex, Weak};

use flui_platform::WindowId;
use flui_platform::text_store::{TextStore, TextStoreHost};
use flui_testing::text_store_kit::InMemoryTextStore;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Proxy};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;

use super::{SeatTexts, WindowText};
use crate::owner::linux::wayland::{
    SeatText, SeatWindowText, TextEvent, TextRecorder, TextWire, WaylandState,
};

/// The seats' queue as the dispatch fills it.
#[derive(Default)]
struct Queued(Mutex<Vec<TextWire>>);

impl Queued {
    fn take(&self) -> Vec<TextWire> {
        std::mem::take(&mut *self.0.lock().expect("BUG: the queue is never poisoned"))
    }
}

impl TextRecorder for Queued {
    fn record_text(&self, wire: TextWire) {
        self.0
            .lock()
            .expect("BUG: the queue is never poisoned")
            .push(wire);
    }
}

#[test]
fn a_seat_gone_in_a_dispatch_keeps_what_its_input_method_sent_first() {
    // The seat goes, or the manager every text input came from.
    for manager_removed in [false, true] {
        // Objects of a connection no compositor answers: requests on them are
        // dropped, and events are the test's to record.
        let (socket, _compositor) = UnixStream::pair().expect("a socket pair");
        let connection = Connection::from_socket(socket).expect("a connection over the socket");
        let backend = connection.backend().downgrade();
        let queue = connection.new_event_queue::<WaylandState>();
        let queued = Arc::new(Queued::default());
        let recorder: Weak<dyn TextRecorder> =
            Arc::downgrade(&(Arc::clone(&queued) as Arc<dyn TextRecorder>));
        let seat = WlSeat::inert(backend.clone());
        let surface = WlSurface::inert(backend.clone());

        let texts = SeatTexts::new();
        texts.objects.with(|objects| {
            objects.manager = Some((7, ZwpTextInputManagerV3::inert(backend.clone())));
        });
        texts.seat_added(&seat, &queue.handle(), &recorder);
        let text = Arc::new(WindowText::new());
        let store = InMemoryTextStore::new("");
        text.client()
            .expect("the window's client on its thread")
            .focus_store(Some(Rc::clone(&store) as Rc<dyn TextStore>));
        texts.attach(
            WindowId(1),
            &surface,
            &(Arc::clone(&text) as Arc<dyn SeatWindowText>),
        );
        let (input, wire) = texts
            .objects
            .with(|objects| {
                let held = &objects.inputs[0];
                (held.wire.object.id(), Rc::clone(&held.wire))
            })
            .expect("the seat's text input");
        texts.deliver(TextWire {
            input: input.clone(),
            event: TextEvent::Enter(surface.clone()),
        });

        // One dispatch records the input method's commit, and then removes
        // the seat before anything recorded is delivered.
        queued.record_text(TextWire {
            input: input.clone(),
            event: TextEvent::Commit(Some("typed".into())),
        });
        queued.record_text(TextWire {
            input,
            event: TextEvent::Done(wire.commits.get()),
        });
        if manager_removed {
            texts.global_removed(7);
        } else {
            texts.seat_removed(&seat);
        }
        for recorded in queued.take() {
            texts.deliver(recorded);
        }

        assert_eq!(
            store.text(),
            "typed",
            "manager_removed={manager_removed}: the commit reached the field"
        );
        assert_eq!(
            texts.objects.with(|objects| objects.inputs.len()),
            Some(0),
            "manager_removed={manager_removed}: the text input is gone once delivered"
        );
    }
}
