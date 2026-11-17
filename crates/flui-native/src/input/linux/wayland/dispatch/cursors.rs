//! What a pointer shows over a window: a system cursor by its
//! `cursor-shape` value or from the cursor theme, an application's image as
//! a buffer of shared memory on the pointer's cursor surface, or nothing (a
//! hidden cursor, or a pointer the compositor holds in place).

use std::fs::File;
use std::num::NonZeroU32;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;

use cursor_icon::CursorIcon;
use flui_platform::{Cursor, CursorImage};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_cursor::CursorTheme;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

#[cfg(any(test, feature = "testing"))]
use super::ShownCursor;
use super::cursor::SurfaceRequest;
use super::{Objects, PointerObjects, Shared, cursor};
use crate::input::assembly::NativeSurface;
use crate::input::linux::cursor_image::premultiplied_argb;
use crate::owner::linux::wayland::{CursorGlobals, WaylandState};

/// The last application image shown as a cursor, as the buffer made of it:
/// a repeated image is that buffer again.
pub(super) struct ImageBuffer {
    image: CursorImage,
    buffer: WlBuffer,
}

/// A pointer's theme-cursor surface.
pub(crate) struct CursorSurface;

impl Dispatch<WlSurface, CursorSurface> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: wl_surface::Event,
        _: &CursorSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A cursor surface's outputs and scale are its window's concern.
    }
}

/// Show `cursor` on every pointer over the window of `surface`, since that
/// pointer's enter ([`show_pointer_cursor`]).
pub(super) fn show_cursor(
    shared: &Shared,
    objects: &mut Objects,
    surface: NativeSurface,
    cursor: &Cursor,
    scale: f64,
) {
    let shapes = objects
        .shapes
        .as_ref()
        .map(|(_, manager)| manager.version());
    let source = objects.cursor_source.clone();
    for pointer in objects
        .seats
        .iter_mut()
        .filter_map(|seat| seat.pointer.as_mut())
    {
        let Some((focus, serial)) = pointer.focus else {
            continue;
        };
        if focus != surface {
            continue;
        }
        show_pointer_cursor(
            shared,
            shapes,
            source.as_ref(),
            pointer,
            serial,
            cursor,
            scale,
        );
    }
}

/// Show `cursor` on `pointer` since its enter `serial`. A system cursor by
/// its `cursor-shape` value where the compositor (`shapes`, its version) has
/// `cursor-shape`, else as the cursor theme `source` draws it at `scale`; an
/// image as a buffer of `source`'s shared memory at `scale`; a hidden one, or
/// any while the compositor holds the pointer in place, as no surface.
fn show_pointer_cursor(
    shared: &Shared,
    shapes: Option<u32>,
    source: Option<&CursorGlobals>,
    pointer: &mut PointerObjects,
    serial: u32,
    cursor: &Cursor,
    scale: f64,
) {
    if pointer.locked || *cursor == Cursor::Hidden {
        pointer.pointer.set_cursor(serial, None, 0, 0);
        #[cfg(any(test, feature = "testing"))]
        shared.shown.with(|shown| {
            *shown = Some(ShownCursor {
                name: "hidden".to_owned(),
                scale: 0,
            });
        });
        return;
    }
    let icon = match cursor {
        Cursor::Icon(icon) => *icon,
        Cursor::Image(image) => {
            if let Some(source) = source
                && let Some(drawn) =
                    draw_image_cursor(shared, source, pointer, serial, image, scale)
            {
                #[cfg(any(test, feature = "testing"))]
                shared.shown.with(|shown| {
                    *shown = Some(ShownCursor {
                        name: "image".to_owned(),
                        scale: i32::try_from(drawn.get()).unwrap_or(i32::MAX),
                    });
                });
                #[cfg(not(any(test, feature = "testing")))]
                let _ = drawn;
            }
            return;
        }
        Cursor::Hidden => return,
    };
    match (&pointer.shape, shapes, source) {
        (Some(shape), Some(version), _) => {
            let value = cursor::shape(icon, version);
            if let Ok(shape_value) = wp_cursor_shape_device_v1::Shape::try_from(value) {
                shape.set_shape(serial, shape_value);
                #[cfg(any(test, feature = "testing"))]
                shared.shown.with(|shown| {
                    *shown = Some(ShownCursor {
                        name: value.to_string(),
                        scale: 0,
                    });
                });
            }
        }
        (_, _, Some(source)) => {
            if let Some(drawn) = draw_theme_cursor(shared, source, pointer, serial, icon, scale) {
                #[cfg(any(test, feature = "testing"))]
                shared.shown.with(|shown| {
                    *shown = Some(ShownCursor {
                        name: drawn.0.to_owned(),
                        scale: i32::try_from(drawn.1.get()).unwrap_or(i32::MAX),
                    });
                });
                #[cfg(not(any(test, feature = "testing")))]
                let _ = drawn;
            }
        }
        _ => {}
    }
}

