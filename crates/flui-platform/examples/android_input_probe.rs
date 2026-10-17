//! The on-device half of `cargo xtask device android-input`.
//!
//! A `NativeActivity` library that runs the Android backend with the
//! native capture installed (`native-capture`, a debug build), so the
//! readings it decodes from real `MotionEvent`s journal as a native
//! recording (ADR-0215 §4). The xtask drives it with `adb shell input` and
//! `uinput` and talks to it through function keys; the probe answers in
//! files under its files directory, which `adb` reads through `run-as`
//! (the APK is debuggable):
//!
//! - `status.txt`: one line per fact, `READY` once the window is open and
//!   painted;
//! - F12 writes `recording.jsonl` (the journal of the window's registry) and
//!   `delivered.txt` (every pointer event the window delivered, one `Debug`
//!   line each), then `FLUSHED`;
//! - F11 performs one semantic haptic and plays one transient pattern on
//!   the window's actuator, and writes their outcomes;
//! - F10 writes the observed `SystemPreferences`;
//! - F9 quits.
//!
//! The window excludes the left screen edge from system gestures from the
//! start, so the xtask can read the exclusion back from the window manager.

#[cfg(all(target_os = "android", debug_assertions))]
#[path = "../tests/native_recording/codec.rs"]
#[allow(
    dead_code,
    reason = "the recording codec is shared with the tests, which use all of it"
)]
mod codec;
#[cfg(all(target_os = "android", debug_assertions))]
#[path = "../tests/native_recording/journal.rs"]
#[allow(
    dead_code,
    reason = "the capture sink is shared with the tests, which use all of it"
)]
mod journal;

#[cfg(all(target_os = "android", debug_assertions))]
mod probe {
    use std::cell::RefCell;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::Duration;

    use android_activity::AndroidApp;
    use android_activity::ndk::hardware_buffer_format::HardwareBufferFormat;
    use flui_foundation::geometry::{Point, Rect, Size};
    use flui_platform::AndroidPlatform;
    use flui_platform::traits::{DispatchEventResult, Platform, PlatformInput, WindowOptions};
    use flui_platform_api::HapticFeedback;
    use flui_platform_api::haptics::{HapticPattern, HapticRequest};
    use flui_platform_api::input_regions::{InputRegion, InputRegionKind};
    use flui_platform_api::keyboard::{Key, KeyState, NamedKey};

    use super::codec::Backend;
    use super::journal::Recorder;

    /// Appends one line to `status.txt`.
    #[derive(Clone)]
    struct Status(Rc<PathBuf>);

