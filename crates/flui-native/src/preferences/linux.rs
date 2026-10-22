//! The Linux host's system preferences: the Settings portal
//! (`org.freedesktop.portal.Settings`) on the session bus, observed for one
//! incarnation of the owner's loop. Every Linux host reads it; today that is
//! the winit host.
//!
//! D-Bus runs on the existing background executor. The owner only reads an
//! immutable accepted snapshot; no portal request runs in build/layout/paint.
//! The portal reports motion, contrast, text scale and the color scheme; the
//! preferred languages come from the process environment, read once, as
//! gettext reads them.

use std::{
    sync::{Arc, Weak, mpsc},
    time::Duration,
};

use flui_platform::{MotionPreference, SystemPreferences, WindowAppearance};
use futures_util::StreamExt;
use parking_lot::Mutex;
use zbus::{Proxy, zvariant::OwnedValue};

use crate::{
    PlatformError, executor::BackgroundExecutor, owner::signal::OwnerSignal,
    owner::sources::SystemPreferenceSource, preferences::locales as preferred_locales,
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME: &str = "org.gnome.desktop.interface";
const GNOME_A11Y: &str = "org.gnome.desktop.a11y.interface";
const RETRY: Duration = Duration::from_secs(1);
const READ_TIMEOUT: Duration = Duration::from_millis(200);
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_millis(500);

struct Observation {
    active: bool,
    accepted: SystemPreferences,
    /// The portal's `color-scheme`, kept beside the host-wide snapshot because
    /// brightness is presentation-specific (ADR-0172): each window reports it
    /// through `PlatformWindow::appearance`.
    appearance: WindowAppearance,
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

/// The winit host on Linux starts the portal's watch as it resumes, and its
/// windows without a theme of their own follow the portal's colour scheme.
/// The watch itself names no winit type, so the Wayland and X11 hosts start
/// it the same way.
#[cfg(feature = "winit-backend")]
mod winit {
    use std::sync::Arc;

    use flui_platform::WindowAppearance;

    use super::PreferenceSource;
    use crate::executor::BackgroundExecutor;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::winit::{PortalPreferences, PreferenceStart};

    /// The Settings portal as the winit loop starts watching it.
    #[derive(Debug)]
    pub(crate) struct WinitSettings;

    impl PreferenceStart for WinitSettings {
        fn start(
            &self,
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn PortalPreferences> {
            PreferenceSource::new(executor, signal)
        }
    }

    impl PortalPreferences for PreferenceSource {
        /// The accepted `color-scheme`; light until the portal says otherwise.
        fn appearance(&self) -> WindowAppearance {
            self.observation.lock().appearance
        }
    }
}

#[cfg(feature = "winit-backend")]
pub(crate) use winit::WinitSettings;

impl PreferenceSource {
    /// Start watching the portal on `executor`, waking the owner behind
    /// `signal` when what it accepted changes. Waits a bounded moment for the
    /// first answer, so the first sample usually has one.
    pub(crate) fn new(executor: &BackgroundExecutor, signal: &Arc<OwnerSignal>) -> Arc<Self> {
        // The environment does not change under a running process.
        let accepted =
            match preferred_locales::from_posix_environment(|name| std::env::var(name).ok()) {
                Some(locales) => SystemPreferences::default().with_locales(locales),
                None => SystemPreferences::default(),
            };
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted,
            appearance: WindowAppearance::Light,
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

async fn observe(
    observation: Arc<Mutex<Observation>>,
    signal: Weak<OwnerSignal>,
    mut ready: Option<mpsc::SyncSender<()>>,
) {
    loop {
        let _ = observe_connection(&observation, &signal, &mut ready).await;
        if let Some(ready) = ready.take() {
            let _ = ready.send(());
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn observe_connection(
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
    ready: &mut Option<mpsc::SyncSender<()>>,
) -> zbus::Result<()> {
    let connection = tokio::time::timeout(BOOTSTRAP_TIMEOUT, async {
        zbus::connection::Builder::session()?
            .method_timeout(READ_TIMEOUT)
            .build()
            .await
    })
    .await
    .map_err(|_| zbus::Error::Failure("session bus connection timed out".into()))??;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await?;
    // Both subscriptions precede the read: a setting change or a replacement
    // service during bootstrap remains queued for another observation.
    let mut changes = proxy.receive_signal("SettingChanged").await?;
    let mut owners = proxy.receive_owner_changed().await?;
    let mut retry = refresh(&proxy, observation, signal).await.is_err();
    if let Some(ready) = ready.take() {
        let _ = ready.send(());
    }
    let mut retries = tokio::time::interval_at(tokio::time::Instant::now() + RETRY, RETRY);
    retries.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            message = changes.next() => {
                let Some(message) = message else { return Ok(()); };
                let body = message.body();
                let Ok((namespace, key, _)) = body.deserialize::<(&str, &str, OwnedValue)>() else {
                    continue;
                };
                if matches!(
                    (namespace, key),
                    (APPEARANCE, "reduced-motion" | "color-scheme" | "contrast")
                        | (GNOME, "enable-animations" | "text-scaling-factor")
                        | (GNOME_A11Y, "high-contrast")
                ) {
                    retry = refresh(&proxy, observation, signal).await.is_err();
                }
            }
            owner = owners.next() => {
                if owner.is_none() { return Ok(()); }
                retry = refresh(&proxy, observation, signal).await.is_err();
            }
            _ = retries.tick(), if retry => {
                retry = refresh(&proxy, observation, signal).await.is_err();
            }
        }
    }
}

/// Read every observed setting. Each accepted value is published on its own,
/// so one failing read neither discards nor delays the other.
async fn refresh(
    proxy: &Proxy<'_>,
    observation: &Mutex<Observation>,
    signal: &Weak<OwnerSignal>,
) -> zbus::Result<()> {
    let motion = read_motion(proxy).await;
    if let Ok(motion) = motion {
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
        publish(observation, signal, |observation| {
            // A factor outside what a snapshot admits is not an observation.
            match observation.accepted.clone().with_text_scale(scale) {
                Ok(accepted) if accepted != observation.accepted => {
                    observation.accepted = accepted;
                    true
                }
                Ok(_) => false,
                Err(error) => {
                    tracing::warn!(scale, %error, "portal text-scaling-factor refused");
                    false
                }
            }
        });
    }
    motion
        .and(appearance)
        .and(contrast)
        .and(text_scale)
        .map(|_| ())
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
async fn read_appearance(proxy: &Proxy<'_>) -> zbus::Result<WindowAppearance> {
    match read(proxy, APPEARANCE, "color-scheme").await {
        Ok(value) => Ok(if u32::try_from(value)? == 1 {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }),
        Err(error) if missing_key(&error) => Ok(WindowAppearance::Light),
        Err(error) => Err(error),
    }
}

async fn read_motion(proxy: &Proxy<'_>) -> zbus::Result<MotionPreference> {
    let motion = match read(proxy, APPEARANCE, "reduced-motion").await {
        Ok(value) => match u32::try_from(value)? {
            1 => MotionPreference::Reduce,
            _ => MotionPreference::NoPreference,
        },
        // An unavailable key permits the GNOME fallback. A transient read
        // failure must preserve the accepted primary observation instead.
        Err(error) if missing_key(&error) => {
            if bool::try_from(read(proxy, GNOME, "enable-animations").await?)? {
                MotionPreference::NoPreference
            } else {
                MotionPreference::Reduce
            }
        }
        Err(error) => return Err(error),
    };
    Ok(motion)
}

fn missing_key(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _) if matches!(name.as_str(),
        "org.freedesktop.portal.Error.NotFound" | "org.freedesktop.DBus.Error.UnknownProperty"))
}

async fn read(proxy: &Proxy<'_>, namespace: &str, key: &str) -> zbus::Result<OwnedValue> {
    match proxy.call("ReadOne", &(namespace, key)).await {
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
        {
            // Settings v1's deprecated Read wraps its value in two variants.
            let value: OwnedValue = proxy.call("Read", &(namespace, key)).await?;
            match zbus::zvariant::Value::from(value) {
                zbus::zvariant::Value::Value(inner) => Ok(OwnedValue::try_from(*inner)?),
                _ => Err(zbus::Error::Failure("invalid legacy Settings value".into())),
            }
        }
        result => result,
    }
}
