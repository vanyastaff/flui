//! What an AppKit window's cursor shows ([`NativeCursor`]) and how the window
//! holds the pointer in place ([`detach_mouse`]).
//!
//! The cursor is an `NSCursor`, set when the presentation asks for one, as
//! the host sets the system's: one of AppKit's own, a transparent cursor for
//! a hidden one (AppKit's `hide` is a counter of the whole process, which a
//! window's region has no business moving), or one made of the application's
//! image, whose pixels are device pixels and whose size AppKit measures in
//! points at the window's backing scale.
//!
//! A locked pointer is the mouse detached from the cursor
//! (`CGAssociateMouseAndMouseCursorPosition`), which leaves the cursor where it
//! is while the mouse events still carry the motion, under a transparent
//! cursor. The association is the process's, not the window's, so the window
//! attaches the mouse again whenever it lets the lock go.

use flui_platform::{Cursor, CursorError, CursorIcon, CursorImage, PlatformFailure};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSSize};

/// The cursor a window shows while the pointer is over it. A clone shares the
/// `NSCursor` it made.
#[derive(Debug, Clone)]
pub(crate) enum NativeCursor {
    /// One of AppKit's cursors, fetched when shown: AppKit keeps each.
    Icon(CursorIcon),
    /// A transparent cursor of the window's own.
    Hidden(Retained<AnyObject>),
    /// The application's image, as a cursor the window made of it.
    Image {
        /// The image it was made of, to know it again.
        image: CursorImage,
        /// The backing scale it was measured at, in points.
        scale: f64,
        /// The `NSCursor`.
        cursor: Retained<AnyObject>,
    },
}

impl NativeCursor {
    /// The arrow, which a window shows before its presentation chose.
    pub(crate) const ARROW: Self = Self::Icon(CursorIcon::Default);

    /// Whether this is the cursor `cursor` asks for at `scale`: what a
    /// repeated request keeps, so an image is made into a cursor once.
    pub(crate) fn shows(&self, cursor: &Cursor, scale: f64) -> bool {
        match (self, cursor) {
            (Self::Icon(shown), Cursor::Icon(asked)) => shown == asked,
            (Self::Hidden(_), Cursor::Hidden) => true,
            (
                Self::Image {
                    image,
                    scale: made_at,
                    ..
                },
                Cursor::Image(asked),
            ) => image == asked && made_at.to_bits() == scale.to_bits(),
            _ => false,
        }
    }

    /// The cursor `cursor` asks for at the backing scale `scale`; an image
    /// or a hidden cursor is made here. Main thread only.
    ///
    /// # Errors
    /// [`CursorError::Platform`] when AppKit makes no cursor of the image.
    pub(crate) fn of(cursor: &Cursor, scale: f64) -> Result<Self, CursorError> {
        Ok(match cursor {
            Cursor::Icon(icon) => Self::Icon(*icon),
            Cursor::Hidden => Self::Hidden(blank_cursor().map_err(CursorError::Platform)?),
            Cursor::Image(image) => Self::Image {
                cursor: image_cursor(image, scale).map_err(CursorError::Platform)?,
                image: image.clone(),
                scale,
            },
        })
    }

    /// This cursor made again at `scale`: an image is measured in points at
    /// the window's backing scale. `None` when nothing depends on the scale
    /// or it is the one it was made at. Main thread only.
    pub(crate) fn rescaled(&self, scale: f64) -> Option<Result<Self, CursorError>> {
        match self {
            Self::Image {
                image,
                scale: made_at,
                ..
            } if made_at.to_bits() != scale.to_bits() => {
                Some(Self::of(&Cursor::Image(image.clone()), scale))
            }
            Self::Icon(_) | Self::Hidden(_) | Self::Image { .. } => None,
        }
    }

