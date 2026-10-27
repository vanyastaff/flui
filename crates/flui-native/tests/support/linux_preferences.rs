//! Public Linux owner acceptance. Run inside dbus-run-session and xvfb-run.
//!
//! `cargo run -p flui-native --features winit-backend --test linux_preferences` runs it.

pub mod linux {
    use flui_native::{Platform, PlatformProxy, WinitPlatform};
    use flui_platform::{MotionPreference, WindowAppearance};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    #[derive(Debug, zbus::DBusError)]
    #[zbus(prefix = "org.freedesktop.portal.Error")]
    enum SettingsError {
        NotFound(String),
        Failed(String),
    }

    struct State {
        value: AtomicU32,
        /// The portal's `color-scheme`: 1 prefers dark, 2 prefers light.
        scheme: AtomicU32,
        failing: AtomicBool,
        failed: tokio::sync::Notify,
    }

    struct Settings {
        state: Arc<State>,
        gnome: bool,
        absent: bool,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl Settings {
        fn read_one(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<zbus::zvariant::OwnedValue, SettingsError> {
            if self.state.failing.load(Ordering::SeqCst) {
                if namespace == "org.freedesktop.appearance" && key == "reduced-motion" {
                    self.state.failed.notify_one();
                }
                return Err(SettingsError::Failed("temporary backend failure".into()));
            }
            if self.absent {
                return Err(SettingsError::NotFound("unsupported setting".into()));
            }
            if !self.gnome && namespace == "org.freedesktop.appearance" && key == "reduced-motion" {
                Ok(self.state.value.load(Ordering::SeqCst).into())
            } else if !self.gnome
                && namespace == "org.freedesktop.appearance"
                && key == "color-scheme"
            {
                Ok(self.state.scheme.load(Ordering::SeqCst).into())
            } else if self.gnome
                && namespace == "org.gnome.desktop.interface"
                && key == "enable-animations"
            {
                Ok((self.state.value.load(Ordering::SeqCst) != 1).into())
            } else {
                Err(SettingsError::NotFound("unknown setting".into()))
            }
        }
    }

    struct LegacySettings {
        value: u32,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl LegacySettings {
        fn read(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<zbus::zvariant::OwnedValue, SettingsError> {
            if namespace == "org.freedesktop.appearance" && key == "reduced-motion" {
                Ok(
                    zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::Value(Box::new(
                        self.value.into(),
                    )))
                    .expect("legacy double variant"),
                )
            } else {
                Err(SettingsError::NotFound("unknown setting".into()))
            }
        }
    }

    pub const CASES: &[&str] = &[
        "reduce",
        "full",
        "unknown-number",
        "gnome",
        "live",
        "recovery",
        "restart",
        "legacy",
        "unsupported",
        "no-service",
        "late-proxy",
        "color-scheme",
        "contrast",
        "gnome-contrast",
        "text-scale",
        "text-scale-out-of-range",
    ];

    /// The parent runs each case with `LANG=fr_CA.UTF-8` and `LANGUAGE=fr_CA:en`.
    fn assert_environment_languages(preferences: &flui_platform::SystemPreferences) {
        assert_eq!(
            preferences.locales().map(|locales| locales
                .iter()
                .map(flui_platform::Locale::to_language_tag)
                .collect::<Vec<_>>()),
            Some(vec!["fr-CA".to_owned(), "en".to_owned()]),
            "LANGUAGE's order, read once from the environment"
        );
    }

    /// A portal answering the contrast and text-scale keys: `contrast` in
    /// `org.freedesktop.appearance`, or GNOME's `high-contrast`, and GNOME's
    /// `text-scaling-factor`. Motion and color scheme are present so a refresh
    /// has nothing to retry.
    struct Values {
        gnome: bool,
        high_contrast: AtomicBool,
        /// `f64` bits; zero is no key.
        text_scale: std::sync::atomic::AtomicU64,
        /// Wakes whoever waits for the source to read `text-scaling-factor`.
        scale_read: Arc<tokio::sync::Notify>,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl Values {
        fn read_one(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<zbus::zvariant::OwnedValue, SettingsError> {
            let scale = f64::from_bits(self.text_scale.load(Ordering::SeqCst));
            if key == "text-scaling-factor" {
                self.scale_read.notify_waiters();
            }
            match (namespace, key) {
                ("org.freedesktop.appearance", "reduced-motion" | "color-scheme") => {
                    Ok(0u32.into())
                }
                ("org.freedesktop.appearance", "contrast") if !self.gnome => {
                    Ok(u32::from(self.high_contrast.load(Ordering::SeqCst)).into())
                }
                ("org.gnome.desktop.a11y.interface", "high-contrast") if self.gnome => {
                    Ok(self.high_contrast.load(Ordering::SeqCst).into())
                }
                ("org.gnome.desktop.interface", "text-scaling-factor") if scale != 0.0 => {
                    Ok(scale.into())
                }
                _ => Err(SettingsError::NotFound("unknown setting".into())),
            }
        }
    }

    /// The value a case changes live, before and after.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Value {
        Contrast(bool),
        /// `None` is no accepted observation.
        TextScale(Option<f64>),
    }

    impl Value {
        fn observed(self, preferences: &flui_platform::SystemPreferences) -> bool {
            match self {
                Self::Contrast(value) => preferences.high_contrast() == Some(value),
                Self::TextScale(value) => preferences.text_scale() == value,
            }
        }
    }

    /// What a case changes the portal to, live.
    #[derive(Clone, Copy, Debug)]
    enum Change {
        Contrast(bool),
        TextScale(f64),
    }

    impl Change {
        /// The observation the owner makes once the change is accepted.
        fn observation(self) -> Value {
            match self {
                Self::Contrast(high) => Value::Contrast(high),
                Self::TextScale(scale) => Value::TextScale(Some(scale)),
            }
        }
    }

    async fn emit_value(
        connection: &zbus::Connection,
        namespace: &str,
        key: &str,
        value: zbus::zvariant::Value<'_>,
    ) {
        connection
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                &(namespace, key, value),
            )
            .await
            .expect("native setting change");
    }

    /// A portal value read before any window, then changed live through
    /// `SettingChanged` until the owner observes the new value. Out of range,
    /// the portal first reports a scale `SystemPreferences` refuses, before
    /// any window and again live; an owner turn that runs after the live
    /// refusal has been processed, and before the valid value exists, still
    /// sees no observation, so a source that clamped instead of refusing fails
    /// the case.
    fn run_value_case(case: &str) {
        let gnome = case == "gnome-contrast";
        let (initial_scale, refused, before, after) = match case {
            "text-scale" => (
                1.25,
                None,
                Value::TextScale(Some(1.25)),
                Change::TextScale(1.5),
            ),
            "text-scale-out-of-range" => (
                100.0,
                Some(-1.0_f64),
                Value::TextScale(None),
                Change::TextScale(1.5),
            ),
            _ => (1.25, None, Value::Contrast(true), Change::Contrast(false)),
        };
        let scale_read = Arc::new(tokio::sync::Notify::new());
        let values = Values {
            gnome,
            high_contrast: AtomicBool::new(true),
            text_scale: std::sync::atomic::AtomicU64::new(f64::to_bits(initial_scale)),
            scale_read: Arc::clone(&scale_read),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("fixture runtime");
        let service = runtime
            .block_on(async {
                zbus::connection::Builder::session()?
                    .name("org.freedesktop.portal.Desktop")?
                    .serve_at("/org/freedesktop/portal/desktop", values)?
                    .build()
                    .await
            })
            .expect("isolated portal service");
        let (start, begin) = tokio::sync::oneshot::channel::<PlatformProxy>();
        // One message per owner turn that saw `before`.
        let (turned, mut turns) = tokio::sync::mpsc::unbounded_channel::<()>();
        let connection = service.clone();
        let update = runtime.spawn(async move {
            let Ok(owner) = begin.await else {
                return;
            };
            let server = connection
                .object_server()
                .interface::<_, Values>("/org/freedesktop/portal/desktop")
                .await
                .expect("served values");
            if let Some(refused) = refused {
                server
                    .get()
                    .await
                    .text_scale
                    .store(refused.to_bits(), Ordering::SeqCst);
                // The source reads the refused value, and a second change makes it
                // read again: its observations run one after another, so by the
                // second read the first has been refused or, wrongly, accepted.
                for _ in 0..2 {
                    let read = scale_read.notified();
                    tokio::pin!(read);
                    read.as_mut().enable();
                    emit_value(
                        &connection,
                        "org.gnome.desktop.interface",
                        "text-scaling-factor",
                        zbus::zvariant::Value::from(refused),
                    )
                    .await;
                    read.await;
                }
                // An owner turn now reads what the source accepted, before the
                // valid value exists: the turn's hook stops the case on anything
                // but `before`.
                while turns.try_recv().is_ok() {}
                owner.wake().expect("owner turn after the refusal");
                turns.recv().await.expect("the owner ran its turn");
            }
            let (namespace, key, value) = match after {
                Change::Contrast(high) => {
                    server
                        .get()
                        .await
                        .high_contrast
                        .store(high, Ordering::SeqCst);
                    if gnome {
                        (
                            "org.gnome.desktop.a11y.interface",
                            "high-contrast",
                            zbus::zvariant::Value::from(high),
                        )
                    } else {
                        (
                            "org.freedesktop.appearance",
                            "contrast",
                            zbus::zvariant::Value::from(u32::from(high)),
                        )
                    }
                }
                Change::TextScale(scale) => {
                    server
                        .get()
                        .await
                        .text_scale
                        .store(scale.to_bits(), Ordering::SeqCst);
                    (
                        "org.gnome.desktop.interface",
                        "text-scaling-factor",
                        zbus::zvariant::Value::from(scale),
                    )
                }
            };
            emit_value(&connection, namespace, key, value).await;
        });
        let watchdog = runtime.spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            eprintln!("Linux preference owner did not complete within 30 seconds");
            std::process::exit(2);
        });
        let platform = Arc::new(WinitPlatform::new());
        let reader = Arc::clone(&platform);
        let changed = Arc::new(AtomicBool::new(false));
        let witness = Arc::clone(&changed);
        let after = after.observation();
        platform
            .run_event_loop(Box::new(move |owner| {
                let initial = owner.preferences().expect("initial owner observation");
                assert!(
                    before.observed(&initial),
                    "the first runtime seed must observe {before:?}, got {initial:?}"
                );
                assert_environment_languages(&initial);
                let proxy = owner.proxy();
                owner.on_wake(Box::new(move || {
                    let current = reader.preferences().expect("live owner observation");
                    if after.observed(&current) {
                        witness.store(true, Ordering::SeqCst);
                        proxy.request_quit().expect("value owner quit");
                    } else if before.observed(&current) {
                        let _ = turned.send(());
                    } else {
                        eprintln!("owner observed neither {before:?} nor {after:?}: {current:?}");
                        proxy.request_quit().expect("stop failed observation");
                    }
                }))?;
                start.send(owner.proxy()).expect("start live change");
                Ok(())
            }))
            .expect("owner loop");
        drop(service);
        update.abort();
        watchdog.abort();
        assert!(
            changed.load(Ordering::SeqCst),
            "a live {after:?} change must reach the owner"
        );
        println!("Linux preference case {case} passed");
    }

    async fn emit_change(connection: &zbus::Connection, value: u32) {
        emit_setting(connection, "reduced-motion", value).await;
    }

    async fn emit_setting(connection: &zbus::Connection, key: &str, value: u32) {
        connection
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                &(
                    "org.freedesktop.appearance",
                    key,
                    zbus::zvariant::Value::from(value),
                ),
            )
            .await
            .expect("native setting change");
    }

