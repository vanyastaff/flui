//! A window's captures, stable captures, masks and baseline comparisons, as [`Window`] methods.
//!
//! Each is a command of a trace: `capture.<source>` for a capture, `pixels.expect_baseline` and
//! `pixels.check` for the comparisons, the pixels compared kept as the trace's evidence.

use std::time::Duration;

use flui_protocol::capture::CaptureSource;
use flui_protocol::locator::LocateParams;
use flui_protocol::method::{self, Method as _};
use flui_protocol::tree::{Rect, WindowId};
use flui_protocol::wait::Condition;
use serde::Serialize;

use super::{Baselines, Capture, Comparison, DeviceRect, Mask, Tolerance};
use crate::driver::Driver;
use crate::error::{Error, Result};
use crate::window::Window;

/// How long a stable capture waits for the window's next frame before it captures again: a
/// window that presents none is as stable as it gets.
const FRAME_WAIT: Duration = Duration::from_millis(250);

/// The most captures a stable capture takes before it gives up, whatever its timeout: a target
/// whose clock does not move (an app that serves no waits) still ends.
pub const MAX_STABLE_CAPTURES: u32 = 64;

/// The most frames [`Window::expect_stable_for`] watches.
pub const MAX_STABLE_FRAMES: u32 = 600;

/// How a baseline comparison captures and compares: see [`Window::expect_baseline`].
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct BaselineOptions {
    source: CaptureSource,
    masks: Vec<Mask>,
    tolerance: Tolerance,
    timeout: Option<Duration>,
}

impl BaselineOptions {
    /// Capturing from `source`, masking nothing, with the default tolerance and the session's
    /// timeout for a stable capture.
    pub fn new(source: CaptureSource) -> Self {
        Self {
            source,
            masks: Vec::new(),
            tolerance: Tolerance::default(),
            timeout: None,
        }
    }

    /// Masking `mask` too.
    pub fn mask(mut self, mask: impl Into<Mask>) -> Self {
        self.masks.push(mask.into());
        self
    }

    /// Comparing within `tolerance`.
    pub fn tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Giving the capture `timeout` to become stable.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// A command's parameters as a trace writes them.
#[derive(Serialize)]
struct CaptureCommand<'a> {
    window: WindowId,
    source: CaptureSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frames: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a str>,
}

