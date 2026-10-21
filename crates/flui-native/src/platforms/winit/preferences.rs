//! Linux Settings portal observation owned by one winit event-loop incarnation.
//!
//! D-Bus runs on the existing background executor. The owner only reads an
//! immutable accepted snapshot; no portal request runs in build/layout/paint.

use std::{
    sync::{Arc, Weak, mpsc},
    time::Duration,
};

use flui_platform::{MotionPreference, SystemPreferences, WindowAppearance};
use futures_util::StreamExt;
use parking_lot::Mutex;
use zbus::{Proxy, zvariant::OwnedValue};

use crate::{executor::BackgroundExecutor, shared::owner_signal::OwnerSignal};

const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME: &str = "org.gnome.desktop.interface";
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

pub(super) struct PreferenceSource {
    observation: Arc<Mutex<Observation>>,
    task: tokio::task::JoinHandle<()>,
}

impl PreferenceSource {
    pub(super) fn new(executor: &BackgroundExecutor, signal: &Arc<OwnerSignal>) -> Arc<Self> {
        let observation = Arc::new(Mutex::new(Observation {
            active: true,
            accepted: SystemPreferences::default(),
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

    pub(super) fn read(&self) -> SystemPreferences {
        self.observation.lock().accepted.clone()
    }

    /// The accepted `color-scheme`; light until the portal says otherwise.
    pub(super) fn appearance(&self) -> WindowAppearance {
        self.observation.lock().appearance
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
                if (namespace == APPEARANCE && matches!(key, "reduced-motion" | "color-scheme"))
                    || (namespace == GNOME && key == "enable-animations") {
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
    motion.and(appearance).map(|_| ())
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
