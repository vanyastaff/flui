//! What a Win32 window's cursor shows and how the window holds the pointer.
//!
//! - [`NativeCursor`]: the cursor the window's presentation selected, as
//!   `WM_SETCURSOR` shows it: a system cursor, none, or a cursor made of the
//!   application's image, which the window owns and destroys once another
//!   replaced it.
//! - [`hold`] and [`let_go`]: a grab as Win32 holds one. `ClipCursor` keeps
//!   the pointer inside the client area (`Confined`) or on one pixel of it
//!   (`Locked`), and a locked window registers for the mouse's raw input,
//!   whose `WM_INPUT` its input reads as raw motion
//!   (`WindowStep::PointerLocked`). The clip is the desktop's, not the
//!   window's, so the window lets it go whenever it loses the focus or is
//!   destroyed.

use flui_platform::{Cursor, CursorError, CursorIcon, CursorImage, PlatformFailure};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{ClientToScreen, CreateBitmap, DeleteObject, HBITMAP},
    UI::{
        Input::{RAWINPUTDEVICE, RIDEV_REMOVE, RegisterRawInputDevices},
        WindowsAndMessaging::{
            ClipCursor, CreateIconIndirect, DestroyIcon, GetClientRect, GetCursorPos, HCURSOR,
            HICON, ICONINFO, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_NO,
            IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT, LoadCursorW,
            SetCursor,
        },
    },
};
use windows_core::PCWSTR;

/// The cursor a window shows over its client area.
#[derive(Debug)]
pub(crate) enum NativeCursor {
    /// A system cursor, by its `IDC_*` resource.
    System(PCWSTR),
    /// No cursor.
    Hidden,
    /// The application's image, as a cursor the window made of it.
    Image {
        /// The image it was made of, to know it again.
        image: CursorImage,
        /// The cursor, destroyed with this value.
        handle: OwnedCursor,
    },
}

impl NativeCursor {
    /// The arrow, which a window shows before its presentation chose.
    pub(crate) const ARROW: Self = Self::System(IDC_ARROW);

    /// Whether this is the cursor `cursor` asks for: what a repeated request
    /// keeps, so an image is made into a cursor once.
    pub(crate) fn shows(&self, cursor: &Cursor) -> bool {
        match (self, cursor) {
            (Self::System(shown), Cursor::Icon(icon)) => *shown == system_resource(*icon),
            (Self::Hidden, Cursor::Hidden) => true,
            (Self::Image { image, .. }, Cursor::Image(asked)) => image == asked,
            _ => false,
        }
    }

    /// The cursor `cursor` asks for; an image is made into a cursor here.
    ///
    /// # Errors
    /// [`CursorError::Platform`] when Win32 refuses to make the image a cursor.
    pub(crate) fn of(cursor: &Cursor) -> Result<Self, CursorError> {
        Ok(match cursor {
            Cursor::Icon(icon) => Self::System(system_resource(*icon)),
            Cursor::Hidden => Self::Hidden,
            Cursor::Image(image) => Self::Image {
                handle: OwnedCursor::of(image)?,
                image: image.clone(),
            },
        })
    }

    /// Show this cursor now: what `WM_SETCURSOR` does over the client area,
    /// and a request does while the pointer is over the window.
    ///
    /// # Errors
    /// [`CursorError::Platform`] when Win32 cannot load the system cursor.
    pub(crate) fn show(&self) -> Result<(), CursorError> {
        // SAFETY: `LoadCursorW(None, resource)` loads a system cursor by its
        // `IDC_*` ordinal, never a caller's pointer, and the system owns the
        // handle it returns; `SetCursor` takes a handle by value, and the
        // image cursor's handle lives as long as `self`, which the window
        // keeps while it is shown.
        unsafe {
            match self {
                Self::System(resource) => {
                    let handle = LoadCursorW(None, *resource)
                        .map_err(|error| CursorError::failed(error.to_string()))?;
                    SetCursor(Some(handle));
                }
                Self::Hidden => {
                    SetCursor(None);
                }
                Self::Image { handle, .. } => {
                    SetCursor(Some(handle.0));
                }
            }
        }
        Ok(())
    }
}

