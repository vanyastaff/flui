//! The native capture as lines of the wire's native recording (ADR-0215 §4), for a consumer that
//! sends them on as they come: the driver agent's recordings (ADR-0288 §11).
//!
//! [`capture_lines`] installs a capture on the calling thread, as [`Recorder`] does, and hands
//! each source's lines to a [`LineSink`] instead of keeping them: its header, once its setup is
//! done (the first call that adds a row, so the header holds the setup the registry made before
//! it), then one line per row. Each line comes kept and redacted (ADR-0215 §5), each made only
//! when the sink asks for it, but a header and a [`setup`](StreamLine::setup) row are always made
//! in both, so a sink can keep them for a reader that joins later.
//!
//! A redacted line pseudonymizes the displays' persistent keys per source, in order of first
//! sight within the capture: a reader that joins later may first see `display-3`.
//!
//! [`Recorder`]: super::recorder::Recorder

use super::capture::{CaptureGuard, CaptureSource, Captured, capture};
use super::codec::{Backend, Header, Row};
use super::recorder::{Privacy, Pseudonyms, fold, opened};
use flui_protocol::native_recording as wire;

/// The forms of a line a sink wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forms {
    /// With its text kept.
    pub kept: bool,
    /// With its text redacted.
    pub redacted: bool,
}

/// One line a capture hands its sink.
#[derive(Debug, Clone, Copy)]
pub struct StreamLine<'a> {
    /// The source the line is of, numbered in the order the capture saw them open.
    pub stream: u32,
    /// Whether it is a header or a row that sets up what later rows are read against: a device
    /// added or removed, the click thresholds, the preferences, the displays or a window's
    /// state. A sink keeps these for a reader that joins later.
    pub setup: bool,
    /// The line with its text kept, when it was made.
    pub kept: Option<&'a wire::Line>,
    /// The line with its text redacted, when it was made.
    pub redacted: Option<&'a wire::Line>,
}

/// Where a [`LineCapture`] sends its lines. It is called on the thread its capture was
/// installed on, or wherever a registry that reports to it is fed from.
pub trait LineSink: Send {
    /// The forms of the next row that is not a setup row; a form nobody wants is not made.
    fn wants(&self) -> Forms;

    /// The next line of a stream.
    fn line(&mut self, line: StreamLine<'_>);

    /// A row of `stream` the format does not mirror was lost, so its readers have a gap.
    fn lost(&mut self, stream: u32);
}

/// A capture installed by [`capture_lines`]; dropping it ends the capture.
#[derive(Debug)]
#[must_use = "the capture ends when it is dropped"]
pub struct LineCapture {
    _guard: CaptureGuard,
}

/// Installs a capture for `backend` on this thread that hands every source's lines to `sink`.
pub fn capture_lines(backend: Backend, mut sink: impl LineSink + 'static) -> LineCapture {
    let mut sources: Vec<Source> = Vec::new();
    let guard = capture(move |source, call| {
        journal(&mut sources, &mut sink, backend, source, call);
    });
    LineCapture { _guard: guard }
}

/// One source of the capture, folded as both forms.
struct Source {
    kept: Header,
    redacted: Header,
    pseudonyms: Pseudonyms,
    /// Unused by a kept line, which pseudonymizes nothing.
    no_pseudonyms: Pseudonyms,
    /// Whether its header was sent: it is, before its first row.
    sent: bool,
}

fn journal(
    sources: &mut Vec<Source>,
    sink: &mut impl LineSink,
    backend: Backend,
    source: CaptureSource,
    call: Captured<'_>,
) {
    let stream = source.0;
    let index = stream as usize;
    if let Some(header) = opened(backend, call) {
        assert_eq!(
            index,
            sources.len(),
            "BUG: a capture numbers its sources in order"
        );
        sources.push(Source {
            kept: header.clone(),
            redacted: header,
            pseudonyms: Pseudonyms::default(),
            no_pseudonyms: Pseudonyms::default(),
            sent: false,
        });
        return;
    }
    let Some(entry) = sources.get_mut(index) else {
        return;
    };
    let setup = is_setup(call);
    let wanted = if setup {
        Forms {
            kept: true,
            redacted: true,
        }
    } else {
        sink.wants()
    };
    // Until a source has sent its header, every call folds in both forms: a setup call may
    // change the header, and the two headers never part. After it, only the forms wanted.
    let unsent = !entry.sent;
    let kept = (unsent || wanted.kept)
        .then(|| {
            fold(
                &mut entry.kept,
                unsent,
                &mut entry.no_pseudonyms,
                Privacy::KeepText,
                call,
            )
        })
        .flatten();
    let redacted = (unsent || wanted.redacted)
        .then(|| {
            fold(
                &mut entry.redacted,
                unsent,
                &mut entry.pseudonyms,
                Privacy::Redact,
                call,
            )
        })
        .flatten();
    if kept.is_none() && redacted.is_none() {
        // Folded into the header, or wanted by nobody.
        return;
    }
    if unsent {
        entry.sent = true;
        match entry.kept.to_wire() {
            Ok(header) => {
                let header = wire::Line::Header(Box::new(header));
                sink.line(StreamLine {
                    stream,
                    setup: true,
                    kept: Some(&header),
                    redacted: Some(&header),
                });
            }
            Err(_) => sink.lost(stream),
        }
    }
    let kept = kept.filter(|_| wanted.kept);
    let redacted = redacted.filter(|_| wanted.redacted);
    send(sink, stream, setup, kept.as_ref(), redacted.as_ref());
}

/// Sends a row in the forms given, or counts it lost when one of them cannot be made.
fn send(
    sink: &mut impl LineSink,
    stream: u32,
    setup: bool,
    kept: Option<&Row>,
    redacted: Option<&Row>,
) {
    let made = |row: Option<&Row>| {
        row.map(|row| row.to_wire().map(|row| wire::Line::Row(Box::new(row))))
            .transpose()
    };
    match (made(kept), made(redacted)) {
        (Ok(kept), Ok(redacted)) if kept.is_some() || redacted.is_some() => {
            sink.line(StreamLine {
                stream,
                setup,
                kept: kept.as_ref(),
                redacted: redacted.as_ref(),
            });
        }
        (Ok(_), Ok(_)) => {}
        _ => sink.lost(stream),
    }
}

/// Whether `call` sets up what later rows of its source are read against.
const fn is_setup(call: Captured<'_>) -> bool {
    matches!(
        call,
        Captured::AddDevice { .. }
            | Captured::RemoveDevice { .. }
            | Captured::ClickSettings(_)
            | Captured::Preferences(_)
            | Captured::Displays(_)
            | Captured::WindowState(_)
    )
}
