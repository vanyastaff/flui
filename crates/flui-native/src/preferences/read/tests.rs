//! The source the AppKit and Android hosts both are, driven through its
//! system seam by the owner signal and its turn, in virtual time.
//!
//! The turn reads the source as the application's turn does on both hosts
//! (`Platform::preferences` inside the turn callback) and delivers a value
//! that differs from the last it delivered. Each post of the signal is one
//! turn of the loop. A notice arrives the way each host hears it: AppKit's
//! notification runs [`NoticedPreferences::notice_handler`]; `FluiView`'s
//! report wakes the owner, and the Android loop notices and reads after the
//! turn; a configuration change is noticed and read by the Android loop with
//! no turn before it. AppKit retries on a timer; the Android loop reads on its
//! own only as [`LoopReads`] says, and blocks as [`poll_timeout`] says.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::GesturePreferences;

use super::*;
use crate::owner::android::poll::{LoopReads, LoopTurn, poll_timeout};
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
    /// The pause of each retry the source asked for, in order.
    pauses: Vec<Duration>,
}

impl NativePreferences for VirtualSystem {
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

    fn retry_admitted(&self) -> bool {
        self.state.lock().active
    }

    fn arm_retry(&self, delay: Duration, signal: Weak<OwnerSignal>) {
        let mut state = self.state.lock();
        state.pauses.push(delay);
        if state.host == Host::AppKit {
            let at = state.now + delay;
            state.armed.push((at, signal));
        }
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
        let slot = OwnerTurnSlot::default();
        signal
            .register_in(&slot, {
                let (source, delivered) = (Arc::clone(&source), Rc::clone(&delivered));
                Box::new(move || {
                    // The application's turn: read, and deliver what differs
                    // from what it holds.
                    if let Ok(current) = source.sample() {
                        let mut delivered = delivered.borrow_mut();
                        if delivered.last() != Some(&current) {
                            delivered.push(current);
                        }
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
        self.source.native().state.lock()
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

    /// The Android loop reads outside a turn; a read that panics is the
    /// rig's to contain, as the turn contains its own.
    fn loop_read(&self) {
        let mut caught = FirstPanic::new();
        let _ = caught.contain(|| self.source.sample());
        if let Some(payload) = caught.into_payload() {
            flui_foundation::panic::discard_payload(payload);
        }
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
    /// the loop notices and reads after the turn.
    fn android_view_report(&self) {
        let _ = self.signal.wake();
        self.run_posted_turns();
        self.source.notice();
        self.loop_read();
        self.run_posted_turns();
    }

    /// `ConfigChanged`: the loop notices and reads with no turn before it.
    fn android_configuration_change(&self) {
        self.source.notice();
        self.loop_read();
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

#[test]
fn cold_native_observation_recovers_without_forged_defaults() {
    let now = web_time::Instant::now();
    let cold = ReadAdmission::default();
    let (_, needs_delivery) = cold
        .read_observation(now, || Ok(SystemPreferences::default()))
        .expect("first legitimate unknown observation");
    assert!(
        needs_delivery,
        "an unknown observation is accepted only after the first successful query"
    );
    let reader = ReadAdmission::default();
    let failed = reader.read_observation(now, || {
        Err(PlatformError::Preferences {
            message: "initial getter failure".into(),
        })
    });
    assert!(matches!(failed, Err(PlatformError::Preferences { .. })));
    assert!(matches!(
        reader.read_observation(now, || panic!("early native retry")),
        Err(PlatformError::PreferencesDeferred)
    ));
    let observation = values(700);
    let (accepted, needs_delivery) = reader
        .read_observation(now + FIRST_RETRY, || Ok(observation.clone()))
        .expect("first accepted native observation");
    assert_eq!(accepted, observation);
    assert!(
        needs_delivery,
        "cold source must notify its first accepted observation"
    );
    let failed = reader.read_observation(now + FIRST_RETRY, || {
        Err(PlatformError::Preferences {
            message: "later getter failure".into(),
        })
    });
    assert!(
        matches!(failed, Err(PlatformError::Preferences { .. })),
        "failure cannot publish a stale success"
    );
    let (accepted, needs_delivery) = reader
        .read_observation(now + FIRST_RETRY * 2, || Ok(observation.clone()))
        .expect("recovered native observation");
    assert_eq!(accepted, observation);
    assert!(
        needs_delivery,
        "successful recovery must retry owner delivery even for an unchanged observation"
    );
    let (_, needs_delivery) = reader
        .read_observation(now + FIRST_RETRY * 2, || Ok(observation))
        .expect("healthy unchanged observation");
    assert!(
        !needs_delivery,
        "healthy unchanged observations remain quiet"
    );
}

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
    admission
        .begin(now + FIRST_RETRY)
        .expect("bounded retry")
        .accept();
    assert_eq!(
        admission.retry_deadline(),
        None,
        "an accepted read owes nothing"
    );
    admission
        .begin(now + FIRST_RETRY)
        .expect("healthy reads remain admitted")
        .accept();
    let mut caught = FirstPanic::new();
    let failure = caught.contain(|| {
        let _attempt = admission
            .begin(now + FIRST_RETRY)
            .expect("unwind admission");
        panic!("native sampling panic");
    });
    assert!(failure.is_none());
    if let Some(payload) = caught.into_payload() {
        flui_foundation::panic::discard_payload(payload);
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
    admission
        .begin(now + FIRST_RETRY * 2)
        .expect("recovery after unwind")
        .accept();
}
