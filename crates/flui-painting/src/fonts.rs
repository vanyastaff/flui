//! The font faces this crate embeds, as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still measures and paints text and icons: the default text face plus the
//! two icon families whose private-use glyphs no system font carries. The
//! realm's `FontCollection` measures on them, and the process-wide font
//! system, which paints, installs each one it lacks at construction and binds
//! its generic families to Roboto, so text paints in the face it was measured
//! in on every host.
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

/// The family name of [`ROBOTO_REGULAR`].
const ROBOTO_FAMILY: &str = "Roboto";

/// Loads the embedded faces `db` lacks: Roboto, and each icon family, when
/// no face of that name is present. Checked per family, not with one shared
/// gate, because a system-installed "Material Icons" must not suppress the
/// Cupertino face.
pub(crate) fn load_missing_into(db: &mut cosmic_text::fontdb::Database) {
    for (family, bytes) in [
        (ROBOTO_FAMILY, ROBOTO_REGULAR),
        ("Material Icons", MATERIAL_ICONS_REGULAR),
        ("CupertinoIcons", CUPERTINO_ICONS),
    ] {
        if !carries_family(db, family) {
            db.load_font_data(bytes.to_vec());
            tracing::debug!(family, "loaded embedded font");
        }
    }
}

/// Points every generic family of `db` at Roboto, the face the realm's
/// `FontCollection` binds them to, so text measured on Parley paints in the
/// face it was measured in (flui-painting `ARCHITECTURE.md`, mapping
/// decision 16). Binds nothing when `db` carries no Roboto.
///
/// Called before `font_resolve::bind_generic_families`, which keeps a
/// generic already bound to a family the database carries.
pub(crate) fn bind_generics_to_bundled(db: &mut cosmic_text::fontdb::Database) {
    if !carries_family(db, ROBOTO_FAMILY) {
        return;
    }
    db.set_sans_serif_family(ROBOTO_FAMILY);
    db.set_serif_family(ROBOTO_FAMILY);
    db.set_cursive_family(ROBOTO_FAMILY);
    db.set_fantasy_family(ROBOTO_FAMILY);
    db.set_monospace_family(ROBOTO_FAMILY);
}

/// Whether `db` holds a face of `family`.
fn carries_family(db: &cosmic_text::fontdb::Database, family: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(name, _)| name == family))
}
