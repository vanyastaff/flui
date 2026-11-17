//! The cursors an X11 window shows: the theme's, the core cursor font's
//! where the server has no theme, and Render cursors made of an
//! application's image or of no pixels at all (a hidden cursor's and a
//! locked pointer's), each made once and kept on the input's state.

use cursor_icon::CursorIcon;
use flui_platform::{CursorError, CursorImage};
use x11rb::connection::Connection as _;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::render::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _, ImageFormat, ImageOrder};

use super::State;
use crate::input::linux::cursor_image::premultiplied_argb;
use crate::owner::linux::x11::X11Connection;

impl State {
    /// The theme's cursor for `icon`, loaded at its first use: by its
    /// CSS name, the names themes used before, then the core cursor
    /// font's glyph.
    pub(super) fn theme_cursor(
        &mut self,
        connection: &X11Connection,
        icon: CursorIcon,
    ) -> Result<xproto::Cursor, CursorError> {
        if let Some(loaded) = self.loaded.get(&icon) {
            return Ok(*loaded);
        }
        let Some(handle) = self.cursors.as_ref() else {
            return Err(CursorError::failed(
                "the X server's cursors cannot be loaded".into(),
            ));
        };
        let names = std::iter::once(icon.name())
            .chain(icon.alt_names().iter().copied())
            .chain(std::iter::once(core_name(icon)));
        let mut loaded = x11rb::NONE;
        for name in names {
            match handle.load_cursor(connection.xcb(), name) {
                Ok(found) if found != x11rb::NONE => {
                    loaded = found;
                    break;
                }
                Ok(_) => {}
                Err(error) => return Err(CursorError::failed(error.to_string())),
            }
        }
        if loaded == x11rb::NONE {
            return Err(CursorError::failed(format!(
                "the cursor theme has no {icon:?} cursor"
            )));
        }
        self.loaded.insert(icon, loaded);
        Ok(loaded)
    }

    /// The cursor of no pixels, made at its first use.
    pub(super) fn blank_cursor(
        &mut self,
        connection: &X11Connection,
    ) -> Result<xproto::Cursor, CursorError> {
        if let Some(blank) = self.blank {
            return Ok(blank);
        }
        let clear = CursorImage::try_new(1, 1, (0, 0), vec![0; 4])
            .map_err(|error| CursorError::failed(error.to_string()))?;
        let blank = self.made_cursor(connection, &clear)?;
        self.blank = Some(blank);
        Ok(blank)
    }

    /// `image` as a cursor: the one made last when it is that image,
    /// else one made now, which replaces it.
    pub(super) fn image_cursor(
        &mut self,
        connection: &X11Connection,
        image: &CursorImage,
    ) -> Result<xproto::Cursor, CursorError> {
        if let Some((made_of, cursor)) = &self.image
            && made_of == image
        {
            return Ok(*cursor);
        }
        let cursor = self.made_cursor(connection, image)?;
        if let Some((_, replaced)) = self.image.replace((image.clone(), cursor)) {
            // The server keeps a freed cursor while a window still shows
            // it. A failed connection says so at its next request.
            drop(connection.xcb().free_cursor(replaced));
        }
        Ok(cursor)
    }

    /// A new cursor of `image`, in the Render format asked for once.
    fn made_cursor(
        &mut self,
        connection: &X11Connection,
        image: &CursorImage,
    ) -> Result<xproto::Cursor, CursorError> {
        if self.argb32.is_none() {
            self.argb32 = argb32_format(connection);
        }
        let format = self.argb32.ok_or_else(|| {
            CursorError::failed(
                "the X server has no RENDER ARGB32 format for a cursor image".into(),
            )
        })?;
        render_cursor(connection, format, image)
            .map_err(|error| CursorError::failed(error.to_string()))
    }
}

/// The server's standard `ARGB32` Render format: 32 bits, alpha in the
/// top byte, then red, green and blue. `None` where the server has no
/// RENDER, or lists no such format.
fn argb32_format(connection: &X11Connection) -> Option<render::Pictformat> {
    let reply = connection
        .xcb()
        .render_query_pict_formats()
        .ok()?
        .reply()
        .ok()?;
    reply
        .formats
        .iter()
        .find(|format| {
            let direct = format.direct;
            format.type_ == render::PictType::DIRECT
                && format.depth == 32
                && (direct.alpha_shift, direct.alpha_mask) == (24, 0xff)
                && (direct.red_shift, direct.red_mask) == (16, 0xff)
                && (direct.green_shift, direct.green_mask) == (8, 0xff)
                && (direct.blue_shift, direct.blue_mask) == (0, 0xff)
        })
        .map(|format| format.id)
}

