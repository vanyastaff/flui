//! The application's floor (ADR-0244 §10): the lowest release of each
//! environment an application runs on, which `flui.toml` may raise above
//! FLUI's own and never lower.
//!
//! ```toml
//! [platforms.ios]
//! min = "15.0"
//!
//! [platforms.android]
//! min = 26
//! ```
//!
//! Only the environments with a running version take one: Windows (the build
//! number `RtlGetVersion` reports), macOS and iOS (a dotted release) and
//! Android (the API level). The web has no version to compare, and Linux's
//! floor is an ABI and a set of drivers and protocols, so a `min` for either is
//! refused. The floor a build uses, [`AppFloors`], is the higher of FLUI's and
//! the application's; every file the system reads its minimum from is written
//! from it, and the same value goes to rustc, so the binary and its bundle
//! agree.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::platform_floor;

/// A dotted Apple release, compared component by component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct AppleRelease {
    major: u32,
    minor: u32,
    patch: u32,
}

impl AppleRelease {
    /// The release `(major, minor, patch)`, as the generated floors state it.
    pub(crate) const fn from_parts((major, minor, patch): (u32, u32, u32)) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for AppleRelease {
    type Err = ();

    /// `"13"`, `"13.4"` or `"13.4.1"`: one to three numeric components.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('.');
        let mut next = |required: bool| match parts.next() {
            Some(part) if !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) => {
                part.parse::<u32>().map_err(|_| ())
            }
            None if !required => Ok(0),
            _ => Err(()),
        };
        let release = Self {
            major: next(true)?,
            minor: next(false)?,
            patch: next(false)?,
        };
        if parts.next().is_some() {
            return Err(());
        }
        Ok(release)
    }
}

impl fmt::Display for AppleRelease {
    /// `major.minor`, with `.patch` only when it is not zero: the form
    /// `MinimumOSVersion` and the deployment targets take.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)?;
        if self.patch != 0 {
            write!(formatter, ".{}", self.patch)?;
        }
        Ok(())
    }
}

/// The `[platforms]` tables of `flui.toml`, as written.
///
/// Kept as written so `flui platform` rewrites the file without touching
/// them; [`PlatformsConfig::floors`] reads and checks them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PlatformsConfig(BTreeMap<String, PlatformTable>);

/// One `[platforms.<environment>]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlatformTable {
    /// The lowest release the application runs on.
    pub(crate) min: StatedMin,
}

/// A `min` as written: a number (a Windows build, an Android API level) or
/// text (an Apple release).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StatedMin {
    Number(u32),
    Text(String),
}

impl fmt::Display for StatedMin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(formatter, "{number}"),
            Self::Text(text) => write!(formatter, "\"{text}\""),
        }
    }
}

/// Why a `[platforms]` table of `flui.toml` was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AppFloorError {
    /// The application's `min` is below FLUI's floor, which it may only raise.
    #[error(
        "flui.toml: [platforms.{environment}] min = {stated} is below FLUI's floor for \
         {environment}, {floor}; raise it, or remove the table to use FLUI's floor"
    )]
    BelowFloor {
        environment: &'static str,
        stated: String,
        floor: String,
    },
    /// The environment has no version an application could state.
    #[error("flui.toml: [platforms.{environment}] takes no `min`: {reason}")]
    NoVersion {
        environment: &'static str,
        reason: &'static str,
    },
    /// The `min` is not the kind of value the environment takes.
    #[error("flui.toml: [platforms.{environment}] min = {stated} is not {expected}")]
    Malformed {
        environment: &'static str,
        stated: String,
        expected: &'static str,
    },
    /// The table names no environment FLUI builds for.
    #[error(
        "flui.toml: [platforms.{environment}] names no environment; the tables are windows, \
         macos, ios and android"
    )]
    UnknownEnvironment { environment: String },
}

/// The floors a build uses: FLUI's, raised wherever the application asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct AppFloors {
    windows: Option<u32>,
    macos: Option<AppleRelease>,
    ios: Option<AppleRelease>,
    android: Option<u32>,
}

impl PlatformsConfig {
    /// Whether `flui.toml` has no `[platforms]` table, so `save` writes none.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The floors these tables state, each checked against FLUI's.
    pub(crate) fn floors(&self) -> Result<AppFloors, AppFloorError> {
        let mut floors = AppFloors::default();
        for (environment, table) in &self.0 {
            let stated = &table.min;
            match environment.as_str() {
                "windows" => {
                    floors.windows = Some(at_least(
                        "windows",
                        number("windows", stated, "a Windows build number, like 19041")?,
                        platform_floor::WINDOWS_BUILD,
                        stated,
                    )?);
                }
                "android" => {
                    floors.android = Some(at_least(
                        "android",
                        number("android", stated, "an Android API level, like 26")?,
                        platform_floor::ANDROID_MIN_SDK,
                        stated,
                    )?);
                }
                "macos" => {
                    // The lower of the two architectures' floors: each build
                    // still takes the higher of this and its own.
                    floors.macos = Some(at_least(
                        "macos",
                        release("macos", stated)?,
                        AppleRelease::from_parts(platform_floor::MACOS_X86_64),
                        stated,
                    )?);
                }
                "ios" => {
                    floors.ios = Some(at_least(
                        "ios",
                        release("ios", stated)?,
                        AppleRelease::from_parts(platform_floor::IOS),
                        stated,
                    )?);
                }
                "web" => {
                    return Err(AppFloorError::NoVersion {
                        environment: "web",
                        reason: "a browser's graphics API is detected, never read from its version",
                    });
                }
                "linux" => {
                    return Err(AppFloorError::NoVersion {
                        environment: "linux",
                        reason: "Linux's floor is the C library, a GPU driver and the display \
                                 protocols, not a release",
                    });
                }
                other => {
                    return Err(AppFloorError::UnknownEnvironment {
                        environment: other.to_owned(),
                    });
                }
            }
        }
        Ok(floors)
    }
}