    /// The `NSCursor` this is. Main thread only.
    ///
    /// # Errors
    /// [`CursorError::Platform`] when AppKit answers no cursor for an icon.
    pub(crate) fn ns_cursor(&self) -> Result<Retained<AnyObject>, CursorError> {
        match self {
            Self::Icon(icon) => system_cursor(*icon)
                .ok_or_else(|| CursorError::failed("AppKit returned a null NSCursor".to_owned())),
            Self::Hidden(cursor) | Self::Image { cursor, .. } => Ok(Retained::clone(cursor)),
        }
    }
}

/// Make `cursor` the current cursor (`-[NSCursor set]`). Main thread only.
pub(crate) fn set(cursor: &AnyObject) {
    // SAFETY: `cursor` is a live `NSCursor`; `set` takes no argument, and the
    // caller is on the main thread, where AppKit's cursors are set.
    unsafe {
        let _: () = msg_send![cursor, set];
    }
}

/// AppKit's cursor for `icon`. AppKit has fewer shapes than the vocabulary,
/// so several share one, and a shape it has no counterpart for is the arrow.
fn system_cursor(icon: CursorIcon) -> Option<Retained<AnyObject>> {
    let cursor = class!(NSCursor);
    // SAFETY: every selector below is an `NSCursor` class getter available on
    // the supported macOS baseline; each answers a cursor AppKit keeps, which
    // the `Retained` retains.
    unsafe {
        match icon {
            CursorIcon::ContextMenu => msg_send![cursor, contextualMenuCursor],
            CursorIcon::Pointer => msg_send![cursor, pointingHandCursor],
            CursorIcon::Cell | CursorIcon::Crosshair => msg_send![cursor, crosshairCursor],
            CursorIcon::Text => msg_send![cursor, IBeamCursor],
            CursorIcon::VerticalText => msg_send![cursor, IBeamCursorForVerticalLayout],
            CursorIcon::Copy => msg_send![cursor, dragCopyCursor],
            CursorIcon::Move | CursorIcon::Grabbing => msg_send![cursor, closedHandCursor],
            CursorIcon::Grab => msg_send![cursor, openHandCursor],
            CursorIcon::NoDrop | CursorIcon::NotAllowed => {
                msg_send![cursor, operationNotAllowedCursor]
            }
            CursorIcon::EResize
            | CursorIcon::WResize
            | CursorIcon::EwResize
            | CursorIcon::ColResize => msg_send![cursor, resizeLeftRightCursor],
            CursorIcon::NResize
            | CursorIcon::SResize
            | CursorIcon::NsResize
            | CursorIcon::RowResize => msg_send![cursor, resizeUpDownCursor],
            CursorIcon::Default
            | CursorIcon::Help
            | CursorIcon::Progress
            | CursorIcon::Wait
            | CursorIcon::Alias
            | CursorIcon::NeResize
            | CursorIcon::NwResize
            | CursorIcon::SeResize
            | CursorIcon::SwResize
            | CursorIcon::NeswResize
            | CursorIcon::NwseResize
            | CursorIcon::AllScroll
            | CursorIcon::ZoomIn
            | CursorIcon::ZoomOut
            | CursorIcon::DndAsk => msg_send![cursor, arrowCursor],
            // The icons named above are AppKit's arrow on purpose; this arm
            // is the ones the vocabulary adds later.
            #[expect(
                clippy::match_same_arms,
                reason = "the named arrow icons are listed apart from the vocabulary's later ones"
            )]
            _ => msg_send![cursor, arrowCursor],
        }
    }
}

/// A transparent cursor of one pixel: a hidden cursor, and a locked one.
/// Main thread only.
///
/// # Errors
/// [`PlatformFailure::Failed`] when AppKit makes no cursor of it.
pub(crate) fn blank_cursor() -> Result<Retained<AnyObject>, PlatformFailure> {
    let blank = CursorImage::try_new(1, 1, (0, 0), vec![0; 4])
        .expect("BUG: one transparent pixel with its hotspot on it is a cursor image");
    image_cursor(&blank, 1.0)
}

