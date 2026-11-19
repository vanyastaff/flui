//! The headless target: the app's view in a `flui-testing` headless host, its agent served in
//! process, the conversation the same lines a socket carries (ADR-0238 §8).
//!
//! The host lives on a thread of its own, the app's "process": the driver stays `Send` while the
//! host, which is not, never leaves its thread. Time is virtual. The host runs no frame by
//! itself: while a call waits for its answer, the driver asks for one frame at a time, each
//! advancing the host's clock by [`FRAME`], and polls the conversation after it at the matching
//! virtual instant. A call's budget is counted in that time, so a wait that the app answers after
//! twenty frames answers after twenty frames on every run, and a call nobody answers fails after
//! the same number of frames on every host.
//!
//! The headless app behaves as a native one does at its edges:
//!
//! - a failure a frame contained (a handler that panicked) is contained here too: the headless
//!   host raises it out of its pump, as a test host does, and the app drops it and serves on, as a
//!   runner logs it and goes on (`docs/PANIC-POLICY.md`);
//! - when its window closes, the app exits with the frame that closed it, as a runner whose last
//!   window closed does: what its agent had not written goes with it, and the call waiting, and
//!   every later one, fails as the app's process gone;
//! - a panic outside a frame (in the root's construction, in the agent's service) ends the app
//!   the same way, naming the panic.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use flui_protocol::events::Event;
use flui_protocol::session::{AppDescription, HelloReply};
use flui_testing::{HeadlessAgentService, HeadlessConversation, HeadlessHost, HeadlessWindow};
use serde_json::Value;
use web_time::Instant;

use super::{Call, Connection, Events, Heard};
use crate::error::{Error, Result};
use crate::launch::HeadlessLaunch;

/// How far the host's clock moves per frame the driver asks for: 60 Hz, as a headless test pumps.
pub(crate) const FRAME: Duration = Duration::from_millis(16);

/// Frames run after the root is attached and before the first call: one builds and lays out the
/// view, the next collects the semantics its agent asked for.
const STARTUP_FRAMES: u32 = 2;

/// What the driver asks of the app's thread.
enum ToApp {
    /// A line the driver wrote.
    Line(Vec<u8>),
    /// Run `frames` frames (none: drain what the owner holds at the same instant), poll the
    /// conversation, and answer [`FromApp::Ran`].
    Run { frames: u32 },
}

/// What the app's thread sends back.
enum FromApp {
    /// A line the agent wrote, without its newline.
    Line(Vec<u8>),
    /// The frames asked for ran, and every line they produced was sent before this.
    Ran,
    /// The app ended: why.
    Ended(String),
}

/// The conversation with a headless app.
pub(crate) struct InProcess {
    to_app: Option<Sender<ToApp>>,
    from_app: Receiver<FromApp>,
    thread: Option<JoinHandle<()>>,
    hello: HelloReply,
    next_id: u64,
    events: Events,
    /// Why the app ended, once it did.
    ended: Option<String>,
}

impl InProcess {
    /// Start `launch`'s app on a thread of its own and greet its agent.
    pub(crate) fn start(launch: HeadlessLaunch) -> Result<Self> {
        let token = crate::launch::launch_token()?;
        let (to_app, inbox) = mpsc::channel();
        let (outbox, from_app) = mpsc::channel();
        let app_token = token.clone();
        let thread = thread::Builder::new()
            .name("flui-driver-headless-app".into())
            .spawn(move || run_app(launch, &app_token, &inbox, &outbox))?;
        let mut connection = Self {
            to_app: Some(to_app),
            from_app,
            thread: Some(thread),
            hello: HelloReply::new(flui_protocol::version::PROTOCOL_VERSION),
            next_id: 1,
            events: Events::default(),
            ended: None,
        };
        connection.send(ToApp::Line(super::hello_line(&token)?))?;
        let mut hello = None;
        let mut refused = None;
        connection.run(0, None, |heard| match heard {
            Heard::Hello(reply) => hello = Some(reply),
            Heard::Answer(Err(error)) => refused = Some(error),
            Heard::Answer(Ok(_)) | Heard::Event(_) | Heard::Other => {}
        })?;
        if let Some(error) = refused.or_else(|| connection.gone()) {
            return Err(error);
        }
        connection.hello = *hello.ok_or_else(|| Error::Protocol {
            message: "the headless app's agent did not answer the hello".into(),
        })?;
        Ok(connection)
    }

