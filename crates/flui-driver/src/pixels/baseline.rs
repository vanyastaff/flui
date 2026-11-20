//! Baselines: the PNG a capture is held to, one per source, backend and OS, since GPU and OS
//! rasterization differ between them (kittest documents the sources: MSAA, filtering, float
//! rounding).
//!
//! A baseline lives at `<dir>/<name>/<source>-<backend>-<os>.png`. A comparison that does not
//! match leaves what it saw beside it, `<stem>.new.png`, and for a mismatch of one size the diff,
//! `<stem>.diff.png`; a match removes both. A missing baseline is a failure, never a pass.
//!
//! The update modes are Playwright's: [`UpdateMode::None`] (the default), `missing` (create
//! absent baselines only), `changed` (rewrite failing ones) and `all`. A check that wrote a
//! baseline still fails, so a run that only created baselines is never green, and the
//! `FLUI_DRIVER_UPDATE_BASELINES` variable that asks for one is refused when `CI` is set: no
//! update runs implicitly in CI. The review of a written baseline is the git diff of its file.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flui_protocol::capture::CaptureSource;

use super::{Capture, DeviceRect, Image, MASK_COLOR, Tolerance, codec, metric};
use crate::error::{Error, Result};

/// Which baselines a comparison may write: see the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "Playwright's four update modes (none, missing, changed, all), the set the driver \
              keeps so a baseline workflow means the same in both; a fifth is a decision, not an \
              addition"
)]
pub enum UpdateMode {
    /// None: a missing or differing baseline fails and leaves `.new.png` beside it.
    #[default]
    None,
    /// Create the baselines that are missing.
    Missing,
    /// Rewrite the baselines that differ, and create the missing ones.
    Changed,
    /// Rewrite every baseline compared.
    All,
}

impl UpdateMode {
    /// The environment variable that asks for an update: `none`, `missing`, `changed` or `all`.
    pub const ENV: &'static str = "FLUI_DRIVER_UPDATE_BASELINES";

    /// The mode's spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Missing => "missing",
            Self::Changed => "changed",
            Self::All => "all",
        }
    }

    /// The mode [`Self::ENV`] asks for, refused when the `CI` variable is set.
    ///
    /// # Errors
    ///
    /// As [`Self::from_setting`].
    pub fn from_env() -> Result<Self> {
        let setting = std::env::var(Self::ENV).ok();
        let ci = std::env::var("CI").ok();
        Self::from_setting(setting.as_deref(), ci.as_deref())
    }

    /// The mode `setting` spells (none when absent or empty), where `ci` is the `CI` variable's
    /// value: an update is refused in CI, where it would run implicitly.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a spelling that is no mode, and for any mode but `none`
    /// when `ci` is set to anything but empty, `0` or `false`.
    pub fn from_setting(setting: Option<&str>, ci: Option<&str>) -> Result<Self> {
        let mode = match setting.map_or_default(str::trim) {
            "" | "none" => Self::None,
            "missing" => Self::Missing,
            "changed" => Self::Changed,
            "all" => Self::All,
            other => {
                return Err(Error::invalid_argument(format!(
                    "{} is `none`, `missing`, `changed` or `all`, not `{other}`",
                    Self::ENV
                )));
            }
        };
        let in_ci = ci.is_some_and(|ci| !matches!(ci.trim(), "" | "0" | "false"));
        if mode != Self::None && in_ci {
            return Err(Error::invalid_argument(format!(
                "{}={} is refused in CI: baselines are updated on a contributor's machine and \
                 reviewed as a diff",
                Self::ENV,
                mode.name()
            )));
        }
        Ok(mode)
    }

    const fn writes_missing(self) -> bool {
        !matches!(self, Self::None)
    }

    const fn writes_changed(self) -> bool {
        matches!(self, Self::Changed | Self::All)
    }
}

/// What a baseline is keyed by besides its name: the capture's source, the backend that produced
/// it, and the OS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct BaselineKey {
    /// Where the pixels came from.
    pub source: CaptureSource,
    /// What produced them: a lane, an app's renderer.
    pub backend: String,
    /// The OS, as Rust names it (`windows`, `linux`, `macos`).
    pub os: String,
}

impl BaselineKey {
    /// The key of `capture`, taken on this OS.
    #[must_use]
    pub fn of(capture: &Capture) -> Self {
        Self {
            source: capture.source(),
            backend: capture.backend().to_owned(),
            os: std::env::consts::OS.to_owned(),
        }
    }

    /// The file stem, `<source>-<backend>-<os>`, each part cut to letters, digits, `_` and `-`.
    #[must_use]
    pub fn stem(&self) -> String {
        format!(
            "{}-{}-{}",
            self.source.name(),
            path_safe(&self.backend),
            path_safe(&self.os)
        )
    }
}

