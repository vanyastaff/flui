//! The window server's capture of the app's window, through ScreenCaptureKit: the window's own
//! surface, wherever it is and whatever covers it, never the screen around it.
//!
//! `SCScreenshotManager` captures one window's image from a filter on that window alone, at the
//! size the configuration asks: the window's frame at its display's scale, shadow left out. The
//! frame holds the title bar above the content, which [`screen::crop_content`] cuts away.
//!
//! ScreenCaptureKit came with macOS 12.3 and its one-shot capture with 14.0, and FLUI runs on
//! older releases, so the framework is loaded when a capture is first asked for and its classes
//! looked up by name: a release without them answers `not_supported`, and nothing links it. The
//! capture answers on a queue of its own; the lane waits for the answer for [`CAPTURE_WITHIN`],
//! and a capture that never answers is a timeout, never a hang.
#![expect(
    unsafe_code,
    reason = "ScreenCaptureKit through the Objective-C runtime, and the bitmap a CGImage is drawn \
              into"
)]

use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use flui_protocol::geometry::LogicalSize;
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, kCGColorSpaceSRGB,
};

use crate::error::{Error, Result};
use crate::lane::macos::callback;
use crate::lane::macos::quartz::{granted, lane_error, os_version};
use crate::lane::macos::safety::{self, Permission, Refusal};
use crate::lane::macos::screen::{self, PointRect};
use crate::lane::macos::version::{MacosVersion, WINDOW_SCREENSHOT};
use crate::pixels::Image;

/// How long the lane waits for ScreenCaptureKit's answer.
pub const CAPTURE_WITHIN: Duration = Duration::from_secs(10);

/// `kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big`: four bytes a pixel, red first.
const RGBA_BITMAP: u32 = 1 | (4 << 12);

/// ScreenCaptureKit's classes, looked up once the framework is loaded.
#[derive(Clone, Copy)]
struct Classes {
    shareable: &'static AnyClass,
    filter: &'static AnyClass,
    configuration: &'static AnyClass,
    screenshot: &'static AnyClass,
}

fn unsupported(why: impl std::fmt::Display) -> Error {
    Error::not_supported(
        "os_capture",
        format!("the macos lane captures no window here: {why}"),
    )
}

impl Classes {
    fn load() -> Result<Self> {
        // SAFETY: a NUL-terminated path; a framework that loads stays loaded for the process, so
        // the handle is not closed and the classes it registers stay valid.
        let handle = unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit".as_ptr(),
                libc::RTLD_LAZY,
            )
        };
        if handle.is_null() {
            return Err(unsupported(
                "ScreenCaptureKit does not load on this release",
            ));
        }
        let class = |name: &CStr| {
            AnyClass::get(name).ok_or_else(|| {
                unsupported(format!(
                    "ScreenCaptureKit has no {}",
                    name.to_string_lossy()
                ))
            })
        };
        Ok(Self {
            shareable: class(c"SCShareableContent")?,
            filter: class(c"SCContentFilter")?,
            configuration: class(c"SCStreamConfiguration")?,
            screenshot: class(c"SCScreenshotManager")?,
        })
    }
}

/// What a capture answers when its completion handler panicked.
fn handler_panicked() -> Error {
    lane_error("ScreenCaptureKit's completion handler panicked while it read the capture")
}

/// What an `NSError` says, for people.
fn reason(error: *mut AnyObject) -> String {
    // SAFETY: a completion handler's error is an `NSError` or null, valid for the call.
    let Some(error) = (unsafe { error.as_ref() }) else {
        return "it gave no reason".to_owned();
    };
    // SAFETY: every `NSError` answers `localizedDescription` with a string.
    let text: Option<Retained<AnyObject>> = unsafe { msg_send![error, localizedDescription] };
    let Some(text) = text else {
        return "it gave no reason".to_owned();
    };
    // SAFETY: an `NSString` answers `UTF8String` with a C string that lives as long as it does.
    let utf8: *const c_char = unsafe { msg_send![&*text, UTF8String] };
    if utf8.is_null() {
        return "it gave no reason".to_owned();
    }
    // SAFETY: a non-null C string `text` keeps alive for this read.
    unsafe { CStr::from_ptr(utf8) }
        .to_string_lossy()
        .into_owned()
}

