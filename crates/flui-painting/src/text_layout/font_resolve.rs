//! Choosing the font family a [`TextStyle`] is shaped with, so the shaper is
//! never handed a family the collection does not hold.
//!
//! # The failure this prevents
//!
//! An emoji face ends up shaping the SPACE of an otherwise ordinary Latin run,
//! at roughly 1.24 em instead of 0.25 — letters correct, word gaps four times
//! too wide. A family the collection lacks sends every cluster down the
//! collection's fallback order; the letters are absent from an emoji face
//! listed early in it and move on, the space is present in it and stays.
//!
//! Two rules close it:
//!
//! * [`resolve_family_name`] degrades a named family the collection lacks,
//!   past the style's own declared chain, to [`Family::SansSerif`]: the
//!   Parley path is handed a held family or a generic, never an absent name.
//!   This is the Cupertino path, whose roles all name `CupertinoSystemText`
//!   — a family the platform aliases to San Francisco and that exists
//!   nowhere else.
//! * [`bind_generic_families`] points the five generic names of a host scan
//!   at families it found that can set Latin text, so that degrade lands on
//!   a text face. A generic already naming a found family is kept.

use crate::typography::TextStyle;

use super::fallback_tables;

/// A family a style resolves to: a name the collection holds, or a CSS
/// generic, whose binding in the collection applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family<'a> {
    /// A family the collection holds, spelled as it holds it.
    Name(&'a str),
    /// The `serif` generic.
    Serif,
    /// The `sans-serif` generic.
    SansSerif,
    /// The `cursive` generic.
    Cursive,
    /// The `fantasy` generic.
    Fantasy,
    /// The `monospace` generic.
    Monospace,
}

/// Maps a family name written in a [`TextStyle`] to a CSS generic, if it names
/// one.
fn generic_family(name: &str) -> Option<Family<'static>> {
    match name {
        "serif" | "Serif" => Some(Family::Serif),
        "sans-serif" | "SansSerif" | "sans" => Some(Family::SansSerif),
        "monospace" | "Monospace" | "mono" => Some(Family::Monospace),
        "cursive" | "Cursive" => Some(Family::Cursive),
        "fantasy" | "Fantasy" => Some(Family::Fantasy),
        _ => None,
    }
}

/// Whether any face in `db` carries `family` as one of its family names.
fn database_carries(db: &fontdb::Database, family: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(name, _)| name == family))
}

/// Points every generic family name of a host scan at a family it carries.
///
/// Idempotent and monotone: a generic whose configured name is already
/// carried is left alone, so a generic that points somewhere valid is never
/// moved.
///
/// Binds nothing when the database holds no face that can render Latin at
/// all — an empty database, or one carrying only, say, Khmer serif faces.
/// The generics then keep naming families the database does not have, and
/// the collection's own binding stands.
///
/// # Where the candidates come from
///
/// Sans and monospace draw from the platform's common fallback list
/// (`fallback_tables::common`), split by each face's `monospaced` flag
/// rather than by guessing from names. Serif, cursive and fantasy have no
/// entry in that list on any platform, so they degrade to the sans-serif
/// choice.
///
/// Average and worst case O(faces × candidates): one pass per generic, and
/// both counts are bounded by the installed font set.
pub(crate) fn bind_generic_families(db: &mut fontdb::Database) {
    use fontdb::Family as Generic;

    let sans = pick_family(db, false);
    let mono = pick_family(db, true).or_else(|| sans.clone());

    // `set_*_family` takes the name by value, and each generic needs its own
    // copy, so the clones are the API's price rather than avoidable churn.
    if let Some(sans) = sans {
        if !database_carries(db, db.family_name(&Generic::SansSerif)) {
            db.set_sans_serif_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Generic::Serif)) {
            db.set_serif_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Generic::Cursive)) {
            db.set_cursive_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Generic::Fantasy)) {
            db.set_fantasy_family(sans);
        }
    }
    if let Some(mono) = mono
        && !database_carries(db, db.family_name(&Generic::Monospace))
    {
        db.set_monospace_family(mono);
    }
}