fn number(
    environment: &'static str,
    stated: &StatedMin,
    expected: &'static str,
) -> Result<u32, AppFloorError> {
    match stated {
        StatedMin::Number(number) => Ok(*number),
        StatedMin::Text(_) => Err(AppFloorError::Malformed {
            environment,
            stated: stated.to_string(),
            expected,
        }),
    }
}

fn release(environment: &'static str, stated: &StatedMin) -> Result<AppleRelease, AppFloorError> {
    match stated {
        StatedMin::Text(text) => text.parse().ok(),
        StatedMin::Number(_) => None,
    }
    .ok_or_else(|| AppFloorError::Malformed {
        environment,
        stated: stated.to_string(),
        expected: "a release in quotes, like \"15.0\"",
    })
}

fn at_least<V: Ord + fmt::Display>(
    environment: &'static str,
    value: V,
    floor: V,
    stated: &StatedMin,
) -> Result<V, AppFloorError> {
    if value < floor {
        return Err(AppFloorError::BelowFloor {
            environment,
            stated: stated.to_string(),
            floor: floor.to_string(),
        });
    }
    Ok(value)
}

impl AppFloors {
    /// The Android `minSdk` the APK declares and the NDK links against.
    pub(crate) fn android_api(&self) -> u32 {
        self.android
            .unwrap_or(platform_floor::ANDROID_MIN_SDK)
            .max(platform_floor::ANDROID_MIN_SDK)
    }

    /// The iOS release the bundle and the binary state.
    pub(crate) fn ios(&self) -> AppleRelease {
        let floor = AppleRelease::from_parts(platform_floor::IOS);
        self.ios.map_or(floor, |app| app.max(floor))
    }

    /// The macOS release a build for `triple`'s architecture states.
    pub(crate) fn macos(&self, triple: &str) -> AppleRelease {
        let floor = AppleRelease::from_parts(if triple.starts_with("aarch64") {
            platform_floor::MACOS_AARCH64
        } else {
            platform_floor::MACOS_X86_64
        });
        self.macos.map_or(floor, |app| app.max(floor))
    }

    /// [`cargo_env`](Self::cargo_env) for a build for the machine the CLI
    /// runs on (`flui run` on the desktop).
    pub(crate) fn host_cargo_env(&self) -> Vec<(&'static str, String)> {
        let os = match std::env::consts::OS {
            "macos" => "apple-darwin",
            "windows" => "pc-windows-msvc",
            other => other,
        };
        self.cargo_env(&format!("{}-{os}", std::env::consts::ARCH))
    }

