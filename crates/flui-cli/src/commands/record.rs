//! `flui record`: write a running app's input recording to a file.
//!
//! See [`crate::agent::record`]. The command connects as an observer, so it records beside a
//! `flui mcp` that drives the app. The recording goes to the named file, or to stdout for `-`;
//! what the command says goes to stderr either way. With `--native <dir>` the recording holds
//! the native layer too, and each backend stream goes to `<dir>/<n>.jsonl` beside
//! `<dir>/index.json` ([`crate::agent::record::native`]).

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use flui_protocol::session::ClientRole;
use flui_protocol::{RecordLayer, RecordParams, WindowId};

use crate::agent::Credentials;
use crate::agent::client::{Connection, Failure};
use crate::agent::record::native::{NativeFiles, WrittenStream};
use crate::agent::record::{Ending, RecordFailure, Recorded, record};
use crate::error::{CliError, CliResult};
use crate::ui;

/// How long the command waits for the app to accept it: it may still be building.
const CONNECT_WAIT: Duration = Duration::from_secs(10);

/// What `flui record` was asked for.
#[derive(Debug)]
pub(crate) struct RecordArgs {
    /// Where the recording goes; `-` is stdout.
    pub(crate) output: PathBuf,
    /// The window whose runtime is recorded, as the protocol spells it (`w3`).
    pub(crate) window: Option<String>,
    /// Keep the text the rows carry.
    pub(crate) keep_text: bool,
    /// Where the native streams go, when the recording holds the native layer.
    pub(crate) native: Option<PathBuf>,
    /// The session file to connect with.
    pub(crate) session: Option<PathBuf>,
}

/// Records until Ctrl-C, or until the app ends the recording.
pub(crate) fn execute(args: RecordArgs) -> CliResult<()> {
    let window = args
        .window
        .as_deref()
        .map(|window| {
            window.parse::<WindowId>().map_err(|_| {
                CliError::Usage(format!(
                    "`{window}` is not a window handle; `flui mcp`'s flui_windows names them (`w3`)"
                ))
            })
        })
        .transpose()?;
    let mut params = RecordParams::input(window).keep_text(args.keep_text);
    if args.native.is_some() {
        params.layers.push(RecordLayer::Native);
    }
    let credentials = Credentials::find(args.session.as_deref()).map_err(CliError::Missing)?;
    let mut connection = Connection::connect(&credentials, ClientRole::Observer, CONNECT_WAIT)
        .map_err(|why| {
            CliError::Missing(format!(
                "{why}. Is the app running? Start it with `flui run --agent`."
            ))
        })?;

    let to_stdout = args.output == Path::new("-");
    let mut out: Box<dyn Write> = if to_stdout {
        Box::new(io::stdout().lock())
    } else {
        let file = File::create(&args.output).map_err(|e| {
            CliError::context(e, format!("could not create {}", args.output.display()))
        })?;
        Box::new(BufWriter::new(file))
    };
    let shown = if to_stdout {
        "stdout".to_owned()
    } else {
        args.output.display().to_string()
    };

    let mut native = args
        .native
        .as_deref()
        .map(NativeFiles::create)
        .transpose()
        .map_err(|e| CliError::context(e, "could not create the native directory"))?;

    let interrupted = super::run::install_interrupt_flag();
    interrupted.store(false, Ordering::SeqCst);
    ui::step(format!("Recording to {shown}; Ctrl-C stops"))?;
    let outcome = record(&mut connection, &params, &mut out, native.as_mut(), || {
        interrupted.load(Ordering::SeqCst)
    });
    drop(out);
    // What arrived of the native streams is written whichever way the recording ended.
    if let Some(native) = native {
        let dir = native.dir().display().to_string();
        let streams = native
            .finish()
            .map_err(|e| CliError::context(e, "could not write the native streams"))?;
        report_native(&dir, &streams)?;
    }
    match outcome {
        Ok(Recorded { summary, ending }) => {
            ui::emit(
                "record.end",
                &serde_json::json!({
                    "rows": summary.rows,
                    "lost_rows": summary.lost_rows,
                    "ended_by_app": ending == Ending::ByApp,
                }),
            );
            let why = match ending {
                Ending::Stopped => "",
                Ending::ByApp => " (the window closed or the app ended)",
            };
            ui::success(format!("Recorded {} rows to {shown}{why}", summary.rows))?;
            if summary.lost_rows > 0 {
                ui::warning(format!(
                    "{} rows were lost, read slower than the app made them, so a replay refuses the recording",
                    summary.lost_rows
                ))?;
            }
            Ok(())
        }
        Err(RecordFailure::Start(failure)) => Err(CliError::Missing(format!(
            "the app did not start a recording: {}",
            describe(&failure)
        ))),
        Err(RecordFailure::Broken { rows, why }) => Err(CliError::Missing(format!(
            "the recording broke off after {rows} rows ({}); {shown} holds them but no end, \
             so a replay refuses it",
            describe(&why)
        ))),
        Err(RecordFailure::Write(error)) => Err(CliError::context(
            error,
            format!("could not write the recording to {shown}"),
        )),
        Err(RecordFailure::Native(error)) => {
            Err(CliError::context(error, "could not write a native stream"))
        }
    }
}

/// Says what went to the native directory `dir`.
fn report_native(dir: &str, streams: &[WrittenStream]) -> CliResult<()> {
    ui::emit(
        "record.native",
        &serde_json::json!({ "dir": dir, "streams": streams }),
    );
    let files = streams
        .iter()
        .filter(|stream| stream.file.is_some())
        .count();
    ui::success(format!("Wrote {files} native streams to {dir}"))?;
    for stream in streams {
        if stream.file.is_none() {
            ui::warning(format!(
                "native stream {} sent no header, so its {} rows have no file",
                stream.stream, stream.lost_rows
            ))?;
        } else if stream.lost_rows > 0 {
            ui::warning(format!(
                "native stream {} lost {} rows, so a replay refuses it at its gap",
                stream.stream, stream.lost_rows
            ))?;
        }
    }
    Ok(())
}

/// `failure` as a sentence part.
fn describe(failure: &Failure) -> String {
    match failure {
        Failure::Wire(error) => error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| error.to_string(), str::to_owned),
        Failure::Transport(why) | Failure::Unsent(why) => why.clone(),
    }
}
