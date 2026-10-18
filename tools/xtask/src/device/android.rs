//! What the Android device checks share: the SDK's packaging of a
//! `NativeActivity` library into a debuggable APK, the attached device over
//! `adb`, and scripts for the device's `uinput` command.
//!
//! An application here is a Rust `cdylib` with no Java of its own: the
//! manifest names `android.app.NativeActivity` and the library, and `FluiView`'s
//! dex travels inside the library (ADR-0237), so the APK has no
//! `classes.dex`. No Gradle: `aapt2` links the manifest, the JDK's `jar`
//! stores the library uncompressed, `zipalign` page-aligns it and
//! `apksigner` signs it with the SDK's debug keystore.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context as _, bail};

/// A `NativeActivity` application: its id, the library its activity loads
/// and the label the launcher shows.
#[derive(Debug, Clone, Copy)]
pub(super) struct Apk {
    /// The application id, which is also the manifest's package.
    pub(super) package: &'static str,
    /// The library the manifest names, without `lib` and `.so`.
    pub(super) library: &'static str,
    /// The launcher label.
    pub(super) label: &'static str,
}

impl Apk {
    /// The component `am start -n` launches.
    pub(super) fn activity(&self) -> String {
        format!("{}/android.app.NativeActivity", self.package)
    }

    /// A debuggable `NativeActivity` with no Java, allowed to vibrate as
    /// every FLUI app is (haptic patterns need it). Debuggable, so `run-as`
    /// reaches its files.
    fn manifest(&self) -> String {
        let Self {
            package,
            library,
            label,
        } = self;
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{package}">
    <uses-permission android:name="android.permission.VIBRATE"/>
    <application android:label="{label}" android:hasCode="false" android:debuggable="true">
        <activity android:name="android.app.NativeActivity" android:exported="true"
            android:configChanges="orientation|keyboardHidden|screenSize|screenLayout|smallestScreenSize|uiMode|density">
            <meta-data android:name="android.app.lib_name" android:value="{library}"/>
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
        )
    }
}

/// The SDK pieces the checks run.
pub(super) struct Sdk {
    pub(super) adb: PathBuf,
    aapt2: PathBuf,
    zipalign: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
    api_level: u32,
}

impl Sdk {
    /// The SDK at `ANDROID_HOME` (or `ANDROID_SDK_ROOT`) with its newest
    /// build tools and platform.
    pub(super) fn locate() -> Option<Self> {
        let root = std::env::var_os("ANDROID_HOME")
            .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
            .map(PathBuf::from)?;
        let build_tools = newest(&root.join("build-tools"), |_| true)?;
        let platform = newest(&root.join("platforms"), |dir| {
            dir.join("android.jar").is_file()
        })?;
        let api_level = platform
            .file_name()?
            .to_str()?
            .strip_prefix("android-")?
            .parse()
            .ok()?;
        let tool = |dir: &Path, name: &str| {
            [
                format!("{name}.exe"),
                format!("{name}.bat"),
                name.to_owned(),
            ]
            .into_iter()
            .map(|file| dir.join(file))
            .find(|path| path.is_file())
        };
        Some(Self {
            adb: tool(&root.join("platform-tools"), "adb")?,
            aapt2: tool(&build_tools, "aapt2")?,
            zipalign: tool(&build_tools, "zipalign")?,
            apksigner: tool(&build_tools, "apksigner")?,
            android_jar: platform.join("android.jar"),
            api_level,
        })
    }

    /// A signed APK of `app` with the `library` built for it, under `work`.
    pub(super) fn package(
        &self,
        app: &Apk,
        library: &Path,
        work: &Path,
    ) -> anyhow::Result<PathBuf> {
        let lib_dir = work.join("lib/arm64-v8a");
        std::fs::create_dir_all(&lib_dir)
            .with_context(|| format!("creating {}", lib_dir.display()))?;
        let staged = format!("lib/arm64-v8a/lib{}.so", app.library);
        std::fs::copy(library, work.join(&staged))
            .with_context(|| format!("copying {}", library.display()))?;
        let manifest = work.join("AndroidManifest.xml");
        std::fs::write(&manifest, app.manifest())?;
        let (base, aligned, apk) = (
            work.join("base.apk"),
            work.join("aligned.apk"),
            work.join(format!("{}.apk", app.library)),
        );
        Run(Command::new(&self.aapt2))
            .args(["link", "--manifest"])
            .arg(&manifest)
            .arg("-I")
            .arg(&self.android_jar)
            .args(["--min-sdk-version", "26", "--target-sdk-version"])
            .arg(self.api_level.to_string())
            .arg("-o")
            .arg(&base)
            .check()?;
        let mut store = Command::new(jdk_tool("jar"));
        store
            .current_dir(work)
            .args(["-u", "-0", "-f", "base.apk"])
            .arg(staged);
        Run(store).check()?;
        Run(Command::new(&self.zipalign))
            .args(["-f", "-p", "4"])
            .arg(&base)
            .arg(&aligned)
            .check()?;
        let keystore = home()?.join(".android/debug.keystore");
        if !keystore.is_file() {
            bail!(
                "no debug keystore at {} (any Android SDK build creates one)",
                keystore.display()
            );
        }
        Run(Command::new(&self.apksigner))
            .args(["sign", "--ks"])
            .arg(&keystore)
            .args([
                "--ks-pass",
                "pass:android",
                "--ks-key-alias",
                "androiddebugkey",
                "--key-pass",
                "pass:android",
                "--out",
            ])
            .arg(&apk)
            .arg(&aligned)
            .check()?;
        Ok(apk)
    }
}