    fn send(&mut self, message: ToApp) -> Result<()> {
        if let Some(why) = &self.ended {
            return Err(Error::process_gone(why.clone(), None));
        }
        let sent = self
            .to_app
            .as_ref()
            .is_some_and(|to_app| to_app.send(message).is_ok());
        if sent {
            Ok(())
        } else {
            // The thread is gone: why it went is waiting on the channel, after the lines it sent.
            let why = self
                .from_app
                .try_iter()
                .find_map(|message| match message {
                    FromApp::Ended(why) => Some(why),
                    FromApp::Line(_) | FromApp::Ran => None,
                })
                .unwrap_or_else(|| "the headless app's thread ended".to_owned());
            Err(self.end(why))
        }
    }

    /// Ask for `frames` frames and hand `heard` every line until they ran or the app ended,
    /// reading answers for request `waiting`. Once the app ended, [`Self::ended`] says why: the
    /// lines it sent before are heard first.
    fn run(
        &mut self,
        frames: u32,
        waiting: Option<u64>,
        mut heard: impl FnMut(Heard),
    ) -> Result<()> {
        self.send(ToApp::Run { frames })?;
        loop {
            match self.from_app.recv() {
                Ok(FromApp::Line(line)) => match super::hear(&line, waiting)? {
                    Heard::Event(event) => self.events.push(event),
                    other => heard(other),
                },
                Ok(FromApp::Ran) => return Ok(()),
                Ok(FromApp::Ended(why)) => {
                    self.end(why);
                    return Ok(());
                }
                Err(_) => {
                    self.end("the headless app's thread ended".to_owned());
                    return Ok(());
                }
            }
        }
    }

    /// The app ended for `why`: this call and every later one fail as its process gone.
    fn end(&mut self, why: String) -> Error {
        let why = self.ended.get_or_insert(why).clone();
        Error::process_gone(why, None)
    }

    /// The error of an app that ended, when it has.
    fn gone(&self) -> Option<Error> {
        self.ended
            .as_ref()
            .map(|why| Error::process_gone(why.clone(), None))
    }
}

impl Connection for InProcess {
    fn hello(&self) -> &HelloReply {
        &self.hello
    }

    fn call(&mut self, call: Call) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(ToApp::Line(super::request_line(id, &call)?))?;
        let mut waited = Duration::ZERO;
        let mut frames = 0;
        loop {
            let mut answer = None;
            let mut broken = None;
            self.run(frames, Some(id), |heard| match heard {
                Heard::Answer(outcome) => answer = Some(outcome),
                Heard::Hello(_) => {
                    broken = Some(Error::Protocol {
                        message: "the app said hello again".into(),
                    });
                }
                Heard::Event(_) | Heard::Other => {}
            })?;
            if let Some(error) = broken {
                return Err(error);
            }
            // Read by the id it carries: a refusal that answers no request is this one's.
            if let Some(answer) = answer {
                return answer;
            }
            if let Some(gone) = self.gone() {
                return Err(gone);
            }
            if waited >= call.budget {
                return Err(super::unanswered(&call));
            }
            waited += FRAME;
            frames = 1;
        }
    }

    fn take_events(&mut self) -> (Vec<Event>, u64) {
        self.events.take()
    }
}

