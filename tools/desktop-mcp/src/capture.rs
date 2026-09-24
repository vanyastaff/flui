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

/// A top-level window as `list_windows` reports it.
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    /// Window id (the `HWND` on Windows); pass as `window_id`.
    pub id: u32,
    /// Owning process.
    pub pid: u32,
    /// Application name.
    pub app_name: String,
    /// Title bar text.
    pub title: String,
    /// Bounds in physical screen pixels.
    pub rect: Rect,
    /// Whether it is minimized.
    pub is_minimized: bool,
    /// Whether it is the foreground window.
    pub is_focused: bool,
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

    use super::{Rect, Shot, ShotTarget, ToolError, ToolResult, WindowInfo, encode};

    fn describe(w: &Window) -> Option<WindowInfo> {
        Some(WindowInfo {
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

    /// Every top-level window xcap can see, front to back.
    pub fn windows() -> ToolResult<Vec<WindowInfo>> {
        let all = Window::all().map_err(|e| ToolError::platform("listing windows", e))?;
        Ok(all.iter().filter_map(describe).collect())
    }

    /// Captures `target`, downscaling so neither side exceeds `max_side`.
    pub fn screenshot(target: ShotTarget, max_side: Option<u32>) -> ToolResult<Shot> {
        let (image, source) = match target {
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
                let image = window
                    .capture_image()
                    .map_err(|e| ToolError::platform(format!("capturing window {id}"), e))?;
                // Bounds read before the capture describe these pixels only
                // if the window stayed put meanwhile.
                let after = describe(window).map(|w| w.rect);
                if after != Some(info.rect) {
                    return Err(ToolError::NotFound(format!(
                        "window {id} moved or resized while it was captured; capture it again"
                    )));
                }
                (image, info.rect)
            }
            ShotTarget::Monitor(_) | ShotTarget::Primary => {
                let monitors =
                    Monitor::all().map_err(|e| ToolError::platform("listing monitors", e))?;
                let monitor = match target {
                    ShotTarget::Monitor(i) => monitors.get(i).ok_or_else(|| {
                        ToolError::NotFound(format!(
                            "no monitor {i}; there are {} (0-based)",
                            monitors.len()
                        ))
                    })?,
                    _ => monitors
                        .iter()
                        .find(|m| m.is_primary().unwrap_or(false))
                        .or_else(|| monitors.first())
                        .ok_or_else(|| ToolError::NotFound("no monitors".into()))?,
                };
                // Coordinates map the pixels back to the desktop; a made-up
                // origin would send later input to the wrong place.
                let meta = |e| ToolError::platform("reading the monitor's position and size", e);
                let source = Rect {
                    x: monitor.x().map_err(meta)?,
                    y: monitor.y().map_err(meta)?,
                    width: monitor.width().map_err(meta)?,
                    height: monitor.height().map_err(meta)?,
                };
                let image = monitor
                    .capture_image()
                    .map_err(|e| ToolError::platform("capturing the monitor", e))?;
                (image, source)
            }
        };
        encode(image, source, max_side)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod backend {
    use super::{Shot, ShotTarget, ToolError, ToolResult, WindowInfo};

    fn unsupported() -> ToolError {
        ToolError::NotSupported(format!(
            "window listing and capture are not supported on {} yet (Windows and macOS only)",
            std::env::consts::OS
        ))
    }

    pub fn windows() -> ToolResult<Vec<WindowInfo>> {
        Err(unsupported())
    }

    pub fn screenshot(_: ShotTarget, _: Option<u32>) -> ToolResult<Shot> {
        Err(unsupported())
    }
}

pub use backend::{screenshot, windows};

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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
