//! Window and monitor listing and capture, through xcap.
//!
//! Built on Windows and macOS. On Linux xcap links PipeWire and XCB, which
//! the workspace's Linux builds do not install, so there [`windows()`] and
//! [`screenshot()`] report that the OS is not supported yet.

#[cfg(any(target_os = "windows", target_os = "macos", test))]
use image::RgbaImage;
use serde::Serialize;

use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;

/// A top-level window as the OS lists it, before the session names it.
#[derive(Debug, Clone)]
pub struct NativeWindow {
    /// The OS's id (the `HWND` on Windows).
    pub id: u32,
    /// Owning process.
    pub pid: u32,
    /// Application name.
    pub app_name: String,
    /// Title bar text.
    pub title: String,
    /// Bounds in screen coordinates.
    pub rect: Rect,
    /// Whether it is minimized.
    pub is_minimized: bool,
    /// Whether it is the foreground window.
    pub is_focused: bool,
}

/// Why a listed window cannot be a target in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Untargetable {
    /// The OS reports no start time for its process, so a later process
    /// under the same pid could not be told from it.
    UnidentifiedProcess,
    /// This PID was already bound to an earlier process in this session.
    ReusedProcess,
}

/// A top-level window as `list_windows` reports it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Window {
    /// Session handle (`"w3"`); pass as `window`.
    pub id: String,
    /// Owning process; pass as `pid`.
    pub pid: u32,
    /// Application name.
    pub app_name: String,
    /// Title bar text.
    pub title: String,
    /// Bounds in screen coordinates.
    pub rect: Rect,
    /// Whether it is minimized.
    pub is_minimized: bool,
    /// Whether it is the foreground window.
    pub is_focused: bool,
    /// Whether this session accepts it (and its pid) as a safety target.
    pub targetable: bool,
    /// Why not, when not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untargetable_reason: Option<Untargetable>,
}

/// A captured image, PNG-encoded.
#[derive(Debug)]
pub struct Shot {
    /// PNG bytes.
    pub png: Vec<u8>,
    /// Encoded width in pixels.
    pub width: u32,
    /// Encoded height in pixels.
    pub height: u32,
    /// Captured area's origin and size in screen coordinates.
    pub source: Rect,
    /// Encoded pixels per screen-coordinate unit, horizontally: from the
    /// image itself, so a HiDPI backing store (2.0 on a Retina display) and
    /// a downscale both show. A point `px` in the image is at
    /// `source.x + px / scale_x` on the screen.
    pub scale_x: f64,
    /// The same, vertically.
    pub scale_y: f64,
    /// The actual display captured, independent of primary/index selection.
    pub monitor: Option<MonitorSnapshot>,
}

/// Native display identity and its coordinate space at capture time.
/// Native ids can be recycled by the OS after disconnect; this is not a
/// physical-device serial number or an attachment-generation guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorSnapshot {
    /// The backend's native display id, not its position in the list.
    pub id: u32,
    /// The captured display's origin and size in screen coordinates.
    pub rect: Rect,
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn monitor_for_id<T>(
    monitors: &[T],
    id: u32,
    identity: impl Fn(&T) -> ToolResult<u32>,
) -> ToolResult<&T> {
    for monitor in monitors {
        if identity(monitor)? == id {
            return Ok(monitor);
        }
    }
    Err(ToolError::NotFound("the captured monitor is gone".into()))
}

/// Refuses pixels from a replaced or reconfigured display, including a
/// replacement with exactly the same geometry.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub fn verify_monitor(
    expected: MonitorSnapshot,
    current: ToolResult<MonitorSnapshot>,
) -> ToolResult<()> {
    if current.is_ok_and(|current| current == expected) {
        return Ok(());
    }
    Err(ToolError::Busy(
        "the captured monitor disappeared, changed identity or changed geometry (or cannot be read); take another screenshot".into(),
    ))
}