impl Window<'_> {
    /// One capture of the window from `source`.
    ///
    /// # Errors
    ///
    /// For `os`: [`Error::NotSupported`] naming `os_capture` without a lane that captures, and
    /// the lane's failure. For `presented` and `rendered`: [`Error::NotSupported`] naming the
    /// method when the app's agent does not serve it, and naming `capture_payload` when it does,
    /// since the pixels follow its answer as a binary frame this driver's connections do not read
    /// yet.
    pub fn capture(&mut self, source: CaptureSource) -> Result<Capture> {
        let id = self.id;
        let command = CaptureCommand {
            window: id,
            source,
            stable_ms: None,
            frames: None,
            baseline: None,
        };
        let mut kept = None;
        self.driver
            .command(capture_method(source), command, |driver, _| {
                let capture = capture(driver, id, source)?;
                driver.trace_evidence(capture.image());
                let meta = Meta::of(&capture);
                kept = Some(capture);
                Ok(meta)
            })?;
        kept.ok_or_else(|| Error::Protocol {
            message: "a capture answered no capture".into(),
        })
    }

    /// A capture of the window from `source` once two consecutive captures, a frame apart, are
    /// equal: what a baseline is compared with (`toHaveScreenshot`).
    ///
    /// # Errors
    ///
    /// As [`Self::capture`]; [`Error::Timeout`] when the captures still changed after `timeout`
    /// on the session's clock, or after [`MAX_STABLE_CAPTURES`] captures.
    pub fn capture_stable(&mut self, source: CaptureSource, timeout: Duration) -> Result<Capture> {
        let id = self.id;
        let command = CaptureCommand {
            window: id,
            source,
            stable_ms: Some(millis(timeout)),
            frames: None,
            baseline: None,
        };
        let mut kept = None;
        self.driver
            .command(capture_method(source), command, |driver, _| {
                let capture = stable(driver, id, source, timeout, &[])?;
                driver.trace_evidence(capture.image());
                let meta = Meta::of(&capture);
                kept = Some(capture);
                Ok(meta)
            })?;
        kept.ok_or_else(|| Error::Protocol {
            message: "a stable capture answered no capture".into(),
        })
    }

    /// Whether the window's captures from `source` stay the same for `frames` presented frames:
    /// the last capture when they do.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming the frame it changed after and by how much; as
    /// [`Self::capture`]; [`Error::InvalidArgument`] past [`MAX_STABLE_FRAMES`].
    pub fn expect_stable_for(&mut self, source: CaptureSource, frames: u32) -> Result<Capture> {
        if frames > MAX_STABLE_FRAMES {
            return Err(Error::invalid_argument(format!(
                "a window is watched for at most {MAX_STABLE_FRAMES} frames, not {frames}"
            )));
        }
        let id = self.id;
        let command = CaptureCommand {
            window: id,
            source,
            stable_ms: None,
            frames: Some(frames),
            baseline: None,
        };
        let mut kept = None;
        self.driver
            .command(method_name::CHECK, command, |driver, _| {
                let first = capture(driver, id, source)?;
                let mut last = first.clone();
                for frame in 1..=frames {
                    next_frame(driver, id)?;
                    let now = capture(driver, id, source)?;
                    if now.image() != first.image() {
                        driver.trace_evidence(first.image());
                        driver.trace_evidence(now.image());
                        let difference =
                            super::metric::compare(first.image(), now.image(), &Tolerance::exact())
                                .map_or_else(
                                    |error| error.to_string(),
                                    |difference| difference.to_string(),
                                );
                        return Err(Error::Expectation {
                            expected: format!("the window unchanged for {frames} frames"),
                            actual: format!("it changed after {frame}: {difference}"),
                        });
                    }
                    last = now;
                }
                driver.trace_evidence(last.image());
                let meta = Meta::of(&last);
                kept = Some(last);
                Ok(meta)
            })?;
        kept.ok_or_else(|| Error::Protocol {
            message: "a stability check answered no capture".into(),
        })
    }

    /// Compare a stable capture of the window with the baseline `name` in `baselines`, after
    /// painting `options`' masks over both: see [`Baselines::compare`] for the files it reads
    /// and writes.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming the baseline and what came out instead (missing, another
    /// size, differing pixels, written by an update) for any comparison but a match; as
    /// [`Self::capture_stable`] and [`Baselines::compare`]; [`Error::NotFound`] for a mask whose
    /// locator matches nothing.
    pub fn expect_baseline(
        &mut self,
        baselines: &Baselines,
        name: &str,
        options: &BaselineOptions,
    ) -> Result<Comparison> {
        let id = self.id;
        let timeout = options.timeout.unwrap_or(self.driver.timeout);
        let command = CaptureCommand {
            window: id,
            source: options.source,
            stable_ms: Some(millis(timeout)),
            frames: None,
            baseline: Some(name),
        };
        let mut kept = None;
        self.driver
            .command(method_name::EXPECT_BASELINE, command, |driver, _| {
                let scale = driver.window(id).placement()?.scale;
                let masks = mask_rects(driver, id, &options.masks, scale)?;
                let capture = stable(driver, id, options.source, timeout, &masks)?;
                let comparison = baselines.compare(name, &capture, &masks, &options.tolerance)?;
                driver.trace_evidence(capture.image());
                let summary = comparison.to_string();
                let passed = comparison.passed();
                kept = Some(comparison);
                if passed {
                    Ok(summary)
                } else {
                    Err(Error::Expectation {
                        expected: format!("the capture to match the baseline `{name}`"),
                        actual: summary,
                    })
                }
            })?;
        kept.ok_or_else(|| Error::Protocol {
            message: "a baseline comparison answered nothing".into(),
        })
    }
}

/// The names of the driver's pixel commands, until their rows join the protocol's table.
mod method_name {
    pub(super) const OS: &str = "capture.os";
    pub(super) const CHECK: &str = "pixels.check";
    pub(super) const EXPECT_BASELINE: &str = "pixels.expect_baseline";
}

/// What a trace records of a capture beside its evidence.
#[derive(Serialize)]
struct Meta {
    source: CaptureSource,
    backend: String,
    width: u32,
    height: u32,
    scale: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<u64>,
}

impl Meta {
    fn of(capture: &Capture) -> Self {
        Self {
            source: capture.source(),
            backend: capture.backend().to_owned(),
            width: capture.image().width(),
            height: capture.image().height(),
            scale: capture.scale(),
            frame: capture.frame_id(),
        }
    }
}