/// Show `cursor` on `pointer`, which is over a window's own frame since the
/// enter `serial`, drawn at `scale`.
pub(super) fn show_frame_cursor(
    shared: &Shared,
    objects: &mut Objects,
    pointer: &WlPointer,
    serial: u32,
    cursor: CursorIcon,
    scale: f64,
) {
    let shapes = objects
        .shapes
        .as_ref()
        .map(|(_, manager)| manager.version());
    let source = objects.cursor_source.clone();
    if let Some(held) = objects
        .seats
        .iter_mut()
        .filter_map(|seat| seat.pointer.as_mut())
        .find(|held| held.pointer == *pointer)
    {
        show_pointer_cursor(
            shared,
            shapes,
            source.as_ref(),
            held,
            serial,
            &Cursor::Icon(cursor),
            scale,
        );
    }
}

/// The cursor theme's name and its size at scale one: `XCURSOR_THEME` and
/// `XCURSOR_SIZE`, else the default theme at [`cursor::THEME_SIZE`].
fn theme_setting() -> (String, u32) {
    let name = std::env::var("XCURSOR_THEME")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "default".to_owned());
    let size = std::env::var("XCURSOR_SIZE")
        .ok()
        .and_then(|size| size.parse::<u32>().ok())
        .filter(|size| (1..=256).contains(size))
        .unwrap_or(cursor::THEME_SIZE);
    (name, size)
}

/// `pointer`'s cursor surface, made the first time a cursor is drawn on it.
fn theme_surface<'pointer>(
    source: &CursorGlobals,
    pointer: &'pointer mut PointerObjects,
) -> &'pointer mut ThemeSurface {
    pointer.cursor_surface.get_or_insert_with(|| ThemeSurface {
        surface: source
            .compositor
            .create_surface(&source.queue, CursorSurface),
        viewport: None,
    })
}

/// Draw `cursor` from the theme on `pointer`'s cursor surface for the scale
/// `scale` rounds up to, put on the surface so the compositor takes its
/// buffer ([`cursor::CursorBuffer::fit`]), and set it since the enter
/// `serial`. What was drawn and its pixels per logical pixel, or `None`
/// where the theme has no image of it.
fn draw_theme_cursor(
    shared: &Shared,
    source: &CursorGlobals,
    pointer: &mut PointerObjects,
    serial: u32,
    cursor: CursorIcon,
    scale: f64,
) -> Option<(&'static str, NonZeroU32)> {
    let scale = cursor::theme_scale(scale);
    let drawn = shared
        .themes
        .with(|themes| {
            let index = if let Some(index) = themes.iter().position(|(held, _)| *held == scale) {
                index
            } else {
                let (name, size) = theme_setting();
                let theme = CursorTheme::load_from_name(
                    &source.connection,
                    source.shm.clone(),
                    &name,
                    size.saturating_mul(scale.get()),
                )
                .ok()?;
                themes.push((scale, theme));
                themes.len() - 1
            };
            let theme = &mut themes[index].1;
            cursor::theme_names(cursor).into_iter().find_map(|name| {
                let image = &theme.get_cursor(name)?[0];
                let (width, height) = image.dimensions();
                let fit =
                    cursor::CursorBuffer::fit(width, height, scale, source.viewporter.is_some());
                let shown =
                    theme_surface(source, pointer).show(source, image, (width, height), fit);
                let (x, y) = image.hotspot();
                let (x, y) = fit.surface_point(x, y);
                pointer.pointer.set_cursor(serial, Some(&shown), x, y);
                Some((name, fit.scale()))
            })
        })
        .flatten();
    if drawn.is_none() {
        tracing::debug!(?cursor, "the cursor theme has no image of the cursor");
    }
    drawn
}