/// A Render cursor of `image` in `format`, the server's `ARGB32`: its
/// premultiplied pixels put in a 32-bit pixmap in the server's byte
/// order, the pixmap made a picture and the picture a cursor; the
/// pixmap, its graphics context and the picture are freed after, as the
/// cursor keeps its own copy. An image is at most 128 by 128 pixels, 64
/// KiB, within any server's request length. Each request that can be
/// refused is checked, so a refusal is this call's error and not an
/// event no one reads, and what was made is freed on every exit; a new
/// image costs those round trips once.
fn render_cursor(
    connection: &X11Connection,
    format: render::Pictformat,
    image: &CursorImage,
) -> Result<xproto::Cursor, ReplyOrIdError> {
    let xcb = connection.xcb();
    let (width, height) = (image.width(), image.height());
    let least_first = xcb.setup().image_byte_order == ImageOrder::LSB_FIRST;
    let pixels: Vec<u8> = premultiplied_argb(image)
        .flat_map(|pixel| {
            if least_first {
                pixel.to_le_bytes()
            } else {
                pixel.to_be_bytes()
            }
        })
        .collect();
    let pixmap = xcb.generate_id()?;
    xcb.create_pixmap(32, pixmap, connection.root(), width, height)?
        .check()?;
    let made = pixmap_cursor(xcb, pixmap, format, image, &pixels);
    xcb.free_pixmap(pixmap)?;
    made
}

/// The cursor of `image` whose server-ordered `pixels` are put in
/// `pixmap` (32 bits deep, the image's size) and made a picture of
/// `format`; the graphics context and the picture made on the way are
/// freed on every exit, the pixmap is the caller's.
fn pixmap_cursor(
    xcb: &x11rb::xcb_ffi::XCBConnection,
    pixmap: xproto::Pixmap,
    format: render::Pictformat,
    image: &CursorImage,
    pixels: &[u8],
) -> Result<xproto::Cursor, ReplyOrIdError> {
    let context = xcb.generate_id()?;
    xcb.create_gc(context, pixmap, &xproto::CreateGCAux::new())?
        .check()?;
    let put: Result<(), ReplyOrIdError> = xcb
        .put_image(
            ImageFormat::Z_PIXMAP,
            pixmap,
            context,
            image.width(),
            image.height(),
            0,
            0,
            0,
            32,
            pixels,
        )
        .map_err(ReplyOrIdError::from)
        .and_then(|cookie| cookie.check().map_err(ReplyOrIdError::from));
    xcb.free_gc(context)?;
    put?;
    let picture = xcb.generate_id()?;
    xcb.render_create_picture(picture, pixmap, format, &render::CreatePictureAux::new())?
        .check()?;
    let (x, y) = image.hotspot();
    let cursor: Result<xproto::Cursor, ReplyOrIdError> = xcb.generate_id().and_then(|cursor| {
        xcb.render_create_cursor(cursor, picture, x, y)?.check()?;
        Ok(cursor)
    });
    xcb.render_free_picture(picture)?;
    cursor
}

/// The name of the core cursor font's glyph closest to `cursor`, which a
/// server without a cursor theme still shows.
fn core_name(cursor: CursorIcon) -> &'static str {
    match cursor {
        CursorIcon::Pointer => "hand2",
        CursorIcon::Text | CursorIcon::VerticalText => "xterm",
        CursorIcon::Wait | CursorIcon::Progress => "watch",
        CursorIcon::Crosshair | CursorIcon::Cell => "crosshair",
        CursorIcon::Move | CursorIcon::AllScroll => "fleur",
        CursorIcon::Help => "question_arrow",
        CursorIcon::EResize
        | CursorIcon::WResize
        | CursorIcon::EwResize
        | CursorIcon::ColResize => "sb_h_double_arrow",
        CursorIcon::NResize
        | CursorIcon::SResize
        | CursorIcon::NsResize
        | CursorIcon::RowResize => "sb_v_double_arrow",
        CursorIcon::NotAllowed | CursorIcon::NoDrop => "circle",
        _ => "left_ptr",
    }
}
