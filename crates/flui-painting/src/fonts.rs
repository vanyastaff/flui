//! The font faces this crate embeds, as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still measures and paints text and icons: the default text face plus the
//! two icon families whose private-use glyphs no system font carries. The
//! realm's `FontCollection` measures on them, and the process-wide font
//! system, which paints, installs them at construction (the bundled Roboto in
//! place of a host one) and binds its generic families to Roboto, so
//! default-family text paints in the face it was measured in on every host.
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

/// Installs the embedded faces into `db`.
///
/// Roboto always, in place of any face the host carries under that name: the
/// realm's `FontCollection` measures "Roboto", and every generic family, in
/// the bundled Regular, so paint must not pick a host build of it, or a host
/// weight the collection does not hold (mapping decision 16). Each icon
/// family only when no face of that name is present, checked per family,
/// because a system-installed "Material Icons" must not suppress the
/// Cupertino face.
pub(crate) fn load_missing_into(db: &mut cosmic_text::fontdb::Database) {
    let host_roboto: Vec<_> = db
        .faces()
        .filter(|face| face.families.iter().any(|(name, _)| name == ROBOTO_FAMILY))
        .map(|face| face.id)
        .collect();
    if !host_roboto.is_empty() {
        tracing::debug!(
            faces = host_roboto.len(),
            "the bundled Roboto replaces the host's"
        );
    }
    for id in host_roboto {
        db.remove_face(id);
    }
    db.load_font_data(ROBOTO_REGULAR.to_vec());
    for (family, bytes) in [
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

/// A host database cannot be handed to the process font system through the
/// public API, which scans the machine it runs on; these build one by hand.
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cosmic_text::fontdb::{
        Database, FaceInfo, Family, ID, Language, Query, Source, Stretch, Style, Weight,
    };

    use super::{ROBOTO_REGULAR, bind_generics_to_bundled, load_missing_into};

    /// A face the host calls "Roboto" at `weight`, over bytes that are not
    /// the bundled face's.
    fn host_roboto(weight: Weight) -> FaceInfo {
        FaceInfo {
            id: ID::dummy(),
            source: Source::Binary(Arc::new(
                include_bytes!("../assets/fonts/probe-sans-400.ttf").to_vec(),
            )),
            index: 0,
            families: vec![("Roboto".to_owned(), Language::English_UnitedStates)],
            post_script_name: "Roboto-Host".to_owned(),
            style: Style::Normal,
            weight,
            stretch: Stretch::Normal,
            monospaced: false,
        }
    }

    fn is_bundled(db: &Database, id: ID) -> bool {
        db.with_face_data(id, |data, _| data == ROBOTO_REGULAR)
            .unwrap_or(false)
    }

    /// A host that installs its own Roboto still paints "Roboto", and every
    /// generic family, in the bundled face the collection measures with, at
    /// every weight it is asked for.
    #[test]
    fn a_host_roboto_does_not_replace_the_bundled_face() {
        let mut db = Database::new();
        db.push_face_info(host_roboto(Weight::NORMAL));
        db.push_face_info(host_roboto(Weight::BOLD));
        load_missing_into(&mut db);
        bind_generics_to_bundled(&mut db);

        let mut failures = Vec::new();
        for family in [Family::Name("Roboto"), Family::SansSerif, Family::Monospace] {
            for weight in [Weight::NORMAL, Weight::MEDIUM, Weight::BOLD] {
                let face = db.query(&Query {
                    families: &[family],
                    weight,
                    ..Query::default()
                });
                if !face.is_some_and(|id| is_bundled(&db, id)) {
                    failures.push(format!("{family:?} at {weight:?}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "resolved to a face other than the bundled Roboto: {failures:?}"
        );
    }
}
