//! What a window's pointer cursor shows ([`Cursor`]), and whether the window
//! holds the pointer ([`CursorGrab`]).
//!
//! The cursor is a verb's argument, sent when the hovered region asks for
//! another one ([`PlatformWindow::set_cursor`](crate::PlatformWindow::set_cursor)):
//! a region that hides the cursor or shows an image of its own asks for it the
//! way it asks for an icon, and the window shows it while the pointer is over
//! that region. The grab is a command and a fact of the snapshot
//! ([`WindowCommand::SetCursorGrab`](crate::WindowCommand::SetCursorGrab),
//! [`WindowState::cursor_grab`](crate::WindowState::cursor_grab)): it holds the
//! whole window's pointer, and a window holds it only while it has the
//! keyboard focus.

use std::sync::Arc;

use cursor_icon::CursorIcon;

use crate::Unsupported;

/// What a window's pointer cursor shows over the region that asked for it.
///
/// Hidden is a cursor, not a window's setting: a region that hides the cursor
/// (a video while it plays, a canvas that draws its own) hides it while the
/// pointer is over that region and gives it back when the pointer leaves, as
/// CSS `cursor: none` does, so nothing has to remember to show it again.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "every host with a pointer cursor maps every cursor: a new kind is a change each host makes, not one it may skip"
)]
pub enum Cursor {
    /// One of the system's cursors.
    Icon(CursorIcon),
    /// An image of the application's, taken where the window's
    /// [`WindowCapabilities::cursor_image`](crate::WindowCapabilities::cursor_image)
    /// is available.
    Image(CursorImage),
    /// No cursor at all while the pointer is over the region.
    Hidden,
}

impl From<CursorIcon> for Cursor {
    fn from(icon: CursorIcon) -> Self {
        Self::Icon(icon)
    }
}

/// An application's own cursor: straight-alpha RGBA pixels and the point of
/// the image that is the pointer's position.
///
/// Its pixels are device pixels: an application that wants one logical size
/// on every display supplies the image for the window's scale
/// ([`WindowState::metrics`](crate::WindowState::metrics)). A window system
/// that measures a cursor in logical units shows it at the nearest size it
/// can (the web at one image pixel per CSS pixel).
///
/// The image is shared: a clone costs a reference count, and equality looks
/// at the pixels only when the two images do not share them, so a host that
/// keeps the last image it made a native cursor of compares it cheaply. It is
/// one pointer wide, so a [`Cursor`] stays two words and every value that can
/// carry one (a hit target's cursor request, each render object that asks
/// for a cursor) pays for an icon, not for an image's size and hotspot.
///
/// ```
/// use flui_platform::{CursorImage, CursorImageError};
///
/// // A 2x2 opaque white square whose top-left pixel is the pointer.
/// let square = CursorImage::try_new(2, 2, (0, 0), vec![255; 16])?;
/// assert_eq!((square.width(), square.height()), (2, 2));
/// assert_eq!(
///     CursorImage::try_new(2, 2, (2, 0), vec![255; 16]),
///     Err(CursorImageError::HotspotOutside { x: 2, y: 0 })
/// );
/// # Ok::<(), CursorImageError>(())
/// ```
#[derive(Clone)]
pub struct CursorImage {
    shared: Arc<CursorPixels>,
}

/// What a [`CursorImage`] shares between its clones.
struct CursorPixels {
    width: u16,
    height: u16,
    hotspot: (u16, u16),
    rgba: Arc<[u8]>,
}

impl CursorImage {
    /// The longest side an image may have, in pixels: the largest cursor every
    /// window system that takes images shows (browsers refuse a larger one).
    pub const MAX_SIDE: u16 = 128;

    /// An image `width` by `height` pixels from `rgba`, four bytes per pixel
    /// (red, green, blue, straight alpha), row by row from the top, whose
    /// pointer position is the pixel at `hotspot` (x, y).
    ///
    /// # Errors
    /// [`CursorImageError::Empty`] for a side of zero,
    /// [`CursorImageError::TooLarge`] for a side above [`MAX_SIDE`](Self::MAX_SIDE),
    /// both checked before the pixels are looked at;
    /// [`CursorImageError::PixelCount`] when `rgba` is not four bytes per
    /// pixel; [`CursorImageError::HotspotOutside`] for a hotspot that is not a
    /// pixel of the image.
    pub fn try_new(
        width: u16,
        height: u16,
        hotspot: (u16, u16),
        rgba: impl Into<Arc<[u8]>>,
    ) -> Result<Self, CursorImageError> {
        if width == 0 || height == 0 {
            return Err(CursorImageError::Empty { width, height });
        }
        if width > Self::MAX_SIDE || height > Self::MAX_SIDE {
            return Err(CursorImageError::TooLarge { width, height });
        }
        let (x, y) = hotspot;
        if x >= width || y >= height {
            return Err(CursorImageError::HotspotOutside { x, y });
        }
        // At most 128 * 128 * 4 bytes: no side above `MAX_SIDE` reaches here.
        let expected = usize::from(width) * usize::from(height) * 4;
        let rgba = rgba.into();
        if rgba.len() != expected {
            return Err(CursorImageError::PixelCount {
                expected,
                found: rgba.len(),
            });
        }
        Ok(Self {
            shared: Arc::new(CursorPixels {
                width,
                height,
                hotspot,
                rgba,
            }),
        })
    }