/// The command a capture from `source` is.
fn capture_method(source: CaptureSource) -> &'static str {
    match source {
        CaptureSource::Presented => method::CapturePresented::NAME,
        CaptureSource::Rendered => method::CaptureRendered::NAME,
        _ => method_name::OS,
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// One capture of window `id` from `source`.
pub(crate) fn capture(driver: &mut Driver, id: WindowId, source: CaptureSource) -> Result<Capture> {
    match source {
        CaptureSource::Presented | CaptureSource::Rendered => {
            let name = capture_method(source);
            if !driver.serves(name) {
                return Err(Error::not_supported(
                    name,
                    format!("the app's agent does not serve `{name}`"),
                ));
            }
            Err(Error::not_supported(
                "capture_payload",
                format!(
                    "the app serves `{name}`, whose pixels follow its answer as a binary frame, \
                     and this driver's connections read no binary frame yet"
                ),
            ))
        }
        _ => {
            let placement = driver.window(id).placement()?;
            let lane = driver
                .lane()
                .ok_or_else(|| Error::not_supported("os_capture", "the session has no OS lane"))?;
            let backend = lane.name().to_owned();
            let scale = placement.scale;
            let image = lane.run(move |lane| lane.capture_window(&placement))??;
            Ok(Capture::new(image, CaptureSource::Os, backend, scale))
        }
    }
}

/// A capture of window `id` once two consecutive ones are equal outside `masks`, which hide what
/// is meant to move.
fn stable(
    driver: &mut Driver,
    id: WindowId,
    source: CaptureSource,
    timeout: Duration,
    masks: &[DeviceRect],
) -> Result<Capture> {
    let masked = |capture: &Capture| {
        let mut image = capture.image().clone();
        for mask in masks {
            image.fill(*mask, super::MASK_COLOR);
        }
        image
    };
    let deadline = driver.clock().saturating_add(timeout);
    let mut last = capture(driver, id, source)?;
    for _ in 1..MAX_STABLE_CAPTURES {
        next_frame(driver, id)?;
        let now = capture(driver, id, source)?;
        if masked(&now) == masked(&last) {
            return Ok(now);
        }
        if driver.clock() >= deadline {
            break;
        }
        last = now;
    }
    Err(Error::Timeout {
        effect: None,
        message: format!(
            "the window's captures still changed after {} ms or {MAX_STABLE_CAPTURES} captures; \
             mask what moves",
            timeout.as_millis()
        ),
        retry: flui_protocol::error::Retry::Soon,
    })
}

/// Wait for window `id`'s next frame where the app serves waits; a window that presents none
/// within [`FRAME_WAIT`] has nothing newer to show.
fn next_frame(driver: &mut Driver, id: WindowId) -> Result<()> {
    if driver.serves(method::WaitUntil::NAME) {
        crate::compose::wait_until(driver, id, Condition::Frames(1), FRAME_WAIT)?;
    }
    Ok(())
}

/// The device rectangles `masks` cover in window `id`, captured at `scale`.
fn mask_rects(
    driver: &mut Driver,
    id: WindowId,
    masks: &[Mask],
    scale: f64,
) -> Result<Vec<DeviceRect>> {
    let mut rects = Vec::new();
    for mask in masks {
        match mask {
            Mask::Locator(locator) => {
                let found = driver.agent::<method::SemanticsLocate>(
                    LocateParams::new(locator.clone().in_window(id)),
                    Duration::ZERO,
                )?;
                if found.truncated > 0 {
                    return Err(Error::invalid_argument(format!(
                        "a mask's locator matches more elements than one read returns ({} left \
                         out): narrow it",
                        found.truncated
                    )));
                }
                if found.matches.is_empty() {
                    return Err(Error::not_found(format!(
                        "a mask's locator matches nothing: {locator:?}"
                    )));
                }
                for matched in &found.matches {
                    let bounds = matched.node.surface_rect.ok_or_else(|| {
                        Error::invalid_argument(format!(
                            "{} has no bounds on the window's surface to mask",
                            matched.element
                        ))
                    })?;
                    rects.push(device_rect(bounds));
                }
            }
            Mask::Region(region) => rects.push(DeviceRect::of_logical(*region, scale)?),
        }
    }
    Ok(rects)
}

/// `bounds` on the surface, the part left of or above it cut off.
fn device_rect(bounds: Rect) -> DeviceRect {
    let cut = |edge: i32, size: u32| -> (u32, u32) {
        if edge >= 0 {
            (edge.unsigned_abs(), size)
        } else {
            (0, size.saturating_sub(edge.unsigned_abs()))
        }
    };
    let (x, width) = cut(bounds.x, bounds.width);
    let (y, height) = cut(bounds.y, bounds.height);
    DeviceRect::new(x, y, width, height)
}
