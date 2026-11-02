//! Interactive native acceptance fixture. No production support is enabled.
//! Run on Windows with `--features testing --example native_multi_owner_acceptance`.

#[cfg(all(windows, feature = "testing"))]
mod native {
    use flui_app::{AppConfig, Application, RuntimeWindowConfig, open_dedicated_window};
    use flui_foundation::geometry::Point;
    use flui_foundation::{Listenable, ListenerId};
    use flui_interaction::focus::FocusNode;
    use flui_painting::{styling::Color, typography::TextStyle};
    use flui_view::prelude::*;
    use flui_widgets::{
        ColoredBox, Column, EditableText, Padding, RawButton, SingleChildScrollView, SizedBox,
        Text, TextEditingController, WidgetsApp,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    struct Field {
        label: &'static str,
        controller: TextEditingController,
        focus: Rc<FocusNode>,
        subscription: Cell<Option<ListenerId>>,
    }
    impl Field {
        fn new(label: &'static str) -> Rc<Self> {
            let field = Rc::new(Self {
                label,
                controller: TextEditingController::new(),
                focus: FocusNode::with_debug_label(label),
                subscription: Cell::new(None),
            });
            let weak = Rc::downgrade(&field);
            let id = field.controller.add_listener(Rc::new(move || {
                if let Some(field) = weak.upgrade() {
                    println!("edit field={:?} owner={:?} text={:?} committed={:?} selection={:?} composing={:?}",
                        field.label, std::thread::current().id(), field.controller.text(),
                        field.controller.committed_text(), field.controller.selection(), field.controller.composing_range());
                }
            }));
            field.subscription.set(Some(id));
            field
        }
        fn view(&self) -> impl View + Clone {
            Column::new((
                Text::new(self.label),
                SizedBox::new(440.0, 58.0).child(
                    ColoredBox::new(Color::WHITE).child(
                        EditableText::new(self.controller.clone(), Rc::clone(&self.focus))
                            .text_style(TextStyle::default().with_font_size(24.0)),
                    ),
                ),
            ))
        }
    }
    impl Drop for Field {
        fn drop(&mut self) {
            if let Some(id) = self.subscription.take() {
                self.controller.remove_listener(id);
            }
            println!(
                "field_retired field={:?} owner={:?}",
                self.label,
                std::thread::current().id()
            );
        }
    }
    #[derive(Clone, StatefulView)]
    struct Editor {
        fields: [Rc<Field>; 2],
        ime: Rc<RefCell<Option<flui_native::testing::windows::JapaneseIme>>>,
        loaded: bool,
    }
    impl Editor {
        fn new(labels: [&'static str; 2]) -> Self {
            Self {
                fields: labels.map(Field::new),
                ime: Rc::new(RefCell::new(None)),
                loaded: std::env::args().any(|argument| argument == "--loaded"),
            }
        }
    }
    struct EditorState {
        root: Editor,
        revision: Arc<AtomicU64>,
        consumed: Cell<u64>,
        stop: Option<mpsc::SyncSender<()>>,
        worker: Option<std::thread::JoinHandle<()>>,
        post: Option<flui_view::PostFrameHandle>,
    }
    impl StatefulView for Editor {
        type State = EditorState;
        fn create_state(&self) -> EditorState {
            EditorState {
                root: self.clone(),
                revision: Arc::new(AtomicU64::new(0)),
                consumed: Cell::new(0),
                stop: None,
                worker: None,
                post: None,
            }
        }
    }
    impl Drop for EditorState {
        fn drop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(worker) = self.worker.take()
                && worker.join().is_err()
            {
                eprintln!("qualification load driver failed");
            }
        }
    }
    impl ViewState<Editor> for EditorState {
        fn init_state(&mut self, cx: &dyn LifecycleContext) {
            self.post = cx.post_frame_handle();
            if self.root.loaded {
                let rebuild = cx.rebuild_handle();
                let revision = Arc::clone(&self.revision);
                let (stop, stopped) = mpsc::sync_channel(1);
                self.stop = Some(stop);
                self.worker = Some(std::thread::spawn(move || {
                    while matches!(
                        stopped.recv_timeout(Duration::from_millis(500)),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        revision
                            .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                                value.checked_add(1)
                            })
                            .expect("BUG: qualification revision exhausted");
                        rebuild.schedule(RebuildReason::StateChange);
                    }
                }));
            }
        }
        fn build(&self, root: &Editor, _: &dyn BuildContext) -> impl IntoView {
            let revision = self.revision.load(Ordering::Acquire);
            if root.loaded && self.consumed.replace(revision) != revision {
                let started = Instant::now();
                let cpu = flui_native::testing::windows::current_thread_cpu_time()
                    .expect("qualification CPU clock");
                let mut value = revision;
                let charged = loop {
                    for _ in 0..4096 {
                        value = std::hint::black_box(
                            value
                                .wrapping_mul(6_364_136_223_846_793_005)
                                .wrapping_add(1),
                        );
                    }
                    let elapsed = flui_native::testing::windows::current_thread_cpu_time()
                        .expect("qualification CPU clock")
                        .saturating_sub(cpu);
                    if elapsed >= Duration::from_millis(200) {
                        break elapsed;
                    }
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "qualification CPU turn watchdog"
                    );
                };
                let label = root.fields[0].label;
                println!(
                    "native_load field={label:?} owner={:?} revision={revision} cpu_ns={} wall_ns={}",
                    std::thread::current().id(),
                    charged.as_nanos(),
                    started.elapsed().as_nanos()
                );
                self.post
                    .as_ref()
                    .expect("mounted post-frame capability")
                    .schedule(move |_| {
                        println!(
                            "native_load_pipeline field={label:?} owner={:?} revision={revision}",
                            std::thread::current().id()
                        );
                    })
                    .expect("qualification post-frame accepted");
            }
            let field = Rc::clone(&root.fields[0]);
            let held = Rc::clone(&root.ime);
            let enable =
                RawButton::new(Text::new("Enable Japanese IME in field A")).on_press(move |_| {
                    println!("ime_field_focus outcome={:?}", field.focus.request_focus());
                    let previous = held.borrow_mut().take();
                    drop(previous);
                    let ime = flui_native::testing::windows::JapaneseIme::try_new()
                        .expect("installed Japanese IME activates on the focused owner");
                    ime.open_keyboard().expect("Japanese IME keyboard opens");
                    println!(
                        "ime_profile_and_keyboard_verified owner={:?} langid=0x0411",
                        std::thread::current().id()
                    );
                    *held.borrow_mut() = Some(ime);
                });
            let rows: Vec<BoxedView> = (0..if root.loaded { 128 } else { 0 })
                .map(|row| {
                    Text::new(format!(
                        "Load revision {revision}, row {row}: text layout and paint"
                    ))
                    .boxed()
                })
                .collect();
            WidgetsApp::new(ColoredBox::new(Color::rgb(224, 234, 244)).child(
                SingleChildScrollView::new().child(Padding::all(24.0).child(Column::new((
                    Text::new("Native multi-owner acceptance"),
                    Text::new("IME composition, selection, focus and clipboard"),
                    enable,
                    root.fields[0].view(),
                    SizedBox::new(1.0, 24.0),
                    root.fields[1].view(),
                    Text::new(format!(
                        "Native load: {} / revision {revision}",
                        root.loaded
                    )),
                    Column::new(rows),
                )))),
            ))
        }
    }
    pub fn run() -> anyhow::Result<()> {
        let application =
            Application::new(move |_| Editor::new(["Initial field A", "Initial field B"]))
                .with_config(
                    AppConfig::new()
                        .with_title("FLUI acceptance — initial owner")
                        .with_size(520, 400)
                        .with_position(Point::new(80.0, 100.0)),
                )
                .on_ready(|_| {
                    let request = open_dedicated_window(|| {
                        (
                            Editor::new(["Dedicated field A", "Dedicated field B"]),
                            RuntimeWindowConfig::new()
                                .with_title("FLUI acceptance — dedicated owner")
                                .with_size(520, 400)
                                .with_position(Point::new(680.0, 100.0)),
                        )
                    })
                    .expect("BUG: explicit qualification admits dedicated placement");
                    std::thread::spawn(move || {
                        let mut request = request;
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(20);
                        while std::time::Instant::now() < deadline {
                            if let Some(outcome) = request.try_result() {
                                println!("dedicated_install result={outcome:?}");
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        eprintln!("dedicated installation did not settle");
                    });
                });
        let frames = flui_app::testing::NativeFrames::default();
        let observations = frames.clone();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = std::sync::Arc::clone(&done);
        let observer = std::thread::spawn(move || {
            let mut previous = std::collections::HashMap::new();
            while !finished.load(std::sync::atomic::Ordering::Acquire) {
                for window in observations.windows() {
                    let snapshot = window.snapshot();
                    let fields = flui_native::testing::windows::read_edit_fields(snapshot.window);
                    if previous.get(&snapshot.window) != Some(&fields) {
                        println!(
                            "uia_fields owner={:?} window={:?} fields={fields:?}",
                            snapshot.owner, snapshot.window
                        );
                        previous.insert(snapshot.window, fields);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
        let result = frames.run(application);
        done.store(true, std::sync::atomic::Ordering::Release);
        observer.join().expect("UIA observer exits");
        result?;
        Ok(())
    }
}

#[cfg(all(windows, feature = "testing"))]
fn main() -> anyhow::Result<()> {
    native::run()
}

#[cfg(not(all(windows, feature = "testing")))]
fn main() {
    eprintln!(
        "requires Windows and the testing feature; production dedicated support remains unavailable"
    );
}
