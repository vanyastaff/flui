//! The font faces this crate embeds, as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still measures and paints text and icons: a text fallback plus the two
//! icon families whose private-use glyphs no system font carries. The
//! process-wide font system installs them at construction — Roboto when the
//! host database is empty, each icon family when it is absent — so a headless
//! test measures an `Icon` in the same face the running app paints it.
//!
//! They are public because font resolution is otherwise *host*-dependent: a
//! test that wants a layout it can commit to a snapshot pins the face set to
//! something the repository ships, and these are it
//! (`flui_testing::fonts::pin_font_faces`).

/// The embedded text fallback face (Roboto Regular).
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");

/// The embedded Material Icons face, family `"Material Icons"`.
pub const MATERIAL_ICONS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");

/// The embedded Cupertino Icons face, family `"CupertinoIcons"`.
pub const CUPERTINO_ICONS: &[u8] = include_bytes!("../assets/fonts/CupertinoIcons.ttf");

/// Loads the embedded faces `db` lacks: Roboto when it carries no face at
/// all, and each icon family when no face of that name is present. Checked
/// per family, not with one shared gate, because a system-installed
/// "Material Icons" must not suppress the Cupertino face.
pub(crate) fn load_missing_into(db: &mut cosmic_text::fontdb::Database) {
    if db.faces().next().is_none() {
        tracing::warn!("the host has no usable fonts; loading the embedded Roboto Regular");
        db.load_font_data(ROBOTO_REGULAR.to_vec());
    }
    for (family, bytes) in [
        ("Material Icons", MATERIAL_ICONS_REGULAR),
        ("CupertinoIcons", CUPERTINO_ICONS),
    ] {
        let present = db
            .faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family));
        if !present {
            db.load_font_data(bytes.to_vec());
            tracing::debug!(family, "loaded embedded icon font");
        }
    }
}
