//! The Linux host's system preferences: the Settings portal
//! (`org.freedesktop.portal.Settings`) on the session bus, observed for one
//! incarnation of the owner's loop. Both Linux hosts read it: the Wayland and
//! X11 hosts.
//!
//! D-Bus runs on the existing background executor. The owner only reads an
//! immutable accepted snapshot; no portal request runs in build/layout/paint.
//! The portal reports motion, contrast, text scale, the color scheme and
//! GNOME's clock format; the preferred languages and the regional formats set
//! apart from them (`LC_TIME`, `LC_MEASUREMENT`) come from the process
//! environment, read once, as gettext and glibc read them.

use std::{
    sync::{Arc, Weak, mpsc},
    time::Duration,
};

use flui_platform::{Brightness, HourCycle, MotionPreference, SystemPreferences};
use futures_util::StreamExt;
use parking_lot::Mutex;
use zbus::{OwnedVariant, Proxy};

use crate::{
    PlatformError, executor::NativeExecutor, owner::signal::OwnerSignal,
    owner::sources::SystemPreferenceSource, preferences::locales as preferred_locales,
    preferences::regional,
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME: &str = "org.gnome.desktop.interface";
const GNOME_A11Y: &str = "org.gnome.desktop.a11y.interface";
const READ_TIMEOUT: Duration = Duration::from_millis(200);
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_millis(500);

/// The retries of a refresh whose read failed in a way that passes by itself
/// (`heals_by_itself`): 1, 2, 4, 8 and 16 s after each failure, then none
/// until the portal next signals a change or changes hands.
const READ_RETRIES: Backoff = Backoff::new(Duration::from_secs(1), 5);

/// The retries of a session bus that does not answer: 1 to 32 s apart, the
/// last a minute after the first attempt, then the watch ends. A bus that
/// was connected and went is retried on the same schedule.
const CONNECT_RETRIES: Backoff = Backoff::new(Duration::from_secs(1), 6);

/// The pauses before the retries of something that failed: doubling from
/// the first, a fixed number of them, then none, so a failure that never
/// heals costs a bounded number of attempts and never a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Backoff {
    next: Duration,
    left: u32,
}

impl Backoff {
    const fn new(first: Duration, retries: u32) -> Self {
        Self {
            next: first,
            left: retries,
        }
    }

    /// The pause before the next retry; `None` once every retry is spent.
    fn next_pause(&mut self) -> Option<Duration> {
        self.left = self.left.checked_sub(1)?;
        let pause = self.next;
        self.next = self.next.saturating_mul(2);
        Some(pause)
    }
}

struct Observation {
    active: bool,
    accepted: SystemPreferences,
    /// The portal's `color-scheme`, kept beside the host-wide snapshot because
    /// brightness is presentation-specific (ADR-0172): each window reports it
    /// through the window state's `brightness`.
    appearance: Brightness,
}

/// The portal's observation and the task that keeps it, which a Linux host's loop
/// starts as it runs.
pub(crate) struct PreferenceSource {
    observation: Arc<Mutex<Observation>>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for PreferenceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferenceSource").finish_non_exhaustive()
    }
}

impl SystemPreferenceSource for PreferenceSource {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        Ok(self.observation.lock().accepted.clone())
    }
}

/// The native Linux loops start the portal's watch as they run, and their
/// windows follow the portal's colour scheme.
impl crate::owner::linux::LinuxPreferences for PreferenceSource {
    /// The accepted `color-scheme`; light until the portal says otherwise.
    fn appearance(&self) -> Brightness {
        self.observation.lock().appearance
    }
}

