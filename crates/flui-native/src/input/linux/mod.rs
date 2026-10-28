//! The Linux hosts' input: what the Wayland and X11 hosts share, and one file
//! for each.
//!
//! What both share is the keyboard: XKB's keymap, its state and compose
//! ([`xkb`]), and the tables that turn its keysyms and the kernel's key codes
//! into FLUI's keys. The Wayland host reads its compositor's seats
//! (`wayland`), the X11 host XInput 2 and XKB from the X server (`x11`).

pub mod wayland;
pub mod x11;
pub mod xkb;