/// What `screenshot` captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotTarget {
    /// One window, by id.
    Window(u32),
    /// A monitor, by index in xcap's list.
    Monitor(usize),
    /// The primary monitor.
    Primary,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod backend {
    use xcap::{Monitor, Window};

    use super::{
        MonitorSnapshot, NativeWindow, Rect, Shot, ShotTarget, ToolError, ToolResult, encode,
        monitor_for_id, verify_monitor, within_pixel_limit,
    };

    fn describe(w: &Window) -> Option<NativeWindow> {
        Some(NativeWindow {
            id: w.id().ok()?,
            pid: w.pid().ok()?,
            app_name: w.app_name().unwrap_or_default(),
            title: w.title().unwrap_or_default(),
            rect: Rect {
                x: w.x().ok()?,
                y: w.y().ok()?,
                width: w.width().ok()?,
                height: w.height().ok()?,
            },
            is_minimized: w.is_minimized().unwrap_or(false),
            is_focused: w.is_focused().unwrap_or(false),
        })
    }

    fn describe_monitor(monitor: &Monitor) -> ToolResult<MonitorSnapshot> {
        let meta = |e| ToolError::platform("reading the monitor's identity and geometry", e);
        Ok(MonitorSnapshot {
            id: monitor.id().map_err(meta)?,
            rect: Rect {
                x: monitor.x().map_err(meta)?,
                y: monitor.y().map_err(meta)?,
                width: monitor.width().map_err(meta)?,
                height: monitor.height().map_err(meta)?,
            },
        })
    }

    /// Looks up the display actually captured, never the current occupant
    /// of an enumeration index or the display currently marked primary.
    pub fn monitor_snapshot(id: u32) -> ToolResult<MonitorSnapshot> {
        let monitors = Monitor::all().map_err(|e| ToolError::platform("listing monitors", e))?;
        let monitor = monitor_for_id(&monitors, id, |monitor| {
            monitor
                .id()
                .map_err(|error| ToolError::platform("reading monitor identity", error))
        })?;
        describe_monitor(monitor)
    }

    /// Every top-level window xcap can see, front to back.
    pub fn windows() -> ToolResult<Vec<NativeWindow>> {
        let all = Window::all().map_err(|e| ToolError::platform("listing windows", e))?;
        Ok(all.iter().filter_map(describe).collect())
    }

    /// Captures `target`, downscaling so neither side exceeds `max_side`.
    pub fn screenshot(target: ShotTarget, max_side: Option<u32>) -> ToolResult<Shot> {
        let (image, source, monitor) = match target {
            ShotTarget::Window(id) => {
                let all = Window::all().map_err(|e| ToolError::platform("listing windows", e))?;
                let window = all
                    .iter()
                    .find(|w| w.id().ok() == Some(id))
                    .ok_or_else(|| ToolError::NotFound(format!("no window with id {id}")))?;
                let info = describe(window)
                    .ok_or_else(|| ToolError::NotFound(format!("window {id} vanished")))?;
                if info.is_minimized {
                    return Err(ToolError::InvalidArgument(format!(
                        "window {id} is minimized and has nothing to capture; activate_window first"
                    )));
                }
                within_pixel_limit(
                    info.rect,
                    window
                        .current_monitor()
                        .ok()
                        .and_then(|m| m.scale_factor().ok()),
                    "make the window smaller or capture another",
                )?;
                // The capture allocates the window's size as it is when it
                // runs, and xcap takes no bounds for a window: the size is
                // read again as the last step before it, so what was
                // checked against the limit is what is allocated, save a
                // resize in the microseconds between.
                if describe(window).map(|w| w.rect) != Some(info.rect) {
                    return Err(ToolError::Busy(format!(
                        "window {id} moved or resized while the capture was prepared"
                    )));
                }
                let image = window
                    .capture_image()
                    .map_err(|e| ToolError::platform(format!("capturing window {id}"), e))?;
                // Bounds read before the capture describe these pixels only
                // if the window stayed put meanwhile.
                // Closed is final; moved is passing.
                match describe(window).map(|w| w.rect) {
                    None => {
                        return Err(ToolError::NotFound(format!(
                            "window {id} closed while it was captured"
                        )));
                    }
                    Some(after) if after != info.rect => {
                        return Err(ToolError::Busy(format!(
                            "window {id} moved or resized while it was captured"
                        )));
                    }
                    Some(_) => {}
                }
                (image, info.rect, None)
            }
            ShotTarget::Monitor(_) | ShotTarget::Primary => {
                let monitors =
                    Monitor::all().map_err(|e| ToolError::platform("listing monitors", e))?;
                let monitor = if let ShotTarget::Monitor(i) = target {
                    monitors.get(i).ok_or_else(|| {
                        ToolError::NotFound(format!(
                            "no monitor {i}; there are {} (0-based)",
                            monitors.len()
                        ))
                    })?
                } else {
                    // The primary, or an error: another monitor's pixels are
                    // not an answer to "the primary monitor".
                    let mut primary = None;
                    for m in &monitors {
                        if m.is_primary().map_err(|e| {
                            ToolError::platform("reading which monitor is primary", e)
                        })? {
                            primary = Some(m);
                            break;
                        }
                    }
                    primary.ok_or_else(|| {
                        ToolError::NotFound(
                            "no monitor reports being the primary one; pass monitor".into(),
                        )
                    })?
                };
                // Coordinates map the pixels back to the desktop; a made-up
                // origin would send later input to the wrong place.
                let captured = describe_monitor(monitor)?;
                let source = captured.rect;
                within_pixel_limit(
                    source,
                    monitor.scale_factor().ok(),
                    "capture a window on it instead",
                )?;
                verify_monitor(captured, monitor_snapshot(captured.id))?;
                let image = monitor
                    .capture_image()
                    .map_err(|e| ToolError::platform("capturing the monitor", e))?;
                // As for a window: the geometry read before describes these
                // pixels only if the display was not reconfigured meanwhile.
                verify_monitor(captured, monitor_snapshot(captured.id))?;
                (image, source, Some(captured))
            }
        };
        #[cfg(target_os = "windows")]
        super::physical_size_matches(image.dimensions(), source)?;
        let mut shot = encode(image, source, max_side)?;
        shot.monitor = monitor;
        Ok(shot)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod backend {
    use super::{NativeWindow, Shot, ShotTarget, ToolError, ToolResult};

    pub(super) fn unsupported() -> ToolError {
        ToolError::NotSupported(format!(
            "window listing and capture are not supported on {} yet (Windows and macOS only)",
            std::env::consts::OS
        ))
    }

    pub fn windows() -> ToolResult<Vec<NativeWindow>> {
        Err(unsupported())
    }

    pub fn screenshot(_: ShotTarget, _: Option<u32>) -> ToolResult<Shot> {
        Err(unsupported())
    }

    pub fn monitor_snapshot(_: u32) -> ToolResult<super::MonitorSnapshot> {
        Err(unsupported())
    }
}

pub use backend::{monitor_snapshot, screenshot, windows};

/// Windows capture is in physical pixels before our explicit downscale.
/// Cropped pixels cannot be mapped by stretching them over the original rect.
#[cfg(any(target_os = "windows", test))]
fn physical_size_matches(size: (u32, u32), source: Rect) -> ToolResult<()> {
    if size != (source.width, source.height) {
        return Err(ToolError::Busy(format!(
            "the capture returned {}x{} pixels for a {}x{} physical-pixel source; its coordinates cannot be verified",
            size.0, size.1, source.width, source.height
        )));
    }
    Ok(())
}

/// Whether capture works on this OS; checked first, so an unsupported OS
/// says so rather than failing on a target's binding.
#[cfg_attr(
    any(target_os = "windows", target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "the error is the answer on the OSes without a capture backend"
    )
)]
pub fn available() -> ToolResult<()> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(backend::unsupported())
    }
}

