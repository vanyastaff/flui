//! Clients of a compositor the harness started, each a Wayland platform of
//! its own on the calling thread with one window: what the selection
//! harnesses (`wayland_clipboard`, `wayland_primary_selection`) drive.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_native::testing::wayland::{present, run_for};
use flui_native::{HostWindow, Platform as _, WaylandPlatform};
use flui_platform::capability::{
    Completion, CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope,
    PendingCompletions, Resolver,
};
use flui_platform::{ClipboardRead, WindowOptions};
use flui_testing::window_handler::RecordingWindowHandler;

/// How long a step may take before the case fails.
pub(crate) const STEP: Duration = Duration::from_secs(10);

/// The completions a read answers through, settled by hand on the thread
/// that made them, as an application's runtime settles them.
pub(crate) struct Answers {
    queue: Arc<Mutex<Vec<OwnerJob>>>,
    pending: PendingCompletions,
    source: CompletionSource,
}

impl Answers {
    fn new() -> Self {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&queue);
        let poster = OwnerPoster::new(move |job| {
            sink.lock().map_err(|_| OwnerRetired)?.push(job);
            Ok(())
        });
        let pending = PendingCompletions::new();
        Self {
            queue,
            source: CompletionSource::new(poster, pending.clone()),
            pending,
        }
    }

    /// Settle what the resolvers answered since the last call.
    fn settle(&self) {
        let jobs = std::mem::take(&mut *self.queue.lock().expect("the answers"));
        let scope = OwnerScope::new(&self.pending);
        for job in jobs {
            job.run(&scope);
        }
    }
}

/// A client: a platform of its own on the calling thread, with one window.
pub(crate) struct Client {
    pub(crate) platform: Rc<WaylandPlatform>,
    pub(crate) window: Rc<dyn HostWindow>,
    answers: Answers,
}

impl Client {
    /// Connect and open a window, which the compositor shows over every
    /// other and gives the keyboard.
    pub(crate) fn open(title: &str) -> Self {
        let platform =
            Rc::new(WaylandPlatform::try_new().expect("the harness's compositor answers"));
        let window = platform
            .open_window(
                WindowOptions::default().with_title(title),
                RecordingWindowHandler::new() as _,
            )
            .expect("a Wayland window opens");
        let client = Self {
            platform,
            window,
            answers: Answers::new(),
        };
        client.until("its window to have the keyboard", || {
            let state = client.window.state();
            state.visible && state.focused
        });
        client
    }

    /// One turn of the loop, drawing a frame the window asks for.
    pub(crate) fn turn(&self) {
        run_for(&self.platform, Duration::from_millis(10)).expect("the loop turns");
        self.window
            .surface_target()
            .pre_present_notify(flui_platform::SwapChainStatistics::None);
        let _ = present(self.window.as_ref());
        self.answers.settle();
    }

    /// Turn until `done` holds, bounded at [`STEP`].
    pub(crate) fn until(&self, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + STEP;
        loop {
            self.turn();
            if done() {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
        }
    }

    /// Start a read with `start`, turning the loop until it answers.
    pub(crate) fn read(&self, start: impl FnOnce(Resolver<ClipboardRead>)) -> Option<Arc<str>> {
        let (mut completion, resolver): (Completion<ClipboardRead>, _) =
            self.answers.source.pending::<ClipboardRead>();
        start(resolver);
        let deadline = Instant::now() + STEP;
        loop {
            self.turn();
            if let Some(outcome) = completion.poll() {
                return outcome
                    .expect("the read is answered, not abandoned")
                    .expect("the Wayland read succeeds");
            }
            assert!(Instant::now() < deadline, "timed out waiting for the read");
        }
    }
}
