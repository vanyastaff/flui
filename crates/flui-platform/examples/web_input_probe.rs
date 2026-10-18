//! The page `cargo xtask device web-input` drives over WebDriver BiDi: the web
//! backend on a real canvas, its pointer registry journaled as a native
//! recording (ADR-0215 §4), and the window's other capabilities reachable
//! from the driver's scripts.
//!
//! Build for wasm32-unknown-unknown with `native-capture` (a debug build) and
//! load wasm-bindgen's `--target web` output with the sibling
//! `web_input_probe/index.html`. The page constructs a [`browser::Probe`] and
//! keeps it; the driver sends trusted input to the canvas, then reads
//! [`browser::Probe::finish`]: the recording, what the window delivered and
//! what the recording replays to, for the driver to hold against each other
//! and against the checked-in golden.

#[cfg(target_arch = "wasm32")]
#[path = "../tests/native_recording/mod.rs"]
#[expect(
    dead_code,
    reason = "the recording tests' own rows and helpers are not the probe's"
)]
mod native_recording;

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::{Cell, RefCell};
    use std::num::{NonZeroU32, NonZeroU64};
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_foundation::{DragSessionId, UiRuntimeId};
    use flui_platform::{
        DispatchEventResult, HostWindow, OwnerPlatform, Platform, WebPlatform, WindowOpen,
        WindowOptions,
    };
    use flui_platform_api::data_transfer::{
        DataTransferOffer, DataTransferSource, DropFeedback, Mime, RepresentationDescriptor,
        TransferActions, TransferFormat, TransferLimits, TransferPayload,
    };
    use flui_platform_api::drag::{DragSessionKey, OsDragRequest, ProcessNonce};
    use flui_platform_api::haptics::{HapticOutcome, HapticRequest};
    use flui_platform_api::keyboard::ImeComposition;
    use flui_platform_api::pointer::PointerEvent;
    use flui_platform_api::text_store::{InMemoryTextStore, TextStore, TextStoreHost};
    use flui_platform_api::{DragDropEvent, HapticFeedback, PlatformInput};
    use wasm_bindgen::prelude::*;

    use super::native_recording::codec::Backend;
    use super::native_recording::journal::Recorder;
    use super::native_recording::transcript;

    /// The backend under the driver: its window, the capture journaling its
    /// registry, and everything the window delivered.
    #[wasm_bindgen]
    pub struct Probe {
        window: Arc<dyn HostWindow>,
        owner: OwnerPlatform,
        host: Rc<dyn TextStoreHost>,
        field: RefCell<Option<Rc<InMemoryTextStore>>>,
        recorder: Option<Recorder>,
        delivered: Rc<RefCell<Vec<PlatformInput>>>,
        drags: Rc<DragPlay>,
    }

    /// What the probe plays of a runtime's drag and drop: a target that
    /// accepts every hover and reads the dropped text, and a source that
    /// starts a session on each press.
    #[derive(Default)]
    struct DragPlay {
        accepting: Cell<bool>,
        armed: Cell<bool>,
        offers: RefCell<Vec<DataTransferOffer>>,
        dropped: RefCell<Vec<String>>,
        ended: RefCell<Vec<String>>,
    }

    /// A session the probe starts: text, then its identity.
    fn os_drag_request(drags: &Rc<DragPlay>) -> OsDragRequest {
        let key = DragSessionKey::new(
            ProcessNonce::mint(),
            UiRuntimeId::new_gen(0, NonZeroU32::MIN),
            DragSessionId::from_wire(NonZeroU64::MIN),
        );
        let identity = key.representation();
        let ended = Rc::clone(drags);
        OsDragRequest {
            key,
            pointer: None,
            representations: vec![
                RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                },
                identity,
            ],
            allowed: TransferActions::COPY | TransferActions::MOVE,
            provider: Box::new(|index| {
                Ok(if index.0 == 0 {
                    TransferPayload::Text("dragged from FLUI".to_owned())
                } else {
                    TransferPayload::Custom {
                        mime: Mime::new(flui_platform_api::drag::IDENTITY_ESSENCE),
                        bytes: Arc::from(Vec::new()),
                    }
                })
            }),
            on_end: Box::new(move |end| ended.ended.borrow_mut().push(format!("{end:?}"))),
        }
    }

    #[wasm_bindgen]
    impl Probe {
        /// Open the backend on the page's canvas, under a capture.
        #[wasm_bindgen(constructor)]
        pub fn new() -> Result<Probe, JsValue> {
            let recorder = Recorder::start(Backend::Web);
            let platform =
                WebPlatform::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
            let opened = Rc::new(RefCell::new(None));
            let out = Rc::clone(&opened);
            Box::new(platform)
                .run(Box::new(move |owner| {
                    let WindowOpen::Ready(window) = owner.open_window(WindowOptions::default())?
                    else {
                        return Err("the web opens its window at once".into());
                    };
                    let host = owner
                        .text_store_host(&window)
                        .ok_or("the web window serves text stores")?;
                    *out.borrow_mut() = Some((owner, window, host));
                    Ok(())
                }))
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let (owner, window, host) = opened
                .take()
                .ok_or_else(|| JsValue::from_str("the window did not open"))?;
            let delivered = Rc::new(RefCell::new(Vec::new()));
            let drags = Rc::new(DragPlay::default());
            let transfer = owner.shared().data_transfer();
            let (record, play, source) = (
                Rc::clone(&delivered),
                Rc::clone(&drags),
                Arc::downgrade(&window),
            );
            window.on_input(Box::new(move |input| {
                play.answer(&input, &transfer, &source);
                record.borrow_mut().push(input);
                DispatchEventResult::resolved(false, true)
            }));
            Ok(Probe {
                window,
                owner,
                host,
                field: RefCell::new(None),
                recorder: Some(recorder),
                delivered,
                drags,
            })
        }

        /// Focus an empty text field: the window's text input serves its
        /// store to the browser's input method.
        pub fn focus_field(&self) {
            let store = InMemoryTextStore::new("");
            self.host
                .focus_store(Some(Rc::clone(&store) as Rc<dyn TextStore>));
            *self.field.borrow_mut() = Some(store);
        }

        /// Unfocus the field.
        pub fn blur_field(&self) {
            self.host.focus_store(None);
        }

        /// The field's text and composition, as JSON.
        pub fn field(&self) -> String {
            let field = self.field.borrow();
            let Some(store) = field.as_ref() else {
                return "null".to_owned();
            };
            let composition = store.composition().map(|composition| {
                [
                    composition.range.start().get(),
                    composition.range.end().get(),
                ]
            });
            serde_json::json!({ "text": store.text(), "composition": composition }).to_string()
        }

        /// The keys the window delivered, each with whether it came marked
        /// as the input method's, as JSON.
        pub fn keys(&self) -> String {
            let keys: Vec<serde_json::Value> = self
                .delivered
                .borrow()
                .iter()
                .filter_map(|input| match input {
                    PlatformInput::Keyboard(key) => Some(serde_json::json!({
                        "key": format!("{:?}", key.key),
                        "state": format!("{:?}", key.state()),
                        "composing": key.composition == ImeComposition::Active,
                    })),
                    _ => None,
                })
                .collect();
            serde_json::Value::from(keys).to_string()
        }

        /// Accept every drag that hovers the canvas, and read its text when
        /// it drops.
        pub fn accept_drops(&self) {
            self.drags.accepting.set(true);
        }

        /// Start a session on each press, as a runtime whose `Draggable`
        /// lifts on a long press does.
        pub fn arm_drag(&self) {
            self.drags.armed.set(true);
        }

        /// The text of each drop the canvas took.
        pub fn dropped(&self) -> String {
            self.drags.dropped.borrow().join(", ")
        }

        /// A process nonce this page mints, which no other page may share.
        pub fn nonce() -> String {
            format!("{:?}", ProcessNonce::mint())
        }

        /// How each started session ended.
        pub fn drag_ends(&self) -> String {
            self.drags.ended.borrow().join(", ")
        }

        /// Everything the window delivered so far, one event per line.
        pub fn delivered(&self) -> String {
            transcript(&self.delivered.borrow()).join("\n")
        }

        /// Play `kind` (a `HapticFeedback` name) on the window's actuator and
        /// report its outcome and the capabilities it claims.
        pub fn haptic(&self, kind: &str) -> String {
            let Some(haptics) = self.window.haptics() else {
                return "no actuator".to_owned();
            };
            let feedback = match kind {
                "SelectionClick" => HapticFeedback::SelectionClick,
                "HeavyImpact" => HapticFeedback::HeavyImpact,
                _ => HapticFeedback::Vibrate,
            };
            let outcome: HapticOutcome = haptics.perform_with(feedback, &HapticRequest::default());
            format!("{outcome:?} {:?}", haptics.capabilities(None))
        }

        /// End the capture and report, as JSON: each recording as JSON
        /// Lines, the pointer events the window delivered through its
        /// registry (scrolls are not: a DOM wheel event has no contact), and
        /// what each recording replays to.
        pub fn finish(&mut self) -> Result<String, JsValue> {
            let recordings = self
                .recorder
                .take()
                .ok_or_else(|| JsValue::from_str("finished already"))?
                .finish();
            let delivered: Vec<String> = self
                .delivered
                .borrow()
                .iter()
                .filter_map(PlatformInput::as_pointer)
                .filter(|event| !matches!(event, PointerEvent::Scroll(_)))
                .map(|event| format!("{event:?}"))
                .collect();
            let mut written = Vec::new();
            let mut replayed = Vec::new();
            for recording in &recordings {
                let mut jsonl = Vec::new();
                recording
                    .write(&mut jsonl)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                written.push(
                    String::from_utf8(jsonl)
                        .map_err(|error| JsValue::from_str(&error.to_string()))?,
                );
                replayed.push(
                    recording
                        .replay()
                        .iter()
                        .filter_map(PlatformInput::as_pointer)
                        .map(|event| format!("{event:?}"))
                        .collect::<Vec<_>>(),
                );
            }
            Ok(serde_json::json!({
                "recordings": written,
                "delivered": delivered,
                "replayed": replayed,
            })
            .to_string())
        }
    }

    impl DragPlay {
        /// Answer one input as a runtime's drag and drop would.
        fn answer(
            self: &Rc<Self>,
            input: &PlatformInput,
            transfer: &Arc<dyn DataTransferSource>,
            window: &std::sync::Weak<dyn HostWindow>,
        ) {
            match input {
                PlatformInput::DragDrop(DragDropEvent::Entered { offer, .. })
                    if self.accepting.get() =>
                {
                    transfer.update_drop_feedback(
                        offer.id(),
                        DropFeedback {
                            accept: Some(TransferActions::COPY),
                        },
                    );
                    self.offers.borrow_mut().push(offer.clone());
                }
                PlatformInput::DragDrop(DragDropEvent::Moved { id, .. })
                    if self.accepting.get() =>
                {
                    transfer.update_drop_feedback(
                        *id,
                        DropFeedback {
                            accept: Some(TransferActions::COPY),
                        },
                    );
                }
                PlatformInput::DragDrop(DragDropEvent::Dropped { id, .. }) => {
                    let offer = self
                        .offers
                        .borrow()
                        .iter()
                        .find(|offer| offer.id() == *id)
                        .cloned();
                    let Some(index) = offer.and_then(|offer| offer.find(&TransferFormat::Text))
                    else {
                        return;
                    };
                    let request = transfer.request(*id, index, TransferLimits::default());
                    let (play, transfer, id) = (Rc::clone(self), Arc::clone(transfer), *id);
                    js_sys::futures::spawn_local(async move {
                        let text = match request.await {
                            Ok(TransferPayload::Text(text)) => text,
                            other => format!("{other:?}"),
                        };
                        play.dropped.borrow_mut().push(text);
                        transfer.conclude_drop(id);
                    });
                }
                PlatformInput::Pointer(PointerEvent::Down(_)) if self.armed.get() => {
                    // The runtime starts a session outside the dispatch, at
                    // its next quiescent point.
                    let (play, window) = (Rc::clone(self), window.clone());
                    js_sys::futures::spawn_local(async move {
                        if let Some(source) =
                            window.upgrade().and_then(|window| window.drag_source())
                        {
                            let started = source.start(os_drag_request(&play));
                            play.ended.borrow_mut().push(format!("start {started:?}"));
                        }
                    });
                }
                _ => {}
            }
        }
    }

    impl std::fmt::Debug for Probe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Probe")
                .field("delivered", &self.delivered.borrow().len())
                .finish_non_exhaustive()
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.window.close();
            self.owner.quit();
        }
    }
}

fn main() {}
