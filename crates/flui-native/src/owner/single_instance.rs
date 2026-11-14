//! What a desktop host's first instance keeps of the launches later processes
//! of its application hand it (the activation spec, design §5): the name an
//! app id gives the instance, and the inbox the launches wait in until the
//! owner takes them. The transport is the host's: the session bus on Linux
//! (`owner::linux::single_instance`), a named pipe on Windows
//! (`owner::windows::single_instance`, whose frame the `frame` module writes and reads).
//!
//! A launch is accepted work: its process waits for the answer and ends only
//! once the application holds the launch. Every launch posted here is either
//! taken, handed to the application and answered, or handed back to its
//! process, which then runs as an instance of its own: when the inbox is
//! full, when the owner's loop has ended, when the owner does not take it
//! within the hold, or when the application cannot hold it.

#[cfg(any(x11_host, wayland_host))]
pub(crate) use inbox::Delivery;
#[cfg(all(windows, feature = "single-instance"))]
pub(crate) use inbox::MAX_WAITING;
#[cfg(any(x11_host, wayland_host, all(windows, feature = "single-instance")))]
pub(crate) use inbox::hand_on;
#[cfg(all(any(x11_host, wayland_host, windows), feature = "single-instance"))]
pub(crate) use inbox::{HOLD, Inbox, OwnerWake};
#[cfg(all(any(x11_host, wayland_host), feature = "single-instance"))]
pub(crate) use inbox::{Posted, Refused};

#[cfg(any(all(windows, feature = "single-instance"), test))]
pub(crate) mod frame;

/// The name the instance of an application of `app_id` is known by: the app
/// id itself, when it is one (reverse DNS: two or more elements of ASCII
/// letters, digits, `_` and `-`, none starting with a digit, 255 bytes at
/// most); `None` otherwise. It is a D-Bus bus name as it is, and names the
/// Windows mutex and pipe with no character either reserves.
#[cfg(any(feature = "single-instance", test))]
pub(crate) fn instance_name(app_id: &str) -> Option<&str> {
    let well_formed = app_id.len() <= 255
        && app_id.split('.').count() >= 2
        && app_id.split('.').all(|element| {
            let mut chars = element.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '-'))
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        });
    well_formed.then_some(app_id)
}