/// `text` with every character but an ASCII letter, digit, `_` and `-` replaced by `_`.
fn path_safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// What a comparison was made in, recorded beside its outcome: environment drift is the main
/// source of flaky baselines Argos and Playwright document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Environment {
    /// The capture's source.
    pub source: CaptureSource,
    /// What produced the capture.
    pub backend: String,
    /// The OS.
    pub os: String,
    /// Device pixels per logical pixel.
    pub scale: f64,
}

/// How a comparison with a baseline came out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaselineStatus {
    /// The capture matched within the tolerance.
    Matched {
        /// The pixels that differed and the tolerance let pass.
        differing: u64,
    },
    /// No baseline: what was captured is at `new`.
    Missing {
        /// The capture, for review.
        new: PathBuf,
    },
    /// The baseline has another size: what was captured is at `new`.
    SizeDiffers {
        /// The baseline's width and height.
        expected: (u32, u32),
        /// The capture's.
        actual: (u32, u32),
        /// The capture, for review.
        new: PathBuf,
    },
    /// More pixels differed than the tolerance lets pass.
    Differs {
        /// The pixels that differed.
        differing: u64,
        /// The pixels compared.
        compared: u64,
        /// The capture, for review.
        new: PathBuf,
        /// The expected image with each differing pixel marked.
        diff: PathBuf,
    },
    /// The update mode wrote the baseline: a check that wrote one is not a pass.
    Written {
        /// Whether a baseline was there before.
        replaced: bool,
    },
}

/// One comparison with a baseline: which file, how it came out, and in what environment.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Comparison {
    /// The baseline's name.
    pub name: String,
    /// Its file.
    pub baseline: PathBuf,
    /// How it came out.
    pub status: BaselineStatus,
    /// What it was made in.
    pub environment: Environment,
}

impl Comparison {
    /// Whether the capture matched its baseline: nothing else is a pass.
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.status, BaselineStatus::Matched { .. })
    }

    /// The comparison when it passed.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming the baseline and what came out instead when it did not.
    pub fn into_result(self) -> Result<Self> {
        if self.passed() {
            return Ok(self);
        }
        Err(Error::Expectation {
            expected: format!("the capture to match {}", self.baseline.display()),
            actual: self.to_string(),
        })
    }

    /// The files a reviewer looks at: the capture and the diff the comparison left.
    #[must_use]
    pub fn left_behind(&self) -> Vec<&Path> {
        match &self.status {
            BaselineStatus::Missing { new } | BaselineStatus::SizeDiffers { new, .. } => {
                vec![new.as_path()]
            }
            BaselineStatus::Differs { new, diff, .. } => vec![new.as_path(), diff.as_path()],
            BaselineStatus::Matched { .. } | BaselineStatus::Written { .. } => Vec::new(),
        }
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.status {
            BaselineStatus::Matched { differing } => {
                write!(f, "matched ({differing} pixels within the tolerance)")?;
            }
            BaselineStatus::Missing { new } => {
                write!(f, "no baseline; the capture is at {}", new.display())?;
            }
            BaselineStatus::SizeDiffers {
                expected,
                actual,
                new,
            } => write!(
                f,
                "the baseline is {} x {} and the capture {} x {}, at {}",
                expected.0,
                expected.1,
                actual.0,
                actual.1,
                new.display()
            )?,
            BaselineStatus::Differs {
                differing,
                compared,
                new,
                diff,
            } => write!(
                f,
                "{differing} of {compared} pixels differ; the capture is at {} and the diff at {}",
                new.display(),
                diff.display()
            )?,
            BaselineStatus::Written { replaced: true } => {
                write!(f, "the baseline was rewritten; review it and run again")?;
            }
            BaselineStatus::Written { replaced: false } => {
                write!(f, "the baseline was created; review it and run again")?;
            }
        }
        write!(
            f,
            " [{} from {} on {} at scale {}]",
            self.environment.source.name(),
            self.environment.backend,
            self.environment.os,
            self.environment.scale
        )
    }
}

/// The baselines under one directory: see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baselines {
    dir: PathBuf,
    update: UpdateMode,
}