/// The system cursor `icon` names: Win32 has fewer shapes than the vocabulary,
/// so several share one, and a shape it has no counterpart for is the arrow.
fn system_resource(icon: CursorIcon) -> PCWSTR {
    match icon {
        CursorIcon::Pointer | CursorIcon::Copy | CursorIcon::Grab | CursorIcon::Grabbing => {
            IDC_HAND
        }
        CursorIcon::Progress => IDC_APPSTARTING,
        CursorIcon::Wait => IDC_WAIT,
        CursorIcon::Cell | CursorIcon::Crosshair => IDC_CROSS,
        CursorIcon::Text | CursorIcon::VerticalText => IDC_IBEAM,
        CursorIcon::Move | CursorIcon::AllScroll => IDC_SIZEALL,
        CursorIcon::NoDrop | CursorIcon::NotAllowed => IDC_NO,
        CursorIcon::EResize
        | CursorIcon::WResize
        | CursorIcon::EwResize
        | CursorIcon::ColResize => IDC_SIZEWE,
        CursorIcon::NResize
        | CursorIcon::SResize
        | CursorIcon::NsResize
        | CursorIcon::RowResize => IDC_SIZENS,
        CursorIcon::NeResize | CursorIcon::SwResize | CursorIcon::NeswResize => IDC_SIZENESW,
        CursorIcon::NwResize | CursorIcon::SeResize | CursorIcon::NwseResize => IDC_SIZENWSE,
        // Default, ContextMenu, Help, Alias, ZoomIn, ZoomOut, DndAsk and
        // every future variant fall back to the arrow cursor.
        _ => IDC_ARROW,
    }
}

/// A cursor the window made, destroyed when dropped.
#[derive(Debug)]
pub(crate) struct OwnedCursor(HCURSOR);

impl OwnedCursor {
    /// A cursor of `image`: a 32-bit colour bitmap whose alpha is the
    /// cursor's, over a mask that leaves every pixel to it, as Win32 makes an
    /// alpha cursor.
    fn of(image: &CursorImage) -> Result<Self, CursorError> {
        let width = i32::from(image.width());
        let height = i32::from(image.height());
        let bgra: Vec<u8> = image
            .rgba()
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
            .collect();
        // A monochrome row is padded to a 16-bit boundary; every bit set.
        let mask_row = usize::from(image.width()).div_ceil(16) * 2;
        let mask = vec![0xff_u8; mask_row * usize::from(image.height())];
        // SAFETY: both buffers outlive the calls that copy them and hold the
        // bits the dimensions ask for (`CursorImage` holds four bytes per
        // pixel; the mask is sized above); every bitmap made here is deleted
        // before returning, and the icon `CreateIconIndirect` makes is owned
        // by the value returned, which destroys it.
        unsafe {
            let color = CreateBitmap(width, height, 1, 32, Some(bgra.as_ptr().cast()));
            let mono = CreateBitmap(width, height, 1, 1, Some(mask.as_ptr().cast()));
            let made = if color.is_invalid() || mono.is_invalid() {
                Err(CursorError::failed(
                    "Win32 made no bitmap of the cursor image".to_owned(),
                ))
            } else {
                let (x, y) = image.hotspot();
                let info = ICONINFO {
                    fIcon: false.into(),
                    xHotspot: u32::from(x),
                    yHotspot: u32::from(y),
                    hbmMask: mono,
                    hbmColor: color,
                };
                CreateIconIndirect(&raw const info)
                    .map(|icon| Self(HCURSOR(icon.0)))
                    .map_err(|error| CursorError::failed(error.to_string()))
            };
            delete_bitmap(color);
            delete_bitmap(mono);
            made
        }
    }
}

/// Delete a bitmap this module made, if it made one.
///
/// # Safety
/// `bitmap` is invalid or a bitmap this module made and no one selected.
unsafe fn delete_bitmap(bitmap: HBITMAP) {
    if !bitmap.is_invalid() {
        // SAFETY: the caller's contract.
        let _ = unsafe { DeleteObject(bitmap.into()) };
    }
}

