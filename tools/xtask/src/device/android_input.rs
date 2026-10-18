//! `android-input`: the Android backend on a real device, driven by the
//! system's own injection, and the native recordings of what it made of it.
//!
//! The probe is `crates/flui-platform/examples/android_input_probe.rs`, a
//! `NativeActivity` library built with `native-capture`. This module packages
//! it into a debuggable APK (`device/android.rs`), installs it over `adb`,
//! and runs one session per launch so
//! each recording starts from a fresh registry:
//!
//! 1. `adb shell input`: a touchscreen tap and swipe, a stylus tap and swipe
//!    and a mouse click, from the system's virtual devices;
//! 2. a `uinput` stylus with pressure and tilt that hovers, touches, lifts
//!    and leaves: the kernel device the real `InputReader` reads, so its
//!    motion ranges, hover and tool type are a pen's;
//! 3. a `uinput` touchpad in pointer mode: a pinch and a two-finger scroll,
//!    which Android's own gesture converter turns into its gesture events.
//!
//! The probe answers through files the xtask reads with `run-as`. After each
//! session the recording and the events the window delivered are pulled
//! into `target/native-recordings/android/`; the host test that follows
//! replays each recording through the shared decoder and requires exactly
//! what the device delivered, and the shape of its golden.
//!
//! A last launch checks what is not a pointer: the left-edge exclusion the
//! probe sets must reach the window manager; the haptics and long-press
//! settings, changed with `settings put`, must reach `SystemPreferences`;
//! and a semantic haptic and a pattern must reach the vibrator as touch
//! vibrations. Every setting is put back.
//!
//! A final launch drives `FluiView` (ADR-0237): a system drag the probe
//! starts from a held touch must drop its text on the probe's own window,
//! and the device's default keyboard, tapped through the keys its
//! accessibility tree names, must compose into the probe's field, leave a
//! hardware Escape to the input method while it composes, commit on space
//! and send its action key as Enter.
//!
//! Exit 0 on PASS, 1 on FAIL, 2 when there is no SDK, no device, or the
//! device cannot show the probe (locked).

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;

use super::android::{Apk, Device, ImeKey, Registration, Screen, Script, Sdk, keyboard_keys};

/// The probe's application.
const PROBE: Apk = Apk {
    package: "dev.flui.android_input_probe",
    library: "android_input_probe",
    label: "FLUI input probe",
};
/// How long a launch may take to open and paint its window.
const READY_WITHIN: Duration = Duration::from_secs(20);
/// How long the probe may take to answer a key.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);
/// Time for the last events of a session to drain, past the probe's hover
/// exit grace, before the recording is flushed.
const SETTLE: Duration = Duration::from_millis(400);

/// One recorded session.
struct Session {
    /// The golden's name.
    name: &'static str,
    /// Injects the session's input.
    inject: fn(&Device, Screen) -> anyhow::Result<()>,
}

/// The sessions, in order.
const SESSIONS: [Session; 3] = [
    Session {
        name: "android_input_touch_stylus_mouse",
        inject: inject_input_commands,
    },
    Session {
        name: "android_uinput_stylus_hover_and_tilt",
        inject: inject_stylus,
    },
    Session {
        name: "android_uinput_touchpad_pinch_and_scroll",
        inject: inject_touchpad,
    },
];