/// `name` from `JAVA_HOME`'s `bin`, else from `PATH`.
fn jdk_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(name))
        .filter(|path| path.with_extension("exe").is_file() || path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("neither HOME nor USERPROFILE is set")
}

/// The subdirectory of `parent` with the highest version-like name that
/// `accept`s.
fn newest(parent: &Path, accept: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && accept(path))
        .max_by_key(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
}

/// A command that must succeed.
pub(super) struct Run(pub(super) Command);

impl Run {
    pub(super) fn arg(mut self, arg: impl AsRef<std::ffi::OsStr>) -> Self {
        self.0.arg(arg);
        self
    }

    pub(super) fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.0.args(args);
        self
    }

    /// Its output, when it exited 0.
    pub(super) fn output(mut self) -> anyhow::Result<Output> {
        let output = self
            .0
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {:?}", self.0))?;
        if !output.status.success() {
            bail!(
                "{:?} failed ({}):\n{}{}",
                self.0,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(output)
    }

    pub(super) fn check(self) -> anyhow::Result<()> {
        self.output().map(drop)
    }
}

/// The display in physical pixels, in its natural orientation.
#[derive(Debug, Clone, Copy)]
pub(super) struct Screen {
    pub(super) width: i32,
    pub(super) height: i32,
}

/// The attached device a check drives.
pub(super) struct Device {
    pub(super) adb: PathBuf,
    pub(super) serial: String,
}

impl Device {
    /// `FLUI_ANDROID_SERIAL`, else the only authorized device `adb` lists.
    pub(super) fn find(adb: &Path) -> anyhow::Result<Option<Self>> {
        if let Some(serial) = std::env::var_os("FLUI_ANDROID_SERIAL") {
            return Ok(Some(Self {
                adb: adb.to_path_buf(),
                serial: serial.to_string_lossy().into_owned(),
            }));
        }
        let listed = Run(Command::new(adb)).arg("devices").output()?;
        let serials: Vec<String> = String::from_utf8_lossy(&listed.stdout)
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (serial, state) = line.split_once('\t')?;
                (state.trim() == "device").then(|| serial.to_owned())
            })
            .collect();
        match serials.as_slice() {
            [serial] => Ok(Some(Self {
                adb: adb.to_path_buf(),
                serial: serial.clone(),
            })),
            [] => Ok(None),
            _ => bail!("several devices are attached; FLUI_ANDROID_SERIAL picks one: {serials:?}"),
        }
    }

    pub(super) fn describe(&self) -> String {
        let property = |name: &str| {
            self.shell(&format!("getprop {name}"))
                .map(|value| value.trim().to_owned())
                .unwrap_or_default()
        };
        format!(
            "{} ({} {}, Android {}, API {})",
            self.serial,
            property("ro.product.manufacturer"),
            property("ro.product.model"),
            property("ro.build.version.release"),
            property("ro.build.version.sdk")
        )
    }

    pub(super) fn adb<I, S>(&self, args: I) -> Run
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Run(Command::new(&self.adb))
            .args(["-s", self.serial.as_str()])
            .args(args)
    }

    /// `adb shell command`'s output.
    pub(super) fn shell(&self, command: &str) -> anyhow::Result<String> {
        let output = self.adb(["shell", command]).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// `package`'s file `name` under its data directory, read as the
    /// application's own user, or `None` when it does not exist.
    pub(super) fn read_app_file(
        &self,
        package: &str,
        name: &str,
    ) -> anyhow::Result<Option<String>> {
        let output = Command::new(&self.adb)
            .args(["-s", self.serial.as_str(), "exec-out", "run-as", package])
            .args(["cat", name])
            .stdin(Stdio::null())
            .output()
            .context("reading an application file")?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    pub(super) fn key(&self, code: &str) -> anyhow::Result<()> {
        self.shell(&format!("input keyevent {code}")).map(drop)
    }

    /// Wakes the display and dismisses an insecure keyguard.
    pub(super) fn wake(&self) -> anyhow::Result<()> {
        self.shell("input keyevent KEYCODE_WAKEUP")?;
        self.shell("wm dismiss-keyguard").map(drop)
    }

    /// The physical display size.
    pub(super) fn screen(&self) -> anyhow::Result<Screen> {
        let size = self.shell("wm size")?;
        let physical = size
            .lines()
            .find_map(|line| line.trim().strip_prefix("Physical size: "))
            .context("`wm size` names no physical size")?;
        let (width, height) = physical
            .split_once('x')
            .context("`wm size` is not WIDTHxHEIGHT")?;
        Ok(Screen {
            width: width.trim().parse()?,
            height: height.trim().parse()?,
        })
    }

    /// Run `script` with the device's `uinput` command.
    pub(super) fn uinput(&self, name: &str, script: &str) -> anyhow::Result<()> {
        let local = std::env::temp_dir().join(format!("flui-android-{name}.json"));
        std::fs::write(&local, script)?;
        let remote = format!("/data/local/tmp/flui-{name}.json");
        self.adb(["push"]).arg(&local).arg(&remote).check()?;
        self.shell(&format!("uinput {remote}")).map(drop)
    }
}

/// One key of the on-screen keyboard, from a `uiautomator dump`.
#[derive(Debug, Clone)]
pub(super) struct ImeKey {
    pub(super) label: String,
    pub(super) center: (i32, i32),
}

/// The keys of the input method's window, by their accessibility labels, in
/// the dump's (reading) order.
pub(super) fn keyboard_keys(dump: &str, package: &str) -> Vec<ImeKey> {
    let node = regex::Regex::new(r"<node [^>]*>").expect("BUG: a valid pattern");
    let attribute = |node: &str, name: &str| {
        let start = node.find(&format!(" {name}=\""))? + name.len() + 3;
        let end = node[start..].find('"')? + start;
        Some(node[start..end].to_owned())
    };
    node.find_iter(dump)
        .filter_map(|found| {
            let node = found.as_str();
            if attribute(node, "package")? != package {
                return None;
            }
            let label = attribute(node, "content-desc")?;
            let bounds = attribute(node, "bounds")?;
            let numbers: Vec<i32> = bounds
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse().ok())
                .collect();
            let [left, top, right, bottom] = <[i32; 4]>::try_from(numbers).ok()?;
            (!label.is_empty()).then(|| ImeKey {
                label,
                center: (i32::midpoint(left, right), i32::midpoint(top, bottom)),
            })
        })
        .collect()
}