/// The most pixels one capture reads: an 8K display, about 127 MiB as RGBA.
/// The bitmap is allocated at full size before `max_side` shrinks it, so an
/// enormous window would otherwise take the server down first.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
const MAX_CAPTURE_PIXELS: u64 = 7680 * 4320;

/// Refuses a capture larger than [`MAX_CAPTURE_PIXELS`] before anything is
/// allocated. `scale` is the display's backing pixels per screen unit, which
/// the bitmap is allocated in: 1 on Windows (the server works in physical
/// pixels there), 2 for a Retina display on macOS, where screen units are
/// points. `advice` is what to do instead, for this kind of target.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn within_pixel_limit(source: Rect, scale: Option<f32>, advice: &str) -> ToolResult<()> {
    let scale = if cfg!(target_os = "macos") {
        // Unknown there means the bitmap's size is unknown: refused rather
        // than guessed at one pixel per point.
        f64::from(scale.ok_or_else(|| {
            ToolError::NotSupported(
                "cannot tell the display's backing scale, so the capture's size is unknown and it is refused".into(),
            )
        })?)
        .max(1.0)
    } else {
        1.0
    };
    let backing = |units: u32| (f64::from(units) * scale).ceil();
    let pixels = backing(source.width) * backing(source.height);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the limit is far below where f64 stops being exact"
    )]
    let limit = MAX_CAPTURE_PIXELS as f64;
    if pixels > limit {
        // `max_side` does not help: the limit is on the bitmap read before
        // it is shrunk.
        return Err(ToolError::NotSupported(format!(
            "the capture would be about {pixels} pixels ({}x{} screen units at {scale}x), above the {MAX_CAPTURE_PIXELS}-pixel limit on what one capture reads (max_side does not change it); {advice}",
            source.width, source.height
        )));
    }
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn encode(image: RgbaImage, source: Rect, max_side: Option<u32>) -> ToolResult<Shot> {
    use std::io::Cursor;

    use image::imageops::FilterType;
    use image::{DynamicImage, ImageFormat};

    let (w, h) = image.dimensions();
    let longest = w.max(h).max(1);
    let image = match max_side {
        Some(max) if max > 0 && longest > max => {
            let shrink = f64::from(max) / f64::from(longest);
            let nw = ((f64::from(w) * shrink).round() as u32).max(1);
            let nh = ((f64::from(h) * shrink).round() as u32).max(1);
            DynamicImage::ImageRgba8(image).resize_exact(nw, nh, FilterType::Triangle)
        }
        _ => DynamicImage::ImageRgba8(image),
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| ToolError::platform("encoding PNG", e))?;
    let per_unit = |pixels: u32, units: u32| {
        if units == 0 {
            1.0
        } else {
            f64::from(pixels) / f64::from(units)
        }
    };
    Ok(Shot {
        png,
        width: image.width(),
        height: image.height(),
        source,
        scale_x: per_unit(image.width(), source.width),
        scale_y: per_unit(image.height(), source.height),
        monitor: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unread_monitor_identity_is_not_reported_as_disappearance() {
        let monitors = [MonitorSnapshot {
            id: 1,
            rect: source(),
        }];
        let result = monitor_for_id(&monitors, 1, |_| {
            Err(ToolError::platform(
                "reading monitor identity",
                "unavailable",
            ))
        });
        assert_eq!(
            result.expect_err("BUG: unread identity is an error").code(),
            "platform"
        );
        assert!(monitor_for_id(&monitors, 1, |monitor| Ok(monitor.id)).is_ok());
    }

    #[test]
    fn monitor_pixels_reject_a_same_geometry_replacement() {
        let captured = MonitorSnapshot {
            id: 1,
            rect: source(),
        };
        let replacement = MonitorSnapshot {
            id: 2,
            rect: source(),
        };
        assert!(
            verify_monitor(captured, Ok(replacement)).is_err(),
            "same geometry does not establish monitor identity"
        );
        let monitors = [replacement];
        let current = monitor_for_id(&monitors, captured.id, |monitor| Ok(monitor.id)).copied();
        assert!(
            verify_monitor(captured, current).is_err(),
            "a replacement primary or index occupant cannot supply the old image's coordinates"
        );
    }

    #[test]
    fn monitor_pixels_follow_native_identity_after_enumeration_reorders() {
        let captured = MonitorSnapshot {
            id: 1,
            rect: source(),
        };
        let other = MonitorSnapshot {
            id: 2,
            rect: source(),
        };
        for monitors in [[captured, other], [other, captured]] {
            let current = monitor_for_id(&monitors, captured.id, |monitor| Ok(monitor.id)).copied();
            assert!(verify_monitor(captured, current).is_ok());
        }
        let moved = MonitorSnapshot {
            rect: Rect {
                x: 999,
                ..captured.rect
            },
            ..captured
        };
        assert!(verify_monitor(captured, Ok(moved)).is_err());
        assert!(
            verify_monitor(
                captured,
                Err(ToolError::platform("reading monitor", "failed"))
            )
            .is_err()
        );
    }

    #[test]
    fn cropped_pixels_are_not_mistaken_for_a_scaled_physical_capture() {
        let source = Rect {
            x: 59,
            y: 52,
            width: 466,
            height: 313,
        };
        assert!(physical_size_matches((466, 313), source).is_ok());
        assert!(matches!(
            physical_size_matches((459, 311), source),
            Err(ToolError::Busy(_))
        ));
    }

    fn source() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 400,
            height: 100,
        }
    }

    #[test]
    fn downscale_keeps_aspect_and_reports_scale() {
        let shot = encode(RgbaImage::new(400, 100), source(), Some(200))
            .expect("BUG: encoding a blank image succeeds");
        assert_eq!((shot.width, shot.height), (200, 50));
        assert!((shot.scale_x - 0.5).abs() < 1e-9);
        assert!((shot.scale_y - 0.5).abs() < 1e-9);
        assert_eq!(&shot.png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let shot = encode(RgbaImage::new(400, 100), source(), Some(1000))
            .expect("BUG: encoding a blank image succeeds");
        assert_eq!(
            (shot.width, shot.height, shot.scale_x, shot.scale_y),
            (400, 100, 1.0, 1.0)
        );
    }

    /// A capture larger than the pixel limit is refused before anything is
    /// allocated; one at the limit is not.
    #[test]
    fn an_enormous_capture_is_refused_first() {
        let rect = |width, height| Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        assert!(within_pixel_limit(rect(30_000, 30_000), Some(1.0), "x").is_err());
        assert!(within_pixel_limit(rect(7680, 4320), Some(1.0), "x").is_ok());
        assert!(within_pixel_limit(rect(8192, 8192), Some(1.0), "x").is_err());
    }

    /// A HiDPI capture has more pixels than screen units; the reply says
    /// so instead of claiming a 1:1 mapping.
    #[test]
    fn a_backing_scale_is_reported_from_the_pixels() {
        let shot = encode(RgbaImage::new(800, 200), source(), None)
            .expect("BUG: encoding a blank image succeeds");
        assert_eq!((shot.scale_x, shot.scale_y), (2.0, 2.0));
        let shot = encode(RgbaImage::new(800, 200), source(), Some(400))
            .expect("BUG: encoding a blank image succeeds");
        assert_eq!((shot.scale_x, shot.scale_y), (1.0, 1.0));
    }
}
