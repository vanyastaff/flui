//! The [`Driver`]: one target, its connection, its session policy, and every method of the table.
//!
//! The agent's methods (`ServedBy::Agent`) go to the app as they are; the driver's own
//! (`ServedBy::Driver`) are composed here from them ([`crate::compose`]); a lane's need one
//! attached. [`Driver::call`] reaches all of them by type, [`Driver::call_value`] by wire name, so
//! every front end reaches the same set (ADR-0238 §1).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use flui_protocol::error::HandleKind;
use flui_protocol::events::Event;
use flui_protocol::input::{InputParams, InputPath, InputPolicy, PointerDevice};
use flui_protocol::method::{self, Method, MethodEffect, ServedBy};
use flui_protocol::session::{
    AppDescription, Capabilities, Closed, Empty, HelloReply, Isolation, WindowList,
};
use flui_protocol::tree::WindowId;
use flui_protocol::version::ProtocolVersion;
use flui_protocol::wait::{FramesParams, SettledParams, WaitParams};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use flui_foundation::panic::FirstPanic;

use crate::connection::{Call, Connection, Events};
use crate::error::{Error, Result};
use crate::lane::{Lane, LaneWorker};
use crate::launch::{AppProcess, Launch};
use crate::os_input::OsInputLock;
use crate::report::Report;
use crate::scenario::Scenario;
use crate::trace::{self, TraceOptions, TraceSummary, TraceWriter};
use crate::window::Window;

/// How long past the agent's own reply bound a call over a socket waits before it calls the app
/// unresponsive: the agent answers `timeout` itself after 5 s, so this fires only for a peer that
/// stopped serving.
const SOCKET_MARGIN: Duration = Duration::from_secs(10);

/// The agent's reply bound (`Limits::reply_timeout` of the app's agent).
const AGENT_REPLY: Duration = Duration::from_secs(5);

/// How long an action may take to find its element actionable, and an expectation to hold, when
/// the call names no timeout.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// One target driven through one connection: the app's methods go to its agent as they are, the
/// driver's are composed from them, and a lane's need one attached.
///
/// `Send` but not `Sync`: a front end that needs concurrency puts it behind a worker. Its methods
/// are synchronous.
pub struct Driver {
    connection: Box<dyn Connection>,
    /// The process a launch started, ended with the session.
    process: Option<AppProcess>,
    /// Where the target's own input lands: in process, or the desktop of a launched app.
    target_isolation: Isolation,
    policy: InputPolicy,
    lane: Option<LaneWorker>,
    /// The OS-input lock's file, when not this user's default.
    os_lock_path: Option<PathBuf>,
    /// Held from the session's first OS event to its end.
    os_lock: Option<OsInputLock>,
    scenario: Option<Scenario>,
    announced: bool,
    announcements: Vec<String>,
    /// How long a call waits past what the app may hold it.
    reply_budget: Duration,
    pub(crate) timeout: Duration,
    /// The trace being recorded, between `start_trace` and `finish_trace`.
    trace: Option<TraceWriter>,
    /// How deep the call being run is in commands: only the outermost is a trace's command.
    command_depth: u32,
    /// Events a trace took from the connection that are the caller's to take.
    kept_events: Events,
}

impl Driver {
    /// Start `launch`'s app and connect to its agent.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming the lane an artifact needs; the app's failure to start or to
    /// bind its endpoint in time, with its exit code when it exited first ([`Error::Gone`]); the
    /// agent refusing the hello.
    pub fn launch(launch: Launch) -> Result<Self> {
        match launch.target {
            #[cfg(feature = "headless")]
            crate::launch::Target::Headless(headless) => {
                let budget = headless.reply_timeout + Duration::from_secs(1);
                let connection = crate::connection::in_process::InProcess::start(headless)?;
                Ok(Self::new(
                    Box::new(connection),
                    None,
                    Isolation::InProcess,
                    budget,
                ))
            }
            _ => Self::launch_process(&launch),
        }
    }