/// Package, install and drive the probe built at `library`, pulling each
/// session's recording into `recordings`. Returns the exit code.
pub(super) fn run(library: &Path, recordings: &Path, work: &Path) -> anyhow::Result<u8> {
    let Some(sdk) = Sdk::locate() else {
        println!(
            "android-input: no Android SDK with build-tools, a platform and platform-tools (set ANDROID_HOME)"
        );
        return Ok(2);
    };
    let Some(device) = Device::find(&sdk.adb)? else {
        println!(
            "android-input: no Android device is attached and authorized (`adb devices`; FLUI_ANDROID_SERIAL picks one)"
        );
        return Ok(2);
    };
    println!("android-input: device {}", device.describe());
    let apk = sdk.package(&PROBE, library, work)?;
    device.adb(["install", "-r", "-t"]).arg(&apk).check()?;
    device.wake()?;
    let screen = device.screen()?;
    std::fs::create_dir_all(recordings)
        .with_context(|| format!("creating {}", recordings.display()))?;
    for session in &SESSIONS {
        if device.launch()?.is_none() {
            println!(
                "android-input: the probe did not open its window within {READY_WITHIN:?} (is the device locked?)"
            );
            return Ok(2);
        }
        (session.inject)(&device, screen).with_context(|| format!("injecting {}", session.name))?;
        std::thread::sleep(SETTLE);
        let Some(flushed) = device.ask("KEYCODE_F12", "FLUSHED")? else {
            println!("android-input FAILED: {} was not flushed", session.name);
            return Ok(1);
        };
        println!("android-input: {} {flushed}", session.name);
        for (file, suffix) in [
            ("recording.jsonl", "jsonl"),
            ("delivered.txt", "delivered.txt"),
        ] {
            let text = device
                .read(file)?
                .with_context(|| format!("reading {file} of {}", session.name))?;
            let path = recordings.join(format!("{}.{suffix}", session.name));
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        }
        device.key("KEYCODE_F9")?;
    }
    let Some(scale) = device.launch()? else {
        println!("android-input: the probe did not reopen its window");
        return Ok(2);
    };
    let mut passed = check_gesture_exclusion(&device, scale)?;
    passed &= check_preferences(&device)?;
    passed &= check_haptics(&device)?;
    device.key("KEYCODE_F9")?;
    if device.launch()?.is_none() {
        println!("android-input: the probe did not reopen its window for FluiView");
        return Ok(2);
    }
    passed &= check_drag_and_drop(&device, screen)?;
    passed &= check_input_method(&device)?;
    device.key("KEYCODE_F9")?;
    Ok(u8::from(!passed))
}

/// The probe's left-edge exclusion, logical `(0, 120)` to `(40, 420)` at
/// the window's `scale`, must be part of the display's system gesture
/// exclusion the window manager enforces.
fn check_gesture_exclusion(device: &Device, scale: f64) -> anyhow::Result<bool> {
    let dump = device.shell("dumpsys window")?;
    let region = dump
        .lines()
        .find_map(|line| line.trim().strip_prefix("mSystemGestureExclusion="))
        .unwrap_or("");
    println!("android-input: display gesture exclusion {region}");
    let rects = skregion_rects(region);
    // The window rounds the region out to whole pixels.
    let (top, bottom, right) = (120.0 * scale, 420.0 * scale, 40.0 * scale);
    let edge: Vec<_> = rects
        .iter()
        .filter(|rect| rect[0] == 0 && f64::from(rect[2]) >= right.floor())
        .collect();
    let mut covered = top.ceil();
    let mut spans: Vec<(f64, f64)> = edge
        .iter()
        .map(|rect| (f64::from(rect[1]), f64::from(rect[3])))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (start, end) in spans {
        if start <= covered {
            covered = covered.max(end);
        }
    }
    let excluded = covered >= bottom.floor();
    if !excluded {
        println!(
            "android-input FAILED: the display excludes no left edge from {top:.0} to {bottom:.0} px"
        );
    }
    Ok(excluded)
}

