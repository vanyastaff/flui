//! A device request answered on a worker of its own, so the thread the
//! executor turns on is not blocked while the driver answers it (ADR-0305
//! §8). wasm32 has no thread to ask on: the web retires its executor and
//! awaits a new context instead.

use std::{any::Any, sync::mpsc};

use flui_foundation::panic::{FirstPanic, discard_payload};

use crate::{
    error::{EngineError, EngineResult},
    raster_executor::{DeviceAnswered, DeviceTicket},
};

/// What the worker hands back: the request's answer, or the payload of a
/// panic in it (wgpu's error handler, a `tracing` subscriber).
type Answer<T> = Result<EngineResult<T>, Box<dyn Any + Send>>;

/// One device request under way on its worker, and the ticket the executor
/// started it under.
///
/// Dropped before its answer is taken (the executor stopped waiting for it,
/// or the context retired), it waits for nothing: an answer still to come is
/// dropped on the worker once it finds nobody to take it, and one already
/// handed over is dropped with the request, on the thread that drops it, so a
/// retiring owner is never held for the driver.
pub(crate) struct DeviceWorker<T> {
    ticket: DeviceTicket,
    answer: mpsc::Receiver<Answer<T>>,
    /// An answer a test waited for, kept for [`Self::try_answer`].
    #[cfg(test)]
    waited: Option<Answer<T>>,
}

impl<T: Send + 'static> DeviceWorker<T> {
    /// Run `request` on its own thread under `ticket`, then call `answered`.
    ///
    /// # Errors
    /// The operating system refused the thread; nothing was asked.
    pub(crate) fn start(
        ticket: DeviceTicket,
        answered: DeviceAnswered,
        request: impl FnOnce() -> EngineResult<T> + Send + 'static,
    ) -> EngineResult<Self> {
        let (hand_over, answer) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("flui-device-request".to_owned())
            .spawn(move || {
                let mut caught = FirstPanic::new();
                let asked = caught.contain(request);
                let answer: Answer<T> = asked.ok_or_else(|| {
                    caught
                        .into_payload()
                        .expect("BUG: a contained request that returned nothing panicked")
                });
                // The receiver is gone only when nobody waits for this
                // request any more: its device goes with this thread.
                if let Err(mpsc::SendError(Err(payload))) = hand_over.send(answer) {
                    discard_payload(payload);
                }
                answered.notify();
            })
            .map_err(EngineError::device_creation)?;
        Ok(Self {
            ticket,
            answer,
            #[cfg(test)]
            waited: None,
        })
    }

    /// Wait until the worker answered, so the next turn installs its device.
    #[cfg(test)]
    pub(crate) fn wait(&mut self) {
        if self.waited.is_none() {
            self.waited = self.answer.recv().ok();
        }
    }

    /// The ticket the executor started this request under.
    pub(crate) fn ticket(&self) -> DeviceTicket {
        self.ticket
    }

    /// The request's answer, once the worker gave it; `None` while it is
    /// still asking. A panic in the request resumes here, on the executor's
    /// thread, which counts it as a failed attempt.
    pub(crate) fn try_answer(&mut self) -> Option<EngineResult<T>> {
        #[cfg(test)]
        let received = match self.waited.take() {
            Some(waited) => Ok(waited),
            None => self.answer.try_recv(),
        };
        #[cfg(not(test))]
        let received = self.answer.try_recv();
        match received {
            Ok(Ok(answer)) => Some(answer),
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                unreachable!("BUG: a device worker hands its answer over before it ends")
            }
        }
    }
}

impl<T> Drop for DeviceWorker<T> {
    fn drop(&mut self) {
        // A panic handed over and never taken is discarded, not resumed: the
        // executor stopped waiting for this request.
        #[cfg(test)]
        if let Some(Err(payload)) = self.waited.take() {
            discard_payload(payload);
        }
        if let Ok(Err(payload)) = self.answer.try_recv() {
            discard_payload(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU64,
        sync::{Arc, mpsc},
        time::Duration,
    };

    use flui_foundation::panic::FirstPanic;

    use super::DeviceWorker;
    use crate::{
        error::EngineError,
        raster_executor::{DeviceAnswered, DeviceTicket},
    };

    fn ticket() -> DeviceTicket {
        DeviceTicket::new(NonZeroU64::MIN)
    }

    /// A wake that reports on a channel, and the channel.
    fn answered() -> (DeviceAnswered, mpsc::Receiver<()>) {
        let (woke, woken) = mpsc::channel();
        (
            DeviceAnswered::new(Arc::new(move || {
                let _ = woke.send(());
            })),
            woken,
        )
    }

    /// A request answers on its worker and wakes the executor; a panic in it
    /// resumes on the thread that takes the answer, so the executor counts
    /// it as a failed attempt; an error is the attempt's failure.
    ///
    /// Red-check: drop the payload on the worker instead of handing it over
    /// and the panic never reaches the taker.
    #[test]
    fn a_worker_hands_over_its_answer_or_its_panic_and_wakes_the_executor() {
        let (wake, woken) = answered();
        let mut worker = DeviceWorker::start(ticket(), wake, || Ok(7_u32)).expect("a worker");
        woken
            .recv_timeout(Duration::from_secs(10))
            .expect("the answer wakes the executor");
        assert_eq!(worker.ticket(), ticket());
        assert!(matches!(worker.try_answer(), Some(Ok(7))));

        let (wake, woken) = answered();
        let mut worker =
            DeviceWorker::<u32>::start(ticket(), wake, || Err(EngineError::GpuUnavailable))
                .expect("a worker");
        woken.recv_timeout(Duration::from_secs(10)).expect("woken");
        assert!(matches!(
            worker.try_answer(),
            Some(Err(EngineError::GpuUnavailable))
        ));

        let (wake, woken) = answered();
        let mut worker =
            DeviceWorker::<u32>::start(ticket(), wake, || panic!("the driver's request panicked"))
                .expect("a worker");
        woken.recv_timeout(Duration::from_secs(10)).expect("woken");
        let mut caught = FirstPanic::new();
        assert!(caught.contain(|| worker.try_answer()).is_none());
        let payload = caught.into_payload().expect("the panic is resumed here");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some("the driver's request panicked")
        );
        flui_foundation::panic::discard_payload(payload);
    }

    /// A request dropped before it answered holds nobody: the drop returns at
    /// once, and the worker's late answer is dropped with it.
    ///
    /// Red-check: join the worker in `Drop` and the drop waits for the
    /// driver.
    #[test]
    fn a_request_nobody_waits_for_holds_nobody() {
        let (wake, woken) = answered();
        let (release, released) = mpsc::channel::<()>();
        let mut worker = DeviceWorker::start(ticket(), wake, move || {
            let _ = released.recv();
            Ok(1_u32)
        })
        .expect("a worker");
        assert!(worker.try_answer().is_none(), "the driver has not answered");
        let (dropped, has_dropped) = mpsc::channel();
        let dropping = std::thread::spawn(move || {
            drop(worker);
            let _ = dropped.send(());
        });
        has_dropped
            .recv_timeout(Duration::from_secs(10))
            .expect("the drop waits for no driver");
        release.send(()).expect("the worker still runs");
        woken
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker ends with its wake");
        dropping.join().expect("the drop returned");
    }
}
