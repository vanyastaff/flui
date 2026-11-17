//! The Linux host's system preferences: the Settings portal
//! (`org.freedesktop.portal.Settings`) on the session bus, observed for one
//! incarnation of the owner's loop. Both Linux hosts read it: the Wayland and
//! X11 hosts.
//!
//! D-Bus runs on the existing background executor. The owner only reads an
//! immutable accepted snapshot; no portal request runs in build/layout/paint.
//! The portal reports motion, contrast, text scale, the color scheme, the
//! accent colour and GNOME's clock format and overlay scrolling; the preferred languages and the regional formats set
//! apart from them (`LC_TIME`, `LC_MEASUREMENT`) come from the process
//! environment, read once, as gettext and glibc read them.

use std::{
    sync::{Arc, Weak, mpsc},
    time::Duration,
};

use flui_platform::{
    Brightness, HourCycle, MotionPreference, PreferenceParts, ScrollBarVisibility, SrgbColor,
    SystemPreferences,
};
use futures_util::StreamExt;
use parking_lot::Mutex;
use zbus::{OwnedVariant, Proxy};

use crate::{
    PlatformError, executor::NativeExecutor, owner::signal::OwnerSignal,
    owner::sources::SystemPreferenceSource, preferences::locales as preferred_locales,
    preferences::regional, preferences::retries::Retries,
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME: &str = "org.gnome.desktop.interface";
const GNOME_A11Y: &str = "org.gnome.desktop.a11y.interface";
const READ_TIMEOUT: Duration = Duration::from_millis(200);
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_millis(500);

/// The retries of a refresh whose read failed in a way that passes by itself
/// (`heals_by_itself`), or that the portal answered with no value at all:
/// 1, 2, 4, 8 and 16 s after each, then none until the portal next signals a
/// change or changes hands.
const READ_RETRIES: Retries = Retries::new(Duration::from_secs(1), 5);

/// The retries of a session bus that does not answer, or of a connection that
/// ended before the portal answered on it: 1 to 32 s apart, the last a minute
/// after the first attempt, then the watch ends. A connection on which the
/// portal answered starts the schedule over when it ends.
const CONNECT_RETRIES: Retries = Retries::new(Duration::from_secs(1), 6);

/// When the watch connects again: on its schedule after a connection that
/// failed or ended with the portal never answering on it, and on a fresh
/// schedule after one on which the portal answered. A bus that accepts a
/// client and drops it costs the schedule's attempts, not a reconnection a
/// second for good.
#[derive(Debug, Clone, Copy)]
struct ConnectRetry {
    schedule: Retries,
    spent: u32,
}

impl ConnectRetry {
    const fn new(schedule: Retries) -> Self {
        Self { schedule, spent: 0 }
    }

    /// The pause before the next connection, after one on which the portal
    /// answered a read (`answered`) or not; `None` once the schedule is spent.
    fn pause_after(&mut self, answered: bool) -> Option<Duration> {
        if answered {
            self.spent = 0;
        }
        let pause = self.schedule.pause(self.spent)?;
        self.spent += 1;
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
    /// The hour cycle `LC_TIME` gives, which a portal without GNOME's
    /// `clock-format` leaves in force.
    environment_hour_cycle: Option<HourCycle>,
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
        let environment_hour_cycle = regional.hour_cycle();
        let accepted = match locales {
            Some(locales) => SystemPreferences::default().with_locales(locales),
            None => SystemPreferences::default(),
        }
        .with_regional(regional);
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted,
            appearance: Brightness::Light,
            environment_hour_cycle,
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
    schedule: Retries,
) where
    F: Future<Output = zbus::Result<zbus::Connection>>,
{
    let mut retries = ConnectRetry::new(schedule);
    loop {
        let mut answered = false;
        if let Ok(connection) = connect().await {
            // A connection that cannot subscribe is a failed attempt like one
            // that does not connect.
            let _: zbus::Result<()> =
                observe_connection(&connection, observation, signal, &mut ready, &mut answered)
                    .await;
        }
        if let Some(ready) = ready.take() {
            let _ = ready.send(());
        }
        let Some(pause) = retries.pause_after(answered) else {
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
/// subscribe to it (`Err`); `answered` is set once a refresh read a value.
async fn observe_connection(
    connection: &zbus::Connection,
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    ready: &mut Option<mpsc::SyncSender<()>>,
    answered: &mut bool,
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
    let mut refreshed = |refresh: Refreshed| {
        *answered |= refresh.answered;
        refresh
    };
    retry.settle(refreshed(
        refresh(&proxy, observation, signal, MissingKey::Withdraws).await,
    ));
    if let Some(ready) = ready.take() {
        let _ = ready.send(());
    }
    loop {
        // A portal without the Settings interface, without a key or with no
        // owner at all is not read again until it signals or changes hands.
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
                    (APPEARANCE, "reduced-motion" | "color-scheme" | "contrast" | "accent-color")
                        | (
                            GNOME,
                            "enable-animations"
                                | "text-scaling-factor"
                                | "clock-format"
                                | "overlay-scrolling"
                        )
                        | (GNOME_A11Y, "high-contrast")
                ) {
                    retry = ReadRetry::new();
                    retry.settle(refreshed(refresh(&proxy, observation, signal, MissingKey::Keeps).await));
                }
            }
            owner = owners.next() => {
                if owner.is_none() { return Ok(()); }
                retry = ReadRetry::new();
                retry.settle(refreshed(refresh(&proxy, observation, signal, MissingKey::Withdraws).await));
            }
            () = sleep_until(due) => {
                retry.settle(refreshed(refresh(&proxy, observation, signal, MissingKey::Keeps).await));
            }
        }
    }
}

/// Sleep until `due`; never, for `None`: no read is due.
async fn sleep_until(due: Option<tokio::time::Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

/// When the watch reads the portal again after a refresh: only after one
/// that leaves a read to retry ([`NextRead::Retry`]), on [`READ_RETRIES`]'
/// schedule, which a signal or a change of owner starts over.
#[derive(Debug)]
struct ReadRetry {
    spent: u32,
    due: Option<tokio::time::Instant>,
}

impl ReadRetry {
    const fn new() -> Self {
        Self {
            spent: 0,
            due: None,
        }
    }

    /// Schedule the next read from the refresh that just ran.
    fn settle(&mut self, refreshed: Refreshed) {
        self.due = match refreshed.next {
            NextRead::Settled => None,
            NextRead::Retry => READ_RETRIES.pause(self.spent).map(|pause| {
                self.spent += 1;
                tokio::time::Instant::now() + pause
            }),
        };
    }
}

/// What a refresh found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refreshed {
    /// The portal answered at least one setting with a value.
    answered: bool,
    next: NextRead,
}

/// Whether a refresh leaves a read to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextRead {
    /// Every read answered, or failed in a way only the portal's next signal
    /// or owner can change: a key missing beside others that answer, a portal
    /// without the Settings interface, no portal on the bus.
    Settled,
    /// A read failed in a way that passes by itself, or the portal answered
    /// no setting at all. The portal folds a backend that has not started or
    /// has gone into `NotFound` for every key, and its return changes no
    /// owner on the bus and sends no signal, so only a read again finds it.
    Retry,
}

impl Refreshed {
    /// What a refresh whose reads failed with `errors` found, where the
    /// portal answered at least one setting with a value (`answered`) or
    /// none.
    fn of<'a>(errors: impl IntoIterator<Item = Option<&'a zbus::Error>>, answered: bool) -> Self {
        let mut failed = false;
        for error in errors.into_iter().flatten() {
            if heals_by_itself(error) {
                return Self {
                    answered,
                    next: NextRead::Retry,
                };
            }
            failed = true;
        }
        let next = if answered || failed {
            NextRead::Settled
        } else {
            NextRead::Retry
        };
        Self { answered, next }
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

/// Whether a key the portal answers it does not have keeps or clears what was
/// accepted from it before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingKey {
    /// The portal that answered before answers now: a key it lacks for a
    /// moment (`NotFound` while its backend restarts) keeps its value.
    Keeps,
    /// The portal changed hands or the watch connected anew: the backend now
    /// answering is the one to believe, so a key it lacks has no observation.
    Withdraws,
}

/// Publish the snapshot `edit` makes of the accepted one, if it differs.
/// Parts taken from an accepted snapshot pass its checks again; a text scale
/// the snapshot refuses goes through `accept_text_scale`, which reports it.
fn publish_parts(
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    edit: impl FnOnce(&mut PreferenceParts),
) {
    publish(observation, signal, |observation| {
        let mut parts = observation.accepted.clone().into_parts();
        edit(&mut parts);
        match parts.try_into_preferences() {
            Ok(next) if next != observation.accepted => {
                observation.accepted = next;
                true
            }
            _ => false,
        }
    });
}

/// Take one key's read: a value replaces the field it feeds; a key the portal
/// lacks clears that field under [`MissingKey::Withdraws`] and keeps it
/// otherwise; a failed read keeps it, for a retry to settle.
fn take_key<T: Copy>(
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    read: &zbus::Result<Option<T>>,
    missing: MissingKey,
    set: impl FnOnce(&mut PreferenceParts, Option<T>),
) {
    let value = match read {
        Ok(Some(value)) => Some(*value),
        Ok(None) if missing == MissingKey::Withdraws => None,
        Ok(None) | Err(_) => return,
    };
    publish_parts(observation, signal, |parts| set(parts, value));
}

/// Read every observed setting. Each accepted value is published on its own,
/// so one failing read neither discards nor delays the other.
async fn refresh(
    proxy: &Proxy<'_>,
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    missing: MissingKey,
) -> Refreshed {
    let motion = read_motion(proxy).await;
    take_key(observation, signal, &motion, missing, |parts, motion| {
        parts.motion = motion;
    });
    let appearance = read_appearance(proxy).await;
    if let Ok(scheme) = appearance {
        // A portal without the key prefers neither: light.
        let appearance = scheme.unwrap_or(Brightness::Light);
        publish(observation, signal, |observation| {
            std::mem::replace(&mut observation.appearance, appearance) != appearance
        });
    }
    let contrast = read_contrast(proxy).await;
    take_key(observation, signal, &contrast, missing, |parts, high| {
        parts.high_contrast = high;
    });
    let text_scale = read_text_scale(proxy).await;
    match text_scale {
        Ok(Some(scale)) => accept_text_scale(observation, signal, scale),
        Ok(None) if missing == MissingKey::Withdraws => {
            publish_parts(observation, signal, |parts| parts.text_scale = None);
        }
        Ok(None) | Err(_) => {}
    }
    let clock = read_clock_format(proxy).await;
    // Without GNOME's clock format the hour cycle is the environment's again.
    let environment_hour_cycle = observation.lock().environment_hour_cycle;
    take_key(observation, signal, &clock, missing, |parts, hour_cycle| {
        parts.regional.hour_cycle = hour_cycle.or(environment_hour_cycle);
    });
    let accent = read_accent(proxy).await;
    take_key(observation, signal, &accent, missing, |parts, accent| {
        parts.accent_color = accent.and_then(PortalAccent::color);
    });
    let scroll_bars = read_scroll_bar_visibility(proxy).await;
    take_key(
        observation,
        signal,
        &scroll_bars,
        missing,
        |parts, visibility| {
            parts.scroll_bars.visibility = visibility;
        },
    );
    let answered = matches!(motion, Ok(Some(_)))
        || matches!(appearance, Ok(Some(_)))
        || matches!(contrast, Ok(Some(_)))
        || matches!(text_scale, Ok(Some(_)))
        || matches!(clock, Ok(Some(_)))
        || matches!(accent, Ok(Some(_)))
        || matches!(scroll_bars, Ok(Some(_)));
    Refreshed::of(
        [
            motion.as_ref().err(),
            appearance.as_ref().err(),
            contrast.as_ref().err(),
            text_scale.as_ref().err(),
            clock.as_ref().err(),
            accent.as_ref().err(),
            scroll_bars.as_ref().err(),
        ],
        answered,
    )
}

/// What the portal's `accent-color` says.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PortalAccent {
    /// The user chose this colour.
    Color(SrgbColor),
    /// The user chose none: the portal spells it as components outside
    /// `0..=1`.
    Unset,
}