/// `FluiView` as the drag source and the drop target (ADR-0237): a touch is
/// held down, the probe starts a system drag of a text from it
/// (`startDragAndDrop`, which needs a valid view surface under
/// `NativeActivity`'s `takeSurface`), the touch moves and lifts over the
/// probe's own window, whose target accepts a copy. The drop must deliver
/// the text, and the source must hear the drop.
fn check_drag_and_drop(device: &Device, screen: Screen) -> anyhow::Result<bool> {
    let installed = device
        .read("status.txt")?
        .is_some_and(|status| status.contains("CAPABILITIES outgoing_drag=Native"));
    if !installed {
        println!("android-input FAILED: FluiView was not installed (no outgoing drag capability)");
        return Ok(false);
    }
    let (x, y) = (screen.width / 2, screen.height * 2 / 5);
    device.shell(&format!("input touchscreen motionevent DOWN {x} {y}"))?;
    std::thread::sleep(Duration::from_millis(300));
    let started = device.ask("KEYCODE_F8", "DRAG_START")?;
    for step in 1..=5 {
        std::thread::sleep(Duration::from_millis(80));
        device.shell(&format!(
            "input touchscreen motionevent MOVE {} {}",
            x + step * 8,
            y + step * 60
        ))?;
    }
    device.shell(&format!(
        "input touchscreen motionevent UP {} {}",
        x + 40,
        y + 300
    ))?;
    let ended = device.wait_for("DRAG_END")?;
    let status = device.read("status.txt")?.unwrap_or_default();
    let dropped = status.lines().rfind(|line| line.starts_with("DROPPED"));
    println!("android-input: drag {started:?} / {dropped:?} / {ended:?}");
    let passed = started.as_deref() == Some("DRAG_START Ok(())")
        && dropped.is_some_and(|line| line.contains("text=Some(\"FLUI drag\")"))
        && ended
            .as_deref()
            .is_some_and(|line| line.starts_with("DRAG_END Dropped"));
    if !passed {
        println!(
            "android-input FAILED: the drag FluiView started did not drop its text on the window"
        );
    }
    Ok(passed)
}

/// `FluiView` as the input method's editor (ADR-0237, ADR-0090): the probe
/// focuses a field, the soft keyboard of the device's default input method
/// opens on it, three letter keys are tapped (the keyboard composes them), a
/// hardware Escape arrives while it composes, the space bar commits, and the
/// action key arrives as Enter. The field's store must hold what the keyboard
/// shows at each step.
fn check_input_method(device: &Device) -> anyhow::Result<bool> {
    if device.ask("KEYCODE_F7", "IME_FOCUSED")?.is_none() {
        println!("android-input FAILED: the probe did not focus its field");
        return Ok(false);
    }
    std::thread::sleep(Duration::from_millis(1500));
    let services = device.shell("dumpsys input_method")?;
    let shown = services.contains("mInputShown=true");
    let editor = services.contains("inputType=0x1 imeOptions=0x2000006");
    let ime = device
        .shell("settings get secure default_input_method")?
        .trim()
        .to_owned();
    let package = ime.split('/').next().unwrap_or_default().to_owned();
    println!(
        "android-input: input method {ime}, keyboard shown {shown}, FluiView's EditorInfo {editor}"
    );
    device.shell("uiautomator dump --windows /sdcard/flui-ime.xml")?;
    let keys = keyboard_keys(&device.shell("cat /sdcard/flui-ime.xml")?, &package);
    let letters: Vec<&ImeKey> = keys
        .iter()
        .filter(|key| {
            let mut chars = key.label.chars();
            chars.next().is_some_and(char::is_alphabetic) && chars.next().is_none()
        })
        .take(3)
        .collect();
    let named = |name: &str| keys.iter().find(|key| key.label == name);
    let (Some(space), Some(action), [_, _, _]) = (named("Space bar"), named("Done"), &letters[..])
    else {
        println!(
            "android-input CANNOT VERIFY: the keyboard of {package} exposes no letter, space and Done keys ({} keys found)",
            keys.len()
        );
        return Ok(shown && editor);
    };
    let typed: String = letters.iter().map(|key| key.label.as_str()).collect();
    for key in &letters {
        device.shell(&format!("input tap {} {}", key.center.0, key.center.1))?;
        std::thread::sleep(Duration::from_millis(400));
    }
    std::thread::sleep(Duration::from_millis(400));
    let composing = device.ask("KEYCODE_F5", "IME text=")?;
    let escape = device.ask("KEYCODE_ESCAPE", "KEY Escape")?;
    device.shell(&format!("input tap {} {}", space.center.0, space.center.1))?;
    std::thread::sleep(Duration::from_millis(800));
    let committed = device.ask("KEYCODE_F5", "IME text=")?;
    device.shell(&format!(
        "input tap {} {}",
        action.center.0, action.center.1
    ))?;
    let enter = device.wait_for("KEY Enter")?;
    let _ = device.ask("KEYCODE_F6", "IME_BLURRED")?;
    println!("android-input: typed {typed:?}: {composing:?}");
    println!("android-input: escape while composing: {escape:?}");
    println!("android-input: space: {committed:?}; action key: {enter:?}");
    let length = typed.encode_utf16().count();
    let passed = shown
        && editor
        && composing.as_deref()
            == Some(
                format!("IME text={typed:?} selection={length}..{length} composition=0..{length}")
                    .as_str(),
            )
        && escape.as_deref() == Some("KEY Escape composition=Active")
        && committed.as_deref()
            == Some(
                format!(
                    "IME text={:?} selection={1}..{1} composition=none",
                    format!("{typed} "),
                    length + 1
                )
                .as_str(),
            )
        && enter.as_deref() == Some("KEY Enter composition=Inactive");
    if !passed {
        println!(
            "android-input FAILED: the field's store did not follow the keyboard, or Escape was not the input method's"
        );
    }
    Ok(passed)
}

