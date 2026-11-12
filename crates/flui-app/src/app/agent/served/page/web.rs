//! The page export the agent serves on, and the task each connection runs.

use std::future::poll_fn;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Poll, Waker};

use flui_native::PlatformError;
use flui_native::page_export::{
    PageConnection, PageExport, PageExportHandler, PageLine, PageTimer,
};
use flui_protocol::AppInfo;
use flui_protocol::framing::MAX_LINE;
use flui_runtime::agent::{Service, ServiceConfig};
use flui_view::dev_agent::AgentWindow;
use parking_lot::Mutex;
use web_time::Instant;

use super::super::super::Disconnect;
use super::super::launch::Launch;
use super::{Conversation, Peer, converse};

// A driver reads the export's shape from the protocol crate, and the page serves flui-native's.
const _: () = assert!(
    flui_native::page_export::PAGE_EXPORT_VERSION == flui_protocol::launch::PAGE_EXPORT_VERSION,
    "the page export's version is the one the protocol tells drivers"
);

/// The export the launch named, and the service its connections hold conversations with.
///
/// The export is the page's: it lives in a task of the page's executor, which drops it once
/// the agent stops, at the page's next turn. This handle, held by the agent's hook, keeps only
/// what may cross a thread.
pub(in super::super) struct Running {
    service: Service,
    export: Arc<ExportTask>,
}

/// What stops the task that holds the page export.
#[derive(Default)]
struct ExportTask {
    stopped: AtomicBool,
    /// The task's waker, as of its last poll.
    waker: Mutex<Option<Waker>>,
}

impl Running {
    /// Serve `window` to the drivers that connect.
    pub(in super::super) fn window_opened(&self, window: AgentWindow) {
        self.service.window_opened(window);
    }

    /// Stop serving: every conversation is told the agent is stopping, and at the page's next
    /// turn the global goes and every connection ends.
    pub(in super::super) fn stop(self) {
        drop(self);
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // Stopped first: the conversations the export's end closes are not a driver going.
        self.service.stop();
        self.export.stopped.store(true, Ordering::SeqCst);
        let waker = self.export.waker.lock().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// Define the global `launch` names and start accepting the connections its scripts open.
pub(in super::super) fn listen(
    launch: &Launch,
    app: AppInfo,
    disconnect: Option<Arc<Disconnect>>,
    records_native: bool,
) -> Result<Running, PlatformError> {
    let on_driver_gone = disconnect
        .map(|disconnect| -> Box<dyn Fn() + Send + Sync> { Box::new(move || disconnect.fire()) });
    let service = Service::new(ServiceConfig {
        records_native,
        token: launch.token.clone(),
        app,
        limits: launch.limits,
        on_driver_gone,
    });
    let export = PageExport::try_new(
        &launch.address,
        MAX_LINE,
        Accepting {
            service: service.clone(),
        },
    )?;
    let task = Arc::new(ExportTask::default());
    let held = Arc::clone(&task);
    wasm_bindgen_futures::spawn_local(async move {
        poll_fn(|ctx| {
            if held.stopped.load(Ordering::SeqCst) {
                return Poll::Ready(());
            }
            let replaced = held.waker.lock().replace(ctx.waker().clone());
            drop(replaced);
            // Read again once the waker is kept: a stop between the two reads is not lost.
            if held.stopped.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        drop(export);
    });
    Ok(Running {
        service,
        export: task,
    })
}

/// Takes each connection a script opens into a conversation of the service.
struct Accepting {
    service: Service,
}

impl PageExportHandler for Accepting {
    fn connect(&self, line: PageLine) -> Result<Rc<dyn PageConnection>, String> {
        let session = self
            .service
            .accept(Instant::now())
            .map_err(|refused| refused.to_string())?;
        let conversation = Conversation::new(session);
        let woken = Rc::downgrade(&conversation);
        let timer = PageTimer::try_new(move || {
            if let Some(conversation) = woken.upgrade() {
                conversation.wake();
            }
        })
        .map_err(|error| error.to_string())?;
        wasm_bindgen_futures::spawn_local(converse(
            Rc::clone(&conversation),
            Script { line, timer },
        ));
        Ok(conversation)
    }
}

/// The script's end of a connection: its `onLine`, and the timer that keeps the session's
/// deadline.
struct Script {
    line: PageLine,
    timer: PageTimer,
}

impl Peer for Script {
    fn send(&self, line: &str) -> bool {
        self.line.send(line).is_ok()
    }

    fn close(&self) {
        self.line.close();
    }

    fn wake_at(&self, deadline: Option<Instant>) -> bool {
        let Some(deadline) = deadline else {
            self.timer.cancel();
            return true;
        };
        self.timer
            .arm(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }
}

impl PageConnection for Conversation {
    fn receive(&self, line: String) {
        Conversation::receive(self, &line);
    }

    fn closed(&self) {
        self.peer_closed();
    }
}