impl PortalAccent {
    /// The accent of an `accent-color` value: red, green and blue in sRGB,
    /// opaque. The Settings portal's specification makes any component
    /// outside `0..=1` mean "unset", so that, a NaN included, is no accent
    /// rather than a failed read.
    fn of((red, green, blue): (f64, f64, f64)) -> Self {
        SrgbColor::try_new(red, green, blue, 1.0).map_or(Self::Unset, Self::Color)
    }

    /// The colour the snapshot records: none for "unset".
    const fn color(self) -> Option<SrgbColor> {
        match self {
            Self::Color(color) => Some(color),
            Self::Unset => None,
        }
    }
}

/// `accent-color` (GNOME 47, KDE Plasma 6); a desktop without the key has no
/// observation.
async fn read_accent(proxy: &Proxy<'_>) -> zbus::Result<Option<PortalAccent>> {
    match read(proxy, APPEARANCE, "accent-color").await {
        Ok(value) => Ok(Some(PortalAccent::of(<(f64, f64, f64)>::try_from(value)?))),
        Err(error) if missing_key(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// GNOME's `overlay-scrolling`: scroll bars that show while the content
/// scrolls, or always; a desktop without the key has no observation. GNOME
/// has no track-click setting the portal carries (GTK's
/// `gtk-primary-button-warps-slider` is an XSETTING), so that stays
/// unobserved.
async fn read_scroll_bar_visibility(
    proxy: &Proxy<'_>,
) -> zbus::Result<Option<ScrollBarVisibility>> {
    match read(proxy, GNOME, "overlay-scrolling").await {
        Ok(value) => Ok(Some(overlay_scrolling(bool::try_from(value)?))),
        Err(error) if missing_key(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn overlay_scrolling(overlay: bool) -> ScrollBarVisibility {
    if overlay {
        ScrollBarVisibility::WhenScrolling
    } else {
        ScrollBarVisibility::Always
    }
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
/// key GNOME's `high-contrast` answers; with neither there is no observation.
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
/// are light; a portal without the key has no observation.
async fn read_appearance(proxy: &Proxy<'_>) -> zbus::Result<Option<Brightness>> {
    match read(proxy, APPEARANCE, "color-scheme").await {
        Ok(value) => Ok(Some(if u32::try_from(value)? == 1 {
            Brightness::Dark
        } else {
            Brightness::Light
        })),
        Err(error) if missing_key(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `reduced-motion`: 1 asks for reduced motion, anything else has no
/// preference. Without that key GNOME's `enable-animations` answers; with
/// neither there is no observation. A read that fails
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

    /// A read is retried 1 to 16 s apart, a connection 1 to 32 s apart, and
    /// both run out.
    #[test]
    fn the_watchs_retries_are_bounded() {
        let pauses = |retries: Retries| {
            (0..10)
                .map_while(|spent| retries.pause(spent))
                .map(|pause| pause.as_secs())
                .collect::<Vec<u64>>()
        };
        assert_eq!(pauses(READ_RETRIES), [1, 2, 4, 8, 16]);
        assert_eq!(pauses(CONNECT_RETRIES), [1, 2, 4, 8, 16, 32]);
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
            environment_hour_cycle: None,
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
            Retries::new(Duration::from_millis(1), 6),
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
                NextRead::Retry
            } else {
                NextRead::Settled
            };
            for answered in [false, true] {
                assert_eq!(
                    Refreshed::of([None, Some(&error), None], answered).next,
                    expected,
                    "{error}, answered: {answered}"
                );
            }
        }
    }

    /// A portal that answers no setting at all is read again, since the
    /// portal answers `NotFound` for every key while its backend is not up;
    /// one that answers some settings and lacks the others is settled.
    #[test]
    fn a_portal_that_answers_nothing_is_read_again() {
        assert_eq!(Refreshed::of([None, None], false).next, NextRead::Retry);
        assert_eq!(Refreshed::of([None, None], true).next, NextRead::Settled);
        let missing = zbus::Error::FDO(Box::new(zbus::fdo::Error::ServiceUnknown(String::new())));
        assert_eq!(
            Refreshed::of([Some(&missing), None], false).next,
            NextRead::Settled,
            "a portal not on the bus is waited for through its owner"
        );
        let mut retry = ReadRetry::new();
        let nothing = Refreshed::of([None], false);
        let reads = std::iter::from_fn(|| {
            retry.settle(nothing);
            retry.due
        })
        .take(10)
        .count();
        assert_eq!(reads, 5, "each read again is one of the schedule's retries");
    }

    /// A connection that ends before the portal answered on it costs one of
    /// the schedule's retries, so a bus that accepts a client and drops it is
    /// not reconnected a second for good; one on which the portal answered
    /// starts the schedule over. Red-check: start over after every ended
    /// connection.
    #[test]
    fn only_a_connection_the_portal_answered_on_starts_the_schedule_over() {
        let mut retries = ConnectRetry::new(Retries::new(Duration::from_secs(1), 3));
        let pauses = |retries: &mut ConnectRetry, answered: &[bool]| {
            answered
                .iter()
                .map(|&answered| retries.pause_after(answered).map(|pause| pause.as_secs()))
                .collect::<Vec<Option<u64>>>()
        };
        assert_eq!(
            pauses(&mut retries, &[false, false, false, false]),
            [Some(1), Some(2), Some(4), None]
        );
        assert_eq!(
            pauses(&mut retries, &[true, false]),
            [Some(1), Some(2)],
            "a connection the portal answered on starts the schedule over"
        );
    }

    /// The portal's accent is an opaque sRGB colour while every component
    /// is in `0..=1`; any component outside that range, NaN included, is the
    /// portal's "unset" and clears an accent accepted before. Red-check: keep
    /// the previous accent on "unset", or clamp an out-of-range component.
    #[test]
    fn the_portal_accent_is_a_colour_in_range_and_unset_outside_it() {
        let blue = SrgbColor::try_new(0.21, 0.52, 0.89, 1.0).expect("in range");
        assert_eq!(
            PortalAccent::of((0.21, 0.52, 0.89)),
            PortalAccent::Color(blue)
        );
        assert_eq!(
            PortalAccent::of((0.0, 1.0, 0.0)),
            PortalAccent::Color(SrgbColor::try_new(0.0, 1.0, 0.0, 1.0).expect("in range"))
        );
        for unset in [
            (-1.0, -1.0, -1.0),
            (0.5, 1.5, 0.5),
            (f64::NAN, 0.5, 0.5),
            (0.5, 0.5, f64::INFINITY),
        ] {
            assert_eq!(PortalAccent::of(unset), PortalAccent::Unset, "{unset:?}");
        }
        assert_eq!(PortalAccent::Color(blue).color(), Some(blue));
        assert_eq!(PortalAccent::Unset.color(), None, "unset clears the accent");
    }

    /// A key's value replaces its field; a key the portal lacks keeps the
    /// field while the same portal answers and clears it once the portal
    /// changed hands; a failed read keeps it. Red-check: publish nothing for a
    /// missing key whatever the portal, or clear on a failed read.
    #[test]
    fn a_missing_key_withdraws_its_field_only_after_the_portal_changed_hands() {
        let blue = SrgbColor::try_new(0.21, 0.52, 0.89, 1.0).expect("in range");
        let observation = Mutex::new(Observation {
            active: true,
            accepted: SystemPreferences::default(),
            appearance: Brightness::Light,
            environment_hour_cycle: Some(HourCycle::H23),
        });
        let accent = |observation: &Mutex<Observation>| observation.lock().accepted.accent_color();
        let set_accent = |parts: &mut PreferenceParts, accent: Option<PortalAccent>| {
            parts.accent_color = accent.and_then(PortalAccent::color);
        };
        let signal = Weak::new();
        let answered: zbus::Result<Option<PortalAccent>> = Ok(Some(PortalAccent::Color(blue)));
        take_key(
            &observation,
            &signal,
            &answered,
            MissingKey::Keeps,
            set_accent,
        );
        assert_eq!(accent(&observation), Some(blue));
        let failed: zbus::Result<Option<PortalAccent>> =
            Err(zbus::Error::Failure("backend failed".into()));
        let missing: zbus::Result<Option<PortalAccent>> = Ok(None);
        for (read, rule) in [
            (&failed, MissingKey::Withdraws),
            (&failed, MissingKey::Keeps),
            (&missing, MissingKey::Keeps),
        ] {
            take_key(&observation, &signal, read, rule, set_accent);
            assert_eq!(accent(&observation), Some(blue), "{rule:?}");
        }
        take_key(
            &observation,
            &signal,
            &missing,
            MissingKey::Withdraws,
            set_accent,
        );
        assert_eq!(accent(&observation), None, "the new portal has no accent");
        // The clock format withdrawn, the environment's hour cycle stands.
        let clock: zbus::Result<Option<HourCycle>> = Ok(Some(HourCycle::H12));
        let set_clock = |parts: &mut PreferenceParts, cycle: Option<HourCycle>| {
            parts.regional.hour_cycle = cycle.or(Some(HourCycle::H23));
        };
        take_key(&observation, &signal, &clock, MissingKey::Keeps, set_clock);
        let missing: zbus::Result<Option<HourCycle>> = Ok(None);
        take_key(
            &observation,
            &signal,
            &missing,
            MissingKey::Withdraws,
            set_clock,
        );
        assert_eq!(
            observation.lock().accepted.regional().hour_cycle(),
            Some(HourCycle::H23)
        );
    }

    /// GNOME's overlay scrolling shows scroll bars while scrolling; without
    /// it they always show.
    #[test]
    fn overlay_scrolling_is_scroll_bars_that_show_while_scrolling() {
        assert_eq!(overlay_scrolling(true), ScrollBarVisibility::WhenScrolling);
        assert_eq!(overlay_scrolling(false), ScrollBarVisibility::Always);
    }

    /// A text scale the snapshot refuses is reported with the observation
    /// unlocked, and leaves what was accepted before as it was.
    #[test]
    fn a_refused_text_scale_is_reported_with_the_observation_unlocked() {
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted: SystemPreferences::default(),
            appearance: Brightness::Light,
            environment_hour_cycle: None,
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
