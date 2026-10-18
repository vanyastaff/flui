//! What a Windows check sees on the screen and what it touches it with:
//! pixel captures of a window's client area as the display composed it, and
//! touch contacts injected through `InjectSyntheticPointerInput`.
//!
//! A capture copies the desktop's pixels with `BitBlt`, after the DWM
//! composed them: what a user would see, which is the GPU-presented frame of
//! a FLUI window rather than a `WM_PAINT` the backend never draws with. It is
//! only meaningful while the window is in front and unoccluded, which the
//! drivers check before every capture.
//!
//! A touch contact enters the input stack through a synthetic pointer device:
//! the OS hit-tests it and the window receives `WM_POINTER*` with
//! `PT_TOUCH`, as from a touchscreen.

use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
    POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_FLAGS, POINTER_INFO, POINTER_TOUCH_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{PT_TOUCH, TOUCH_MASK_CONTACTAREA};

pub(super) use super::gallery::Rgb;

/// One capture of a screen rectangle, top row first.
pub(super) struct Capture {
    origin: POINT,
    width: usize,
    height: usize,
    /// BGRA, as GDI hands it back.
    bgra: Vec<u8>,
}

impl Capture {
    /// Copies the screen pixels under `rect` (physical screen pixels).
    pub(super) fn screen(rect: RECT) -> anyhow::Result<Self> {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        anyhow::ensure!(width > 0 && height > 0, "empty capture rectangle {rect:?}");
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>())
                    .expect("BUG: the header is 40 bytes"),
                biWidth: width,
                // Negative: top-down rows, so row 0 is the rectangle's top.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let columns = usize::try_from(width)?;
        let rows = usize::try_from(height)?;
        let mut bgra = vec![0_u8; columns * rows * 4];
        // SAFETY: every handle created here is released below in reverse
        // order on every path; the bitmap is selected out of the memory DC
        // before either is deleted, and `bgra` holds exactly the
        // `width * height` 32-bit pixels `info` describes.
        let copied = unsafe {
            let screen = GetDC(None);
            anyhow::ensure!(
                !screen.is_invalid(),
                "the screen's device context is unavailable"
            );
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
            let blitted = BitBlt(
                memory,
                0,
                0,
                width,
                height,
                Some(screen),
                rect.left,
                rect.top,
                SRCCOPY | CAPTUREBLT,
            );
            SelectObject(memory, previous);
            let lines = GetDIBits(
                memory,
                bitmap,
                0,
                u32::try_from(height)?,
                Some(bgra.as_mut_ptr().cast()),
                &raw mut info,
                DIB_RGB_COLORS,
            );
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            blitted.map(|()| lines)
        }?;
        anyhow::ensure!(
            usize::try_from(copied).ok() == Some(rows),
            "GetDIBits copied {copied} of {rows} rows"
        );
        Ok(Self {
            origin: POINT {
                x: rect.left,
                y: rect.top,
            },
            width: columns,
            height: rows,
            bgra,
        })
    }

    /// The colour at `point` in screen pixels, or `None` outside the capture.
    pub(super) fn at(&self, point: POINT) -> Option<Rgb> {
        let x = usize::try_from(point.x - self.origin.x).ok()?;
        let y = usize::try_from(point.y - self.origin.y).ok()?;
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = (y * self.width + x) * 4;
        Some(Rgb(
            self.bgra[index + 2],
            self.bgra[index + 1],
            self.bgra[index],
        ))
    }

    /// Writes the capture as a PNG.
    pub(super) fn save_png(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let rgba: Vec<u8> = self
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[blue, green, red, _]| [red, green, blue, 255])
            .collect();
        let image = image::RgbaImage::from_raw(
            u32::try_from(self.width)?,
            u32::try_from(self.height)?,
            rgba,
        )
        .ok_or_else(|| anyhow::anyhow!("capture buffer does not match its size"))?;
        image.save(path)?;
        Ok(())
    }
}

/// A synthetic touchscreen with up to `contacts` simultaneous contacts,
/// destroyed when dropped.
pub(super) struct Touchscreen {
    device: HSYNTHETICPOINTERDEVICE,
    contacts: u32,
}

