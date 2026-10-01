//! The font faces this crate embeds, as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still measures and paints text and icons: the default text face plus the
//! two icon families whose private-use glyphs no system font carries. Every
//! `FontCollection` holds them and binds its generic families to Roboto; a
//! host feed never adds a host face of these families and never rebinds a
//! generic, so text naming no family measures in Roboto on every host
//! (flui-painting `ARCHITECTURE.md`, mapping decision 16).

/// The embedded text fallback face (Roboto Regular).
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");

/// The embedded Material Icons face, family `"Material Icons"`.
pub const MATERIAL_ICONS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");

/// The embedded Cupertino Icons face, family `"CupertinoIcons"`.
pub const CUPERTINO_ICONS: &[u8] = include_bytes!("../assets/fonts/CupertinoIcons.ttf");