    #[cfg(any(unix, windows))]
    fn launch_process(launch: &Launch) -> Result<Self> {
        use crate::connection::socket::{Endpoint, SocketConnection};

        let executable = launch.executable_path()?;
        let launched = launch.spawn(&executable)?;
        let mut process = launched.process;
        let endpoint = Endpoint::Local(launched.endpoint.clone());
        let connected = SocketConnection::connect(
            &endpoint,
            &launched.token,
            launch.connect_timeout(),
            &mut || process.is_running(),
        );
        match connected {
            Ok(connection) => Ok(Self::new(
                Box::new(connection),
                Some(process),
                Isolation::Shared,
                AGENT_REPLY + SOCKET_MARGIN,
            )),
            Err(error) => match process.exit_code() {
                Some(code) => Err(Error::process_gone(
                    format!(
                        "{} exited before its agent answered: {error}",
                        executable.display()
                    ),
                    Some(code),
                )),
                None => Err(error),
            },
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn launch_process(_launch: &Launch) -> Result<Self> {
        Err(Error::not_supported(
            "local_socket",
            "this host has no local sockets to reach an app with",
        ))
    }

    /// Connect to an app already running with its agent at `endpoint`, presenting `token`. An
    /// attached app is never ended by the session.
    ///
    /// # Errors
    ///
    /// When the endpoint cannot be reached within `wait`, or the agent refuses the hello.
    #[cfg(any(unix, windows))]
    pub fn attach(
        endpoint: &crate::connection::socket::Endpoint,
        token: &str,
        wait: Duration,
    ) -> Result<Self> {
        let connection = crate::connection::socket::SocketConnection::connect(
            endpoint,
            token,
            wait,
            &mut || true,
        )?;
        Ok(Self::new(
            Box::new(connection),
            None,
            Isolation::Shared,
            AGENT_REPLY + SOCKET_MARGIN,
        ))
    }

    fn new(
        connection: Box<dyn Connection>,
        process: Option<AppProcess>,
        target_isolation: Isolation,
        reply_budget: Duration,
    ) -> Self {
        Self {
            connection,
            process,
            target_isolation,
            policy: InputPolicy::default(),
            lane: None,
            os_lock_path: None,
            os_lock: None,
            scenario: None,
            announced: false,
            announcements: Vec::new(),
            reply_budget,
            timeout: DEFAULT_TIMEOUT,
            trace: None,
            command_depth: 0,
            kept_events: Events::default(),
        }
    }

    /// The session's input policy: synthetic unless asked otherwise (ADR-0238 §5).
    #[must_use]
    pub fn input_policy(&self) -> InputPolicy {
        self.policy
    }

    /// Take `policy` as the session's: the path an action that names none takes.
    pub fn set_input_policy(&mut self, policy: InputPolicy) {
        self.policy = policy;
    }

    /// With `policy` as the session's input policy.
    #[must_use]
    pub fn with_input_policy(mut self, policy: InputPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// How long an action finds its element actionable and an expectation holds when the call
    /// names no timeout; 5 s when not set.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Deliver OS input through `lane`, moved onto a worker thread of its own.
    ///
    /// # Errors
    ///
    /// When the lane's thread cannot be started.
    pub fn with_lane(mut self, lane: impl Lane + 'static) -> Result<Self> {
        self.lane = Some(LaneWorker::start(lane)?);
        Ok(self)
    }

    /// Take the desktop's OS-input lock at `path` instead of this user's default.
    #[must_use]
    pub fn with_os_input_lock(mut self, path: impl Into<PathBuf>) -> Self {
        self.os_lock_path = Some(path.into());
        self
    }

    /// Run as `scenario`: its window is checked against the app's main window, its devices and
    /// duration are what an OS-input run announces, and a certification run takes the OS path.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] naming the expected and the actual window when the app's main
    /// window is not the one the scenario is written for; the reading's failure.
    pub fn declare(&mut self, scenario: Scenario) -> Result<()> {
        if let Some(spec) = scenario.window_spec() {
            let mut window = self.main_window()?;
            let placement = window.placement()?;
            spec.check(&placement)?;
        }
        if scenario.is_certification() {
            self.policy = InputPolicy::require(InputPath::Os);
        }
        self.scenario = Some(scenario);
        Ok(())
    }

    /// Where OS input lands: the lane's isolation when one is attached, else the target's.
    #[must_use]
    pub fn isolation(&self) -> Isolation {
        self.lane
            .as_ref()
            .map_or(self.target_isolation, |lane| lane.capabilities().isolation)
    }

    /// What the app answered the hello with.
    #[must_use]
    pub fn hello(&self) -> &HelloReply {
        self.connection.hello()
    }

    /// The app, as it introduced itself.
    #[must_use]
    pub fn app(&self) -> Option<&AppDescription> {
        self.hello().app.as_ref()
    }

    /// The schema the app speaks on this connection.
    #[must_use]
    pub fn protocol(&self) -> ProtocolVersion {
        self.hello().protocol
    }

    /// The process id of a launched app.
    #[must_use]
    pub fn app_pid(&self) -> Option<u32> {
        self.process.as_ref().map(AppProcess::id)
    }

    /// What the target can do: the app's answer to the hello with the paths, devices and isolation
    /// the driver adds.
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        let mut capabilities = self.hello().capabilities.clone().unwrap_or_default();
        let serves = |name: &str| capabilities.methods.iter().any(|method| method == name);
        let mut paths = Vec::new();
        if serves(method::SemanticsAct::NAME) {
            paths.push(InputPath::Semantic);
        }
        if serves(method::InputInject::NAME) {
            paths.push(InputPath::Synthetic);
        }
        let mut pointers = if serves(method::InputInject::NAME) {
            vec![
                PointerDevice::Mouse,
                PointerDevice::Touch,
                PointerDevice::Pen,
            ]
        } else {
            Vec::new()
        };
        if let Some(lane) = &self.lane {
            paths.push(InputPath::Os);
            for device in &lane.capabilities().pointers {
                if !pointers.contains(device) {
                    pointers.push(*device);
                }
            }
        }
        capabilities.paths = paths;
        capabilities.pointers = pointers;
        capabilities.isolation = Some(self.isolation());
        for row in method::METHODS {
            if row.served_by == ServedBy::Driver && crate::compose::composes(row.name) {
                capabilities.methods.push(row.name.to_owned());
            }
        }
        capabilities
    }

    /// Whether the app's agent serves `name`.
    pub(crate) fn serves(&self, name: &str) -> bool {
        self.hello()
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.methods.iter().any(|method| method == name))
    }

    /// Call `M` with `params`.
    ///
    /// # Errors
    ///
    /// What the method fails with; [`Error::NotSupported`] naming the method when neither the
    /// app's agent nor this driver serves it.
    pub fn call<M: Method>(&mut self, params: M::Params) -> Result<M::Output>
    where
        M::Params: Serialize,
        M::Output: DeserializeOwned,
    {
        let params = serde_json::to_value(params).map_err(|error| {
            Error::invalid_argument(format!(
                "the parameters of `{}` did not serialize: {error}",
                M::NAME
            ))
        })?;
        let output = self.call_value(M::NAME, params)?;
        decode(M::NAME, output)
    }

    /// Call the method named `method` on the wire with `params` as JSON: what `flui drive call`
    /// and the MCP adapter reach every method through.
    ///
    /// # Errors
    ///
    /// As [`Self::call`]; [`Error::InvalidArgument`] for a name the table does not have, or
    /// parameters that do not read as the method's.
    pub fn call_value(&mut self, method: &str, params: Value) -> Result<Value> {
        let row = method::find(method)
            .ok_or_else(|| Error::invalid_argument(format!("no method is named `{method}`")))?;
        self.command(row.name, params, |driver, params| {
            driver.dispatch_value(row, params)
        })
    }

    /// Send `row`'s method to whoever serves it.
    fn dispatch_value(&mut self, row: &'static method::MethodRow, params: Value) -> Result<Value> {
        match row.served_by {
            ServedBy::Agent => {
                if !self.serves(row.name) {
                    return Err(Error::not_supported(
                        row.name,
                        format!("the app's agent does not serve `{}`", row.name),
                    ));
                }
                let holds = holds_for(row.name, &params);
                self.agent_value(row.name, row.effect, params, holds)
            }
            ServedBy::Driver => crate::compose::call(self, row.name, params),
            _ => Err(Error::not_supported(
                row.name,
                format!("`{}` needs an OS lane that serves it", row.name),
            )),
        }
    }

    /// Send `method` to the app's agent. A process that went answers with its exit code.
    pub(crate) fn agent_value(
        &mut self,
        method: &'static str,
        effect: MethodEffect,
        params: Value,
        holds: Duration,
    ) -> Result<Value> {
        let call = Call {
            method,
            effect,
            params,
            budget: self.reply_budget.saturating_add(holds),
        };
        self.connection
            .call(call)
            .map_err(|error| self.with_exit(error))
    }

    /// Call the agent's `M`, which holds the request at most `holds` past serving it.
    pub(crate) fn agent<M: Method>(
        &mut self,
        params: M::Params,
        holds: Duration,
    ) -> Result<M::Output>
    where
        M::Params: Serialize,
        M::Output: DeserializeOwned,
    {
        let params = serde_json::to_value(params).map_err(|error| {
            Error::invalid_argument(format!(
                "the parameters of `{}` did not serialize: {error}",
                M::NAME
            ))
        })?;
        let output = self.command(M::NAME, params, |driver, params| {
            if !driver.serves(M::NAME) {
                return Err(Error::not_supported(
                    M::NAME,
                    format!("the app's agent does not serve `{}`", M::NAME),
                ));
            }
            driver.agent_value(M::NAME, M::EFFECT, params, holds)
        })?;
        decode(M::NAME, output)
    }

    /// Run `run` with `params` as the command `method`: when a trace is being recorded and no
    /// command is running already, the trace gets the command as written, then its result, the
    /// events the app sent meanwhile and the evidence it left. A command run inside another (the
    /// reads and waits an `element.tap` is made of) is the outer command's work, not a row.
    ///
    /// A panic out of `run` is resumed after the depth is restored, its result never written, so
    /// the trace ends incomplete if nothing else is written, and the next command is traced.
    pub(crate) fn command<P: Serialize, O: Serialize>(
        &mut self,
        method: &'static str,
        params: P,
        run: impl FnOnce(&mut Self, P) -> Result<O>,
    ) -> Result<O> {
        if self.trace.is_none() || self.command_depth > 0 {
            return run(self, params);
        }
        let written = serde_json::to_value(&params)
            .unwrap_or_else(|error| serde_json::json!({ "unserializable": error.to_string() }));
        let now = self.clock();
        let seq = self
            .trace
            .as_mut()
            .map_or(0, |trace| trace.command(method, written, now));
        self.command_depth += 1;
        let mut first = FirstPanic::new();
        let result = first.contain(|| run(&mut *self, params));
        self.command_depth -= 1;
        let Some(result) = result else {
            std::panic::resume_unwind(
                first
                    .into_payload()
                    .expect("BUG: a command that returned nothing held its panic"),
            );
        };
        self.drain_into_trace();
        let now = self.clock();
        if let Some(trace) = self.trace.as_mut() {
            let outcome = match &result {
                Ok(output) => Ok(serde_json::to_value(output).unwrap_or(Value::Null)),
                Err(error) => Err(error),
            };
            trace.result(seq, outcome, now);
        }
        result
    }

    /// Hand the events the app sent to the trace, keeping for the caller every one that is not
    /// the trace's own recording's.
    fn drain_into_trace(&mut self) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        let (events, dropped) = self.connection.take_events();
        let now = self.connection.clock();
        let taken = trace.events(&events, dropped, now);
        for (event, taken) in events.into_iter().zip(taken) {
            if !taken {
                self.kept_events.push(event);
            }
        }
    }