/// The rectangles of an `SkRegion((l,t,r,b)(l,t,r,b)…)` dump.
fn skregion_rects(region: &str) -> Vec<[i32; 4]> {
    region
        .split(['(', ')'])
        .filter_map(|part| {
            let values: Vec<i32> = part
                .split(',')
                .map(|value| value.trim().parse().ok())
                .collect::<Option<_>>()?;
            <[i32; 4]>::try_from(values).ok()
        })
        .collect()
}

/// The user's haptics switch and long-press timeout, changed through
/// `settings`, must reach the probe's `SystemPreferences`; both are put back.
fn check_preferences(device: &Device) -> anyhow::Result<bool> {
    let haptics = device.shell("settings get system haptic_feedback_enabled")?;
    let long_press = device.shell("settings get secure long_press_timeout")?;
    let (haptics, long_press) = (haptics.trim().to_owned(), long_press.trim().to_owned());
    let restore = |device: &Device| -> anyhow::Result<()> {
        for (namespace, key, value) in [
            ("system", "haptic_feedback_enabled", haptics.as_str()),
            ("secure", "long_press_timeout", long_press.as_str()),
        ] {
            if value == "null" {
                device.shell(&format!("settings delete {namespace} {key}"))?;
            } else {
                device.shell(&format!("settings put {namespace} {key} {value}"))?;
            }
        }
        Ok(())
    };
    let observed = (|| -> anyhow::Result<(Option<String>, Option<String>)> {
        device.shell("settings put system haptic_feedback_enabled 1")?;
        device.shell("settings put secure long_press_timeout 400")?;
        let before = device.preferences()?;
        device.shell("settings put system haptic_feedback_enabled 0")?;
        device.shell("settings put secure long_press_timeout 1500")?;
        let after = device.preferences()?;
        Ok((before, after))
    })();
    restore(device)?;
    let (before, after) = observed?;
    let (Some(before), Some(after)) = (before, after) else {
        println!("android-input FAILED: the probe did not report its preferences");
        return Ok(false);
    };
    println!("android-input: preferences before: {before}");
    println!("android-input: preferences after:  {after}");
    let reached = before.contains("haptics=Some(true)")
        && before.contains("400ms")
        && after.contains("haptics=Some(false)")
        && after.contains("1.5s");
    if !reached {
        println!(
            "android-input FAILED: the haptics switch or the long-press timeout did not reach SystemPreferences"
        );
    }
    Ok(reached)
}

