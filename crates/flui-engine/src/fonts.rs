//! The font faces this engine embeds, exposed as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still renders text and icons: a text fallback plus the two icon families
//! whose private-use glyphs no system font carries. The text fallback is
//! installed by `flui-painting` when it constructs the shared `FontSystem`
//! (ADR-0016 — the lowest owner loads the baseline); the renderer installs the
//! two icon faces when it finds the families absent.
//!
//! The **assets live in `flui-painting`**. `flui-painting` owns the shared
//! `FontSystem`, and a hot-reload worker `cdylib` links `flui-painting` but
//! never `flui-engine`, so the text fallback has to be reachable from the
//! lower crate. `ROBOTO_REGULAR` is therefore a re-export; the icon faces are
//! engine-only and remain here.
//!
//! They are public because font resolution is otherwise *host*-dependent:
//! `FontSystem::new()` loads whatever fonts the machine has installed, so the
//! family a piece of text resolves to — and therefore its advance widths, and
//! therefore the layout of anything sized to it — differs between machines.
//! A test that wants a layout it can commit to a snapshot must pin the face
//! set to something the repository ships, and these are it. See
//! `flui_testing::fonts::pin_font_faces`.

/// The embedded text fallback face (Roboto Regular).
///
/// Re-exported from `flui-painting`, which installs it into the shared
/// `FontSystem` when host discovery finds no Latin-capable face. See
/// [`flui_painting::fonts`].
pub use flui_painting::fonts::ROBOTO_REGULAR;

/// The embedded Material Icons face, family `"Material Icons"`.
pub const MATERIAL_ICONS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");

/// The embedded Cupertino Icons face, family `"CupertinoIcons"`.
pub const CUPERTINO_ICONS: &[u8] = include_bytes!("../assets/fonts/CupertinoIcons.ttf");