/// `NSBitmapFormatAlphaNonpremultiplied`: the alpha is straight, as a
/// `CursorImage`'s is.
const ALPHA_NONPREMULTIPLIED: usize = 1 << 1;

/// An `NSCursor` of `image`: its pixels in a bitmap representation, in an
/// image measured in points at the backing `scale`, with the hotspot measured
/// likewise. A scale that is no positive number measures one point per pixel.
fn image_cursor(image: &CursorImage, scale: f64) -> Result<Retained<AnyObject>, PlatformFailure> {
    let failed = |what: &str| PlatformFailure::Failed {
        message: format!("AppKit made no {what} of the cursor image"),
    };
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let (width, height) = (image.width(), image.height());
    // At most 128 pixels a side (`CursorImage::MAX_SIDE`), so every count
    // below is an `NSInteger`.
    let integer = |count: usize| {
        isize::try_from(count).expect("BUG: a cursor image's counts are at most 512")
    };
    let row = usize::from(width) * 4;
    let points = NSSize::new(f64::from(width) / scale, f64::from(height) / scale);
    let (x, y) = image.hotspot();
    let hotspot = NSPoint::new(f64::from(x) / scale, f64::from(y) / scale);
    // SAFETY: on the main thread, every message goes to a class AppKit always
    // has or to an object made here and checked for nil first. The bitmap
    // representation allocates its own buffer of `row * height` bytes for the
    // geometry it is given (one plane, four 8-bit samples, `row` bytes a row),
    // which the copy fills exactly: a `CursorImage` holds four bytes per pixel.
    // Each `alloc`/`init…` pair answers a +1 reference that `from_raw` takes
    // over; the image and the cursor retain what they are given.
    unsafe {
        let color_space: &AnyObject = objc2_app_kit::NSDeviceRGBColorSpace;
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut AnyObject = msg_send![
            rep,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: integer(usize::from(width)),
            pixelsHigh: integer(usize::from(height)),
            bitsPerSample: 8isize,
            samplesPerPixel: 4isize,
            hasAlpha: Bool::YES,
            isPlanar: Bool::NO,
            colorSpaceName: color_space,
            bitmapFormat: ALPHA_NONPREMULTIPLIED,
            bytesPerRow: integer(row),
            bitsPerPixel: 32isize,
        ];
        let rep = Retained::from_raw(rep).ok_or_else(|| failed("bitmap"))?;
        let data: *mut u8 = msg_send![&*rep, bitmapData];
        if data.is_null() {
            return Err(failed("bitmap buffer"));
        }
        std::ptr::copy_nonoverlapping(image.rgba().as_ptr(), data, image.rgba().len());
        let _: () = msg_send![&*rep, setSize: points];

        let ns_image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let ns_image: *mut AnyObject = msg_send![ns_image, initWithSize: points];
        let ns_image = Retained::from_raw(ns_image).ok_or_else(|| failed("image"))?;
        let _: () = msg_send![&*ns_image, addRepresentation: &*rep];

        let cursor: *mut AnyObject = msg_send![class!(NSCursor), alloc];
        let cursor: *mut AnyObject = msg_send![cursor, initWithImage: &*ns_image, hotSpot: hotspot];
        Retained::from_raw(cursor).ok_or_else(|| failed("cursor"))
    }
}

/// Detach the mouse from the cursor (`detached: true`), so its motion moves
/// nothing on screen and reaches the window as the mouse events' deltas, or
/// attach it again. The association is the whole process's.
///
/// # Errors
/// [`PlatformFailure::Failed`] when Quartz refuses.
pub(crate) fn detach_mouse(detached: bool) -> Result<(), PlatformFailure> {
    core_graphics::display::CGDisplay::associate_mouse_and_mouse_cursor_position(!detached).map_err(
        |error| PlatformFailure::Failed {
            message: format!("Quartz refused to change the mouse's association: {error}"),
        },
    )
}