    /// Keep `image` as evidence of the command being traced.
    pub(crate) fn trace_evidence(&mut self, image: &crate::pixels::Image) {
        if self.command_depth > 0
            && let Some(trace) = self.trace.as_mut()
        {
            trace.evidence_png(image);
        }
    }

    /// Begin recording a trace as `options` describe (ADR-0241): every command from here on is
    /// written with its result, and the app's input and native recordings beside them where it
    /// serves them.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when a trace is being recorded already, or the directory holds
    /// one; [`Error::Io`] when it cannot be written; the app's refusal of its recording.
    pub fn start_trace(&mut self, options: TraceOptions) -> Result<()> {
        use flui_protocol::record::{RecordLayer, RecordParams};

        if self.trace.is_some() {
            return Err(Error::invalid_argument(
                "a trace is being recorded already: finish it first",
            ));
        }
        let placement = self
            .main_window()
            .and_then(|mut window| window.placement())
            .ok();
        let mut layers = Vec::new();
        let mut recording = None;
        if options.record_layers && self.serves(method::RecordStart::NAME) {
            let native = self
                .hello()
                .capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.record.contains(&RecordLayer::Native));
            let mut params = RecordParams::input(None).keep_text(options.keep_text);
            if native {
                params.layers.push(RecordLayer::Native);
            }
            let started = self.agent::<method::RecordStart>(params.clone(), Duration::ZERO)?;
            layers.clone_from(&params.layers);
            recording = Some((started, native));
        }
        let header = self.trace_header(&options, placement, layers);
        let mut writer = TraceWriter::create(&options, &header, self.clock())?;
        if let Some((started, native)) = recording {
            writer.hold_recording(started.recording, &started.header, native)?;
        }
        self.trace = Some(writer);
        Ok(())
    }

    fn trace_header(
        &self,
        options: &TraceOptions,
        placement: Option<flui_protocol::session::Placement>,
        layers: Vec<flui_protocol::record::RecordLayer>,
    ) -> trace::Header {
        let kind = if self.target_isolation == Isolation::InProcess {
            "headless"
        } else if self.process.is_some() {
            "launched"
        } else {
            "attached"
        };
        let positions = match self.lane_capabilities().map(|lane| lane.fidelity) {
            Some(crate::lane::PositionFidelity::OrderOnly) => trace::Positions::Order,
            _ => trace::Positions::Exact,
        };
        let started_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        trace::Header {
            flui_trace: trace::FORMAT_VERSION,
            protocol: self.protocol().to_string(),
            target: trace::TraceTarget {
                kind: kind.to_owned(),
                os: std::env::consts::OS.to_owned(),
                app: self.app().map(|app| app.name.clone()),
                lane: self.lane_name().map(str::to_owned),
            },
            window: placement.map(|placement| trace::TraceWindow {
                width: placement.size.width,
                height: placement.size.height,
                scale: placement.scale,
            }),
            isolation: self.isolation(),
            fidelity: trace::Fidelity { positions },
            seed: None,
            started_unix_ms,
            privacy: if options.keep_text {
                trace::Privacy::KeepText
            } else {
                trace::Privacy::Redact
            },
            layers,
        }
    }

    /// End the trace being recorded: the app's recording is stopped and its rows written, the
    /// trace ends with its `end` row, and `report` names it, unless the trace is kept only on
    /// failure and `report` did not fail, which removes it.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when no trace is being recorded; the trace's first failed write,
    /// the trace then left without its `end`. A recording the app could not stop leaves its
    /// input file without its `end` row, which the summary's missing recording says.
    pub fn finish_trace(&mut self, report: &mut Report) -> Result<TraceSummary> {
        use flui_protocol::record::RecordStopParams;

        let Some(number) = self.trace.as_ref().map(TraceWriter::recording) else {
            return Err(Error::invalid_argument("no trace is being recorded"));
        };
        // The stop is the trace's own, not a command of the session: its rows, and the `end`
        // row the app sends before it answers, are taken from the events after it.
        let stopped = number.and_then(|number| {
            self.command_depth += 1;
            let stopped = self
                .agent::<method::RecordStop>(RecordStopParams::new(number), Duration::ZERO)
                .ok();
            self.command_depth -= 1;
            stopped
        });
        self.drain_into_trace();
        let writer = self
            .trace
            .take()
            .expect("BUG: the trace is held until it is finished");
        let dir = writer.dir().to_owned();
        let summary = writer.finish(stopped, report.verdict())?;
        if summary.dir.is_some() {
            report.set_trace(dir);
        }
        Ok(summary)
    }

    /// A process that went, with the exit code its OS reported.
    fn with_exit(&mut self, error: Error) -> Error {
        match error {
            Error::Gone {
                kind: Some(HandleKind::Process),
                handle,
                exit: None,
                effect,
                message,
                retry,
            } => {
                let exit = self.process.as_mut().and_then(AppProcess::exit_code);
                Error::Gone {
                    kind: Some(HandleKind::Process),
                    handle,
                    exit,
                    effect,
                    message,
                    retry,
                }
            }
            other => other,
        }
    }

    /// The app's windows with content, in the order they opened.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn windows(&mut self) -> Result<Vec<WindowId>> {
        self.agent::<method::WindowsList>(Empty {}, Duration::ZERO)
            .map(|WindowList { windows, .. }| windows)
    }

    /// The app's first window.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when the app has no window with content; when the app cannot answer.
    pub fn main_window(&mut self) -> Result<Window<'_>> {
        let first = self
            .windows()?
            .into_iter()
            .next()
            .ok_or_else(|| Error::not_found("the app has no window with content"))?;
        Ok(Window::new(self, first))
    }

    /// The window `id`.
    pub fn window(&mut self, id: WindowId) -> Window<'_> {
        Window::new(self, id)
    }

    /// The events the app sent since the last take, and how many were dropped unread. While a
    /// trace is recorded, the rows of its own recording are the trace's.
    pub fn take_events(&mut self) -> (Vec<Event>, u64) {
        self.drain_into_trace();
        let (mut events, dropped) = self.kept_events.take();
        let (more, more_dropped) = self.connection.take_events();
        events.extend(more);
        (events, dropped.saturating_add(more_dropped))
    }

    /// The OS-input announcements made since the last take.
    pub fn take_announcements(&mut self) -> Vec<String> {
        std::mem::take(&mut self.announcements)
    }

    /// End the session: the connection closes, a launched app exits on its driver's disconnect
    /// and is ended if it has not, and its exit code is answered. An attached app is never ended.
    #[must_use]
    pub fn close(mut self) -> Closed {
        let mut closed = Closed::default();
        let process = self.process.take();
        drop(self);
        if let Some(mut process) = process {
            closed.exit = process.exit_code();
            process.end();
        }
        closed
    }

    /// Whether the session launched its app, which therefore ends with it.
    #[must_use]
    pub fn launched_app(&self) -> bool {
        self.process.is_some() || self.target_isolation == Isolation::InProcess
    }

    /// How long the session has run on the clock its waits are counted in.
    pub(crate) fn clock(&self) -> Duration {
        self.connection.clock()
    }

    /// The OS lane the session's OS input goes through, when it has one.
    #[must_use]
    pub fn lane_name(&self) -> Option<&str> {
        self.lane.as_ref().map(LaneWorker::name)
    }

    /// What the session's OS lane can do, when it has one.
    #[must_use]
    pub fn lane_capabilities(&self) -> Option<&crate::lane::LaneCapabilities> {
        self.lane.as_ref().map(LaneWorker::capabilities)
    }

    pub(crate) fn lane(&self) -> Option<&LaneWorker> {
        self.lane.as_ref()
    }

    pub(crate) fn scenario(&self) -> Option<&Scenario> {
        self.scenario.as_ref()
    }

    /// Take the desktop's OS-input lock, once per session.
    pub(crate) fn lock_os_input(&mut self) -> Result<()> {
        if self.os_lock.is_none() {
            self.os_lock = Some(match &self.os_lock_path {
                Some(path) => OsInputLock::acquire(path)?,
                None => OsInputLock::acquire_default()?,
            });
        }
        Ok(())
    }

    /// Announce `line` before the session's first OS event: on stderr, and into the report.
    pub(crate) fn announce_once(&mut self, line: impl FnOnce() -> String) {
        if self.announced {
            return;
        }
        self.announced = true;
        let line = line();
        eprintln!("{line}");
        self.announcements.push(line);
    }
}