    /// Where a live case stands, as the owner has observed it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Live {
        /// The first turn, with the portal's initial reduced motion.
        Seed,
        /// The fixture changed the setting to full motion.
        Changing,
        /// Full motion is accepted and the portal's reads fail.
        Failing,
        /// The reads succeed again; the retained retry brings reduced motion.
        Recovering,
    }

    struct RetireCheck {
        platform: Arc<WinitPlatform>,
        refused: Arc<AtomicBool>,
    }

    impl Drop for RetireCheck {
        fn drop(&mut self) {
            self.refused
                .store(self.platform.preferences().is_err(), Ordering::SeqCst);
        }
    }

    pub fn run_case(case: &str) {
        if matches!(
            case,
            "contrast" | "gnome-contrast" | "text-scale" | "text-scale-out-of-range"
        ) {
            return run_value_case(case);
        }
        let live = matches!(case, "live" | "recovery" | "restart");
        let recovery = case == "recovery";
        let restart = case == "restart";
        let scheme = case == "color-scheme";
        let unsupported = case == "unsupported";
        let initial = match case {
            "full" => 0,
            "unknown-number" => 42,
            _ => 1,
        };
        let expected = if matches!(case, "unsupported" | "no-service") {
            None
        } else if initial == 1 {
            Some(MotionPreference::Reduce)
        } else {
            Some(MotionPreference::NoPreference)
        };
        let state = Arc::new(State {
            value: AtomicU32::new(initial),
            scheme: AtomicU32::new(u32::from(scheme)),
            failing: AtomicBool::new(false),
            failed: tokio::sync::Notify::new(),
        });
        let settings = Settings {
            state: Arc::clone(&state),
            gnome: case == "gnome",
            absent: case == "unsupported",
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("fixture runtime");
        let service = runtime
            .block_on(async {
                let builder = zbus::connection::Builder::session()?;
                if case == "no-service" {
                    builder.build().await
                } else if case == "legacy" {
                    builder
                        .name("org.freedesktop.portal.Desktop")?
                        .serve_at(
                            "/org/freedesktop/portal/desktop",
                            LegacySettings { value: 1 },
                        )?
                        .build()
                        .await
                } else {
                    builder
                        .name("org.freedesktop.portal.Desktop")?
                        .serve_at("/org/freedesktop/portal/desktop", settings)?
                        .build()
                        .await
                }
            })
            .expect("isolated portal service");
        let platform = Arc::new(WinitPlatform::new());
        let stopped = Arc::clone(&platform);
        let reader = Arc::clone(&platform);
        let (start, begin) = tokio::sync::oneshot::channel::<PlatformProxy>();
        let (late_send, late_receive) = tokio::sync::oneshot::channel::<PlatformProxy>();
        let late_proxy = case == "late-proxy";
        let (step, mut steps) = tokio::sync::mpsc::unbounded_channel();
        // Set once both failed reads finished, before the inspecting wake.
        let failed_reads = Arc::new(AtomicBool::new(false));
        let failed_reads_done = Arc::clone(&failed_reads);
        let connection = service.clone();
        let update = runtime.spawn(async move {
            let Ok(proxy) = begin.await else {
                return None;
            };
            if scheme {
                state.scheme.store(2, Ordering::SeqCst);
                emit_setting(&connection, "color-scheme", 2).await;
                return None;
            }
            state.value.store(0, Ordering::SeqCst);
            if restart {
                connection
                    .release_name("org.freedesktop.portal.Desktop")
                    .await
                    .expect("release old owner");
                return Some(
                    zbus::connection::Builder::session()
                        .expect("replacement session")
                        .name("org.freedesktop.portal.Desktop")
                        .expect("replacement name")
                        .serve_at(
                            "/org/freedesktop/portal/desktop",
                            Settings {
                                state,
                                gnome: false,
                                absent: false,
                            },
                        )
                        .expect("replacement service")
                        .build()
                        .await
                        .expect("replacement owner"),
                );
            }
            emit_change(&connection, 0).await;
            if recovery {
                steps.recv().await.expect("accepted full update");
                state.failing.store(true, Ordering::SeqCst);
                state.value.store(1, Ordering::SeqCst);
                emit_change(&connection, 1).await;
                state.failed.notified().await;
                // The next primary read proves the previous failed refresh
                // finished. A wake immediately after the first error reply
                // could race source admission and pass with broken cleanup.
                state.failed.notified().await;
                failed_reads_done.store(true, Ordering::SeqCst);
                proxy
                    .wake()
                    .expect("inspect accepted value after failed read");
                steps
                    .recv()
                    .await
                    .expect("failed read preserved last accepted value");
                state.failing.store(false, Ordering::SeqCst);
                // No new SettingChanged: recovery must come from the retained retry.
            }
            None
        });
        let watchdog = runtime.spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            eprintln!("Linux preference owner did not complete within 30 seconds");
            std::process::exit(2);
        });
        let refused = Arc::new(AtomicBool::new(false));
        let scheme_seen = Arc::new(AtomicBool::new(false));
        let scheme_witness = Arc::clone(&scheme_seen);
        let delivery_failed = Arc::new(AtomicBool::new(false));
        let callback_failed = Arc::clone(&delivery_failed);
        let capture = RetireCheck {
            platform: Arc::clone(&platform),
            refused: Arc::clone(&refused),
        };
        platform
            .run_event_loop(Box::new(move |owner| {
                let initial = owner.preferences().expect("initial owner observation");
                assert_eq!(
                    initial.motion(),
                    expected,
                    "the first runtime seed must observe the portal before a user window exists"
                );
                assert_environment_languages(&initial);
                if unsupported {
                    assert_eq!(
                        initial.high_contrast(),
                        None,
                        "no contrast key is unobserved"
                    );
                    assert_eq!(
                        initial.text_scale(),
                        None,
                        "no text scale key is unobserved"
                    );
                }
                let foreign = std::thread::scope(|scope| {
                    scope
                        .spawn(|| reader.preferences())
                        .join()
                        .expect("foreign read")
                });
                assert!(
                    foreign.is_err(),
                    "foreign reads cannot become a second source"
                );
                if scheme {
                    let window = owner
                        .open_window(flui_platform::WindowOptions::default().with_visible(false))?
                        .try_ready()?;
                    assert_eq!(
                        window.state().appearance,
                        WindowAppearance::Dark,
                        "the first window must observe the portal's dark scheme"
                    );
                    let proxy = owner.proxy();
                    let observed = Arc::downgrade(&window);
                    let seen = Arc::clone(&scheme_witness);
                    window.on_state_changed(Box::new(move || {
                        if observed.upgrade().is_some_and(|window| {
                            window.state().appearance == WindowAppearance::Light
                        }) && !seen.swap(true, Ordering::SeqCst)
                        {
                            proxy.request_quit().expect("color-scheme owner quit");
                        }
                    }));
                    start
                        .send(owner.proxy())
                        .expect("start color-scheme change");
                } else if live {
                    let proxy = owner.proxy();
                    let mut start = Some(start);
                    let mut phase = Live::Seed;
                    owner.on_wake(Box::new(move || {
                        let _ = &capture;
                        let motion = reader
                            .preferences()
                            .expect("live owner observation")
                            .motion();
                        let fail = |expected: MotionPreference| {
                            eprintln!(
                                "owner delivered {motion:?} in {phase:?}; expected {expected:?}"
                            );
                            callback_failed.store(true, Ordering::SeqCst);
                            proxy.request_quit().expect("stop failed observation");
                        };
                        // A turn may also come from another owner-turn source (a
                        // display change, a host notice): every turn must see the
                        // accepted value, and a phase ends on what the owner
                        // observes, never on how many turns it had.
                        phase = match (phase, motion) {
                            (Live::Seed, Some(MotionPreference::Reduce)) => {
                                start
                                    .take()
                                    .expect("BUG: the seed turn runs once")
                                    .send(proxy.clone())
                                    .expect("start live update");
                                Live::Changing
                            }
                            (Live::Seed, _) => {
                                fail(MotionPreference::Reduce);
                                return;
                            }
                            // The full update is not accepted yet.
                            (Live::Changing, Some(MotionPreference::Reduce)) => Live::Changing,
                            (Live::Changing, Some(MotionPreference::NoPreference)) if recovery => {
                                step.send(()).expect("advance recovery fixture");
                                Live::Failing
                            }
                            (Live::Changing, Some(MotionPreference::NoPreference)) => {
                                proxy.request_quit().expect("live owner quit");
                                Live::Changing
                            }
                            // Failed refresh cannot replace accepted motion.
                            (Live::Failing, Some(MotionPreference::NoPreference)) => {
                                if failed_reads.load(Ordering::SeqCst) {
                                    step.send(()).expect("advance recovery fixture");
                                    Live::Recovering
                                } else {
                                    Live::Failing
                                }
                            }
                            // The retained retry has not run yet.
                            (Live::Recovering, Some(MotionPreference::NoPreference)) => {
                                Live::Recovering
                            }
                            (Live::Recovering, Some(MotionPreference::Reduce)) => {
                                proxy.request_quit().expect("live owner quit");
                                Live::Recovering
                            }
                            (Live::Changing | Live::Failing | Live::Recovering, _) => {
                                fail(if phase == Live::Recovering {
                                    MotionPreference::Reduce
                                } else {
                                    MotionPreference::NoPreference
                                });
                                return;
                            }
                        };
                    }))?;
                } else {
                    if late_proxy {
                        late_send
                            .send(owner.proxy())
                            .expect("retain residual proxy");
                    }
                    owner.proxy().request_quit()?;
                }
                Ok(())
            }))
            .expect("owner loop");
        assert!(
            stopped.preferences().is_err(),
            "retired owner refuses reads"
        );
        drop(service);
        update.abort();
        watchdog.abort();
        drop(stopped);
        if late_proxy {
            runtime.block_on(async move {
                let proxy = late_receive.await.expect("retained proxy");
                assert!(proxy.wake().is_err(), "retired proxy refuses admission");
                drop(proxy);
            });
        }
        if live {
            assert!(
                !delivery_failed.load(Ordering::SeqCst),
                "failed refresh cannot replace accepted motion"
            );
            assert!(
                refused.load(Ordering::SeqCst),
                "callback retirement sees closed preference admission"
            );
        }
        if scheme {
            assert!(
                scheme_seen.load(Ordering::SeqCst),
                "a live color-scheme change must reach the window"
            );
        }
        println!("Linux preference case {case} passed");
    }
}