    /// The width in pixels, `1..=MAX_SIDE`.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.shared.width
    }

    /// The height in pixels, `1..=MAX_SIDE`.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.shared.height
    }

    /// The pixel that is the pointer's position (x, y), inside the image.
    #[must_use]
    pub fn hotspot(&self) -> (u16, u16) {
        self.shared.hotspot
    }

    /// The pixels: `width * height * 4` bytes, red, green, blue and straight
    /// alpha, row by row from the top.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.shared.rgba
    }
}

impl PartialEq for CursorImage {
    fn eq(&self, other: &Self) -> bool {
        let (this, that) = (&*self.shared, &*other.shared);
        Arc::ptr_eq(&self.shared, &other.shared)
            || (this.width == that.width
                && this.height == that.height
                && this.hotspot == that.hotspot
                && (Arc::ptr_eq(&this.rgba, &that.rgba) || this.rgba == that.rgba))
    }
}

impl Eq for CursorImage {}

impl std::fmt::Debug for CursorImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CursorImage")
            .field("width", &self.shared.width)
            .field("height", &self.shared.height)
            .field("hotspot", &self.shared.hotspot)
            .finish_non_exhaustive()
    }
}

/// Why pixels are not a [`CursorImage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CursorImageError {
    /// A side is zero.
    #[error("a cursor image {width}x{height} has no pixels")]
    Empty {
        /// The width asked for.
        width: u16,
        /// The height asked for.
        height: u16,
    },
    /// A side is above [`CursorImage::MAX_SIDE`].
    #[error(
        "a cursor image {width}x{height} is larger than {max}x{max}",
        max = CursorImage::MAX_SIDE
    )]
    TooLarge {
        /// The width asked for.
        width: u16,
        /// The height asked for.
        height: u16,
    },
    /// The bytes are not four per pixel.
    #[error("a cursor image needs {expected} bytes of RGBA, not {found}")]
    PixelCount {
        /// Four bytes per pixel.
        expected: usize,
        /// The bytes given.
        found: usize,
    },
    /// The hotspot is not a pixel of the image.
    #[error("the cursor hotspot ({x}, {y}) is outside the image")]
    HotspotOutside {
        /// Its column.
        x: u16,
        /// Its row.
        y: u16,
    },
}

/// Whether a window holds the pointer
/// ([`WindowCommand::SetCursorGrab`](crate::WindowCommand::SetCursorGrab),
/// [`WindowState::cursor_grab`](crate::WindowState::cursor_grab)).
///
/// A window holds the pointer only while it has the keyboard focus: the
/// window system lets a grab go when the user switches away (the web ends a
/// pointer lock on Escape and on blur), so every host lets it go then, and the
/// snapshot shows [`None`](Self::None). The application asks again when it
/// wants the pointer back; nothing re-grabs it on its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the pointer moves freely, stays inside the window, or stays put; there is no fourth hold"
)]
pub enum CursorGrab {
    /// The pointer moves freely (the default).
    #[default]
    None,
    /// The pointer moves, but cannot leave the window's content area: a
    /// canvas whose edge scrolls, a strategy game's map. Moves arrive as
    /// before.
    Confined,
    /// The pointer stays where it is and is not shown, and its motion arrives
    /// as [`PointerEvent::RawMotion`](crate::pointer::PointerEvent::RawMotion) instead of
    /// moves: a 3D view that turns with the mouse, the web's pointer lock.
    Locked,
}

/// Failure to apply a cursor to one exact platform window.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CursorError {
    /// The backend has no pointer-cursor facility for this window, or shows
    /// no cursor image ([`WindowCapabilities::cursor_image`](crate::WindowCapabilities::cursor_image)).
    #[error("the window shows no such pointer cursor: {0}")]
    Unsupported(#[from] Unsupported),
    /// The window system refused or failed the cursor update, in the shape
    /// every refusal of a window's window system takes
    /// ([`ApplyError::Platform`](crate::ApplyError::Platform)).
    #[error("platform cursor update failed: {0}")]
    Platform(#[from] crate::PlatformFailure),
}

impl CursorError {
    /// A window-system failure with this message.
    #[must_use]
    pub fn failed(message: String) -> Self {
        Self::Platform(crate::PlatformFailure::Failed { message })
    }

    /// What is not here, when that is why the cursor was not applied.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::Platform(_) => None,
        }
    }
}