/// `logical` points at `scale` as whole device pixels.
fn device(logical: f64, scale: f64) -> Result<usize> {
    let value = (logical * scale).round();
    if value.is_finite() && value >= 1.0 && value <= f64::from(u32::MAX) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a whole, positive number checked inside u32's range"
        )]
        let value = value as u32;
        usize::try_from(value).map_err(|_| unsupported("the window is larger than memory"))
    } else {
        Err(unsupported(format!(
            "a window of {logical} points at scale {scale} has no size in device pixels"
        )))
    }
}

/// `image` as tightly packed RGBA rows, top row first.
fn rgba(image: &CGImage) -> Result<Image> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let (Ok(width32), Ok(height32)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(lane_error(
            "the window's capture is larger than a capture holds",
        ));
    };
    let row = width
        .checked_mul(4)
        .ok_or_else(|| lane_error("the window's capture does not fit in memory"))?;
    let mut pixels = vec![
        0_u8;
        row.checked_mul(height).ok_or_else(|| lane_error(
            "the window's capture does not fit in memory"
        ))?
    ];
    // SAFETY: a constant name Quartz exports.
    let srgb = unsafe { kCGColorSpaceSRGB };
    let space = CGColorSpace::with_name(Some(srgb))
        .ok_or_else(|| lane_error("Quartz has no sRGB colour space"))?;
    // SAFETY: `pixels` holds `row * height` bytes, `row` being four bytes for each of `width`
    // pixels; the context draws into it only until it is dropped below, before `pixels` is read.
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            width,
            height,
            8,
            row,
            Some(&*space),
            RGBA_BITMAP,
        )
    }
    .ok_or_else(|| lane_error("Quartz made no bitmap to draw the capture into"))?;
    let bounds = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(f64::from(width32), f64::from(height32)),
    );
    CGContext::draw_image(Some(&*context), bounds, Some(image));
    drop(context);
    Image::try_new(width32, height32, pixels)
}

/// The window server's capture of the window `number`, framed by `frame`, whose content is `size`
/// at `scale`: see the [module docs](self).
///
/// # Errors
///
/// `not_supported` naming `macos_permission_screen_recording` without that permission, and naming
/// `os_capture` before macOS 14; [`Error::Timeout`] when ScreenCaptureKit does not answer within
/// [`CAPTURE_WITHIN`]; [`Error::NotFound`] when it lists no such window; a lane failure when the
/// capture fails.
pub(super) fn capture(
    number: u32,
    frame: PointRect,
    size: LogicalSize,
    scale: f64,
) -> Result<Image> {
    safety::permitted(
        Permission::ScreenRecording,
        granted(Permission::ScreenRecording),
    )
    .map_err(Refusal::into_error)?;
    if !os_version().is_some_and(MacosVersion::has_window_screenshot) {
        return Err(unsupported(format!(
            "ScreenCaptureKit's window capture needs macOS {}.{}",
            WINDOW_SCREENSHOT.major, WINDOW_SCREENSHOT.minor
        )));
    }
    let classes = Classes::load()?;
    let (width, height) = (device(frame.width, scale)?, device(frame.height, scale)?);
    let (answer, answered) = mpsc::channel::<Result<Image>>();
    let content_handler: RcBlock<dyn Fn(*mut AnyObject, *mut AnyObject)> =
        RcBlock::new(move |content: *mut AnyObject, error: *mut AnyObject| {
            // A panic never unwinds into ScreenCaptureKit's queue: the waiting side hears of it.
            let shot = callback::contained_or(Err(handler_panicked()), || {
                shoot(classes, content, error, number, width, height, &answer)
            });
            if let Err(failure) = shot {
                let _ = answer.send(Err(failure));
            }
        });
    // SAFETY: a class method taking two booleans and a completion handler, which it copies.
    unsafe {
        let () = msg_send![
            classes.shareable,
            getShareableContentExcludingDesktopWindows: Bool::YES,
            onScreenWindowsOnly: Bool::NO,
            completionHandler: &*content_handler
        ];
    }
    let whole = answered
        .recv_timeout(CAPTURE_WITHIN)
        .map_err(|_| Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: None,
            message: format!(
                "ScreenCaptureKit did not capture the app's window within {} ms",
                CAPTURE_WITHIN.as_millis()
            ),
            retry: flui_protocol::error::ErrorCode::Timeout.default_retry(),
        })??;
    screen::crop_content(&whole, size, scale)
}