    impl Status {
        fn line(&self, text: &str) {
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.0.as_path())
                .and_then(|mut file| writeln!(file, "{text}"));
            if let Err(error) = written {
                // Nothing else can carry the failure off the device.
                eprintln!("android_input_probe: status not written: {error}");
            }
        }
    }

    fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // Through a temporary name, so a reader never sees half a file.
        let partial = path.with_extension("partial");
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, path)
    }

    /// Paint the native window once, so the window manager treats it as drawn
    /// and dispatches input to it.
    fn paint(app: &AndroidApp) {
        let Some(window) = app.native_window() else {
            return;
        };
        if window
            .set_buffers_geometry(0, 0, Some(HardwareBufferFormat::R8G8B8A8_UNORM))
            .is_err()
        {
            return;
        }
        if let Ok(mut buffer) = window.lock(None)
            && let Some(lines) = buffer.lines()
        {
            for line in lines {
                for (index, byte) in line.iter_mut().enumerate() {
                    byte.write(match index % 4 {
                        0 => 0x20,
                        1 => 0x60,
                        2 => 0xA0,
                        _ => 0xFF,
                    });
                }
            }
        }
    }

    pub(super) fn run(app: AndroidApp) {
        let Some(directory) = app.internal_data_path() else {
            eprintln!("android_input_probe: no files directory");
            return;
        };
        let _ = std::fs::create_dir_all(&directory);
        let status_path = directory.join("status.txt");
        let _ = std::fs::remove_file(&status_path);
        // A panic ends the activity with nothing on logcat: say why first.
        let panic_path = status_path.clone();
        std::panic::set_hook(Box::new(move |panic| {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&panic_path)
                .and_then(|mut file| writeln!(file, "PANIC {panic}"));
        }));
        let status = Status(Rc::new(status_path));
        status.line("STARTED");
        // Installed before the platform exists, so the window's registry is
        // the capture's first source.
        let recorder = Recorder::start(Backend::Android);
        let journals = recorder.journals();
        let platform = Box::new(AndroidPlatform::new(app.clone()));
        let painter = app;
        let ready_status = status.clone();
        let result = platform.run(Box::new(move |owner| {
            let window = owner.open_window(WindowOptions::default())?.try_ready()?;
            ready_status.line("OPENED");
            paint(&painter);
            ready_status.line("PAINTED");
            window.set_input_regions(&[InputRegion::new(
                InputRegionKind::SystemGestureExclusion,
                Rect::from_origin_size(Point::new(0.0, 120.0), Size::new(40.0, 300.0)),
            )]);
            let delivered: Rc<RefCell<Vec<String>>> = Rc::default();
            let status = ready_status.clone();
            let haptics_window = window.clone();
            window.on_input(Box::new(move |input| {
                match &input {
                    PlatformInput::Pointer(event) => {
                        delivered.borrow_mut().push(format!("{event:?}"));
                    }
                    PlatformInput::Keyboard(key) if key.state() == KeyState::Down => {
                        match key.key {
                            Key::Named(NamedKey::F12) => {
                                let recordings = journals.snapshot();
                                let Some(recording) = recordings.first() else {
                                    status.line("NO_RECORDING");
                                    return DispatchEventResult::default();
                                };
                                let mut text = Vec::new();
                                if let Err(error) = recording.write(&mut text) {
                                    status.line(&format!("RECORDING_UNWRITABLE {error}"));
                                    return DispatchEventResult::default();
                                }
                                let lines = delivered.borrow().join("\n");
                                let written = write(&directory.join("recording.jsonl"), &text)
                                    .and_then(|()| {
                                        write(&directory.join("delivered.txt"), lines.as_bytes())
                                    });
                                match written {
                                    Ok(()) => status.line(&format!(
                                        "FLUSHED rows={} delivered={}",
                                        recording.rows.len(),
                                        delivered.borrow().len()
                                    )),
                                    Err(error) => status.line(&format!("FLUSH_FAILED {error}")),
                                }
                            }
                            Key::Named(NamedKey::F11) => {
                                let Some(haptics) = haptics_window.haptics() else {
                                    status.line("HAPTICS none");
                                    return DispatchEventResult::default();
                                };
                                let request = HapticRequest::default();
                                let semantic =
                                    haptics.perform_with(HapticFeedback::Confirm, &request);
                                status.line(&format!("HAPTIC feedback={semantic:?}"));
                                std::thread::sleep(Duration::from_millis(300));
                                let taps = HapticPattern::builder()
                                    .transient(Duration::ZERO, 1.0, 0.9)
                                    .transient(Duration::from_millis(120), 0.6, 0.3)
                                    .build()
                                    .expect("a two-transient pattern is valid");
                                let taps = haptics.play(&taps, HapticFeedback::Confirm, &request);
                                std::thread::sleep(Duration::from_millis(300));
                                let swell = HapticPattern::builder()
                                    .continuous(
                                        Duration::ZERO,
                                        [
                                            (Duration::from_millis(60), 0.3, 0.5),
                                            (Duration::from_millis(160), 1.0, 0.5),
                                            (Duration::from_millis(260), 0.0, 0.5),
                                        ],
                                    )
                                    .build()
                                    .expect("a continuous swell is valid");
                                let swell = haptics.play(&swell, HapticFeedback::Confirm, &request);
                                status.line(&format!(
                                    "HAPTIC pattern taps={taps:?} swell={swell:?} capabilities={:?}",
                                    haptics.capabilities(None)
                                ));
                            }
                            Key::Named(NamedKey::F10) => match owner.preferences() {
                                Ok(observed) => status.line(&format!(
                                    "PREFERENCES haptics={:?} gestures={:?}",
                                    observed.haptics(),
                                    observed.gestures()
                                )),
                                Err(error) => status.line(&format!("PREFERENCES_FAILED {error}")),
                            },
                            Key::Named(NamedKey::F9) => {
                                status.line("QUIT");
                                owner.quit();
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                DispatchEventResult::default()
            }));
            ready_status.line(&format!(
                "READY scale={} size={:?}",
                window.scale_factor(),
                window.logical_size()
            ));
            Ok(())
        }));
        if let Err(error) = result {
            status.line(&format!("RUN_FAILED {error}"));
        }
        drop(recorder.finish());
    }
}

/// The activity's entry point.
#[cfg(all(target_os = "android", debug_assertions))]
#[expect(
    unsafe_code,
    reason = "NativeActivity finds the entry point by its unmangled name"
)]
#[expect(
    clippy::no_mangle_with_rust_abi,
    reason = "android-activity declares `android_main` with the Rust ABI"
)]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    probe::run(app);
}
