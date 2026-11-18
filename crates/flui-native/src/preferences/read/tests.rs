//! The source the AppKit and Android hosts both are, driven through its
//! system seam by the owner signal and its turn, in virtual time.
//!
//! The turn reads the source as the application's turn does on both hosts
//! (`Platform::preferences` inside the turn callback), delivers a value that
//! differs from the last it delivered and confirms the delivery
//! (`Platform::preferences_delivered`), unless the rig tells it to drop the
//! delivery. Each post of the signal is one turn of the loop. A notice
//! arrives the way each host hears it: AppKit's notification runs
//! [`NoticedPreferences::notice_handler`]; the Android loop hears `FluiView`'s
//! report (after the turn its wake posts), a configuration change and `Start`
//! through [`LoopReads::hear`], as the loop does. AppKit retries on a timer;
//! the Android loop reads on its own only as [`LoopReads`] says, and blocks as
//! [`poll_timeout`] says. Nothing in the rig contains a read's panic: the
//! source does.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::GesturePreferences;

use super::*;
use crate::owner::android::poll::{LoopReads, LoopTurn, PreferenceNotice, poll_timeout};
use crate::owner::signal::OwnerTurnSlot;

/// Which host's loop the rig runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Host {
    /// A retry is a timer that wakes the owner.
    AppKit,
    /// A retry is the loop's own read at the source's retry deadline.
    Android,
}

/// How the virtual system answers a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Values,
    Error,
    Panic,
}

/// The system behind the source: its values, its clock, whether the
/// application is active, and the retries armed on its timer.
struct VirtualSystem {
    state: Mutex<SystemState>,
}

struct SystemState {
    host: Host,
    now: web_time::Instant,
    values: SystemPreferences,
    answer: Answer,
    active: bool,
    reads: usize,
    /// Each AppKit retry armed: when it fires, and the owner it wakes.
    armed: Vec<(web_time::Instant, Weak<OwnerSignal>)>,
    /// The pause before each retry, as the source scheduled it: armed on the
    /// AppKit timer, or left as the deadline the Android loop waits for.
    pauses: Vec<Duration>,
}

impl SystemReads for VirtualSystem {
    fn read(&self) -> Result<SystemPreferences, PlatformError> {
        let (answer, values) = {
            let mut state = self.state.lock();
            state.reads += 1;
            (state.answer, state.values.clone())
        };
        match answer {
            Answer::Values => Ok(values),
            Answer::Error => Err(PlatformError::Preferences {
                message: "the getter failed".into(),
            }),
            Answer::Panic => panic!("the getter panicked"),
        }
    }

    fn now(&self) -> web_time::Instant {
        self.state.lock().now
    }

    fn retrying(&self) -> Retrying<'_> {
        match self.state.lock().host {
            Host::AppKit => Retrying::OnTimer(self),
            Host::Android => Retrying::AtLoopDeadline,
        }
    }
}

impl RetryTimer for VirtualSystem {
    fn application_active(&self) -> bool {
        self.state.lock().active
    }

    fn arm(&self, delay: Duration, signal: Weak<OwnerSignal>) {
        let mut state = self.state.lock();
        state.pauses.push(delay);
        let at = state.now + delay;
        state.armed.push((at, signal));
    }
}

/// The values whose double-click interval is `millis`.
fn values(millis: u64) -> SystemPreferences {
    SystemPreferences::default().with_gestures(
        GesturePreferences::default().with_double_click_interval(Duration::from_millis(millis)),
    )
}

/// The source behind a counting owner signal, its turn, what the turns
/// delivered, and the Android loop's own reads.
struct Rig {
    source: Arc<NoticedPreferences<VirtualSystem>>,
    signal: Arc<OwnerSignal>,
    slot: OwnerTurnSlot,
    posts: Arc<AtomicUsize>,
    /// The posts a turn of the loop ran for.
    turns_run: RefCell<usize>,
    delivered: Rc<RefCell<Vec<SystemPreferences>>>,
    /// How many of the next turns drop what they read instead of
    /// delivering it.
    drops: Rc<Cell<usize>>,
    loop_reads: RefCell<LoopReads>,
}