/// The probe's semantic haptic, its two-tap pattern and its continuous
/// swell must each reach the vibrator as the degradation table says this
/// device can play them, every vibration with the touch usage.
fn check_haptics(device: &Device) -> anyhow::Result<bool> {
    // The service keeps a bounded list stamped `MM-dd HH:mm:ss.SSS` on the
    // device clock: what this run played starts at or after `since`.
    let since = device.shell("date '+%m-%d %H:%M:%S'")?.trim().to_owned();
    let Some(outcomes) = device.ask("KEYCODE_F11", "HAPTIC pattern")? else {
        println!("android-input FAILED: the probe did not report its haptics");
        return Ok(false);
    };
    println!("android-input: {outcomes}");
    // The vibrator service records a vibration when it ends.
    std::thread::sleep(Duration::from_secs(1));
    let fresh: Vec<String> = probe_vibrations(device)?
        .into_iter()
        .filter(|line| {
            line.get(..since.len())
                .is_some_and(|stamp| stamp >= since.as_str())
        })
        .collect();
    for line in &fresh {
        println!("android-input: vibrator: {line}");
    }
    let capabilities = device
        .shell("dumpsys vibrator_manager | grep -m3 -E 'capabilities =|supportedPrimitives'")?;
    println!(
        "android-input: vibrator {}",
        capabilities.trim().replace('\n', "; ")
    );
    let feedback = fresh
        .iter()
        .filter(|line| line.contains("performHapticFeedback"))
        .count();
    let patterns = fresh.len() - feedback;
    // Each pattern either plays on the vibrator or falls back to feedback.
    let played = ["taps=", "swell="]
        .iter()
        .filter(|label| {
            outcomes
                .split_whitespace()
                .any(|word| word.starts_with(*label) && !word.ends_with("FellBack"))
        })
        .count();
    let all_touch = fresh.iter().all(|line| line.contains("usage: TOUCH"));
    let consistent = feedback == 1 + (2 - played) && patterns == played;
    if !all_touch || !consistent {
        println!(
            "android-input FAILED: {feedback} feedback and {patterns} pattern vibrations for {played} played patterns; every one must have the touch usage"
        );
        return Ok(false);
    }
    Ok(true)
}

/// The vibrations the vibrator service lists for the probe, oldest first.
fn probe_vibrations(device: &Device) -> anyhow::Result<Vec<String>> {
    let dump = device.shell("dumpsys vibrator_manager")?;
    Ok(dump
        .lines()
        .filter(|line| line.contains(&format!("| {} (uid=", PROBE.package)))
        .map(|line| line.trim().to_owned())
        .collect())
}

/// Taps, swipes and a click from `input`'s virtual devices.
fn inject_input_commands(device: &Device, screen: Screen) -> anyhow::Result<()> {
    let (cx, cy) = (screen.width / 2, screen.height / 2);
    for command in [
        format!("input tap {cx} {cy}"),
        format!(
            "input swipe {} {} {} {} 200",
            cx - 200,
            cy + 300,
            cx + 200,
            cy + 300
        ),
        format!("input stylus tap {} {}", cx - 100, cy - 400),
        format!(
            "input stylus swipe {} {} {} {} 150",
            cx - 300,
            cy - 600,
            cx + 100,
            cy - 300
        ),
        format!("input mouse tap {} {}", cx + 60, cy - 300),
    ] {
        device.shell(&command)?;
    }
    Ok(())
}

/// A pen that hovers, touches with pressure and tilt, lifts and leaves.
fn inject_stylus(device: &Device, screen: Screen) -> anyhow::Result<()> {
    let (x, y) = (screen.width * 4 / 10, screen.height / 2);
    let mut script = Script::new(&Registration {
        id: 1,
        name: "FLUI Test Stylus",
        product: 0xabcd,
        keys: &["BTN_TOOL_PEN", "BTN_TOUCH", "BTN_STYLUS"],
        axes: &[
            ("ABS_X", 0, screen.width - 1, 0),
            ("ABS_Y", 0, screen.height - 1, 0),
            ("ABS_PRESSURE", 0, 4095, 0),
            ("ABS_TILT_X", -90, 90, 0),
            ("ABS_TILT_Y", -90, 90, 0),
        ],
        props: &["INPUT_PROP_DIRECT"],
    });
    script.delay(1500);
    // Hover in, and move while hovering.
    script.inject(&[
        ("EV_KEY", "BTN_TOOL_PEN", 1),
        ("EV_ABS", "ABS_X", x),
        ("EV_ABS", "ABS_Y", y),
    ]);
    for step in 1..=3 {
        script.delay(16);
        script.inject(&[("EV_ABS", "ABS_X", x + step * 6)]);
    }
    // Touch down tilted, press harder while moving, lift, hover, leave.
    script.delay(16);
    script.inject(&[
        ("EV_KEY", "BTN_TOUCH", 1),
        ("EV_ABS", "ABS_PRESSURE", 1200),
        ("EV_ABS", "ABS_TILT_X", 30),
    ]);
    for step in 1..=4 {
        script.delay(16);
        script.inject(&[
            ("EV_ABS", "ABS_X", x + 18 + step * 8),
            ("EV_ABS", "ABS_PRESSURE", 1200 + step * 500),
        ]);
    }
    script.delay(16);
    script.inject(&[("EV_KEY", "BTN_TOUCH", 0), ("EV_ABS", "ABS_PRESSURE", 0)]);
    script.delay(16);
    script.inject(&[("EV_ABS", "ABS_X", x + 60)]);
    script.delay(16);
    script.inject(&[("EV_KEY", "BTN_TOOL_PEN", 0)]);
    script.delay(300);
    device.uinput("stylus", &script.finish())
}