impl Drop for InProcess {
    fn drop(&mut self) {
        // Closing the channel is the driver leaving: the app drops the conversation, which lets go
        // of the input it injected, runs the frame that delivers the release, and ends.
        drop(self.to_app.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The app's thread: host `launch`'s view, serve its agent, and run what the driver asks until
/// the driver leaves or the app ends.
fn run_app(launch: HeadlessLaunch, token: &str, inbox: &Receiver<ToApp>, outbox: &Sender<FromApp>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a panic out of the headless host is the app crashing, reported to the \
                  driver as its process gone"
    )]
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve(launch, token, inbox, outbox)
    }));
    let ended = match ran {
        Ok(Ok(())) => return,
        Ok(Err(why)) => why,
        Err(payload) => {
            let why = format!(
                "the headless app panicked: {}",
                flui_foundation::panic::payload_text(payload.as_ref())
                    .unwrap_or("a payload that is not text")
            );
            flui_foundation::panic::discard_payload(payload);
            why
        }
    };
    let _ = outbox.send(FromApp::Ended(ended));
}

fn serve(
    launch: HeadlessLaunch,
    token: &str,
    inbox: &Receiver<ToApp>,
    outbox: &Sender<FromApp>,
) -> std::result::Result<(), String> {
    let HeadlessLaunch {
        name,
        width,
        height,
        reply_timeout,
        attach,
    } = launch;
    let service =
        HeadlessAgentService::new(token, AppDescription::new(name, "headless"), reply_timeout);
    let mut agent = service.dev_agent();
    let mut host = HeadlessHost::new(HeadlessWindow::new(width, height)).with_dev_agent(&mut agent);
    attach(&host)?;
    for _ in 0..STARTUP_FRAMES {
        frame(&mut host, FRAME);
    }
    let start = Instant::now();
    let mut elapsed = Duration::ZERO;
    let mut conversation = service
        .connect(start + elapsed)
        .map_err(|refused| refused.to_string())?;
    while let Ok(message) = inbox.recv() {
        let ran = matches!(message, ToApp::Run { .. });
        match message {
            ToApp::Line(line) => {
                conversation.receive(&line, start + elapsed);
                conversation.poll(start + elapsed);
            }
            ToApp::Run { frames } => {
                if frames == 0 {
                    frame(&mut host, Duration::ZERO);
                }
                for _ in 0..frames {
                    elapsed += FRAME;
                    frame(&mut host, FRAME);
                    if host.is_closed(host.primary_window()) {
                        break;
                    }
                    // Each frame's answers are taken at that frame's instant.
                    conversation.poll(start + elapsed);
                }
                if host.is_closed(host.primary_window()) {
                    // The process exits with the frame that closed its window: what the agent had
                    // not written yet goes with it, as with a process that ends.
                    return Err("the app's window closed, and the app exited".to_owned());
                }
                conversation.poll(start + elapsed);
            }
        }
        if !flush(&mut conversation, outbox) {
            break;
        }
        if conversation.is_closed() {
            return Err("the headless app's agent closed the conversation".to_owned());
        }
        if ran && outbox.send(FromApp::Ran).is_err() {
            break;
        }
    }
    // The driver left: the conversation's input leases go with it, and the frame after delivers
    // their release before the app ends.
    conversation.peer_closed();
    conversation.poll(start + elapsed);
    drop(conversation);
    frame(&mut host, FRAME);
    Ok(())
}

/// Run one frame `dt` after the last. A failure the frame contained, which the headless host
/// raises out of its pump, is dropped, as a runner logs it and goes on: the panic hook has
/// printed it, and the next frame runs.
fn frame(host: &mut HeadlessHost, dt: Duration) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a frame failure the runtime contained is raised by the headless host \
                  for tests; the headless app drops it and serves on, as a runner does"
    )]
    let pumped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = host.pump(dt);
    }));
    if let Err(payload) = pumped {
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Send every line the conversation has; whether the driver is still there to take them.
fn flush(conversation: &mut HeadlessConversation, outbox: &Sender<FromApp>) -> bool {
    while let Some(mut line) = conversation.transmit() {
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if outbox.send(FromApp::Line(line)).is_err() {
            return false;
        }
    }
    true
}
