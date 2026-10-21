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
    ];

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
        let live = matches!(case, "live" | "recovery" | "restart");
        let recovery = case == "recovery";
        let restart = case == "restart";
        let scheme = case == "color-scheme";
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
                assert_eq!(
                    owner
                        .preferences()
                        .expect("initial owner observation")
                        .motion(),
                    expected,
                    "the first runtime seed must observe the portal before a user window exists"
                );
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
                        .open_window(flui_native::WindowOptions::default().with_visible(false))?
                        .try_ready()?;
                    assert_eq!(
                        window.appearance(),
                        WindowAppearance::Dark,
                        "the first window must observe the portal's dark scheme"
                    );
                    let proxy = owner.proxy();
                    let observed = Arc::downgrade(&window);
                    let seen = Arc::clone(&scheme_witness);
                    window.on_appearance_changed(Box::new(move || {
                        if observed
                            .upgrade()
                            .is_some_and(|window| window.appearance() == WindowAppearance::Light)
                        {
                            seen.store(true, Ordering::SeqCst);
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