impl Drop for OwnedCursor {
    fn drop(&mut self) {
        // SAFETY: the handle is the icon `CreateIconIndirect` made for this
        // value alone, destroyed once here; the window shows another cursor
        // before it drops this one.
        let _ = unsafe { DestroyIcon(HICON(self.0.0)) };
    }
}

/// The usage page and usage of a mouse in raw input.
const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;

/// Hold the pointer as `grab` asks, for the focused window `hwnd`: inside its
/// client area, or on the pixel it is on (the client area's centre when it
/// is outside), registering for the mouse's raw input to read its motion.
///
/// # Errors
/// [`PlatformFailure::Failed`] when Win32 refuses the clip or the
/// registration; nothing is held then.
pub(crate) fn hold(hwnd: HWND, locked: bool) -> Result<(), PlatformFailure> {
    let client = client_on_screen(hwnd)?;
    let clip = if locked {
        let mut at = POINT::default();
        // SAFETY: a plain query into a live local.
        let inside = unsafe { GetCursorPos(&raw mut at) }.is_ok()
            && (client.left..client.right).contains(&at.x)
            && (client.top..client.bottom).contains(&at.y);
        if !inside {
            at = POINT {
                x: client.left + (client.right - client.left) / 2,
                y: client.top + (client.bottom - client.top) / 2,
            };
        }
        RECT {
            left: at.x,
            top: at.y,
            right: at.x + 1,
            bottom: at.y + 1,
        }
    } else {
        client
    };
    // SAFETY: the rectangle is a live local the call reads.
    unsafe { ClipCursor(Some(&raw const clip)) }.map_err(failed)?;
    if locked && let Err(error) = register_raw_mouse(Some(hwnd)) {
        let_go(false);
        return Err(error);
    }
    Ok(())
}

/// Clip the pointer to `hwnd`'s client area again, after it moved or
/// resized while it confines the pointer.
pub(crate) fn confine_again(hwnd: HWND) {
    if let Ok(client) = client_on_screen(hwnd) {
        // SAFETY: as in `hold`.
        let _ = unsafe { ClipCursor(Some(&raw const client)) };
    }
}

/// Let the pointer go: no clip, and no raw input once a lock held it.
pub(crate) fn let_go(locked: bool) {
    // SAFETY: `ClipCursor(None)` frees the clip; it takes no pointer.
    let _ = unsafe { ClipCursor(None) };
    if locked {
        let _ = register_raw_mouse(None);
    }
}

/// Register `target` for the mouse's raw input, or remove the registration.
fn register_raw_mouse(target: Option<HWND>) -> Result<(), PlatformFailure> {
    let device = RAWINPUTDEVICE {
        usUsagePage: GENERIC_DESKTOP,
        usUsage: MOUSE,
        dwFlags: if target.is_some() {
            Default::default()
        } else {
            RIDEV_REMOVE
        },
        hwndTarget: target.unwrap_or_default(),
    };
    // SAFETY: the slice and the size describe one live `RAWINPUTDEVICE`.
    unsafe {
        RegisterRawInputDevices(
            std::slice::from_ref(&device),
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    }
    .map_err(failed)
}

/// `hwnd`'s client area in screen device pixels.
fn client_on_screen(hwnd: HWND) -> Result<RECT, PlatformFailure> {
    let mut client = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: plain queries of a live window into live locals.
    unsafe {
        GetClientRect(hwnd, &raw mut client).map_err(failed)?;
        if !ClientToScreen(hwnd, &raw mut origin).as_bool() {
            return Err(PlatformFailure::Failed {
                message: "Win32 placed no client area on the screen".to_owned(),
            });
        }
    }
    Ok(RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x.saturating_add(client.right),
        bottom: origin.y.saturating_add(client.bottom),
    })
}

fn failed(error: windows_core::Error) -> PlatformFailure {
    PlatformFailure::Failed {
        message: error.to_string(),
    }
}