impl PreferenceSource {
    /// Start watching the portal on `executor`, waking the owner behind
    /// `signal` when what it accepted changes. Waits a bounded moment for the
    /// first answer, so the first sample usually has one.
    pub(crate) fn new(executor: &NativeExecutor, signal: &Arc<OwnerSignal>) -> Arc<Self> {
        // The environment does not change under a running process.
        let var = |name: &str| std::env::var(name).ok();
        let locales = preferred_locales::from_posix_environment(var);
        let regional =
            regional::from_posix_environment(var, locales.as_deref().and_then(<[_]>::first));
        let accepted = match locales {
            Some(locales) => SystemPreferences::default().with_locales(locales),
            None => SystemPreferences::default(),
        }
        .with_regional(regional);
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted,
            appearance: Brightness::Light,
        }));
        let (ready, receiver) = mpsc::sync_channel(1);
        let task = executor.handle().spawn(observe(
            Arc::clone(&observation),
            Arc::downgrade(signal),
            Some(ready),
        ));
        // Bootstrap is an IO edge before on_ready, not a frame callback. A
        // missing or stalled bus cannot prevent the host from starting. Its
        // observation remains unknown until a later successful owner update.
        let _ = receiver.recv_timeout(BOOTSTRAP_TIMEOUT);
        Arc::new(Self { observation, task })
    }
}

impl Drop for PreferenceSource {
    fn drop(&mut self) {
        self.observation.lock().active = false;
        self.task.abort();
    }
}

/// Commit an accepted value, then wake the owner if `accept` changed anything.
fn publish(
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    accept: impl FnOnce(&mut Observation) -> bool,
) {
    let changed = {
        let mut observation = observation.lock();
        observation.active && accept(&mut observation)
    };
    if changed && let Some(signal) = signal.upgrade() {
        // OwnerSignal retains delivery debt across missing/replaced hooks and
        // failed transport. It never invokes the UI callback on this worker.
        let _ = signal.wake();
    }
}

/// Accept the portal's text scale. A factor outside what a snapshot admits
/// is not an observation, and is reported once the observation is unlocked:
/// a `tracing` subscriber is the application's code, which may sample the
/// preferences as it hears the report.
fn accept_text_scale(observation: &Mutex<Observation>, signal: &Weak<OwnerSignal>, scale: f64) {
    let mut refusal = None;
    publish(observation, signal, |observation| {
        match observation.accepted.clone().with_text_scale(scale) {
            Ok(accepted) if accepted != observation.accepted => {
                observation.accepted = accepted;
                true
            }
            Ok(_) => false,
            Err(error) => {
                refusal = Some(error);
                false
            }
        }
    });
    if let Some(error) = refusal {
        tracing::warn!(scale, %error, "portal text-scaling-factor refused");
    }
}

/// Keep the observation for as long as a session bus answers: connect, and
/// connect again on [`CONNECT_RETRIES`]' schedule when the bus does not
/// answer or a connection ends. With no bus for that long the watch ends:
/// the observation stays what the environment and any earlier read gave.
async fn observe(
    observation: Arc<Mutex<Observation>>,
    signal: Weak<OwnerSignal>,
    ready: Option<mpsc::SyncSender<()>>,
) {
    observe_through(
        &observation,
        &signal,
        ready,
        session_connection,
        CONNECT_RETRIES,
    )
    .await;
}

/// [`observe`] over the connections `connect` makes, retried on `schedule`.
async fn observe_through<F>(
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    mut ready: Option<mpsc::SyncSender<()>>,
    mut connect: impl FnMut() -> F,
    schedule: Backoff,
) where
    F: Future<Output = zbus::Result<zbus::Connection>>,
{
    let mut retries = schedule;
    loop {
        let watched = match connect().await {
            Ok(connection) => {
                observe_connection(&connection, observation, signal, &mut ready).await
            }
            Err(error) => Err(error),
        };
        if watched.is_ok() {
            // A connection that was up and ended starts the schedule over.
            retries = schedule;
        }
        if let Some(ready) = ready.take() {
            let _ = ready.send(());
        }
        let Some(pause) = retries.next_pause() else {
            return;
        };
        tokio::time::sleep(pause).await;
    }
}