/// From the shareable `content` (or its `error`), capture the window `number` at `width` by
/// `height` device pixels, its answer sent to `answer`.
fn shoot(
    classes: Classes,
    content: *mut AnyObject,
    error: *mut AnyObject,
    number: u32,
    width: usize,
    height: usize,
    answer: &mpsc::Sender<Result<Image>>,
) -> Result<()> {
    // SAFETY: the handler's content is an `SCShareableContent` or null, valid for the call.
    let Some(content) = (unsafe { content.as_ref() }) else {
        return Err(lane_error(format!(
            "ScreenCaptureKit listed no windows: {}",
            reason(error)
        )));
    };
    // SAFETY: `SCShareableContent.windows` is an array of `SCWindow`.
    let windows: Option<Retained<AnyObject>> = unsafe { msg_send![content, windows] };
    let windows = windows.ok_or_else(|| lane_error("ScreenCaptureKit listed no windows"))?;
    // SAFETY: an `NSArray` answers its count.
    let count: usize = unsafe { msg_send![&*windows, count] };
    let mut found = None;
    for index in 0..count {
        // SAFETY: an index under the array's count.
        let window: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*windows, objectAtIndex: index] };
        let Some(window) = window else {
            continue;
        };
        // SAFETY: an `SCWindow` answers its window number, a `CGWindowID`.
        let id: u32 = unsafe { msg_send![&*window, windowID] };
        if id == number {
            found = Some(window);
            break;
        }
    }
    let window = found.ok_or_else(|| {
        Error::not_found(format!(
            "ScreenCaptureKit lists no window {number} the app shows"
        ))
    })?;
    // SAFETY: `alloc` on a class, then its designated initializer on the allocation, with an
    // `SCWindow`.
    let filter: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![classes.filter, alloc];
        msg_send![allocated, initWithDesktopIndependentWindow: &*window]
    };
    let filter = filter.ok_or_else(|| lane_error("ScreenCaptureKit made no window filter"))?;
    // SAFETY: `new` on a class with a plain initializer.
    let configuration: Option<Retained<AnyObject>> =
        unsafe { msg_send![classes.configuration, new] };
    let configuration =
        configuration.ok_or_else(|| lane_error("ScreenCaptureKit made no configuration"))?;
    // SAFETY: setters of an `SCStreamConfiguration`, each taking a plain value; the shadow
    // setter is macOS 14's, as is the capture this function is gated on.
    unsafe {
        let () = msg_send![&*configuration, setWidth: width];
        let () = msg_send![&*configuration, setHeight: height];
        let () = msg_send![&*configuration, setShowsCursor: Bool::NO];
        let () = msg_send![&*configuration, setIgnoreShadowsSingleWindow: Bool::YES];
    }
    let sender = answer.clone();
    let image_handler: RcBlock<dyn Fn(*mut c_void, *mut AnyObject)> =
        RcBlock::new(move |image: *mut c_void, error: *mut AnyObject| {
            let captured = callback::contained_or(Err(handler_panicked()), || {
                NonNull::new(image.cast::<CGImage>())
                    .ok_or_else(|| {
                        lane_error(format!(
                            "ScreenCaptureKit captured nothing: {}",
                            reason(error)
                        ))
                    })
                    .and_then(|image| {
                        // SAFETY: the handler's image is a `CGImage`, valid for the call.
                        rgba(unsafe { image.as_ref() })
                    })
            });
            let _ = sender.send(captured);
        });
    // SAFETY: a class method taking a filter, a configuration and a completion handler, which it
    // copies.
    unsafe {
        let () = msg_send![
            classes.screenshot,
            captureImageWithFilter: &*filter,
            configuration: &*configuration,
            completionHandler: &*image_handler
        ];
    }
    Ok(())
}
