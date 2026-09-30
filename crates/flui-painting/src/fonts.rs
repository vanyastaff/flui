//! The font faces this crate embeds, as bytes.
//!
//! Three faces ship inside the binary so a host with no usable system fonts
//! still measures and paints text and icons: the default text face plus the
//! two icon families whose private-use glyphs no system font carries. The
//! realm's `FontCollection` measures, paints and places carets on them, and
//! the process-wide font system a host-fed collection is built from installs
//! them at construction (each in place of a host face of the same family)
//! and binds its generic families to Roboto, so the collection's generics
//! name Roboto on every host.
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

/// Every embedded face with the family it names.
const BUNDLED: [(&str, &[u8]); 3] = [
    (ROBOTO_FAMILY, ROBOTO_REGULAR),
    ("Material Icons", MATERIAL_ICONS_REGULAR),
    ("CupertinoIcons", CUPERTINO_ICONS),
];

/// Installs the embedded faces into `db`, each in place of every face the
/// host carries under that family name.
///
/// The realm's `FontCollection` measures each of these families in the
/// bundled face alone: a host source naming a family the collection already
/// holds is never fed into it, and the host's generic bindings, read from
/// this database, must name the bundled Roboto rather than a host build of it
/// (mapping decisions 16 and 17). Checked per family, so a host
/// "Material Icons" does not touch the Cupertino face.
pub(crate) fn install_bundled(db: &mut cosmic_text::fontdb::Database) {
    for (family, bytes) in BUNDLED {
        let host: Vec<_> = db
            .faces()
            .filter(|face| face.families.iter().any(|(name, _)| name == family))
            .map(|face| face.id)
            .collect();
        if !host.is_empty() {
            tracing::debug!(
                family,
                faces = host.len(),
                "the bundled face replaces the host's"
            );
        }
        for id in host {
            db.remove_face(id);
        }
        db.load_font_data(bytes.to_vec());
    }
}

/// Points every generic family of `db` at Roboto, the face the realm's
/// `FontCollection` binds them to, so the caret layout of text measured and
/// painted on Parley is shaped in that face too (flui-painting
/// `ARCHITECTURE.md`, mapping decision 16). Binds nothing when `db` carries
/// no Roboto.
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

    use super::{BUNDLED, bind_generics_to_bundled, install_bundled};

    /// A face the host calls `family` at `weight`, over bytes that are not
    /// the bundled face's.
    fn host_face(family: &str, weight: Weight) -> FaceInfo {
        FaceInfo {
            id: ID::dummy(),
            source: Source::Binary(Arc::new(
                include_bytes!("../assets/fonts/probe-sans-400.ttf").to_vec(),
            )),
            index: 0,
            families: vec![(family.to_owned(), Language::English_UnitedStates)],
            post_script_name: format!("{family}-Host"),
            style: Style::Normal,
            weight,
            stretch: Stretch::Normal,
            monospaced: false,
        }
    }

    fn is_bundled(db: &Database, id: ID, bytes: &[u8]) -> bool {
        db.with_face_data(id, |data, _| data == bytes)
            .unwrap_or(false)
    }

    /// A host that installs its own copy of a bundled family (Roboto, or
    /// either icon family) still paints that family, and every generic
    /// family, in the bundled face the collection measures with, at every
    /// weight it is asked for.
    #[test]
    fn a_host_copy_does_not_replace_a_bundled_face() {
        let mut db = Database::new();
        for (family, _) in BUNDLED {
            db.push_face_info(host_face(family, Weight::NORMAL));
            db.push_face_info(host_face(family, Weight::BOLD));
        }
        install_bundled(&mut db);
        bind_generics_to_bundled(&mut db);

        let roboto = BUNDLED[0].1;
        let mut queries: Vec<(Family<'_>, &[u8])> = BUNDLED
            .iter()
            .map(|&(family, bytes)| (Family::Name(family), bytes))
            .collect();
        queries.push((Family::SansSerif, roboto));
        queries.push((Family::Monospace, roboto));
        let mut failures = Vec::new();
        for (family, bytes) in queries {
            for weight in [Weight::NORMAL, Weight::MEDIUM, Weight::BOLD] {
                let face = db.query(&Query {
                    families: &[family],
                    weight,
                    ..Query::default()
                });
                if !face.is_some_and(|id| is_bundled(&db, id, bytes)) {
                    failures.push(format!("{family:?} at {weight:?}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "resolved to a face other than the bundled one: {failures:?}"
        );
    }
}