/// A virtual input device for Android's `uinput` command: its keys, its
/// absolute axes as `(code, minimum, maximum, resolution)` and its
/// properties. Every name is a Linux input constant, so none needs escaping.
pub(super) struct Registration<'a> {
    pub(super) id: u32,
    pub(super) name: &'a str,
    pub(super) product: u32,
    pub(super) keys: &'a [&'a str],
    pub(super) axes: &'a [(&'a str, i32, i32, i32)],
    pub(super) props: &'a [&'a str],
}

impl Registration<'_> {
    /// The register command. The command reads its fields in order and a
    /// configuration's `data` by its `type`, so the JSON is written by hand
    /// in that order rather than through a map.
    pub(super) fn command(&self) -> String {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",")
        };
        let codes: Vec<&str> = self.axes.iter().map(|axis| axis.0).collect();
        let infos = self
            .axes
            .iter()
            .map(|(code, minimum, maximum, resolution)| {
                format!(
                    "{{\"code\":\"{code}\",\"info\":{{\"value\":0,\"minimum\":{minimum},\"maximum\":{maximum},\"fuzz\":0,\"flat\":0,\"resolution\":{resolution}}}}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"id\":{},\"command\":\"register\",\"name\":\"{}\",\"vid\":6353,\"pid\":{},\"bus\":\"usb\",\"configuration\":[{{\"type\":\"UI_SET_EVBIT\",\"data\":[\"EV_KEY\",\"EV_ABS\"]}},{{\"type\":\"UI_SET_KEYBIT\",\"data\":[{}]}},{{\"type\":\"UI_SET_ABSBIT\",\"data\":[{}]}},{{\"type\":\"UI_SET_PROPBIT\",\"data\":[{}]}}],\"abs_info\":[{infos}]}}",
            self.id,
            self.name,
            self.product,
            names(self.keys),
            names(&codes),
            names(self.props),
        )
    }
}

/// A script for Android's `uinput` command: JSON objects, one per line.
pub(super) struct Script {
    id: u32,
    lines: Vec<String>,
}

impl Script {
    /// A script that registers `device` first.
    pub(super) fn new(device: &Registration<'_>) -> Self {
        Self {
            id: device.id,
            lines: vec![device.command()],
        }
    }

    /// A script for the already registered device `id`.
    pub(super) fn continuing(id: u32) -> Self {
        Self {
            id,
            lines: Vec::new(),
        }
    }

    pub(super) fn delay(&mut self, millis: u32) {
        self.lines.push(format!(
            "{{\"id\":{},\"command\":\"delay\",\"duration\":{millis}}}",
            self.id
        ));
    }

    /// One report: the events, then `SYN_REPORT`.
    pub(super) fn inject(&mut self, events: &[(&str, &str, i32)]) {
        let events = events
            .iter()
            .chain([&("EV_SYN", "SYN_REPORT", 0)])
            .map(|(kind, code, value)| format!("\"{kind}\",\"{code}\",{value}"))
            .collect::<Vec<_>>()
            .join(",");
        self.lines.push(format!(
            "{{\"id\":{},\"command\":\"inject\",\"events\":[{events}]}}",
            self.id
        ));
    }

    pub(super) fn finish(self) -> String {
        self.lines.join("\n") + "\n"
    }
}