impl Rig {
    /// A settled source on `host`: the host's first read woke the owner,
    /// whose turn delivered it; the counts start after that.
    fn settled(host: Host) -> Self {
        let posts = Arc::new(AtomicUsize::new(0));
        let signal = OwnerSignal::new({
            let posts = Arc::clone(&posts);
            Arc::new(move || {
                posts.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        signal.start().expect("the owner signal starts");
        let source = Arc::new(NoticedPreferences::new(
            VirtualSystem {
                state: Mutex::new(SystemState {
                    host,
                    now: web_time::Instant::now(),
                    values: values(500),
                    answer: Answer::Values,
                    active: true,
                    reads: 0,
                    armed: Vec::new(),
                    pauses: Vec::new(),
                }),
            },
            &signal,
        ));
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let drops = Rc::new(Cell::new(0));
        let slot = OwnerTurnSlot::default();
        signal
            .register_in(&slot, {
                let (source, delivered, drops) = (
                    Arc::clone(&source),
                    Rc::clone(&delivered),
                    Rc::clone(&drops),
                );
                Box::new(move || {
                    // The application's turn: read, deliver what differs from
                    // what it holds, and confirm the delivery; or drop it.
                    if let Ok(current) = source.sample() {
                        if drops.get() > 0 {
                            drops.set(drops.get() - 1);
                            return;
                        }
                        {
                            let mut delivered = delivered.borrow_mut();
                            if delivered.last() != Some(&current) {
                                delivered.push(current.clone());
                            }
                        }
                        source.delivered(&current);
                    }
                })
            })
            .expect("the owner takes its turn callback");
        let rig = Self {
            source,
            signal,
            slot,
            posts,
            turns_run: RefCell::new(0),
            delivered,
            drops,
            loop_reads: RefCell::new(LoopReads::first()),
        };
        // The host reads once as it starts, before any turn: AppKit as it
        // builds the source, the Android loop on its first turn.
        let now = rig.system().now;
        assert!(rig.loop_reads.borrow_mut().take_due(None, now));
        rig.source.sample().expect("the first read");
        rig.run_posted_turns();
        assert_eq!(rig.delivered.borrow().len(), 1, "the first reading");
        rig.posts.store(0, Ordering::SeqCst);
        *rig.turns_run.borrow_mut() = 0;
        rig.system().reads = 0;
        rig
    }

    fn system(&self) -> parking_lot::MutexGuard<'_, SystemState> {
        self.source.system().state.lock()
    }

    /// The turns posted since the source settled.
    fn posts(&self) -> usize {
        self.posts.load(Ordering::SeqCst)
    }

    /// Run one turn of the loop for each post not yet run.
    fn run_posted_turns(&self) {
        while *self.turns_run.borrow() < self.posts() {
            *self.turns_run.borrow_mut() += 1;
            assert!(!self.signal.drive_in(&self.slot), "the owner quit");
        }
    }

    /// The Android loop reads outside a turn, as the looper does: nothing
    /// around it contains a panic.
    fn loop_read(&self) {
        let before = self.source.retry_deadline();
        let _ = self.source.sample();
        self.note_loop_retry(before);
    }

    /// Record the pause of a retry the loop's own read scheduled.
    fn note_loop_retry(&self, before: Option<web_time::Instant>) {
        let after = self.source.retry_deadline();
        let mut system = self.system();
        if system.host == Host::Android
            && let Some(deadline) = after
            && after != before
        {
            let pause = deadline.saturating_duration_since(system.now);
            system.pauses.push(pause);
        }
    }

    /// The Android loop hears `heard`, as `AndroidLoop::hear_preference_notice`
    /// does.
    fn hear(&self, heard: PreferenceNotice) {
        let mut reads = *self.loop_reads.borrow();
        reads.hear(heard, || self.source.notice(), || self.loop_read());
        *self.loop_reads.borrow_mut() = reads;
    }

    /// Move the clock by `span`, a second at a time. Each AppKit retry due
    /// wakes the owner; the Android loop reads when its own read is due.
    fn idle(&self, span: Duration) {
        let host = self.system().host;
        let mut elapsed = Duration::ZERO;
        while elapsed < span {
            let step = Duration::from_secs(1).min(span.saturating_sub(elapsed));
            elapsed += step;
            let (now, due) = {
                let mut system = self.system();
                system.now += step;
                let now = system.now;
                let (due, armed) = std::mem::take(&mut system.armed)
                    .into_iter()
                    .partition::<Vec<_>, _>(|(at, _)| *at <= now);
                system.armed = armed;
                (now, due)
            };
            for (_, signal) in due {
                if let Some(signal) = signal.upgrade() {
                    let _ = signal.wake();
                }
            }
            if host == Host::Android
                && self
                    .loop_reads
                    .borrow_mut()
                    .take_due(self.source.retry_deadline(), now)
            {
                self.loop_read();
            }
            self.run_posted_turns();
        }
    }

    /// Whether the Android loop would block until it is woken now.
    fn android_loop_blocks(&self) -> bool {
        let now = self.system().now;
        let next = self
            .loop_reads
            .borrow()
            .next(self.source.retry_deadline(), now);
        poll_timeout(
            LoopTurn {
                should_render: false,
                awaiting_vsync: false,
                running: true,
                wake_deadline: None,
                next_preference_sample: next,
                hand_off_deadline: None,
                open_deadline: None,
            },
            now,
        )
        .is_none()
    }

    /// The system's values become `next`.
    fn change(&self, next: SystemPreferences) {
        self.system().values = next;
    }

    /// AppKit posts one of the notifications the source observes.
    fn appkit_notification(&self) {
        (self.source.notice_handler())();
        self.run_posted_turns();
    }

    /// `FluiView` reports a changed setting: its report wakes the owner, and
    /// the loop hears it after the turn.
    fn android_view_report(&self) {
        let _ = self.signal.wake();
        self.run_posted_turns();
        self.hear(PreferenceNotice::SettingsReported);
        self.run_posted_turns();
    }

    /// `ConfigChanged`: the loop hears it with no turn before it.
    fn android_configuration_change(&self) {
        self.hear(PreferenceNotice::ConfigurationChanged);
        self.run_posted_turns();
    }
}

/// How a change reaches the source: its name, the host that hears it so, and
/// what the rig runs for it.
type Notice = (&'static str, Host, fn(&Rig));

/// Each way a change reaches the source, on the host that hears it so.
const NOTICES: [Notice; 3] = [
    (
        "an AppKit notification",
        Host::AppKit,
        Rig::appkit_notification,
    ),
    ("a FluiView report", Host::Android, Rig::android_view_report),
    (
        "a configuration change",
        Host::Android,
        Rig::android_configuration_change,
    ),
];

/// An hour with no change costs the owner nothing on either host: no post,
/// no read, no retry, and the Android loop blocks with no timer. A periodic
/// timer anywhere in the source, its retry or the loop's own reads posts,
/// reads or names an instant within the hour.
#[test]
fn an_idle_source_never_wakes_the_owner() {
    for host in [Host::AppKit, Host::Android] {
        let rig = Rig::settled(host);
        rig.idle(Duration::from_secs(3600));
        assert_eq!(rig.posts(), 0, "{host:?}: idle posted a turn");
        assert_eq!(rig.system().reads, 0, "{host:?}: idle read the system");
        assert!(
            rig.system().pauses.is_empty(),
            "{host:?}: idle asked for a retry"
        );
        assert_eq!(rig.source.retry_deadline(), None, "{host:?}");
        assert!(
            rig.android_loop_blocks(),
            "{host:?}: the loop keeps a timer"
        );
    }
}

/// One change is one owner turn, however it is noticed, and the turn
/// delivers it; nothing follows it. A read inside the turn that found the
/// change and woke the owner again would post a second turn.
#[test]
fn one_change_notice_is_one_owner_turn() {
    for (notice, host, deliver) in NOTICES {
        let rig = Rig::settled(host);
        rig.change(values(700));
        deliver(&rig);
        assert_eq!(rig.posts(), 1, "{notice}: posts for one change");
        assert_eq!(
            *rig.delivered.borrow(),
            [values(500), values(700)],
            "{notice}"
        );
        rig.idle(Duration::from_secs(60));
        assert_eq!(rig.posts(), 1, "{notice}: a turn after the change");
    }
}

/// A read that fails or panics is retried 0.5 to 16 s apart, six times, and
/// then not again: the seventh failure is the last read, the owner hears
/// nothing for an hour after it, and each later change notice reads once.
/// AppKit's retries are owner turns; the Android loop retries before its turn
/// and wakes the owner only when a read recovers.
#[test]
fn failed_reads_back_off_and_stop_until_the_next_notice() {
    let pauses: Vec<Duration> = [500, 1_000, 2_000, 4_000, 8_000, 16_000]
        .into_iter()
        .map(Duration::from_millis)
        .collect();
    for (host, notice, retry_posts) in [
        (Host::AppKit, Rig::appkit_notification as fn(&Rig), 7),
        (Host::Android, Rig::android_configuration_change, 0),
    ] {
        for answer in [Answer::Error, Answer::Panic] {
            let case = format!("{host:?}, {answer:?}");
            let rig = Rig::settled(host);
            rig.system().answer = answer;
            notice(&rig);
            rig.idle(Duration::from_secs(60));
            assert_eq!(rig.system().pauses, pauses, "{case}: the retries");
            assert_eq!(rig.system().reads, 7, "{case}: reads of a failing system");
            assert_eq!(rig.posts(), retry_posts, "{case}: turns for the retries");
            assert_eq!(
                rig.source.retry_deadline(),
                None,
                "{case}: a retry after the last"
            );

            rig.idle(Duration::from_secs(3600));
            assert_eq!(
                rig.system().reads,
                7,
                "{case}: read after the retries ran out"
            );
            assert_eq!(
                rig.posts(),
                retry_posts,
                "{case}: woke after the retries ran out"
            );
            assert!(rig.android_loop_blocks(), "{case}: the loop keeps a timer");

            // A later notice reads once while the system still fails.
            notice(&rig);
            assert_eq!(rig.system().reads, 8, "{case}: the notice reads once");
            assert_eq!(
                rig.system().pauses.len(),
                6,
                "{case}: a retry after the notice"
            );

            // The system heals; the next notice reads and a turn delivers.
            {
                let mut system = rig.system();
                system.answer = Answer::Values;
                system.values = values(900);
            }
            notice(&rig);
            assert_eq!(rig.delivered.borrow().last(), Some(&values(900)), "{case}");
            assert_eq!(rig.source.retry_deadline(), None, "{case}");
        }
    }
}

/// While the application is inactive a failed read arms no retry: the
/// activation notice, a notice like any other, reads again.
#[test]
fn an_inactive_application_retries_only_on_its_next_notice() {
    let rig = Rig::settled(Host::AppKit);
    {
        let mut system = rig.system();
        system.active = false;
        system.answer = Answer::Error;
    }
    rig.appkit_notification();
    assert!(
        rig.system().pauses.is_empty(),
        "an inactive application armed a retry"
    );
    assert_eq!(rig.source.retry_deadline(), None);
    rig.idle(Duration::from_secs(3600));
    assert_eq!(rig.system().reads, 1, "read while inactive");
    assert_eq!(rig.posts(), 1, "woke while inactive");

    {
        let mut system = rig.system();
        system.active = true;
        system.answer = Answer::Values;
        system.values = values(800);
    }
    rig.appkit_notification();
    assert_eq!(rig.posts(), 2, "the activation notice");
    assert_eq!(rig.delivered.borrow().last(), Some(&values(800)));
}

/// A read outside a turn that recovers wakes the owner: the read that failed
/// before it delivered nothing.
#[test]
fn a_recovered_read_outside_a_turn_wakes_the_owner() {
    let rig = Rig::settled(Host::Android);
    rig.change(values(600));
    rig.system().answer = Answer::Error;
    rig.android_configuration_change();
    assert_eq!(rig.posts(), 0, "a failed read woke the owner");
    rig.system().answer = Answer::Values;
    rig.idle(Duration::from_secs(1));
    assert_eq!(rig.posts(), 1, "the recovery");
    assert_eq!(rig.delivered.borrow().last(), Some(&values(600)));
}

/// A change the application's turn reads and then drops (its host went, its
/// delivery failed) is read again in one more turn, which delivers it; one
/// that turn drops too waits for the next notice. Every way a change reaches
/// the source ends in the turn's read. A `FluiView` report is heard by the
/// loop after the turn its wake posts, so its own read wakes once more.
#[test]
fn a_change_a_turn_drops_is_read_again_once() {
    for ((notice, host, deliver), turns_for_a_dropped_change) in NOTICES.into_iter().zip([2, 3, 2])
    {
        let rig = Rig::settled(host);
        rig.change(values(700));
        rig.drops.set(1);
        deliver(&rig);
        assert_eq!(rig.posts(), 2, "{notice}: the change and its one retry");
        assert_eq!(
            rig.delivered.borrow().last(),
            Some(&values(700)),
            "{notice}"
        );

        rig.change(values(800));
        rig.drops.set(usize::MAX);
        deliver(&rig);
        assert_eq!(
            rig.posts(),
            2 + turns_for_a_dropped_change,
            "{notice}: one retry, then quiet"
        );
        rig.idle(Duration::from_secs(60));
        assert_eq!(
            rig.posts(),
            2 + turns_for_a_dropped_change,
            "{notice}: a dropped change woke again"
        );
        assert_eq!(
            rig.delivered.borrow().last(),
            Some(&values(700)),
            "{notice}"
        );

        // The next notice tries it again.
        rig.drops.set(0);
        deliver(&rig);
        assert_eq!(
            rig.delivered.borrow().last(),
            Some(&values(800)),
            "{notice}"
        );
    }
}

/// A read in the owner-work part of a turn (a window's work), which runs no
/// application callback, wakes the owner for the change it finds, and the
/// application's turn delivers it.
#[test]
fn a_change_read_in_owner_work_wakes_the_owner() {
    let rig = Rig::settled(Host::AppKit);
    rig.change(values(650));
    let source = Arc::clone(&rig.source);
    rig.signal.run_for_window(Arc::new(move || {
        let _ = source.sample();
    }));
    rig.run_posted_turns();
    assert_eq!(rig.delivered.borrow().last(), Some(&values(650)));
    assert_eq!(
        rig.posts(),
        2,
        "the window's work, then the application's turn"
    );
}

/// A read off the owner thread is refused and changes nothing: the next read
/// on the owner thread reads and delivers.
#[test]
fn a_read_off_the_owner_thread_is_refused_and_changes_nothing() {
    let rig = Rig::settled(Host::AppKit);
    rig.change(values(750));
    rig.system().answer = Answer::Error;
    let refused = std::thread::scope(|scope| {
        scope
            .spawn(|| rig.source.sample())
            .join()
            .expect("the off-thread read returns")
    });
    assert!(
        matches!(refused, Err(PlatformError::Preferences { .. })),
        "{refused:?}"
    );
    assert_eq!(rig.system().reads, 0, "the system was read off the thread");
    assert_eq!(rig.source.retry_deadline(), None);
    assert_eq!(rig.posts(), 0);

    rig.system().answer = Answer::Values;
    rig.appkit_notification();
    assert_eq!(rig.delivered.borrow().last(), Some(&values(750)));
}

/// A panicking read is a failed read on every path, never a panic out of
/// the source: the Android loop's own read, which nothing contains, retries
/// like the AppKit turn does.
#[test]
fn a_panicking_read_is_a_failed_read() {
    let rig = Rig::settled(Host::Android);
    rig.system().answer = Answer::Panic;
    let read = rig.source.sample();
    assert!(
        matches!(&read, Err(PlatformError::Preferences { message }) if message.contains("the getter panicked")),
        "{read:?}"
    );
    assert_eq!(
        rig.source.retry_deadline(),
        Some(rig.system().now + FIRST_RETRY),
        "the panic owes a retry"
    );
}

/// A cold source has delivered nothing: its first read is a change, and an
/// error never manufactures a value. A value counts as delivered only once
/// its delivery is confirmed.
#[test]
fn a_value_is_accepted_only_once_delivered() {
    let now = web_time::Instant::now();
    let admission = ReadAdmission::default();
    let failed = admission.read_observation(now, || {
        Err(PlatformError::Preferences {
            message: "initial getter failure".into(),
        })
    });
    assert!(matches!(failed, Err(PlatformError::Preferences { .. })));
    assert!(matches!(
        admission.read_observation(now, || panic!("early native retry")),
        Err(PlatformError::PreferencesDeferred)
    ));
    let observation = values(700);
    let read = |at| {
        admission
            .read_observation(at, || Ok(observation.clone()))
            .expect("the system answers")
            .1
    };
    assert_eq!(read(now + FIRST_RETRY), Change::New, "a cold source");
    assert_eq!(read(now + FIRST_RETRY), Change::New, "read, not delivered");
    admission.owe(&observation);
    assert_eq!(
        read(now + FIRST_RETRY),
        Change::OwedBefore,
        "owed, not delivered"
    );
    let other = values(900);
    let (_, change) = admission
        .read_observation(now + FIRST_RETRY, || Ok(other.clone()))
        .expect("the system answers");
    assert_eq!(change, Change::New, "another value is owed afresh");
    admission.deliver(&observation);
    assert_eq!(read(now + FIRST_RETRY), Change::Unchanged, "delivered");
}

/// One read at a time; a failed or unwound read owes a retry, and a read
/// before it is due is deferred.
#[test]
fn bounded_native_read_admission() {
    let admission = ReadAdmission::default();
    let now = web_time::Instant::now();
    let failed = admission.begin(now).expect("first admission");
    assert!(matches!(
        admission.begin(now),
        Err(PlatformError::PreferencesDeferred)
    ));
    assert_eq!(
        admission.retry_deadline(),
        None,
        "a read in flight owes nothing yet"
    );
    drop(failed);
    assert_eq!(
        admission.retry_deadline(),
        Some(now + FIRST_RETRY),
        "a failed read owes a retry"
    );
    for elapsed in [0, 10, 100, 499] {
        assert!(matches!(
            admission.begin(now + Duration::from_millis(elapsed)),
            Err(PlatformError::PreferencesDeferred)
        ));
    }
    let answered = admission
        .begin(now + FIRST_RETRY)
        .expect("bounded retry")
        .answered(&values(500));
    assert_eq!(answered, Change::New);
    assert_eq!(
        admission.retry_deadline(),
        None,
        "an answered read owes nothing"
    );
    let mut caught = FirstPanic::new();
    let failure = caught.contain(|| {
        let _attempt = admission
            .begin(now + FIRST_RETRY)
            .expect("unwind admission");
        panic!("native sampling panic");
    });
    assert!(failure.is_none());
    if let Some(payload) = caught.into_payload() {
        discard_payload(payload);
    }
    assert_eq!(
        admission.retry_deadline(),
        Some(now + FIRST_RETRY * 2),
        "an unwound read owes a retry too"
    );
    assert!(matches!(
        admission.begin(now + FIRST_RETRY),
        Err(PlatformError::PreferencesDeferred)
    ));
    let _ = admission
        .begin(now + FIRST_RETRY * 2)
        .expect("recovery after unwind")
        .answered(&values(500));
}
