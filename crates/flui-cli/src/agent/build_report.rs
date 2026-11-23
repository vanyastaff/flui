//! The build reports `flui run` sends to the running application's windows (`build.report`,
//! ADR-0395): that a rebuild started, or that it failed and with which errors, so the window
//! the author is looking at says so over the code it keeps running.
//!
//! A [`BuildReporter`] connects to the app's agent as an observer, so it never takes the seat of
//! the driver `flui mcp` may hold, and calls `build.report` in every window `windows.list`
//! names. It does that on a thread of its own: a report never holds up the build it reports, and
//! an app that does not answer (stopped in a debugger, gone) costs the run nothing but the
//! report. Reports that arrive while one is being sent are folded into the latest, which is the
//! only one a window shows.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use flui_protocol::build::{BuildReportParams, BuildState};
use flui_protocol::method::{BuildReport, Method as _, WindowsList};
use flui_protocol::session::{ClientRole, WindowList};

use super::Credentials;
use super::client::{Connection, Failure};

/// How long a report waits for the app's endpoint to accept it: an app that is still starting
/// binds it within this, and one that is gone is given up on.
const CONNECT_WAIT: Duration = Duration::from_secs(2);

/// Sends build reports to the app a session launched; see the module docs.
pub(crate) struct BuildReporter {
    reports: mpsc::Sender<BuildState>,
}

impl BuildReporter {
    /// A reporter to the app `credentials` reach. Its thread ends once the reporter is dropped
    /// and the report it is sending, if any, is done.
    pub(crate) fn start(credentials: Credentials) -> std::io::Result<Self> {
        let (reports, pending) = mpsc::channel();
        thread::Builder::new()
            .name("flui-run-build-report".into())
            .spawn(move || send_reports(&credentials, &pending))?;
        Ok(Self { reports })
    }

    /// Show `state` in every window of the app. Returns at once.
    pub(crate) fn report(&self, state: BuildState) {
        // The thread ends only once every sender is gone, so this send cannot fail while `self`
        // lives unless the thread panicked, which leaves nothing to report to.
        let _ = self.reports.send(state);
    }
}

/// The reporter's thread: each report, or the latest of those that waited, to every window.
fn send_reports(credentials: &Credentials, pending: &mpsc::Receiver<BuildState>) {
    let mut connection = None;
    while let Ok(mut state) = pending.recv() {
        while let Ok(newer) = pending.try_recv() {
            state = newer;
        }
        // A connection to an app that has since exited breaks on first use: the run replaced
        // the app, so the report goes to its successor over a new connection, once.
        for _ in 0..2 {
            match show(credentials, &mut connection, &state) {
                Ok(()) => break,
                Err(why) => {
                    connection = None;
                    crate::ui::debug(format!("build report not shown: {why}"));
                }
            }
        }
    }
}

/// Show `state` in every window over `connection`, opening it first when there is none.
fn show(
    credentials: &Credentials,
    connection: &mut Option<Connection>,
    state: &BuildState,
) -> Result<(), String> {
    let connection = match connection {
        Some(connection) => connection,
        None => connection.insert(Connection::connect(
            credentials,
            ClientRole::Observer,
            CONNECT_WAIT,
        )?),
    };
    let listed = connection
        .request(WindowsList::NAME, serde_json::json!({}), Duration::ZERO)
        .map_err(|failure| describe(&failure))?;
    let windows: WindowList = serde_json::from_value(listed)
        .map_err(|error| format!("`windows.list` answered {error}"))?;
    for window in windows.windows {
        let params = serde_json::to_value(BuildReportParams::new(window, state.clone()))
            .map_err(|error| format!("the report did not serialize: {error}"))?;
        match connection.request(BuildReport::NAME, params, Duration::ZERO) {
            Ok(_) => {}
            // A window that closed since the list, or a build without the method: the other
            // windows still get the report.
            Err(Failure::Wire(error)) => {
                crate::ui::debug(format!("{window} did not show the build report: {error}"));
            }
            Err(failure) => return Err(describe(&failure)),
        }
    }
    Ok(())
}

fn describe(failure: &Failure) -> String {
    match failure {
        Failure::Wire(error) => error.to_string(),
        Failure::Transport(why) | Failure::Unsent(why) => why.clone(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};

    use flui_protocol::build::{Diagnostic, SourceLocation};
    use flui_protocol::version::PROTOCOL_VERSION;
    use serde_json::{Value, json};

    use super::*;
    use crate::agent::Endpoint;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// One conversation of a stand-in agent with two windows: the hello it was opened with,
    /// then each `build.report`'s parameters, until the client closes it.
    fn serve(stream: UnixStream) -> (Value, Vec<Value>) {
        let mut out = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream).lines();
        let hello: Value =
            serde_json::from_str(&lines.next().expect("a hello").expect("a line")).expect("json");
        writeln!(out, r#"{{"hello":{{"protocol":"{PROTOCOL_VERSION}"}}}}"#).expect("hello");
        let mut reports = Vec::new();
        for line in lines {
            let Ok(line) = line else { break };
            let request: Value = serde_json::from_str(&line).expect("json");
            let result = match request["method"].as_str() {
                Some("windows.list") => json!({ "windows": ["w1", "w2"] }),
                Some("build.report") => {
                    reports.push(request["params"].clone());
                    json!({})
                }
                other => panic!("unexpected request {other:?}"),
            };
            writeln!(out, "{}", json!({ "id": request["id"], "result": result })).expect("answer");
        }
        (hello, reports)
    }

    fn credentials(socket: &std::path::Path) -> Credentials {
        Credentials {
            endpoint: Endpoint::Local(socket.to_str().expect("a UTF-8 path").to_owned()),
            token: TOKEN.into(),
            pid: 0,
        }
    }

    #[test]
    fn a_report_reaches_every_window_as_an_observer() {
        let dir = tempfile::tempdir().expect("dir");
        let socket = dir.path().join("agent.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let failed = BuildState::Failed {
            diagnostics: vec![Diagnostic::new(
                "mismatched types".into(),
                Some(SourceLocation::new("src/lib.rs".into(), 12, 5)),
            )],
            omitted: 0,
        };
        let reporter = BuildReporter::start(credentials(&socket)).expect("reporter");
        reporter.report(failed.clone());
        // A report sent before the drop is still sent; then the thread ends and closes the
        // conversation, which ends the stand-in's.
        drop(reporter);
        let (stream, _) = listener.accept().expect("accept");
        let (hello, reports) = serve(stream);
        assert_eq!(hello["hello"]["role"], "observer", "{hello}");
        let expected: Vec<Value> = ["w1", "w2"]
            .into_iter()
            .map(|window| {
                serde_json::to_value(BuildReportParams::new(
                    window.parse().expect("a window handle"),
                    failed.clone(),
                ))
                .expect("json")
            })
            .collect();
        assert_eq!(reports, expected);
    }
}