    /// The environment variables a cargo build for `triple` runs with.
    ///
    /// The deployment target rustc links the binary's own minimum with, and
    /// `FLUI_APP_MIN_<ENVIRONMENT>` when the application raised the floor,
    /// which `flui-native`'s build script reads so the start-up refusal
    /// compares against the higher of the two.
    pub(crate) fn cargo_env(&self, triple: &str) -> Vec<(&'static str, String)> {
        let mut env = Vec::new();
        if triple.contains("-apple-ios") {
            env.push(("IPHONEOS_DEPLOYMENT_TARGET", self.ios().to_string()));
            if let Some(app) = self.ios {
                env.push(("FLUI_APP_MIN_IOS", app.to_string()));
            }
        } else if triple.contains("-apple-darwin") {
            env.push(("MACOSX_DEPLOYMENT_TARGET", self.macos(triple).to_string()));
            if let Some(app) = self.macos {
                env.push(("FLUI_APP_MIN_MACOS", app.to_string()));
            }
        } else if triple.contains("-windows") {
            if let Some(app) = self.windows {
                env.push(("FLUI_APP_MIN_WINDOWS", app.to_string()));
            }
        } else if triple.contains("-android")
            && let Some(app) = self.android
        {
            env.push(("FLUI_APP_MIN_ANDROID", app.to_string()));
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::{AppFloorError, AppFloors, AppleRelease, PlatformsConfig};

    fn floors(toml: &str) -> Result<AppFloors, AppFloorError> {
        toml::from_str::<PlatformsConfig>(toml)
            .expect("the tables parse")
            .floors()
    }

    fn release(text: &str) -> AppleRelease {
        text.parse().expect("a release")
    }

    /// A `min` at or above FLUI's floor raises it; below it, for the web or
    /// Linux, of the wrong kind or for no environment, it is refused, naming
    /// what was written and, below the floor, FLUI's floor.
    #[test]
    fn app_floor_raises_and_never_lowers() {
        let refused = [
            (
                "[android]\nmin = 21",
                "flui.toml: [platforms.android] min = 21 is below FLUI's floor for android, 23; \
                 raise it, or remove the table to use FLUI's floor",
            ),
            (
                "[ios]\nmin = \"12.4\"",
                "flui.toml: [platforms.ios] min = \"12.4\" is below FLUI's floor for ios, 13.0; \
                 raise it, or remove the table to use FLUI's floor",
            ),
            (
                "[macos]\nmin = \"10.12\"",
                "flui.toml: [platforms.macos] min = \"10.12\" is below FLUI's floor for macos, \
                 10.13; raise it, or remove the table to use FLUI's floor",
            ),
            (
                "[windows]\nmin = 9600",
                "flui.toml: [platforms.windows] min = 9600 is below FLUI's floor for windows, \
                 10240; raise it, or remove the table to use FLUI's floor",
            ),
            (
                "[web]\nmin = 1",
                "flui.toml: [platforms.web] takes no `min`: a browser's graphics API is detected, \
                 never read from its version",
            ),
            (
                "[linux]\nmin = \"2.31\"",
                "flui.toml: [platforms.linux] takes no `min`: Linux's floor is the C library, a \
                 GPU driver and the display protocols, not a release",
            ),
            (
                "[ios]\nmin = 15",
                "flui.toml: [platforms.ios] min = 15 is not a release in quotes, like \"15.0\"",
            ),
            (
                "[android]\nmin = \"26\"",
                "flui.toml: [platforms.android] min = \"26\" is not an Android API level, like 26",
            ),
            (
                "[tvos]\nmin = \"17.0\"",
                "flui.toml: [platforms.tvos] names no environment; the tables are windows, macos, \
                 ios and android",
            ),
        ];
        let mut failures = Vec::new();
        for (toml, expected) in refused {
            match floors(toml) {
                Err(error) if error.to_string() == expected => {}
                other => failures.push(format!("{toml:?}: {other:?}")),
            }
        }

        let raised = floors(
            "[windows]\nmin = 19041\n[macos]\nmin = \"12.0\"\n[ios]\nmin = \"15.4.1\"\n\
             [android]\nmin = 26",
        )
        .expect("every min is above FLUI's floor");
        let inherited = AppFloors::default();
        for (label, found, expected) in [
            ("android raised", raised.android_api().to_string(), "26"),
            ("ios raised", raised.ios().to_string(), "15.4.1"),
            (
                "macos raised",
                raised.macos("aarch64-apple-darwin").to_string(),
                "12.0",
            ),
            (
                "android inherited",
                inherited.android_api().to_string(),
                "23",
            ),
            ("ios inherited", inherited.ios().to_string(), "13.0"),
            (
                "macos arm64",
                inherited.macos("aarch64-apple-darwin").to_string(),
                "11.0",
            ),
            (
                "macos intel",
                inherited.macos("x86_64-apple-darwin").to_string(),
                "10.13",
            ),
        ] {
            if found != expected {
                failures.push(format!("{label}: {found}"));
            }
        }
        // 10.15 is above the Intel floor but below the arm64 one: each build
        // takes the higher of the two.
        let between = floors("[macos]\nmin = \"10.15\"").expect("above the lower floor");
        if between.macos("aarch64-apple-darwin") != release("11.0")
            || between.macos("x86_64-apple-darwin") != release("10.15")
        {
            failures.push(format!("macos between the floors: {between:?}"));
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The deployment target always goes to rustc; the application's own
    /// value goes to the build only when `flui.toml` states one.
    #[test]
    fn cargo_env_carries_the_deployment_target_and_the_app_floor() {
        let raised = floors("[ios]\nmin = \"15.0\"\n[windows]\nmin = 19041").expect("raised");
        let inherited = AppFloors::default();
        let cases = [
            (
                &raised,
                "aarch64-apple-ios",
                vec![
                    ("IPHONEOS_DEPLOYMENT_TARGET", "15.0"),
                    ("FLUI_APP_MIN_IOS", "15.0"),
                ],
            ),
            (
                &inherited,
                "aarch64-apple-ios-sim",
                vec![("IPHONEOS_DEPLOYMENT_TARGET", "13.0")],
            ),
            (
                &inherited,
                "x86_64-apple-darwin",
                vec![("MACOSX_DEPLOYMENT_TARGET", "10.13")],
            ),
            (
                &raised,
                "x86_64-pc-windows-msvc",
                vec![("FLUI_APP_MIN_WINDOWS", "19041")],
            ),
            (&inherited, "aarch64-linux-android", vec![]),
            (&raised, "wasm32-unknown-unknown", vec![]),
        ];
        let mut failures = Vec::new();
        for (floors, triple, expected) in cases {
            let env = floors.cargo_env(triple);
            let found: Vec<(&str, &str)> = env
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            if found != expected {
                failures.push(format!("{triple}: {found:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
