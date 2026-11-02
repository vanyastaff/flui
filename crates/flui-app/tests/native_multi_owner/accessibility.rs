use super::{Release, until};
use flui_app::testing::NativeFrames;
use flui_app::{Application, RuntimeWindowConfig, open_dedicated_window};
use flui_interaction::focus::FocusNode;
use flui_painting::styling::Color;
use flui_view::prelude::*;
use flui_widgets::{
    ColoredBox, Column, EditableText, SingleChildScrollView, SizedBox, Text, TextEditingController,
    WidgetsApp,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

struct Work {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    completed: mpsc::SyncSender<Duration>,
}
struct Model {
    revision: u64,
    work: Option<Work>,
    grant: Option<Grant>,
}
struct Grant {
    window: flui_platform::WindowId,
    text: String,
    result: mpsc::SyncSender<Result<flui_native::testing::windows::NativeTextGrant, String>>,
}
struct PendingGrant {
    request: flui_native::testing::windows::NativeTextGrantRequest,
    result: mpsc::SyncSender<Result<flui_native::testing::windows::NativeTextGrant, String>>,
}
struct Client {
    initial: bool,
    owner: std::thread::ThreadId,
    model: Arc<Mutex<Model>>,
    rebuild: RebuildHandle,
}
#[derive(Clone, StatefulView)]
struct Editor {
    initial: bool,
    model: Arc<Mutex<Model>>,
    ready: mpsc::SyncSender<Client>,
}
struct EditorState {
    root: Editor,
    fields: [(TextEditingController, Rc<FocusNode>); 2],
    pending: RefCell<Option<PendingGrant>>,
}
impl StatefulView for Editor {
    type State = EditorState;
    fn create_state(&self) -> Self::State {
        EditorState {
            root: self.clone(),
            fields: std::array::from_fn(|_| (TextEditingController::new(), FocusNode::new())),
            pending: RefCell::new(None),
        }
    }
}
impl ViewState<Editor> for EditorState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.root
            .ready
            .try_send(Client {
                initial: self.root.initial,
                owner: std::thread::current().id(),
                model: Arc::clone(&self.root.model),
                rebuild: cx.rebuild_handle(),
            })
            .expect("two mounted editor clients");
    }
    fn build(&self, root: &Editor, _: &dyn BuildContext) -> impl IntoView {
        let completed = {
            let mut pending = self.pending.borrow_mut();
            let result = pending
                .as_ref()
                .and_then(|pending| pending.request.take_result());
            result.map(|result| (pending.take().expect("observed pending grant"), result))
        };
        if let Some((pending, result)) = completed {
            let _ = pending.result.send(result);
        }
        let (revision, work, grant) = {
            let mut model = root.model.lock().expect("qualification model");
            (model.revision, model.work.take(), model.grant.take())
        };
        if let Some(grant) = grant {
            use flui_native::testing::windows::request_focused_text_through_tsf;
            use flui_platform::text_store::LockTiming;
            assert!(self.pending.borrow().is_none());
            assert!(
                matches!(
                    request_focused_text_through_tsf(
                        grant.window,
                        "must not be committed",
                        LockTiming::Sync
                    ),
                    Err(
                        flui_native::testing::windows::NativeTextGrantError::SynchronousUnavailable
                    )
                ),
                "a synchronous native grant is refused during build"
            );
            let request =
                request_focused_text_through_tsf(grant.window, &grant.text, LockTiming::Async)
                    .expect("deferred native grant accepted");
            assert!(
                request.take_result().is_none(),
                "accepted request is not an inline edit during build"
            );
            *self.pending.borrow_mut() = Some(PendingGrant {
                request,
                result: grant.result,
            });
        }
        if let Some(work) = work {
            let start = Instant::now();
            let cpu = flui_native::testing::windows::current_thread_cpu_time().expect("CPU clock");
            let mut value = revision;
            let mut entered = false;
            loop {
                for _ in 0..4096 {
                    value = std::hint::black_box(
                        value
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1),
                    );
                }
                let elapsed = flui_native::testing::windows::current_thread_cpu_time()
                    .expect("CPU clock")
                    .saturating_sub(cpu);
                if !entered && elapsed >= Duration::from_millis(31) {
                    if work.entered.send(()).is_err() {
                        // The observer already failed; do not replace its
                        // diagnostic with a second fixture teardown panic.
                        break;
                    }
                    entered = true;
                }
                if entered && work.release.try_recv().is_ok() {
                    let _ = work.completed.send(elapsed);
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(12),
                    "bounded CPU load release"
                );
            }
            std::hint::black_box(value);
        }
        let mut children: Vec<BoxedView> =
            vec![Text::new(format!("Owner {} revision {revision}", root.initial)).boxed()];
        for (controller, focus) in &self.fields {
            children.push(
                SizedBox::new(420.0, 60.0)
                    .child(EditableText::new(controller.clone(), Rc::clone(focus)))
                    .boxed(),
            );
        }
        children.extend((0..128).map(|row| {
            Text::new(format!(
                "Layout revision {revision}, row {row}: native accessibility under CPU load"
            ))
            .boxed()
        }));
        WidgetsApp::new(
            ColoredBox::new(Color::WHITE)
                .child(SingleChildScrollView::new().child(Column::new(children))),
        )
    }
}
fn editor(initial: bool, ready: mpsc::SyncSender<Client>) -> Editor {
    Editor {
        initial,
        ready,
        model: Arc::new(Mutex::new(Model {
            revision: 0,
            work: None,
            grant: None,
        })),
    }
}