#[cfg(any(x11_host, wayland_host, all(windows, feature = "single-instance")))]
mod inbox {
    #![cfg_attr(
        not(feature = "single-instance"),
        expect(
            dead_code,
            reason = "without one instance a Linux host only hands on what no first instance takes"
        )
    )]
    use std::collections::VecDeque;
    use std::future::Future as _;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Poll;
    use std::time::Duration;

    use parking_lot::Mutex;
    use tokio::sync::oneshot;

    use flui_platform::Opened;

    use crate::owner::app_delivery::AppDelivery;

    /// How many launches wait for the owner at once; a later one is refused.
    pub(crate) const MAX_WAITING: usize = 16;

    /// How long a launch waits for the owner to take it before it is handed
    /// back to its process: a loop that long in one turn is not taking work,
    /// and the launch, which then runs on its own, is never delivered here
    /// too.
    pub(crate) const HOLD: Duration = Duration::from_secs(15);

    /// Wakes the owner so that it takes what waits: the host's own wake (the
    /// owner signal, a message posted to the owner window).
    pub(crate) trait OwnerWake: Send + Sync {
        /// Wake the owner: `false` once its loop no longer takes work.
        fn wake(&self) -> bool;
    }

    /// Why a launch was refused when it was posted.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Refused {
        /// [`MAX_WAITING`] launches wait already.
        Full,
        /// The owner's loop has ended.
        Ending,
    }

    impl Refused {
        /// The refusal as the launching process hears it.
        pub(crate) const fn reason(self) -> &'static str {
            match self {
                Self::Full => "the running instance is not taking launches",
                Self::Ending => "the running instance is ending",
            }
        }
    }

    /// A launch waiting for the owner, and the claim its caller waits for.
    type Waiting<T> = (T, oneshot::Sender<oneshot::Receiver<()>>);

    /// What the caller of [`Inbox::post`] waits on: the owner's claim of
    /// the launch, which carries the owner's answer to come.
    pub(crate) struct Posted(oneshot::Receiver<oneshot::Receiver<()>>);

    /// The owner's answer to a launch it took: [`Self::delivered`] once the
    /// application holds what the launch opens; dropped, the launch is
    /// refused to its caller, which then runs as an instance of its own.
    #[must_use = "a delivery dropped refuses its launch"]
    pub(crate) struct Delivery(oneshot::Sender<()>);

    impl Delivery {
        /// The application holds the launch: its caller hears it was taken.
        pub(crate) fn delivered(self) {
            // A caller that stopped waiting (its process ended) hears
            // nothing; the launch is delivered all the same.
            let _ = self.0.send(());
        }
    }

    /// Hand what a launch the owner took opens to the application, and
    /// answer its caller as taken once the application holds it, before its
    /// handler runs. `false` when the application holds as many opened
    /// launches as it keeps waiting (a handler in a modal loop while
    /// launches come): the launch is refused to its caller, which runs as an
    /// instance of its own, so it is neither lost nor opened twice.
    pub(crate) fn hand_on(app: &AppDelivery, opened: Opened, delivery: Delivery) -> bool {
        match app.try_opened(opened, move || delivery.delivered()) {
            Ok(called) => {
                called.or_contain((), "a launch another instance forwarded");
                true
            }
            Err(refused) => {
                drop(refused);
                tracing::warn!(
                    "the application holds as many opened launches as it keeps waiting; a \
                     forwarded launch is refused and runs as an instance of its own"
                );
                false
            }
        }
    }

    /// The launches waiting for the owner, and the way to wake it.
    pub(crate) struct Inbox<T> {
        /// Each launch with its caller's answer. The owner takes and answers
        /// under this lock, and a caller that stops waiting withdraws under
        /// it, so a launch is either taken and answered or handed back.
        queue: Mutex<VecDeque<Waiting<T>>>,
        /// The owner's wake, once the owner is attached.
        wake: Mutex<Option<Arc<dyn OwnerWake>>>,
        /// How long a launch waits for the owner before it is handed back.
        hold: Duration,
        /// The runtime whose timer measures the hold: a transport may run
        /// its calls on an executor of its own, which has none.
        timers: tokio::runtime::Handle,
    }

    impl<T> Inbox<T> {
        /// An inbox that hands a launch back after `hold`, measured on
        /// `timers`.
        pub(crate) fn new(hold: Duration, timers: tokio::runtime::Handle) -> Self {
            Self {
                queue: Mutex::default(),
                wake: Mutex::default(),
                hold,
                timers,
            }
        }

        /// Wake the owner through `wake` for each launch from now on, and
        /// for any that came before.
        pub(crate) fn attach(&self, wake: Arc<dyn OwnerWake>) {
            let replaced = self.wake.lock().replace(wake);
            // Dropped with the lock released: a wake's handle may own what
            // runs application code as it goes.
            drop(replaced);
            if !self.queue.lock().is_empty() {
                self.wake();
            }
        }

        /// Queue `launch` and wake the owner, once it is attached; its caller
        /// waits on what this returns with [`Self::answer`].
        ///
        /// # Errors
        ///
        /// [`Refused::Full`] past [`MAX_WAITING`], [`Refused::Ending`] once
        /// the owner's loop has ended: the launching process hears the
        /// refusal and runs as an instance of its own, so the launch is not
        /// lost.
        pub(crate) fn post(&self, launch: T) -> Result<Posted, Refused> {
            let (claim, claimed) = oneshot::channel();
            let refused = {
                let mut queue = self.queue.lock();
                if queue.len() >= MAX_WAITING {
                    Some(launch)
                } else {
                    queue.push_back((launch, claim));
                    None
                }
            };
            // Dropped and logged with the queue released: a launch's drop
            // and a subscriber are code the inbox does not own.
            if let Some(refused) = refused {
                drop(refused);
                tracing::warn!("the owner is not taking forwarded launches; one is refused");
                return Err(Refused::Full);
            }
            if self.wake() {
                return Ok(Posted(claimed));
            }
            // The owner's loop ended: what waits is never taken, and each
            // waiting caller hears it as its sender drops.
            let dropped = self.refuse_waiting();
            tracing::warn!(
                launches = dropped,
                "the running instance is ending; its waiting launches are refused"
            );
            Err(Refused::Ending)
        }

        /// The oldest waiting launch, with the answer its caller now waits
        /// for past the hold. A launch whose caller stopped waiting (it was
        /// handed back and, if its process still runs, runs on its own) is
        /// not taken.
        ///
        /// One at a time: the owner hands each to the application before it
        /// takes the next, so a launch behind one whose handler blocks stays
        /// withdrawable by its hold, and a caller that gives up never sees
        /// its launch opened here as well.
        pub(crate) fn take_next(&self) -> Option<(T, Delivery)> {
            let mut queue = self.queue.lock();
            // Claimed under the lock a withdrawing caller takes: a oneshot's
            // send wakes only the caller's task, no application code.
            while let Some((launch, claim)) = queue.pop_front() {
                let (answer, heard) = oneshot::channel();
                if claim.send(heard).is_ok() {
                    return Some((launch, Delivery(answer)));
                }
            }
            None
        }

        /// Whether a launch waits for the owner.
        pub(crate) fn has_waiting(&self) -> bool {
            !self.queue.lock().is_empty()
        }

        /// Refuse every waiting launch to its caller: the owner ends without
        /// taking them. How many there were.
        pub(crate) fn refuse_waiting(&self) -> usize {
            let left = std::mem::take(&mut *self.queue.lock());
            let count = left.len();
            // Each caller hears the refusal as its sender drops, with the
            // queue released.
            drop(left);
            count
        }

        /// How many launches wait for the owner.
        #[cfg(test)]
        pub(crate) fn waiting(&self) -> usize {
            self.queue.lock().len()
        }

        /// Wait for the answer to the launch [`post`](Self::post) queued.
        /// The owner takes it within the inbox's hold, or it is withdrawn
        /// and refused, so its process runs on its own and the owner never
        /// takes it; once taken, the owner's answer is awaited, which comes
        /// once the application holds the launch or refuses it.
        ///
        /// # Errors
        ///
        /// Why the launch was refused: the owner ended before taking it, held
        /// it past the hold, or could not deliver it.
        pub(crate) async fn answer(&self, posted: Posted) -> Result<(), &'static str> {
            const ENDED: &str = "the running instance ended before it took the launch";
            let Posted(mut claimed) = posted;
            // The timer is made on its runtime, not on the transport's executor.
            let hold = self.hold;
            let mut held = self
                .timers
                .spawn(async move { tokio::time::sleep(hold).await });
            // The claim first, so a launch taken as the hold ends counts as
            // taken.
            let claim = std::future::poll_fn(|ctx| {
                if let Poll::Ready(claim) = Pin::new(&mut claimed).poll(ctx) {
                    return Poll::Ready(Some(claim));
                }
                Pin::new(&mut held).poll(ctx).map(|_| None)
            })
            .await;
            held.abort();
            let claim = match claim {
                Some(claim) => claim.map_err(|_| ENDED)?,
                None => self.withdraw(claimed)?,
            };
            claim
                .await
                .map_err(|_| "the running instance could not deliver the launch")
        }

        /// Withdraw a launch held past the hold, unless the owner took it
        /// just now: then the owner's answer to it.
        fn withdraw(
            &self,
            mut claimed: oneshot::Receiver<oneshot::Receiver<()>>,
        ) -> Result<oneshot::Receiver<()>, &'static str> {
            let claim = {
                let mut queue = self.queue.lock();
                let claim = claimed.try_recv();
                // Closing the claim withdraws the launch: `take` skips it,
                // and it leaves the queue now.
                drop(claimed);
                let (kept, withdrawn): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut *queue)
                    .into_iter()
                    .partition(|(_, waiting)| !waiting.is_closed());
                *queue = kept;
                drop(queue);
                // A withdrawn launch is dropped with the queue released.
                drop(withdrawn);
                claim
            };
            match claim {
                Ok(claim) => Ok(claim),
                Err(oneshot::error::TryRecvError::Closed) => {
                    Err("the running instance ended before it took the launch")
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    Err("the running instance did not take the launch in time")
                }
            }
        }

        /// Wake the owner, once it is attached: `false` when its loop no
        /// longer takes work.
        fn wake(&self) -> bool {
            let wake = self.wake.lock().clone();
            // Woken with the lock released: waking may run application code
            // (the last handle of an owner signal drops its callback).
            wake.is_none_or(|wake| wake.wake())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::time::Duration;

        use super::{Delivery, Inbox, MAX_WAITING, OwnerWake, Posted, Refused};

        /// A runtime whose timer measures an inbox's hold.
        fn timers() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("a runtime")
        }

        /// An owner that counts its wakes and stops taking work when told.
        #[derive(Default)]
        struct Owner {
            wakes: AtomicUsize,
            ended: AtomicBool,
        }

        impl OwnerWake for Owner {
            fn wake(&self) -> bool {
                self.wakes.fetch_add(1, Ordering::SeqCst);
                !self.ended.load(Ordering::SeqCst)
            }
        }

        /// At most sixteen launches wait for an owner that takes none; the
        /// next is refused, and the ones taken and delivered are answered.
        #[test]
        fn launches_waiting_for_the_owner_are_bounded() {
            let runtime = timers();
            let inbox = Inbox::new(Duration::from_secs(15), runtime.handle().clone());
            let posted: Vec<Posted> = (0..MAX_WAITING)
                .map(|at| inbox.post(at).expect("room to wait"))
                .collect();
            assert_eq!(
                inbox.post(MAX_WAITING).err(),
                Some(Refused::Full),
                "past the bound"
            );
            let taken: Vec<(usize, Delivery)> = std::iter::from_fn(|| inbox.take_next()).collect();
            assert_eq!(
                taken
                    .iter()
                    .map(|(launch, _)| *launch)
                    .collect::<Vec<usize>>(),
                (0..MAX_WAITING).collect::<Vec<usize>>()
            );
            for (_, delivery) in taken {
                delivery.delivered();
            }
            for posted in posted {
                assert_eq!(
                    runtime.block_on(inbox.answer(posted)),
                    Ok(()),
                    "each caller hears its launch was taken"
                );
            }
            assert!(inbox.post(0).is_ok(), "room again");
        }

        /// A launch the owner does not take within the hold is handed back:
        /// its caller hears a refusal, runs on its own, and the owner never
        /// takes it too. One taken in time is answered.
        #[test]
        fn a_launch_held_past_the_hold_is_handed_back_once() {
            let runtime = timers();
            let inbox = Inbox::new(Duration::from_millis(20), runtime.handle().clone());
            let posted = inbox.post("flui:a").expect("room to wait");
            let handed_back = runtime.block_on(inbox.answer(posted));
            assert!(handed_back.is_err(), "the caller hears the refusal");
            assert!(
                inbox.take_next().is_none(),
                "a launch handed back is not delivered as well"
            );
            assert_eq!(inbox.waiting(), 0, "nor kept waiting");

            let posted = inbox.post("flui:b").expect("room to wait");
            let (launch, delivery) = inbox.take_next().expect("the one launch is taken");
            assert_eq!(launch, "flui:b");
            delivery.delivered();
            assert_eq!(runtime.block_on(inbox.answer(posted)), Ok(()));
        }

        /// A launch the owner took is answered by the owner, not by the hold:
        /// as taken once the application holds it, even past the hold, and
        /// refused when the owner cannot hand it on (the application's queue
        /// is full), so its process runs on its own and the launch is not
        /// lost.
        #[test]
        fn a_taken_launch_is_answered_by_its_delivery() {
            let runtime = timers();
            let inbox = Arc::new(Inbox::new(
                Duration::from_millis(20),
                runtime.handle().clone(),
            ));
            let posted = inbox.post("flui:held").expect("room to wait");
            let (_, delivery) = inbox.take_next().expect("the one launch is taken");
            let answering = runtime.spawn({
                let inbox = Arc::clone(&inbox);
                async move { inbox.answer(posted).await }
            });
            runtime.block_on(async { tokio::time::sleep(Duration::from_millis(80)).await });
            assert!(
                !answering.is_finished(),
                "a taken launch waits for its delivery past the hold"
            );
            delivery.delivered();
            assert_eq!(runtime.block_on(answering).expect("the answer ran"), Ok(()));

            let posted = inbox.post("flui:undelivered").expect("room to wait");
            drop(inbox.take_next());
            assert_eq!(
                runtime.block_on(inbox.answer(posted)),
                Err("the running instance could not deliver the launch"),
                "a launch the owner could not deliver is refused to its caller"
            );
        }

        /// A launch waiting behind one the owner took is not claimed with
        /// it: while the application handles the first past the hold, the
        /// second is handed back to its caller, which runs on its own, and
        /// the owner does not take it afterwards.
        #[test]
        fn a_launch_behind_a_taken_one_is_still_handed_back() {
            let runtime = timers();
            let inbox = Inbox::new(Duration::from_millis(20), runtime.handle().clone());
            let first = inbox.post("flui:first").expect("room to wait");
            let second = inbox.post("flui:second").expect("room to wait");
            let (launch, delivery) = inbox.take_next().expect("the first launch is taken");
            assert_eq!(launch, "flui:first");
            assert_eq!(
                runtime.block_on(inbox.answer(second)),
                Err("the running instance did not take the launch in time"),
                "the launch behind is handed back while the first is handled"
            );
            delivery.delivered();
            assert_eq!(runtime.block_on(inbox.answer(first)), Ok(()));
            assert!(
                inbox.take_next().is_none(),
                "a launch handed back is not delivered as well"
            );
        }

        /// A launch that came before the owner attached wakes it as it
        /// attaches; each later one wakes it as it comes. Once the owner's
        /// loop ends, a launch is refused, and so is every one still
        /// waiting, each to its own caller.
        #[test]
        fn the_owner_is_woken_once_attached_and_refuses_once_ended() {
            let runtime = timers();
            let inbox = Inbox::new(Duration::from_secs(15), runtime.handle().clone());
            let mut early = inbox.post(1).expect("room to wait");
            let owner = Arc::new(Owner::default());
            inbox.attach(Arc::clone(&owner) as Arc<dyn OwnerWake>);
            assert_eq!(
                owner.wakes.load(Ordering::SeqCst),
                1,
                "woken for the early launch"
            );

            let mut waiting = inbox.post(2).expect("room to wait");
            assert_eq!(owner.wakes.load(Ordering::SeqCst), 2, "woken for the next");

            owner.ended.store(true, Ordering::SeqCst);
            assert_eq!(inbox.post(3).err(), Some(Refused::Ending));
            assert_eq!(
                inbox.waiting(),
                0,
                "nothing is left waiting for an ended owner"
            );
            for answer in [&mut early, &mut waiting] {
                assert_eq!(
                    answer.0.try_recv().err(),
                    Some(tokio::sync::oneshot::error::TryRecvError::Closed),
                    "each waiting caller hears the refusal"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::instance_name;

    /// An app id in reverse DNS names the instance as it is; any other names
    /// none.
    #[test]
    fn an_app_id_in_reverse_dns_names_the_instance() {
        assert_eq!(
            instance_name("com.example.Notes"),
            Some("com.example.Notes")
        );
        assert_eq!(
            instance_name("org.flui_dev.my-app"),
            Some("org.flui_dev.my-app")
        );
        for refused in [
            "",
            "notes",
            "com..notes",
            "com.1notes",
            "com.example.notes/x",
            "com.example.no tes",
            "com.example.\\pipe",
            "com.example.",
            "com.example.Nötes",
        ] {
            assert_eq!(instance_name(refused), None, "{refused:?}");
        }
        let long = format!("com.{}", "a".repeat(252));
        assert_eq!(instance_name(&long), None, "past 255 bytes");
    }
}