/// Why a touch could not be injected: the host refuses synthetic pointers
/// (no permission, unsupported), as opposed to a malformed packet.
pub(super) fn touch_refused_by_host(error: &windows::core::Error) -> bool {
    // E_ACCESSDENIED, ERROR_NOT_SUPPORTED, ERROR_CALL_NOT_IMPLEMENTED,
    // E_NOTIMPL: the capability or the permission is missing.
    matches!(
        error.code().0.cast_unsigned(),
        0x8007_0005 | 0x8007_0032 | 0x8007_0078 | 0x8000_4001
    )
}

impl Touchscreen {
    /// A touchscreen injecting up to `contacts` contacts at once.
    pub(super) fn new(contacts: u32) -> windows::core::Result<Self> {
        // SAFETY: creates a device this value uniquely owns and destroys.
        let device =
            unsafe { CreateSyntheticPointerDevice(PT_TOUCH, contacts, POINTER_FEEDBACK_NONE) }?;
        Ok(Self { device, contacts })
    }

    /// Moves every contact along its own straight path from its start to
    /// its end in `steps` frames `interval` apart: down at the starts, moving,
    /// optionally holding still at the ends for `hold`, then up. The paths
    /// share their frames, so two contacts make a pinch.
    pub(super) fn stroke(
        &self,
        paths: &[(POINT, POINT)],
        steps: u32,
        interval: Duration,
        hold: Duration,
    ) -> windows::core::Result<()> {
        assert!(
            !paths.is_empty() && paths.len() <= self.contacts as usize,
            "BUG: a stroke has between one and the device's contact count of paths"
        );
        let at = |step: u32| -> Vec<POINT> {
            paths
                .iter()
                .map(|&(from, to)| {
                    let t = f64::from(step) / f64::from(steps.max(1));
                    let lerp = |a: i32, b: i32| a + ((f64::from(b - a) * t).round() as i32);
                    POINT {
                        x: lerp(from.x, to.x),
                        y: lerp(from.y, to.y),
                    }
                })
                .collect()
        };
        let down = POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        let moving = POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        self.frame(&at(0), down)?;
        std::thread::sleep(interval);
        for step in 1..=steps {
            if let Err(error) = self.frame(&at(step), moving) {
                // Lift what is down before reporting, so no contact stays held.
                let _ = self.frame(&at(step - 1), POINTER_FLAG_UP);
                return Err(error);
            }
            std::thread::sleep(interval);
        }
        // Touch has no "no motion" frame of its own: holding still is the
        // same position repeated, as a digitizer reports at its rate.
        let mut held = Duration::ZERO;
        while held < hold {
            self.frame(&at(steps), moving)?;
            std::thread::sleep(interval);
            held += interval;
        }
        self.frame(&at(steps), POINTER_FLAG_UP)
    }

    /// A tap: down and up at `point`, `hold` apart.
    pub(super) fn tap(&self, point: POINT, hold: Duration) -> windows::core::Result<()> {
        self.stroke(&[(point, point)], 1, hold, Duration::ZERO)
    }

    fn frame(&self, points: &[POINT], flags: POINTER_FLAGS) -> windows::core::Result<()> {
        let packets: Vec<POINTER_TYPE_INFO> = points
            .iter()
            .zip(1_u32..)
            .map(|(&point, id)| {
                let contact = RECT {
                    left: point.x - 4,
                    top: point.y - 4,
                    right: point.x + 4,
                    bottom: point.y + 4,
                };
                POINTER_TYPE_INFO {
                    r#type: PT_TOUCH,
                    Anonymous: POINTER_TYPE_INFO_0 {
                        touchInfo: POINTER_TOUCH_INFO {
                            pointerInfo: POINTER_INFO {
                                pointerType: PT_TOUCH,
                                pointerId: id,
                                pointerFlags: flags,
                                ptPixelLocation: point,
                                ptPixelLocationRaw: point,
                                ..POINTER_INFO::default()
                            },
                            touchMask: TOUCH_MASK_CONTACTAREA,
                            rcContact: contact,
                            rcContactRaw: contact,
                            ..POINTER_TOUCH_INFO::default()
                        },
                    },
                }
            })
            .collect();
        // SAFETY: each packet's union arm is the touch arm its `type` names,
        // the device is this value's live touch device, and the slice
        // outlives the synchronous call that copies it.
        unsafe { InjectSyntheticPointerInput(self.device, &packets) }
    }
}

impl Drop for Touchscreen {
    fn drop(&mut self) {
        // SAFETY: the device this value created and uniquely owns.
        unsafe { DestroySyntheticPointerDevice(self.device) };
    }
}