/// A touchpad in pointer mode: a pinch, then a two-finger scroll.
fn inject_touchpad(device: &Device, _screen: Screen) -> anyhow::Result<()> {
    // A resolution makes the gesture library measure in millimetres, as it
    // does a real touchpad's.
    let mut script = Script::new(&Registration {
        id: 2,
        name: "FLUI Test Touchpad",
        product: 0xabce,
        keys: &[
            "BTN_LEFT",
            "BTN_TOOL_FINGER",
            "BTN_TOOL_DOUBLETAP",
            "BTN_TOOL_TRIPLETAP",
            "BTN_TOOL_QUADTAP",
            "BTN_TOUCH",
        ],
        axes: &[
            ("ABS_X", 0, 3000, 30),
            ("ABS_Y", 0, 2000, 30),
            ("ABS_MT_SLOT", 0, 4, 0),
            ("ABS_MT_TOUCH_MAJOR", 0, 255, 0),
            ("ABS_MT_TOUCH_MINOR", 0, 255, 0),
            ("ABS_MT_POSITION_X", 0, 3000, 30),
            ("ABS_MT_POSITION_Y", 0, 2000, 30),
            ("ABS_MT_TRACKING_ID", 0, 65535, 0),
            ("ABS_MT_PRESSURE", 0, 255, 0),
        ],
        props: &["INPUT_PROP_POINTER", "INPUT_PROP_BUTTONPAD"],
    });
    script.delay(2000);
    // Two fingers spread apart: a pinch.
    two_fingers(&mut script, (1300, 1000), (1700, 1000), |step| {
        ((1300 - step * 20, 1000), (1700 + step * 20, 1000))
    });
    script.delay(600);
    // Two fingers move down together: a scroll.
    two_fingers(&mut script, (1400, 800), (1700, 800), |step| {
        ((1400, 800 + step * 25), (1700, 800 + step * 25))
    });
    script.delay(500);
    device.uinput("touchpad", &script.finish())
}