/// Whether `id` can render basic Latin text — the property a generic family
/// has to have, tested by asking the face rather than by matching its name.
///
/// This is what keeps an emoji, symbols or icon face out of the generic
/// bindings: no name pattern identifies an application's private icon font,
/// and coverage of `'A'` and `' '` is the actual requirement.
///
/// Reached only while binding the generics, never per shaped run — but it is
/// not cheap: `fontdb::with_face_data` opens, maps and parses the file on
/// every call with no cache of its own, and the cost is candidates × faces.
/// That is why [`pick_family`] puts every cheaper discriminator ahead of it.
fn can_render_latin(db: &fontdb::Database, id: fontdb::ID) -> bool {
    db.with_face_data(id, |data, index| {
        let font = swash::FontRef::from_index(data, usize::try_from(index).ok()?)?;
        let charmap = font.charmap();
        Some(charmap.map('A') != 0 && charmap.map(' ') != 0)
    }) == Some(Some(true))
}

/// The family a generic should point at: the first entry of the platform's
/// common list `db` carries that matches `want_monospace` **and can render
/// Latin text**, else any such family in the database.
///
/// # Why the Latin check is not optional
///
/// The common list is a *glyph-coverage* list, not a table of generic
/// families — its unix tail is `Noto Sans Symbols`, `Noto Sans Symbols2`,
/// `Noto Color Emoji`. Taking its first carried entry verbatim binds
/// monospace to Noto Color Emoji, which reports `monospaced == true`, and
/// sans-serif to a symbols font. Both were measured.
///
/// The coverage test excludes the faces that produce the *space* defect — the
/// ones carrying `' '` but no letters. It does **not** rank typefaces:
/// `Noto Sans Symbols` carries both `'A'` and `' '`, so a database holding it
/// and no earlier list entry still binds sans-serif to it even when a better
/// text family sits alongside. The consequence there is an odd but readable
/// typeface, not the defect.
///
/// # Where the last resort applies
///
/// The common list is empty on Android and wasm, so there the last-resort
/// branch is the only path: the first face in database order that can
/// render Latin — arbitrary, but bounded to a face that can draw text.
fn pick_family(db: &fontdb::Database, want_monospace: bool) -> Option<String> {
    // Predicate order is load-bearing, not just tidy: `can_render_latin` opens,
    // maps and parses the face file on every call and `fontdb` caches none of
    // it, so it runs LAST — after the `monospaced` flag, and in the candidate
    // loop after the family-name compare. Measured against a 466-face
    // database, testing coverage before the name cost 201 face parses and
    // 1.3 ms where the same result takes 2 parses and 16 µs.
    let usable = |face: &fontdb::FaceInfo| {
        face.monospaced == want_monospace && can_render_latin(db, face.id)
    };

    fallback_tables::common(fallback_tables::HOST)
        .iter()
        .copied()
        .find(|candidate| {
            db.faces().any(|face| {
                face.families.iter().any(|(name, _)| name == *candidate) && usable(face)
            })
        })
        .map(str::to_owned)
        .or_else(|| {
            db.faces()
                .find(|face| usable(face))
                .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
        })
}

/// The family to shape `style` with, given which families the collection
/// holds.
///
/// Returns the style's own family when `carries` says so, the matching
/// generic when the style names one, then the first entry of the style's
/// declared fallback chain that is a generic or is carried, and
/// [`Family::SansSerif`] when nothing is. A generic is returned *as a
/// generic*, so the collection's binding for it applies.
///
/// The Parley path asks it with the families its collection holds, spelled
/// exactly (ADR-0092 §7); past that family, Parley walks the collection's
/// fallback order (`fallback_chain`).
///
/// The returned `Family` borrows `style`, so resolution allocates nothing.
pub(crate) fn resolve_family_name(
    style: Option<&TextStyle>,
    mut carries: impl FnMut(&str) -> bool,
) -> Family<'_> {
    let Some(requested) = style.and_then(|style| style.font_family.as_deref()) else {
        return Family::SansSerif;
    };

    if let Some(generic) = generic_family(requested) {
        return generic;
    }

    if carries(requested) {
        return Family::Name(requested);
    }

    // The declared chain, before giving up on it: `flui-cupertino`'s default
    // text theme fills it on every style and `Icon` propagates it from
    // `IconData`, and both reached a generic instead of the family they asked
    // for whenever the primary was absent (issue #928).
    //
    // A generic name in the chain resolves as that generic and therefore ends
    // it, which is what makes Cupertino's own chain work end to end: it
    // terminates in "sans-serif", so the walk always has a defined stop before
    // the degrade below.
    for candidate in style.map_or(&[][..], |style| style.font_family_fallback.as_slice()) {
        if let Some(generic) = generic_family(candidate) {
            return generic;
        }
        if carries(candidate) {
            return Family::Name(candidate);
        }
    }

    Family::SansSerif
}
