//! The native journal a debug build serving the driver agent records (ADR-0288 §11, ADR-0215
//! §4): the backend's line capture, installed on the loop's thread before its platform starts,
//! since a registry reports only to the capture installed when it was created. The launch that
//! says whether the agent is served is read from the platform, so the capture is ended right
//! after it when none is: until then nobody wants an ordinary row, and only each stream's
//! bounded setup is kept. A served loop keeps it and attaches its lines to each UI runtime it
//! hands the agent.

use flui_native::journal::codec::Backend;
use flui_native::journal::lines::{Forms, LineCapture, LineSink, StreamLine, capture_lines};
use flui_runtime::ui_runtime::NativeLines;

use super::host::APP_RUNTIME;
use crate::app::ui_runtime::UiRuntime;

/// The loop's journal: its lines and the capture that feeds them.
#[derive(Debug)]
pub(crate) struct NativeJournal {
    lines: NativeLines,
    _capture: LineCapture,
}

/// Hands the capture's lines to the runtime's fan-out.
struct Sink(NativeLines);

impl LineSink for Sink {
    fn wants(&self) -> Forms {
        let wanted = self.0.wants();
        Forms {
            kept: wanted.kept,
            redacted: wanted.redacted,
        }
    }

    fn line(&mut self, line: StreamLine<'_>) {
        self.0
            .line(line.stream, line.setup, line.kept, line.redacted);
    }

    fn lost(&mut self, stream: u32) {
        self.0.lost(stream);
    }
}

/// Install the journal of `backend` on this thread, before the loop starts its platform. A
/// journal an earlier loop on this thread left is replaced.
pub(super) fn begin(backend: Backend) {
    let lines = NativeLines::default();
    install(backend, Sink(lines.clone()), lines);
}

/// Install a capture of `backend` that hands its lines to `sink`, as the journal of `lines`.
fn install(backend: Backend, sink: impl LineSink + 'static, lines: NativeLines) {
    let journal = NativeJournal {
        _capture: capture_lines(backend, sink),
        lines,
    };
    let stale = APP_RUNTIME.with(|slot| slot.borrow_mut().native_journal.replace(journal));
    // Ended outside the borrow.
    drop(stale);
}

/// Whether this loop journals its backend: the agent's hello lists the `native` layer then.
pub(super) fn is_open() -> bool {
    APP_RUNTIME.with(|slot| slot.borrow().native_journal.is_some())
}

/// End the journal unless the agent is `served`: an app nobody drives pays nothing for it.
pub(super) fn keep_if(served: bool) {
    if !served {
        end();
    }
}

/// End this loop's journal.
pub(super) fn end() {
    let journal = APP_RUNTIME.with(|slot| slot.borrow_mut().native_journal.take());
    drop(journal);
}

/// Attach the journal's lines to `ui_runtime`, so its recordings can hold the `native` layer.
pub(super) fn attach(ui_runtime: &UiRuntime) {
    let lines = APP_RUNTIME.with(|slot| {
        slot.borrow()
            .native_journal
            .as_ref()
            .map(|journal| journal.lines.clone())
    });
    if let Some(lines) = lines {
        ui_runtime.attach_native_journal(&lines);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_native::input::assembly::{ContactRegistry, NativeClock, NativeDevice};
    use flui_platform::EventTime;
    use flui_platform::pointer::PointerKind;

    use super::*;

    /// Counts the lines a capture hands on, wanting every form.
    struct Counted(Arc<AtomicUsize>);

    impl LineSink for Counted {
        fn wants(&self) -> Forms {
            Forms {
                kept: true,
                redacted: true,
            }
        }

        fn line(&mut self, _line: StreamLine<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn lost(&mut self, _stream: u32) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A loop the driver agent does not serve ends its journal once the launch is read: a
    /// registry its platform makes afterwards reaches no sink. A served loop keeps it.
    #[test]
    fn a_loop_nobody_serves_ends_its_native_journal_so_no_line_reaches_it() {
        for served in [true, false] {
            let seen = Arc::new(AtomicUsize::new(0));
            install(
                Backend::Headless,
                Counted(Arc::clone(&seen)),
                NativeLines::default(),
            );
            assert!(is_open(), "served: {served}");
            keep_if(served);
            assert_eq!(is_open(), served);

            let mut registry = ContactRegistry::new(NativeClock::epoch_only());
            let _ = registry.add_device(
                NativeDevice(1),
                PointerKind::Mouse,
                EventTime::from_nanos(1_000_000),
            );
            end();
            assert_eq!(
                seen.load(Ordering::SeqCst) > 0,
                served,
                "served: {served}: lines reached the sink"
            );
        }
    }
}