/// A connection to the session bus, or why there is none within
/// [`BOOTSTRAP_TIMEOUT`].
async fn session_connection() -> zbus::Result<zbus::Connection> {
    tokio::time::timeout(BOOTSTRAP_TIMEOUT, async {
        zbus::connection::Builder::session()
            .method_timeout(READ_TIMEOUT)
            .build()
            .await
    })
    .await
    .map_err(|_| zbus::Error::Failure("session bus connection timed out".into()))?
}

/// Watch the portal over `connection` until it ends (`Ok`), or fail to
/// subscribe to it (`Err`).
async fn observe_connection(
    connection: &zbus::Connection,
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    ready: &mut Option<mpsc::SyncSender<()>>,
) -> zbus::Result<()> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await?;
    // Both subscriptions precede the read: a setting change or a replacement
    // service during bootstrap remains queued for another observation.
    let mut changes = proxy.receive_signal("SettingChanged").await?;
    let mut owners = proxy.receive_owner_changed().await?;
    let mut retry = ReadRetry::new();
    retry.settle(refresh(&proxy, observation, signal).await);
    if let Some(ready) = ready.take() {
        let _ = ready.send(());
    }
    loop {
        // A portal with no Settings backend, without a key, or with no owner
        // at all is not read again until it signals or changes hands.
        let due = retry.due;
        tokio::select! {
            message = changes.next() => {
                let Some(message) = message else { return Ok(()); };
                let body = message.body();
                let Ok((namespace, key, _)) = body.deserialize::<(&str, &str, OwnedVariant)>() else {
                    continue;
                };
                if matches!(
                    (namespace, key),
                    (APPEARANCE, "reduced-motion" | "color-scheme" | "contrast")
                        | (GNOME, "enable-animations" | "text-scaling-factor" | "clock-format")
                        | (GNOME_A11Y, "high-contrast")
                ) {
                    retry = ReadRetry::new();
                    retry.settle(refresh(&proxy, observation, signal).await);
                }
            }
            owner = owners.next() => {
                if owner.is_none() { return Ok(()); }
                retry = ReadRetry::new();
                retry.settle(refresh(&proxy, observation, signal).await);
            }
            () = sleep_until(due), if due.is_some() => {
                retry.settle(refresh(&proxy, observation, signal).await);
            }
        }
    }
}

/// Sleep until `due`; never, for `None`, which the `select!` above guards.
async fn sleep_until(due: Option<tokio::time::Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

/// When the watch reads the portal again after a refresh: only after a read
/// that failed in a way that passes by itself, on [`READ_RETRIES`]' schedule,
/// which a signal or a change of owner starts over.
#[derive(Debug)]
struct ReadRetry {
    backoff: Backoff,
    due: Option<tokio::time::Instant>,
}

impl ReadRetry {
    const fn new() -> Self {
        Self {
            backoff: READ_RETRIES,
            due: None,
        }
    }

    /// Schedule the next read from the refresh that just ran.
    fn settle(&mut self, refreshed: Refreshed) {
        self.due = match refreshed {
            Refreshed::Settled => None,
            Refreshed::Retry => self
                .backoff
                .next_pause()
                .map(|pause| tokio::time::Instant::now() + pause),
        };
    }
}

/// What a refresh leaves to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refreshed {
    /// Every read answered, or failed in a way only the portal's next signal
    /// or owner can change: a missing key, a portal without Settings, no
    /// portal on the bus.
    Settled,
    /// A read failed in a way that passes by itself.
    Retry,
}

impl Refreshed {
    /// What the refresh whose reads failed with `errors` leaves to do.
    fn of<'a>(errors: impl IntoIterator<Item = Option<&'a zbus::Error>>) -> Self {
        if errors.into_iter().flatten().any(heals_by_itself) {
            Self::Retry
        } else {
            Self::Settled
        }
    }
}

