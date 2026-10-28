//! Input on the native Linux hosts: the Wayland seats (`wayland`), and the
//! keyboard as XKB describes it (`xkb`), which a Linux display server's
//! keymap compiles to.

#[doc(hidden)]
pub mod wayland;
#[doc(hidden)]
pub mod xkb;
