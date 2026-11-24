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
const CONNECT_WAIT: Duration = Duration::from_secs(5);

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
    while let Ok(state) = pending.recv() {
        let state = latest(state, pending);
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

/// `state`, or the newest of the reports that arrived after it: a window shows only the latest.
fn latest(state: BuildState, pending: &mpsc::Receiver<BuildState>) -> BuildState {
    pending.try_iter().last().unwrap_or(state)
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

#[cfg(test)]
mod tests {
    use flui_protocol::build::Diagnostics;

    use super::*;

    /// The reports that waited while one was sent fold into the newest: the window shows only
    /// that one.
    #[test]
    fn the_reports_that_waited_fold_into_the_newest() {
        let (reports, pending) = mpsc::channel();
        let failed = BuildState::Failed(Diagnostics::new());
        reports.send(failed).expect("send");
        reports.send(BuildState::Succeeded).expect("send");
        assert_eq!(
            latest(BuildState::Building, &pending),
            BuildState::Succeeded
        );
        assert_eq!(pending.try_recv(), Err(mpsc::TryRecvError::Empty));
        assert_eq!(
            latest(BuildState::Building, &pending),
            BuildState::Building,
            "nothing waited"
        );
    }
}

#[cfg(all(test, unix))]
mod conversation_tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};

    use flui_protocol::build::{Diagnostic, Diagnostics, SourceLocation};
    use flui_protocol::version::PROTOCOL_VERSION;
    use serde_json::{Value, json};

    use super::*;
    use crate::agent::Endpoint;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// The hello a stand-in agent read and answered.
    fn greet(stream: &UnixStream) -> (Value, impl Iterator<Item = std::io::Result<String>>) {
        // A client that goes quiet fails the test instead of hanging it.
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        let mut out = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream.try_clone().expect("clone")).lines();
        let hello: Value =
            serde_json::from_str(&lines.next().expect("a hello").expect("a line")).expect("json");
        writeln!(out, r#"{{"hello":{{"protocol":"{PROTOCOL_VERSION}"}}}}"#).expect("hello");
        (hello, lines)
    }

    /// One conversation of a stand-in agent with the windows `w1` and `w2`: the hello it was
    /// opened with, then each `build.report`'s parameters, until `expected` reports were asked or
    /// the client went quiet for ten seconds. A report to a window in `refused` is answered
    /// `not_found`, as a window that closed since the list.
    fn serve(stream: &UnixStream, refused: &[&str], expected: usize) -> (Value, Vec<Value>) {
        let mut out = stream.try_clone().expect("clone");
        let (hello, lines) = greet(stream);
        let mut reports = Vec::new();
        let mut asked = 0;
        for line in lines {
            let Ok(line) = line else { break };
            let request: Value = serde_json::from_str(&line).expect("json");
            let answer = match request["method"].as_str() {
                Some("windows.list") => {
                    json!({ "id": request["id"], "result": { "windows": ["w1", "w2"] } })
                }
                Some("build.report") => {
                    asked += 1;
                    let window = request["params"]["window"].as_str().unwrap_or_default();
                    if refused.contains(&window) {
                        json!({ "id": request["id"], "error": {
                            "code": "not_found", "message": "no such window", "retry": "never"
                        } })
                    } else {
                        reports.push(request["params"].clone());
                        json!({ "id": request["id"], "result": {} })
                    }
                }
                other => panic!("unexpected request {other:?}"),
            };
            writeln!(out, "{answer}").expect("answer");
            if asked == expected {
                break;
            }
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

    fn failed() -> BuildState {
        let mut errors = Diagnostics::new();
        errors.push(Diagnostic::new(
            "mismatched types".into(),
            Some(SourceLocation::new("src/lib.rs".into(), 12, 5)),
        ));
        BuildState::Failed(errors)
    }

    fn reports_to(windows: &[&str], state: &BuildState) -> Vec<Value> {
        windows
            .iter()
            .map(|window| {
                serde_json::to_value(BuildReportParams::new(
                    window.parse().expect("a window handle"),
                    state.clone(),
                ))
                .expect("json")
            })
            .collect()
    }

    /// The next conversation the client opens, within ten seconds: a client that never opens
    /// one fails the test instead of hanging it.
    fn accept(listener: &UnixListener) -> UnixStream {
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).expect("a blocking stream");
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the client opened no conversation"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    }

    /// A stand-in agent listening at a fresh socket.
    fn listen() -> (tempfile::TempDir, std::path::PathBuf, UnixListener) {
        let dir = tempfile::tempdir().expect("dir");
        let socket = dir.path().join("agent.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        (dir, socket, listener)
    }

    #[test]
    fn a_report_reaches_every_window_as_an_observer() {
        let (_dir, socket, listener) = listen();
        let reporter = BuildReporter::start(credentials(&socket)).expect("reporter");
        reporter.report(failed());
        // A report sent before the drop is still sent; then the thread ends and closes the
        // conversation, which ends the stand-in's.
        drop(reporter);
        let stream = accept(&listener);
        let (hello, reports) = serve(&stream, &[], 2);
        assert_eq!(hello["hello"]["role"], "observer", "{hello}");
        assert_eq!(reports, reports_to(&["w1", "w2"], &failed()));
    }

    /// Reports made while the app is not reachable yet wait for it, and the window gets the
    /// newest of them: the reporter folds what waited instead of replaying each one.
    #[test]
    fn the_reports_made_before_the_app_listens_reach_it_folded() {
        let dir = tempfile::tempdir().expect("dir");
        let socket = dir.path().join("agent.sock");
        let reporter = BuildReporter::start(credentials(&socket)).expect("reporter");
        reporter.report(BuildState::Building);
        reporter.report(BuildState::Succeeded);
        reporter.report(failed());
        drop(reporter);
        // The reporter's thread is connecting to an endpoint nobody listens at yet, with every
        // report but the first one or all three waiting behind it.
        std::thread::sleep(Duration::from_millis(300));
        let listener = UnixListener::bind(&socket).expect("bind");
        let stream = accept(&listener);
        let (_, reports) = serve(&stream, &[], 4);
        let succeeded = reports_to(&["w1", "w2"], &BuildState::Succeeded);
        assert!(
            !reports.iter().any(|report| succeeded.contains(report)),
            "a report that waited behind a newer one was sent: {reports:?}"
        );
        assert_eq!(
            reports[reports.len().saturating_sub(2)..],
            reports_to(&["w1", "w2"], &failed()),
            "the newest report is what the windows show last"
        );
    }

    /// A window that refuses the report (it closed since the list) leaves the others to get it.
    #[test]
    fn a_window_that_refuses_the_report_leaves_the_others_to_get_it() {
        let (_dir, socket, listener) = listen();
        let reporter = BuildReporter::start(credentials(&socket)).expect("reporter");
        reporter.report(failed());
        drop(reporter);
        let stream = accept(&listener);
        let (_, reports) = serve(&stream, &["w1"], 2);
        assert_eq!(reports, reports_to(&["w2"], &failed()));
    }

    /// The app the conversation reached was replaced: the conversation breaks on its first
    /// request, and the report goes to the successor over a new one.
    #[test]
    fn a_report_whose_app_was_replaced_reaches_its_successor() {
        let (_dir, socket, listener) = listen();
        let reporter = BuildReporter::start(credentials(&socket)).expect("reporter");
        reporter.report(failed());
        drop(reporter);
        let gone = accept(&listener);
        let (_, mut lines) = greet(&gone);
        let _ = lines.next().expect("the first request");
        drop(lines);
        gone.shutdown(std::net::Shutdown::Both).expect("shutdown");
        let successor = accept(&listener);
        let (_, reports) = serve(&successor, &[], 2);
        assert_eq!(reports, reports_to(&["w1", "w2"], &failed()));
    }
}