/// Whether a failed read can succeed on its own if asked again: the portal or
/// its backend took too long or failed for a moment. A service that is not
/// on the bus is waited for through `NameOwnerChanged`, and a method or key
/// the portal lacks stays lacking.
fn heals_by_itself(error: &zbus::Error) -> bool {
    match error {
        // `method_timeout` elapsed.
        zbus::Error::InputOutput(error) => error.kind() == std::io::ErrorKind::TimedOut,
        zbus::Error::FDO(error) => matches!(
            **error,
            zbus::fdo::Error::NoReply(_)
                | zbus::fdo::Error::Timeout(_)
                | zbus::fdo::Error::TimedOut(_)
        ),
        zbus::Error::MethodError(name, _, _) => {
            name.as_str() == "org.freedesktop.portal.Error.Failed"
        }
        _ => false,
    }
}

/// Read every observed setting. Each accepted value is published on its own,
/// so one failing read neither discards nor delays the other.
async fn refresh(
    proxy: &Proxy<'_>,
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
) -> Refreshed {
    let motion = read_motion(proxy).await;
    if let Ok(Some(motion)) = motion {
        publish(observation, signal, |observation| {
            let changed = observation.accepted.motion() != Some(motion);
            if changed {
                observation.accepted = observation.accepted.clone().with_motion(motion);
            }
            changed
        });
    }
    let appearance = read_appearance(proxy).await;
    if let Ok(appearance) = appearance {
        publish(observation, signal, |observation| {
            std::mem::replace(&mut observation.appearance, appearance) != appearance
        });
    }
    let contrast = read_contrast(proxy).await;
    if let Ok(Some(high_contrast)) = contrast {
        publish(observation, signal, |observation| {
            let changed = observation.accepted.high_contrast() != Some(high_contrast);
            if changed {
                observation.accepted = observation
                    .accepted
                    .clone()
                    .with_high_contrast(high_contrast);
            }
            changed
        });
    }
    let text_scale = read_text_scale(proxy).await;
    if let Ok(Some(scale)) = text_scale {
        accept_text_scale(observation, signal, scale);
    }
    let clock = read_clock_format(proxy).await;
    if let Ok(Some(hour_cycle)) = clock {
        publish(observation, signal, |observation| {
            let changed = observation.accepted.regional().hour_cycle() != Some(hour_cycle);
            if changed {
                let regional = observation
                    .accepted
                    .regional()
                    .clone()
                    .with_hour_cycle(hour_cycle);
                observation.accepted = observation.accepted.clone().with_regional(regional);
            }
            changed
        });
    }
    Refreshed::of([
        motion.as_ref().err(),
        appearance.as_ref().err(),
        contrast.as_ref().err(),
        text_scale.as_ref().err(),
        clock.as_ref().err(),
    ])
}