#[test]
#[ignore = "actual Win32 UIA actions and GPU windows with each sibling held in CPU work"]
fn native_multi_owner_accessibility_reads_and_actions() {
    editor_actions(false);
}

#[test]
#[ignore = "actual Win32 TSF grants on mounted editors with each sibling held in CPU work"]
fn native_multi_owner_mounted_editor_tsf_grants_wait_for_idle() {
    editor_actions(true);
}

fn editor_actions(tsf: bool) {
    let frames = NativeFrames::default().with_semantics(true);
    let observed = frames.clone();
    let (ready, clients) = mpsc::sync_channel(2);
    let initial = editor(true, ready.clone());
    let (done, result) = mpsc::sync_channel(1);
    let application = Application::new(move |_| initial.clone())
        .with_config(flui_app::AppConfig::new().with_title("FLUI UIA initial").with_size(520, 420))
        .on_ready(move |app| {
            let mut launched = open_dedicated_window(move || (
                editor(false, ready),
                RuntimeWindowConfig::new().with_title("FLUI UIA dedicated").with_size(520, 420),
            )).expect("qualification dedicated launch");
            let app = app.clone();
            std::thread::spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let address = until("dedicated mounted", || launched.try_result()).expect("dedicated installed");
                    let sibling = until("dedicated renderer", || observed.for_presentation(address));
                    let main = until("initial renderer", || observed.windows().into_iter().find(|frame| frame.snapshot().presentation.is_some_and(|id| id != address)));
                    let one = clients.recv_timeout(Duration::from_secs(8)).expect("first mounted editor");
                    let two = clients.recv_timeout(Duration::from_secs(8)).expect("second mounted editor");
                    let (first, second) = if one.initial { (one, two) } else { (two, one) };
                    assert!(first.initial && !second.initial);
                    assert_ne!(first.owner, second.owner);
                    assert_eq!(main.snapshot().owner, first.owner);
                    assert_eq!(sibling.snapshot().owner, second.owner);
                    for frame in [&main, &sibling] {
                        until("mounted UIA editor providers", || {
                            flui_native::testing::windows::read_edit_fields(frame.snapshot().window)
                                .ok().filter(|fields| fields.len() == 2 && fields.iter().all(|field| field.value.is_empty()))
                        });
                    }
                    for (heavy, active, held_frame, active_frame, text) in [
                        (&first, &second, &main, &sibling, "Owner dedicated 東京 🦀"),
                        (&second, &first, &sibling, &main, "Owner initial 日本 🦀"),
                    ] {
                        let before = flui_native::testing::windows::read_edit_fields(held_frame.snapshot().window).expect("held editor baseline");
                        let (entered, started) = mpsc::sync_channel(1);
                        let (release, released) = mpsc::channel();
                        let release = Release(release);
                        let (completed, cpu) = mpsc::sync_channel(1);
                        let active_presented = active_frame.snapshot().presented;
                        {
                            let mut model = heavy.model.lock().expect("qualification model");
                            model.revision += 1;
                            assert!(model.work.replace(Work {entered, release: released, completed}).is_none());
                        }
                        heavy.rebuild.schedule(RebuildReason::StateChange);
                        started.recv_timeout(Duration::from_secs(8)).expect("heavy owner has charged real CPU work");
                        let held_presented = held_frame.snapshot().presented;
                        let actions = flui_native::testing::windows::exercise_edit_field(active_frame.snapshot().window, 0, text).expect("real UIA edit/select/focus under sibling load");
                        assert_eq!(actions.value, text);
                        assert_eq!(actions.selected, "Owner");
                        assert!(actions.keyboard_focus);
                        assert_eq!(actions.other_values, vec![String::new()]);
                        assert_ne!(actions.bounds, [] as [[f64; 4]; 0]);
                        let addressed = flui_native::testing::windows::read_edit_fields(active_frame.snapshot().window).expect("independent addressed-field readback");
                        assert_eq!(addressed.len(), 2);
                        assert_eq!(addressed[0].value, text, "the original addressed field changed");
                        assert!(addressed[1].value.is_empty(), "the replacement field stayed unchanged");
                        if tsf {
                            let replacement = format!("TSF {text}");
                            let (result, grant_result) = mpsc::sync_channel(1);
                            {
                                let mut model = active.model.lock().expect("active qualification model");
                                assert!(model.grant.replace(Grant { window: active_frame.snapshot().window, text: replacement.clone(), result }).is_none());
                                model.revision += 1;
                            }
                            active.rebuild.schedule(RebuildReason::StateChange);
                            let grant = until("deferred native grant observed", || {
                                let result = grant_result.try_recv().ok();
                                if result.is_none() { active.rebuild.schedule(RebuildReason::StateChange); }
                                result
                            }).expect("native grant completed");
                            assert_eq!(grant.owner, active.owner);
                            assert_eq!(grant.before, text);
                            assert_eq!(grant.after, replacement);
                            assert!(grant.bounds[2] > grant.bounds[0] && grant.bounds[3] > grant.bounds[1], "nonempty native text geometry: {grant:?}");
                            let fields = flui_native::testing::windows::read_edit_fields(active_frame.snapshot().window).expect("TSF editor readback");
                            assert_eq!(fields[0].value, replacement, "accepted edit changes the original field");
                            assert!(fields[1].value.is_empty(), "replacement field remains unchanged");
                            assert!(fields[0].keyboard_focus, "focus remains on the addressed field");
                            println!("Mounted editor TSF grant: {grant:?}");
                        }
                        until("active owner presents its edited content", || (active_frame.snapshot().presented > active_presented).then_some(()));
                        assert_eq!(held_frame.snapshot().presented, held_presented, "heavy frame remains inside its CPU turn");
                        assert!(cpu.try_recv().is_err(), "UIA actions finished before heavy turn release");
                        drop(release);
                        assert!(cpu.recv_timeout(Duration::from_secs(8)).expect("CPU turn completed") >= Duration::from_millis(31));
                        until("released owner completes layout and paint", || (held_frame.snapshot().presented > held_presented).then_some(()));
                        let after = flui_native::testing::windows::read_edit_fields(held_frame.snapshot().window).expect("held editor unchanged");
                        assert_eq!(before.iter().map(|field| &field.value).collect::<Vec<_>>(), after.iter().map(|field| &field.value).collect::<Vec<_>>());
                        println!("UIA actions under load: held={:?}, active={:?}, observation={actions:?}", heavy.owner, active_frame.snapshot().owner);
                    }
                    vec![main, sibling]
                }));
                let _ = app.request_quit();
                done.send(outcome).expect("UIA qualification observer");
            });
        });
    frames
        .run(application)
        .expect("UIA qualification clean native retirement");
    let windows = result
        .recv_timeout(Duration::from_secs(15))
        .expect("UIA qualification result")
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
    assert!(windows.iter().all(|frame| frame.snapshot().released));
}
