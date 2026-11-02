//! The Linux hosts' text input: the focused field served to the input
//! method through `zwp_text_input_v3` on Wayland ([`text_input`], the
//! protocol's rules over the field's store, and `wayland`, the seats' text
//! inputs that drive it), and through an input method's D-Bus input context
//! on X11 ([`input_context`], the key hold and the protocols' values, and
//! `x11`, the bus and the windows' contexts).
//!
//! The model is pure and built for tests on every host; the live halves bind
//! it to a compositor's seats and to IBus or Fcitx5.

#[cfg(any(x11_host, test))]
pub(crate) mod input_context;
pub(crate) mod text_input;
#[cfg(wayland_host)]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;
