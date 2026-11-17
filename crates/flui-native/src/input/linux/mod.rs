//! The Linux hosts' input: what the Wayland and X11 hosts share, and one file
//! for each.
//!
//! What both share is the keyboard: XKB's keymap, its state and compose
//! ([`xkb`]), and the tables that turn its keysyms and the kernel's key codes
//! into FLUI's keys; and an application's cursor image as both window systems
//! take it ([`cursor_image`]). The Wayland host reads its compositor's seats
//! (`wayland`), the X11 host XInput 2 and XKB from the X server (`x11`).

pub mod wayland;
pub mod x11;
pub mod xkb;

pub mod cursor_image {
    //! An application's cursor image as both Linux window systems take it:
    //! 32-bit ARGB pixels with premultiplied alpha, a Render picture of the
    //! X server's standard `ARGB32` format and a `wl_shm` `ARGB8888` buffer
    //! alike.

    #![forbid(unsafe_code)]

    use flui_platform::CursorImage;

    /// The pixels of `image`, row by row from the top, as `0xAARRGGBB`
    /// values whose colour is premultiplied by their alpha, rounded to the
    /// nearest.
    pub fn premultiplied_argb(image: &CursorImage) -> impl Iterator<Item = u32> + '_ {
        image
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[red, green, blue, alpha]| {
                let premultiply = |channel: u8| (u32::from(channel) * u32::from(alpha) + 127) / 255;
                (u32::from(alpha) << 24)
                    | (premultiply(red) << 16)
                    | (premultiply(green) << 8)
                    | premultiply(blue)
            })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Each pixel becomes one ARGB value: an opaque one keeps its
        /// colour, a transparent one is black, and a translucent one carries
        /// its colour times its alpha.
        #[test]
        fn a_cursor_image_becomes_premultiplied_argb() {
            let image = CursorImage::try_new(
                3,
                1,
                (0, 0),
                vec![
                    255, 128, 0, 255, // opaque orange
                    255, 255, 255, 0, // transparent white
                    200, 100, 50, 128, // translucent
                ],
            )
            .expect("a 3x1 image");
            assert_eq!(
                premultiplied_argb(&image).collect::<Vec<u32>>(),
                vec![0xff_ff_80_00, 0x00_00_00_00, 0x80_64_32_19]
            );
        }
    }
}