/// GNOME's `clock-format`; a desktop without the key, or with a value it
/// does not name, has no observation.
async fn read_clock_format(proxy: &Proxy<'_>) -> zbus::Result<Option<HourCycle>> {
    match read(proxy, GNOME, "clock-format").await {
        Ok(value) => Ok(regional::gnome_clock_format(&String::try_from(value)?)),
        Err(error) if missing_key(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `contrast`: 1 asks for higher contrast, 0 has no preference. Without that
/// key GNOME's `high-contrast` answers; with neither there is no observation,
/// and nothing to retry.
async fn read_contrast(proxy: &Proxy<'_>) -> zbus::Result<Option<bool>> {
    match read(proxy, APPEARANCE, "contrast").await {
        Ok(value) => Ok(Some(u32::try_from(value)? == 1)),
        Err(error) if missing_key(&error) => match read(proxy, GNOME_A11Y, "high-contrast").await {
            Ok(value) => Ok(Some(bool::try_from(value)?)),
            Err(error) if missing_key(&error) => Ok(None),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

/// GNOME's `text-scaling-factor`, which GTK applies to every font size alike;
/// a desktop without the key has no observation.
async fn read_text_scale(proxy: &Proxy<'_>) -> zbus::Result<Option<f64>> {
    match read(proxy, GNOME, "text-scaling-factor").await {
        Ok(value) => Ok(Some(f64::try_from(value)?)),
        Err(error) if missing_key(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `color-scheme`: 1 prefers dark; 0 (no preference) and 2 (prefers light)
/// are light, as is a portal without the key.
async fn read_appearance(proxy: &Proxy<'_>) -> zbus::Result<Brightness> {
    match read(proxy, APPEARANCE, "color-scheme").await {
        Ok(value) => Ok(if u32::try_from(value)? == 1 {
            Brightness::Dark
        } else {
            Brightness::Light
        }),
        Err(error) if missing_key(&error) => Ok(Brightness::Light),
        Err(error) => Err(error),
    }
}

/// `reduced-motion`: 1 asks for reduced motion, anything else has no
/// preference. Without that key GNOME's `enable-animations` answers; with
/// neither there is no observation, and nothing to retry. A read that fails
/// otherwise keeps what was accepted before.
async fn read_motion(proxy: &Proxy<'_>) -> zbus::Result<Option<MotionPreference>> {
    match read(proxy, APPEARANCE, "reduced-motion").await {
        Ok(value) => Ok(Some(match u32::try_from(value)? {
            1 => MotionPreference::Reduce,
            _ => MotionPreference::NoPreference,
        })),
        Err(error) if missing_key(&error) => match read(proxy, GNOME, "enable-animations").await {
            Ok(value) => Ok(Some(if bool::try_from(value)? {
                MotionPreference::NoPreference
            } else {
                MotionPreference::Reduce
            })),
            Err(error) if missing_key(&error) => Ok(None),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

/// Whether the portal has no value for the key. The portal's own `NotFound`
/// arrives as a method error; a name `fdo::Error` defines arrives as that type.
fn missing_key(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::MethodError(name, _, _) => {
            name.as_str() == "org.freedesktop.portal.Error.NotFound"
        }
        zbus::Error::FDO(error) => matches!(**error, zbus::fdo::Error::UnknownProperty(_)),
        _ => false,
    }
}

async fn read(proxy: &Proxy<'_>, namespace: &str, key: &str) -> zbus::Result<OwnedVariant> {
    match proxy.call("ReadOne", &(namespace, key)).await {
        Err(zbus::Error::FDO(error)) if matches!(*error, zbus::fdo::Error::UnknownMethod(_)) => {
            // Settings v1's deprecated Read wraps its value in two variants.
            let value: OwnedVariant = proxy.call("Read", &(namespace, key)).await?;
            match zbus::Variant::from(value) {
                zbus::Variant::Variant(inner) => Ok(OwnedVariant::try_from(*inner)?),
                _ => Err(zbus::Error::Failure("invalid legacy Settings value".into())),
            }
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Counts the diagnostics emitted, and those emitted while the
    /// observation was locked: a subscriber that sampled the preferences then
    /// would wait on that lock for good.
    struct LockedAtDiagnostic {
        observation: Arc<Mutex<Observation>>,
        events: Arc<AtomicUsize>,
        locked: Arc<AtomicUsize>,
    }

    impl tracing::Subscriber for LockedAtDiagnostic {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            self.events.fetch_add(1, Ordering::SeqCst);
            if self.observation.try_lock().is_none() {
                self.locked.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// Each schedule doubles its pause and runs out: a failure that never
    /// heals costs a bounded number of attempts.
    #[test]
    fn a_backoff_doubles_and_runs_out() {
        let pauses = |mut backoff: Backoff| {
            std::iter::from_fn(move || backoff.next_pause())
                .map(|pause| pause.as_secs())
                .collect::<Vec<u64>>()
        };
        assert_eq!(pauses(READ_RETRIES), [1, 2, 4, 8, 16]);
        assert_eq!(pauses(CONNECT_RETRIES), [1, 2, 4, 8, 16, 32]);
        assert_eq!(pauses(Backoff::new(Duration::MAX, 2)), [u64::MAX, u64::MAX]);
    }

    /// A session bus that never answers is asked once and then once per
    /// retry of its schedule, and the watch ends; the host's start hears of
    /// the first failure at once. Red-check: sleep a fixed pause and loop.
    #[test]
    fn a_bus_that_never_answers_is_asked_a_bounded_number_of_times() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a test runtime");
        let observation = Mutex::new(Observation {
            active: true,
            accepted: SystemPreferences::default(),
            appearance: Brightness::Light,
        });
        let attempts = AtomicUsize::new(0);
        let (ready, started) = mpsc::sync_channel(1);
        let signal = Weak::new();
        let watch = observe_through(
            &observation,
            &signal,
            Some(ready),
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err(zbus::Error::Failure("no session bus".into())))
            },
            Backoff::new(Duration::from_millis(1), 6),
        );
        let ended =
            runtime.block_on(async { tokio::time::timeout(Duration::from_secs(5), watch).await });
        assert!(ended.is_ok(), "the watch ends once its retries are spent");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            7,
            "one attempt and six retries"
        );
        assert_eq!(started.try_recv(), Ok(()), "the start heard of the failure");
    }

    /// Only a failure that can pass by itself is retried: a missing service
    /// waits for its owner, a missing method stays missing.
    #[test]
    fn only_a_failure_that_passes_by_itself_is_retried() {
        use zbus::fdo::Error as Fdo;
        let fdo = |error: Fdo| zbus::Error::FDO(Box::new(error));
        let cases = [
            (
                zbus::Error::InputOutput(Arc::new(std::io::ErrorKind::TimedOut.into())),
                true,
            ),
            (fdo(Fdo::NoReply(String::new())), true),
            (fdo(Fdo::Timeout(String::new())), true),
            (fdo(Fdo::TimedOut(String::new())), true),
            (fdo(Fdo::ServiceUnknown(String::new())), false),
            (fdo(Fdo::NameHasNoOwner(String::new())), false),
            (fdo(Fdo::UnknownMethod(String::new())), false),
            (fdo(Fdo::UnknownInterface(String::new())), false),
            (
                zbus::Error::InputOutput(Arc::new(std::io::ErrorKind::BrokenPipe.into())),
                false,
            ),
            (zbus::Error::IncorrectType, false),
        ];
        for (error, retried) in cases {
            assert_eq!(heals_by_itself(&error), retried, "{error}");
            let expected = if retried {
                Refreshed::Retry
            } else {
                Refreshed::Settled
            };
            assert_eq!(
                Refreshed::of([None, Some(&error), None]),
                expected,
                "{error}"
            );
        }
    }

    /// A text scale the snapshot refuses is reported with the observation
    /// unlocked, and leaves what was accepted before as it was.
    #[test]
    fn a_refused_text_scale_is_reported_with_the_observation_unlocked() {
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted: SystemPreferences::default(),
            appearance: Brightness::Light,
        }));
        let events = Arc::new(AtomicUsize::new(0));
        let locked = Arc::new(AtomicUsize::new(0));
        let subscriber = LockedAtDiagnostic {
            observation: Arc::clone(&observation),
            events: Arc::clone(&events),
            locked: Arc::clone(&locked),
        };
        tracing::subscriber::with_default(subscriber, || {
            accept_text_scale(&observation, &Weak::new(), 1.5);
            accept_text_scale(&observation, &Weak::new(), f64::NAN);
        });
        assert_eq!(events.load(Ordering::SeqCst), 1, "the refusal is reported");
        assert_eq!(locked.load(Ordering::SeqCst), 0, "reported under the lock");
        assert_eq!(observation.lock().accepted.text_scale(), Some(1.5));
    }
}
