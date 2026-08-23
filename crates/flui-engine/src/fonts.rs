//! The font faces this engine embeds, exposed as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still renders text and icons: a text fallback plus the two icon families
//! whose private-use glyphs no system font carries. The wgpu backend's text
//! renderer installs them into the shared `FontSystem` when it finds the
//! database empty (text) or the family absent (icons).
//!
//! They are public because font resolution is otherwise *host*-dependent:
//! `FontSystem::new()` loads whatever fonts the machine has installed, so the
//! family a piece of text resolves to — and therefore its advance widths, and
//! therefore the layout of anything sized to it — differs between machines.
//! A test that wants a layout it can commit to a snapshot must pin the face
//! set to something the repository ships, and these are it. See
//! `flui_testing::fonts::pin_font_faces`.

/// The embedded text fallback face (Roboto Regular).
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");

/// The embedded Material Icons face, family `"Material Icons"`.
pub const MATERIAL_ICONS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");

/// The embedded Cupertino Icons face, family `"CupertinoIcons"`.
pub const CUPERTINO_ICONS: &[u8] = include_bytes!("../assets/fonts/CupertinoIcons.ttf");