impl Baselines {
    /// The baselines under `dir`, written by no update.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            update: UpdateMode::None,
        }
    }

    /// The baselines under `dir`, updated as [`UpdateMode::ENV`] asks.
    ///
    /// # Errors
    ///
    /// As [`UpdateMode::from_env`].
    pub fn from_env(dir: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self::new(dir).update(UpdateMode::from_env()?))
    }

    /// These baselines, written as `mode` lets.
    #[must_use]
    pub fn update(mut self, mode: UpdateMode) -> Self {
        self.update = mode;
        self
    }

    /// The directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The update mode.
    #[must_use]
    pub fn update_mode(&self) -> UpdateMode {
        self.update
    }

    /// The file of the baseline `name` under `key`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a name that is not one path segment of letters, digits,
    /// `_`, `-` and `.`, starting with neither `.` nor `-`.
    pub fn path(&self, name: &str, key: &BaselineKey) -> Result<PathBuf> {
        check_name(name)?;
        Ok(self.dir.join(name).join(format!("{}.png", key.stem())))
    }

    /// Compare `capture` with the baseline `name`, after painting `masks` over both: see the
    /// [module docs](self) for what it writes.
    ///
    /// # Errors
    ///
    /// As [`Self::path`]; [`Error::Io`] when a file cannot be written or the baseline read;
    /// [`Error::Unreadable`] for a baseline past a bound or not a PNG. A capture that does not
    /// match is an answer, not an error: see [`Comparison::into_result`].
    pub fn compare(
        &self,
        name: &str,
        capture: &Capture,
        masks: &[DeviceRect],
        tolerance: &Tolerance,
    ) -> Result<Comparison> {
        let key = BaselineKey::of(capture);
        let baseline = self.path(name, &key)?;
        let stem = key.stem();
        let folder = self.dir.join(name);
        let new = folder.join(format!("{stem}.new.png"));
        let diff = folder.join(format!("{stem}.diff.png"));
        let actual = masked(capture.image(), masks);

        let status = if baseline.exists() {
            let expected = masked(&codec::read(&baseline)?, masks);
            self.against(&expected, &actual, tolerance, (&baseline, &new, &diff))?
        } else if self.update.writes_missing() {
            write_png(&baseline, &actual)?;
            BaselineStatus::Written { replaced: false }
        } else {
            write_png(&new, &actual)?;
            BaselineStatus::Missing { new: new.clone() }
        };
        if matches!(
            status,
            BaselineStatus::Matched { .. } | BaselineStatus::Written { .. }
        ) {
            remove_if_present(&new)?;
            remove_if_present(&diff)?;
        }
        Ok(Comparison {
            name: name.to_owned(),
            baseline,
            status,
            environment: Environment {
                source: key.source,
                backend: key.backend,
                os: key.os,
                scale: capture.scale(),
            },
        })
    }

    /// How `actual` came out against the `expected` baseline, writing what the update mode or
    /// the outcome asks for.
    fn against(
        &self,
        expected: &Image,
        actual: &Image,
        tolerance: &Tolerance,
        (baseline, new, diff): (&Path, &Path, &Path),
    ) -> Result<BaselineStatus> {
        if (expected.width(), expected.height()) != (actual.width(), actual.height()) {
            if self.update.writes_changed() {
                write_png(baseline, actual)?;
                return Ok(BaselineStatus::Written { replaced: true });
            }
            write_png(new, actual)?;
            remove_if_present(diff)?;
            return Ok(BaselineStatus::SizeDiffers {
                expected: (expected.width(), expected.height()),
                actual: (actual.width(), actual.height()),
                new: new.to_owned(),
            });
        }
        let difference = metric::compare(expected, actual, tolerance)?;
        let matched = difference.within(tolerance);
        if self.update == UpdateMode::All || (!matched && self.update.writes_changed()) {
            write_png(baseline, actual)?;
            return Ok(BaselineStatus::Written { replaced: true });
        }
        if matched {
            return Ok(BaselineStatus::Matched {
                differing: difference.differing,
            });
        }
        write_png(new, actual)?;
        write_png(diff, &difference.diff)?;
        Ok(BaselineStatus::Differs {
            differing: difference.differing,
            compared: difference.compared,
            new: new.to_owned(),
            diff: diff.to_owned(),
        })
    }
}

/// `image` with each of `masks` painted [`MASK_COLOR`].
fn masked(image: &Image, masks: &[DeviceRect]) -> Image {
    let mut image = image.clone();
    for mask in masks {
        image.fill(*mask, MASK_COLOR);
    }
    image
}

/// Whether `name` is one safe path segment.
fn check_name(name: &str) -> Result<()> {
    let safe = !name.is_empty()
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if safe {
        return Ok(());
    }
    Err(Error::invalid_argument(format!(
        "a baseline's name is one path segment of letters, digits, `_`, `-` and `.`, starting \
         with neither `.` nor `-`: `{name}` is not"
    )))
}

/// Write `image` to `path` as PNG, its directory created when missing.
fn write_png(path: &Path, image: &Image) -> Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(path, codec::encode(image)?)?;
    Ok(())
}

/// Remove the file at `path` when there is one.
fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}