/// Two touchpad fingers down at `first` and `second`, twenty steps of
/// `at(step)`, then both up.
fn two_fingers(
    script: &mut Script,
    first: (i32, i32),
    second: (i32, i32),
    at: impl Fn(i32) -> ((i32, i32), (i32, i32)),
) {
    script.inject(&[
        ("EV_ABS", "ABS_MT_SLOT", 0),
        ("EV_ABS", "ABS_MT_TRACKING_ID", 1),
        ("EV_ABS", "ABS_MT_POSITION_X", first.0),
        ("EV_ABS", "ABS_MT_POSITION_Y", first.1),
        ("EV_ABS", "ABS_MT_PRESSURE", 60),
        ("EV_ABS", "ABS_MT_SLOT", 1),
        ("EV_ABS", "ABS_MT_TRACKING_ID", 2),
        ("EV_ABS", "ABS_MT_POSITION_X", second.0),
        ("EV_ABS", "ABS_MT_POSITION_Y", second.1),
        ("EV_ABS", "ABS_MT_PRESSURE", 60),
        ("EV_KEY", "BTN_TOUCH", 1),
        ("EV_KEY", "BTN_TOOL_DOUBLETAP", 1),
        ("EV_ABS", "ABS_X", first.0),
        ("EV_ABS", "ABS_Y", first.1),
    ]);
    for step in 1..=20 {
        let (a, b) = at(step);
        script.delay(16);
        script.inject(&[
            ("EV_ABS", "ABS_MT_SLOT", 0),
            ("EV_ABS", "ABS_MT_POSITION_X", a.0),
            ("EV_ABS", "ABS_MT_POSITION_Y", a.1),
            ("EV_ABS", "ABS_MT_SLOT", 1),
            ("EV_ABS", "ABS_MT_POSITION_X", b.0),
            ("EV_ABS", "ABS_MT_POSITION_Y", b.1),
            ("EV_ABS", "ABS_X", a.0),
            ("EV_ABS", "ABS_Y", a.1),
        ]);
    }
    script.delay(16);
    script.inject(&[
        ("EV_ABS", "ABS_MT_SLOT", 0),
        ("EV_ABS", "ABS_MT_TRACKING_ID", -1),
        ("EV_ABS", "ABS_MT_SLOT", 1),
        ("EV_ABS", "ABS_MT_TRACKING_ID", -1),
        ("EV_KEY", "BTN_TOUCH", 0),
        ("EV_KEY", "BTN_TOOL_DOUBLETAP", 0),
    ]);
}

/// What only the probe answers: its files, its window's readiness and its
/// replies to the function keys it listens for.
impl Device {
    /// The probe's file `name`, or `None` when it does not exist yet.
    fn read(&self, name: &str) -> anyhow::Result<Option<String>> {
        self.read_app_file(PROBE.package, &format!("files/{name}"))
    }

    /// Start the probe afresh; its window's scale once the window is ready,
    /// `None` when it did not get ready.
    fn launch(&self) -> anyhow::Result<Option<f64>> {
        self.shell(&format!("am force-stop {}", PROBE.package))?;
        self.shell(&format!("run-as {} rm -f files/status.txt", PROBE.package))?;
        self.shell(&format!("am start -W -n {}", PROBE.activity()))?;
        let deadline = Instant::now() + READY_WITHIN;
        while Instant::now() < deadline {
            let ready = self.read("status.txt")?.and_then(|status| {
                status.lines().find_map(|line| {
                    let scale = line.strip_prefix("READY scale=")?;
                    scale.split_whitespace().next()?.parse().ok()
                })
            });
            if ready.is_some() {
                return Ok(ready);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(None)
    }

    /// Press `key` and wait for the probe's answer: the newest status line
    /// starting with `marker`, once one more arrived than before the press.
    fn ask(&self, key: &str, marker: &str) -> anyhow::Result<Option<String>> {
        let count = |status: &str| {
            status
                .lines()
                .filter(|line| line.starts_with(marker))
                .count()
        };
        let before = self.read("status.txt")?.map_or(0, |status| count(&status));
        self.key(key)?;
        let deadline = Instant::now() + ANSWER_WITHIN;
        while Instant::now() < deadline {
            if let Some(status) = self.read("status.txt")?
                && count(&status) > before
            {
                return Ok(status
                    .lines()
                    .rfind(|line| line.starts_with(marker))
                    .map(str::to_owned));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    }

    /// Wait for the probe's newest status line starting with `marker`.
    fn wait_for(&self, marker: &str) -> anyhow::Result<Option<String>> {
        let deadline = Instant::now() + ANSWER_WITHIN;
        while Instant::now() < deadline {
            if let Some(line) = self.read("status.txt")?.and_then(|status| {
                status
                    .lines()
                    .rfind(|line| line.starts_with(marker))
                    .map(str::to_owned)
            }) {
                return Ok(Some(line));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    }

    /// Ask the probe for its preferences, past its 500 ms preference sample.
    fn preferences(&self) -> anyhow::Result<Option<String>> {
        std::thread::sleep(Duration::from_millis(1200));
        self.ask("KEYCODE_F10", "PREFERENCES")
    }
}
