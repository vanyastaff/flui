//! The font faces this crate embeds, as bytes.
//!
//! Each face is in the binary only under its feature. `bundled-fonts`, on by
//! default, embeds the text face: every `FontCollection` holds it and binds
//! its generic families to it; a host feed never adds a host face of a
//! bundled family and never rebinds a generic, so text naming no family
//! measures in Roboto on every host (flui-painting `ARCHITECTURE.md`, mapping
//! decision 16). `material-icons` and `cupertino-icons` embed the two icon
//! families, whose private-use glyphs no system font carries; the package
//! that draws with a family turns its feature on (ADR-0397), and every
//! `FontCollection` then holds that face too.

/// The embedded text fallback face (Roboto Regular).
#[cfg(feature = "bundled-fonts")]
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");

/// The embedded Material Icons face, family `"Material Icons"`.
#[cfg(feature = "material-icons")]
pub const MATERIAL_ICONS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");

/// The embedded Cupertino Icons face, family `"CupertinoIcons"`.
#[cfg(feature = "cupertino-icons")]
pub const CUPERTINO_ICONS: &[u8] = include_bytes!("../assets/fonts/CupertinoIcons.ttf");