/// Show `image` on `pointer`'s cursor surface since the enter `serial`, as
/// a buffer of `source`'s shared memory put on the surface as a theme image
/// drawn for the window's `scale` is ([`cursor::CursorBuffer::fit`]: the
/// image's pixels are device pixels), with its hotspot. The buffer is the
/// last one made when the image is the same. Its pixels per logical pixel,
/// or `None` where no buffer could be made.
fn draw_image_cursor(
    shared: &Shared,
    source: &CursorGlobals,
    pointer: &mut PointerObjects,
    serial: u32,
    image: &CursorImage,
    scale: f64,
) -> Option<NonZeroU32> {
    let buffer = shared
        .image
        .with(|held| {
            if let Some(held) = held.as_ref().filter(|held| held.image == *image) {
                return Some(held.buffer.clone());
            }
            let buffer = image_buffer(source, image)?;
            // Another surface may still show the buffer replaced: its storage
            // is never written again, so it keeps showing what it showed.
            if let Some(replaced) = held.replace(ImageBuffer {
                image: image.clone(),
                buffer: buffer.clone(),
            }) {
                replaced.buffer.destroy();
            }
            Some(buffer)
        })
        .flatten()?;
    let (width, height) = (u32::from(image.width()), u32::from(image.height()));
    let fit = cursor::CursorBuffer::fit(
        width,
        height,
        cursor::theme_scale(scale),
        source.viewporter.is_some(),
    );
    let shown = theme_surface(source, pointer).show(source, &buffer, (width, height), fit);
    let (x, y) = image.hotspot();
    let (x, y) = fit.surface_point(u32::from(x), u32::from(y));
    pointer.pointer.set_cursor(serial, Some(&shown), x, y);
    Some(fit.scale())
}

/// A `wl_shm` buffer of `image`: its premultiplied pixels as `ARGB8888`,
/// which is little-endian, in a memory file of its own. `None` where the
/// memory cannot be had. An image is at most 128 by 128 pixels, 64 KiB.
fn image_buffer(source: &CursorGlobals, image: &CursorImage) -> Option<WlBuffer> {
    let width = i32::from(image.width());
    let height = i32::from(image.height());
    let stride = width.checked_mul(4)?;
    let size = stride.checked_mul(height)?;
    let pixels: Vec<u8> = premultiplied_argb(image)
        .flat_map(u32::to_le_bytes)
        .collect();
    let memory = File::from(
        rustix::fs::memfd_create("flui-cursor-image", rustix::fs::MemfdFlags::CLOEXEC).ok()?,
    );
    memory.write_all_at(&pixels, 0).ok()?;
    let pool = source
        .shm
        .create_pool(memory.as_fd(), size, &source.queue, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        stride,
        wl_shm::Format::Argb8888,
        &source.queue,
        CursorImageBuffer,
    );
    // The buffer keeps the pool's memory mapped in the compositor.
    pool.destroy();
    Some(buffer)
}

/// A cursor image's buffer, kept until another image replaces it.
pub(crate) struct CursorImageBuffer;

impl Dispatch<WlBuffer, CursorImageBuffer> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: wl_buffer::Event,
        _: &CursorImageBuffer,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A released buffer is attached again for the next cursor of the
        // same image; it is destroyed when another image replaces it.
    }
}

/// The surface a pointer's theme cursor is drawn on, and its viewport, made
/// the first time an image needs one ([`cursor::CursorBuffer::Viewported`]).
pub(super) struct ThemeSurface {
    surface: WlSurface,
    viewport: Option<WpViewport>,
}

impl ThemeSurface {
    /// Put `buffer`, an image of `width` by `height` pixels, on the surface
    /// as `fit` says: send it what [`cursor::CursorBuffer::requests`] lists.
    /// The surface, which the pointer's cursor is set to.
    fn show(
        &mut self,
        source: &CursorGlobals,
        buffer: &WlBuffer,
        (width, height): (u32, u32),
        fit: cursor::CursorBuffer,
    ) -> WlSurface {
        let requests = fit.requests(
            width,
            height,
            self.viewport.is_some(),
            self.surface.version(),
        );
        for request in requests {
            match request {
                SurfaceRequest::MakeViewport => {
                    self.viewport = source.viewporter.as_ref().map(|viewporter| {
                        viewporter.get_viewport(&self.surface, &source.queue, ())
                    });
                }
                SurfaceRequest::Destination(size) => {
                    if let Some(viewport) = &self.viewport {
                        let (width, height) = size.unwrap_or((-1, -1));
                        viewport.set_destination(width, height);
                    }
                }
                SurfaceRequest::BufferScale(scale) => self.surface.set_buffer_scale(scale),
                SurfaceRequest::Attach => self.surface.attach(Some(buffer), 0, 0),
                SurfaceRequest::DamageBuffer(width, height) => {
                    self.surface.damage_buffer(0, 0, width, height);
                }
                SurfaceRequest::Damage(width, height) => self.surface.damage(0, 0, width, height),
                SurfaceRequest::Commit => self.surface.commit(),
            }
        }
        self.surface.clone()
    }

    /// Destroy the viewport, then the surface.
    pub(super) fn destroy(self) {
        if let Some(viewport) = self.viewport {
            viewport.destroy();
        }
        self.surface.destroy();
    }
}