impl fmt::Debug for Driver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Driver")
            .field("app", &self.app())
            .field("isolation", &self.isolation())
            .field("policy", &self.policy)
            .field("lane", &self.lane)
            .finish_non_exhaustive()
    }
}

/// `output` read as `method`'s output.
pub(crate) fn decode<T: DeserializeOwned>(method: &str, output: Value) -> Result<T> {
    serde_json::from_value(output).map_err(|error| Error::Protocol {
        message: format!("the answer to `{method}` does not read as its output: {error}"),
    })
}

/// How long the app may hold a request for `method` with `params` past serving it: a wait's
/// timeout, an input sequence's span.
fn holds_for(method: &str, params: &Value) -> Duration {
    use flui_protocol::method::Addressed as _;
    let read = |value: &Value| value.clone();
    match method {
        "wait.until" => serde_json::from_value::<WaitParams>(read(params)).map(|p| p.holds_for()),
        "wait.frames" => serde_json::from_value::<FramesParams>(read(params))
            .map(|p| Duration::from_millis(p.timeout_ms.unwrap_or(WaitParams::DEFAULT_TIMEOUT_MS))),
        "wait.settled" => serde_json::from_value::<SettledParams>(read(params))
            .map(|p| Duration::from_millis(p.timeout_ms.unwrap_or(WaitParams::DEFAULT_TIMEOUT_MS))),
        "input.inject" => {
            serde_json::from_value::<InputParams>(read(params)).map(|p| p.holds_for())
        }
        _ => return Duration::ZERO,
    }
    .unwrap_or(Duration::ZERO)
}
