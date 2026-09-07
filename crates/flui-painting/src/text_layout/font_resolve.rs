// PORT-TARGET: flui-painting::TextLayout, flui-engine::wgpu::TextRenderer
//! Choosing the font family a [`TextStyle`] is shaped with, so the shaper is
//! never handed a family this machine does not carry.
//!
//! # The failure this prevents
//!
//! An emoji face ends up shaping the SPACE of an otherwise ordinary Latin run,
//! at roughly 1.24 em instead of 0.25 — letters correct, word gaps four times
//! too wide. Shaping runs per word, which is what lets the two diverge: the
//! letters are absent from an emoji face and move on, the space is present in
//! it and stays.
//!
//! There are **two independent routes** into that face, and a fix that closes
//! only one leaves the defect reachable:
//!
//! 1. **The platform fallback walk itself.** `"Noto Color Emoji"` is the last
//!    entry of cosmic-text's unix `common_fallback()` list, so the walk hands
//!    the space to it whenever no *earlier* text family from that list is
//!    installed — at **any** weight, 400 included. Measured on a database of
//!    Liberation Sans plus Noto Color Emoji: the space comes back at 1.245 em
//!    at both weight 400 and weight 600.
//! 2. **The exact-weight filter.** cosmic-text accepts a candidate only when
//!    `font_weight_diff == 0 || variable_weight_match || is_mono`, so a family
//!    shipping 400 and 700 matches nothing at 500 or 600. The preferred,
//!    script and common lists all empty, and `FontFallbackIter` falls through
//!    to its **unfiltered tail** — which walks candidates in `FontMatchKey`'s
//!    derived order, whose first field is `not_emoji`, sorted ascending, so
//!    emoji faces come first.
//!
//! The escape hatch both routes share is that `FontSystem::get_font_matches`
//! moves the result of `Database::query` to the front of the candidate list.
//! When the requested family *resolves*, route 1 finds it before reaching the
//! emoji entry and route 2's tail starts on a real text face. Both halves of
//! this module exist to make that query resolve:
//!
//! * [`bind_generic_families`] points the five generic family names at
//!   families the database actually carries. `FontSystem::new` hard-codes
//!   `sans-serif` to *Open Sans*, `monospace` to *Noto Sans Mono* and `serif`
//!   to *DejaVu Serif*, leaving `cursive`/`fantasy` at fontdb's *Comic Sans
//!   MS* / *Impact*; whether any of those is installed is a property of the
//!   host, not of the request. On a stock Debian/Ubuntu desktop the monospace
//!   and serif choices happen to exist and *Open Sans* does not — so
//!   `Family::SansSerif` resolves to nothing, and every style naming no family
//!   at all (which is every Material role: `flui-material`'s type scale sets
//!   size, weight, letter-spacing and height, and no family) falls into the
//!   tail. Each generic is re-pointed only when its configured family is
//!   missing.
//! * [`resolve_family`] degrades a named family the database lacks to
//!   `Family::SansSerif`; the binding above points that generic at a carried
//!   family whenever the database holds any Latin-capable face. This is
//!   the Cupertino path, whose roles all name `CupertinoSystemText` — a family
//!   Flutter's engine aliases to San Francisco and that exists nowhere else.
//!
//! Deliberately absent: any adjustment of the requested **weight**. Snapping it
//! to one the family provides would make the match exact, but `FontSystem::get_font`
//! instances a variable face at the requested weight, so a snap strips the
//! requested instance off every variable face reached afterwards. It is also
//! unnecessary: with the family resolved, `Database::query` applies the CSS
//! font-matching algorithm itself and the front-inserted face is already the
//! right one.

use std::collections::HashSet;

use cosmic_text::fontdb::{self, Database, Family};
use cosmic_text::{Fallback as _, FontSystem, PlatformFallback};
use flui_types::typography::TextStyle;

/// Maps a family name written in a [`TextStyle`] to a CSS generic, if it names
/// one.
///
/// The spellings mirror what the two `style_to_attrs` conversions accepted
/// before this module existed, so no style that resolved to a generic stops
/// doing so.
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
fn database_carries(db: &Database, family: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(name, _)| name == family))
}

/// Whether any generic family currently names a family the database does not
/// carry — the read-only precondition for [`bind_generic_families`].
///
/// Exists because `bind_generic_families` needs `&mut Database` to do its
/// writes, and `FontSystem::db_mut()` is not a plain borrow: it **clears
/// cosmic-text's `font_matches_cache`** (`font/system.rs`), so the next
/// shaping pass rebuilds a `FontMatchKey` for every installed face. Measured
/// on a 466-face host: a shape with a warm match cache takes 26 µs, and the
/// same shape immediately after one `db_mut()` takes 3.46 ms — 133×. The
/// database-mutation generation is bumped by every `SharedFontSystem::with_mut`,
/// including the shaping call the renderer makes each frame, so taking
/// `db_mut()` on every generation change put that 3.46 ms on the frame path.
///
/// On a database with no Latin-capable face this stays `true` forever, since
/// `bind_generic_families` binds nothing there and the generics keep naming
/// families that are absent. That is harmless: an empty or letterless database
/// has no match cache worth preserving.
///
/// Average and worst case O(faces), the same scan `bind_generic_families`
/// would do anyway — the saving is the cache, not the scan.
fn generics_need_rebinding(db: &Database) -> bool {
    [
        Family::SansSerif,
        Family::Serif,
        Family::Cursive,
        Family::Fantasy,
        Family::Monospace,
    ]
    .iter()
    .any(|generic| !database_carries(db, db.family_name(generic)))
}

/// Points every generic family name at a family this database carries.
///
/// Called on a freshly built [`Database`] and again, through
/// [`InstalledFamilies::sync`], whenever the database changes — generic names
/// live in the database and are read at query time, so a later call takes
/// effect on a live `FontSystem` too.
///
/// Idempotent and monotone, which is what makes repeating it safe: a generic
/// whose configured name is already carried is left alone, so an explicitly
/// configured database (a test pinning its own faces) is never overridden, and
/// a generic that already points somewhere valid cannot be moved by later
/// growth.
///
/// Binds nothing when the database holds no face that can render Latin at all
/// — an empty database, or one carrying only, say, Khmer serif faces. The
/// generics then keep naming families the database does not have, exactly as
/// before this function existed.
///
/// # Where the candidates come from
///
/// Not a new table. Sans and monospace draw from
/// `PlatformFallback::common_fallback()` — cosmic-text's own curated,
/// per-platform list, which it will walk anyway — split by each face's
/// `monospaced` flag rather than by guessing from names. Serif, cursive and
/// fantasy have no entry in that list on any platform, so they degrade to the
/// sans-serif choice; that is what they already rendered as, since cosmic's
/// own fallback walk ended in the same place.
///
/// Average and worst case O(faces × candidates): one pass per generic, and
/// both counts are bounded by the installed font set.
pub(crate) fn bind_generic_families(db: &mut Database) {
    let sans = pick_family(db, false);
    let mono = pick_family(db, true).or_else(|| sans.clone());

    // `set_*_family` takes the name by value, and each generic needs its own
    // copy, so the clones are the API's price rather than avoidable churn.
    if let Some(sans) = sans {
        if !database_carries(db, db.family_name(&Family::SansSerif)) {
            db.set_sans_serif_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Family::Serif)) {
            db.set_serif_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Family::Cursive)) {
            db.set_cursive_family(sans.clone());
        }
        if !database_carries(db, db.family_name(&Family::Fantasy)) {
            db.set_fantasy_family(sans);
        }
    }
    if let Some(mono) = mono
        && !database_carries(db, db.family_name(&Family::Monospace))
    {
        db.set_monospace_family(mono);
    }
}

/// Whether `id` can render basic Latin text — the property a generic family
/// has to have, tested by asking the face rather than by matching its name.
///
/// This is what keeps an emoji, symbols or icon face out of the generic
/// bindings. Name-based exclusion was the alternative and it is unreliable in
/// both directions: cosmic-text's own emoji test is the heuristic
/// `post_script_name.contains("Emoji")`, which carries an upstream TODO, and
/// no name pattern at all identifies an application's private icon font.
/// Coverage of `'A'` and `' '` is the actual requirement, and every face that
/// should win a generic binding has it.
///
/// Reached only while (re)building [`InstalledFamilies`], never per shaped
/// run — but it is not cheap and it is not paid once: `fontdb::with_face_data`
/// opens, maps and parses the file on every call with no cache of its own, and
/// the cost is candidates × faces, repeated on every observed database change
/// (at least twice on a host that starts with an empty database). That is why
/// [`pick_family`] puts every cheaper discriminator ahead of it.
fn can_render_latin(db: &Database, id: fontdb::ID) -> bool {
    use cosmic_text::skrifa::{self, MetadataProvider as _};

    db.with_face_data(id, |data, index| {
        let font = skrifa::FontRef::from_index(data, index).ok()?;
        let charmap = font.charmap();
        Some(charmap.map('A').is_some() && charmap.map(' ').is_some())
    }) == Some(Some(true))
}

/// The family a generic should point at: the first candidate `db` carries that
/// matches `want_monospace` **and can render Latin text**, else any such
/// family in the database.
///
/// # Why the Latin check is not optional
///
/// `PlatformFallback::common_fallback()` is a *glyph-coverage* list, not a
/// table of generic families — its unix tail is `Noto Sans Symbols`,
/// `Noto Sans Symbols2`, `Noto Color Emoji`. Taking its first carried entry
/// verbatim binds monospace to Noto Color Emoji, which reports
/// `monospaced == true`, and sans-serif to a symbols font. Both were measured.
/// cosmic-text's own use of this list applies a comparable guard
/// (`!post_script_name.contains("Emoji")`).
///
/// The coverage test excludes the faces that produce the *space* defect — the
/// ones carrying `' '` but no letters — which is what this module is for. It
/// does **not** rank typefaces: `Noto Sans Symbols` carries both `'A'` and
/// `' '`, so a database holding it and no earlier list entry still binds
/// sans-serif to it even when a better text family (say Liberation Sans, which
/// the list does not name) sits alongside. Measured. The consequence there is
/// an odd but readable typeface, not the defect; ranking families by quality
/// would need a heuristic no signal in the database supports.
///
/// # Where the last resort applies
///
/// `common_fallback()` is empty on any target that is neither unix-not-Android
/// nor Windows, so on **Android and wasm the last-resort branch is the only
/// path**, and on iOS it is the effective one (the list is routed to the unix
/// table, which names Linux-only families). There it returns the first face in
/// database order that can render Latin — arbitrary, but bounded to a face
/// that can actually draw text, which is the property that matters.
fn pick_family(db: &Database, want_monospace: bool) -> Option<String> {
    // Predicate order is load-bearing, not just tidy: `can_render_latin` opens,
    // maps and parses the face file on every call and `fontdb` caches none of
    // it, so it runs LAST — after the `monospaced` flag, and in the candidate
    // loop after the family-name compare. Measured against this host's 466-face
    // database, testing coverage before the name cost 201 face parses and
    // 1.3 ms where the same result takes 2 parses and 16 µs.
    let usable = |face: &fontdb::FaceInfo| {
        face.monospaced == want_monospace && can_render_latin(db, face.id)
    };

    PlatformFallback
        .common_fallback()
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

/// The set of family names a font database carries, so resolution costs a hash
/// lookup rather than a scan of every face.
///
/// The scan it replaces is not cheap in context: text measurement reaches
/// resolution three times per `TextPainter::layout` (main plus both
/// intrinsics) and again, uncached, from `dry_size`, `dry_baseline` and
/// `intrinsic_height`.
#[derive(Debug, Default)]
pub(crate) struct InstalledFamilies {
    names: HashSet<Box<str>>,
    /// The database-mutation generation the set was built from.
    built_at: u64,
    /// Distinguishes "never built" from "built against an empty database",
    /// which a `names.is_empty()` check would conflate into a rebuild on
    /// every call.
    built: bool,
    /// Families already reported absent, so the diagnostic below is emitted
    /// once per family rather than once per shaped run.
    reported_absent: HashSet<Box<str>>,
}

impl InstalledFamilies {
    /// Brings the set — and the generic family bindings — up to date with the
    /// font database, if it has gained or lost faces since the last build.
    ///
    /// # Why the generic bindings are re-established here and not only at
    /// construction
    ///
    /// The shared database grows *after* text has already been measured. On a
    /// host where `FontSystem::new()` finds no faces at all — headless, CI, a
    /// minimal container — construction-time binding has nothing to choose
    /// from and binds nothing, leaving sans-serif pointing at cosmic-text's
    /// hard-coded `"Open Sans"`. `TextRenderer::new` then loads the embedded
    /// Roboto and the two icon fonts into that same database. Without a
    /// rebind, every later run resolves through a generic that names a family
    /// the database still does not carry, which is the exact condition this
    /// module exists to prevent — and no test would catch it, because the
    /// test harness pins its generics explicitly.
    ///
    /// Rebinding costs a `db_mut()` (which clears cosmic-text's own family
    /// match cache) and is therefore done only on the rebuild path, never on
    /// a call that finds the set already fresh.
    ///
    /// # Staleness signal
    ///
    /// `db_generation` counts *mutations*, not faces: it is bumped once per
    /// `SharedFontSystem::with_mut`, the single door through which anything
    /// outside this module reaches the database. Face count was the obvious
    /// signal and is the wrong one — `with_mut` hands out `&mut FontSystem`,
    /// so a caller can remove one face and load another and leave the count
    /// identical, after which the set describes a database that no longer
    /// exists and every style resolves through the wrong family
    /// indefinitely. Counting the door cannot be defeated that way, whatever
    /// happens behind it.
    ///
    /// Average and worst case O(1) when fresh, O(faces) on the rebuild.
    fn sync(&mut self, font_system: &mut FontSystem, db_generation: u64) {
        if self.built && db_generation == self.built_at {
            return;
        }

        // Probed through `db()` first: `db_mut()` clears cosmic-text's
        // font-match cache, and this runs on a generation the renderer's own
        // per-frame `with_mut` bumps. See `generics_need_rebinding`.
        if generics_need_rebinding(font_system.db()) {
            bind_generic_families(font_system.db_mut());
        }

        let db = font_system.db();
        self.names.clear();
        for face in db.faces() {
            for (name, _) in &face.families {
                if !self.names.contains(name.as_str()) {
                    self.names.insert(name.as_str().into());
                }
            }
        }
        self.built_at = db_generation;
        self.built = true;
    }

    fn carries(&self, family: &str) -> bool {
        self.names.contains(family)
    }
}

/// The family to shape `style` with.
///
/// Returns the style's own family when the database carries it, the matching
/// generic when the style names one, and `Family::SansSerif` when the style
/// names a family that is absent. The generic binding points that fallback at
/// a carried family whenever the database holds a Latin-capable face. A generic is returned *as a generic*, so
/// `Family::Monospace` keeps cosmic-text's monospace-specific fallback path —
/// its `is_mono` bypass of the exact-weight filter, and its panose-driven
/// monospace candidate set.
///
/// Every path here, the generic ones included, first brings
/// [`InstalledFamilies`] and the generic bindings up to date with
/// `font_system`, so a database that gained faces after construction resolves
/// against what it now holds rather than what it held then.
///
/// The returned `Family` borrows `style`, so resolution allocates nothing.
///
/// Average and worst case O(1) once `installed` is in sync, O(faces) on the
/// call that observes a database change.
pub(crate) fn resolve_family<'a>(
    style: Option<&'a TextStyle>,
    font_system: &mut FontSystem,
    installed: &mut InstalledFamilies,
    db_generation: u64,
) -> Family<'a> {
    installed.sync(font_system, db_generation);

    let Some(requested) = style.and_then(|style| style.font_family.as_deref()) else {
        // Matches what `Attrs::new()` has always defaulted to; a style naming
        // no family is unchanged by this module beyond the generic binding.
        return Family::SansSerif;
    };

    if let Some(generic) = generic_family(requested) {
        return generic;
    }

    if installed.carries(requested) {
        return Family::Name(requested);
    }

    // The declared chain, before giving up on it. `TextStyle::font_family_fallback`
    // was read by nothing until here — `flui-cupertino`'s default text theme
    // fills it on every style and `Icon` propagates it from `IconData`, and both
    // reached a generic instead of the family they asked for whenever the
    // primary was absent (issue #928).
    //
    // A generic name in the chain resolves as that generic and therefore ends
    // it, which is what makes Cupertino's own chain work end to end: it
    // terminates in "sans-serif", so the walk always has a defined stop before
    // the degrade below.
    for candidate in style.map_or(&[][..], |style| style.font_family_fallback.as_slice()) {
        if let Some(generic) = generic_family(candidate) {
            return generic;
        }
        if installed.carries(candidate) {
            return Family::Name(candidate);
        }
    }

    if installed.reported_absent.insert(requested.into()) {
        tracing::debug!(
            family = requested,
            "font family not installed, and no declared fallback is either; \
             shaping through the sans-serif generic instead"
        );
    }
    Family::SansSerif
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../../../flui-engine/assets/fonts/Roboto-Regular.ttf");
    const ARIAL: &[u8] = include_bytes!("../../../flui-engine/assets/fonts/Arial.ttf");
    const MATERIAL_ICONS: &[u8] =
        include_bytes!("../../../flui-engine/assets/fonts/MaterialIcons-Regular.ttf");

    fn database(faces: &[&[u8]]) -> Database {
        let mut db = Database::new();
        for face in faces {
            db.load_font_data((*face).to_vec());
        }
        db
    }

    fn font_system(db: Database) -> FontSystem {
        FontSystem::new_with_locale_and_db("en-US".to_owned(), db)
    }

    /// The read-only probe that keeps `sync` off `db_mut()` must actually
    /// answer "no" once the generics are bound — otherwise the gate is inert
    /// and every generation change still wipes cosmic-text's match cache
    /// (measured 26 µs → 3.46 ms on a 466-face host).
    ///
    /// Both arms matter: the `true` arm alone would pass for a predicate that
    /// is always `true`, which is exactly the bug this replaces.
    #[test]
    fn the_rebinding_probe_answers_no_once_the_generics_are_bound() {
        let mut db = database(&[ROBOTO]);
        assert!(
            generics_need_rebinding(&db),
            "precondition: a fresh database names cosmic-text's hard-coded \
             generics, which it does not carry"
        );

        bind_generic_families(&mut db);

        assert!(
            !generics_need_rebinding(&db),
            "once bound, the probe must report nothing to do -- this is what \
             keeps `sync` from taking `db_mut()` on every frame"
        );
    }

    /// Unbinding one generic is enough to ask for a rebind: the probe is an
    /// "any", not an "all", so a single stale generic still gets repaired.
    #[test]
    fn the_rebinding_probe_answers_yes_for_a_single_stale_generic() {
        let mut db = database(&[ROBOTO]);
        bind_generic_families(&mut db);
        assert!(!generics_need_rebinding(&db), "precondition: all bound");

        db.set_cursive_family("A Family Nothing Carries");

        assert!(
            generics_need_rebinding(&db),
            "one generic naming an absent family must still ask for a rebind"
        );
    }

    /// The recorded divergence from Flutter, pinned so it cannot drift
    /// unnoticed in either direction.
    ///
    /// Flutter searches `fontFamilyFallback` **per glyph**: a family that is
    /// installed but lacks the glyph is skipped and the next one is tried
    /// (`painting/text_style.dart`, the `fontFamily` doc). `Attrs::family`
    /// holds exactly one family, so the walk here can only ask "is this
    /// family installed" — and `Material Icons` IS installed while carrying no
    /// Latin at all. The chain therefore stops on it, and the `Roboto` entry
    /// behind it is never reached; Flutter would render the text.
    ///
    /// The two directions this guards:
    ///
    /// * If someone "fixes" the walk to skip a present family, this fails and
    ///   points at `ARCHITECTURE.md`'s mapping decision — the change would
    ///   need to be per-glyph to be a fix rather than a different guess.
    /// * If per-glyph fallback ever does land, this fails too, which is the
    ///   signal to retire the divergence record instead of leaving it stale.
    ///
    /// The control matters: the same chain with an ABSENT primary reaches
    /// `Roboto`, so the stop is about presence and not about the chain being
    /// unread.
    #[test]
    fn a_present_but_narrow_family_stops_the_chain_where_flutter_would_not() {
        let mut system = font_system(database(&[ROBOTO, MATERIAL_ICONS]));
        let mut installed = InstalledFamilies::default();

        let narrow = TextStyle {
            font_family: Some("Material Icons".to_owned()),
            font_family_fallback: vec!["Roboto".to_owned()],
            ..TextStyle::default()
        };
        assert_eq!(
            resolve_family(Some(&narrow), &mut system, &mut installed, 0),
            Family::Name("Material Icons"),
            "an installed family stops the walk even though it carries no \
             Latin -- Flutter would fall through to Roboto per glyph"
        );

        let absent = TextStyle {
            font_family: Some("Nothing Carries This".to_owned()),
            font_family_fallback: vec!["Roboto".to_owned()],
            ..TextStyle::default()
        };
        assert_eq!(
            resolve_family(Some(&absent), &mut system, &mut installed, 0),
            Family::Name("Roboto"),
            "control: the chain IS walked -- an absent primary reaches it"
        );
    }

    fn styled(family: Option<&str>) -> TextStyle {
        TextStyle {
            font_family: family.map(str::to_owned),
            ..TextStyle::default()
        }
    }

    /// Resolves against a freshly-built index, so the generation is
    /// irrelevant — a test that MUTATES the database between resolves must
    /// keep its own generation instead (see
    /// `a_face_loaded_after_the_first_resolve_is_picked_up`).
    fn resolved(system: &mut FontSystem, style: &TextStyle) -> String {
        let mut installed = InstalledFamilies::default();
        format!(
            "{:?}",
            resolve_family(Some(style), system, &mut installed, 0)
        )
    }

    fn styled_with_fallback(family: Option<&str>, fallback: &[&str]) -> TextStyle {
        TextStyle {
            font_family: family.map(str::to_owned),
            font_family_fallback: fallback.iter().map(|f| (*f).to_owned()).collect(),
            ..TextStyle::default()
        }
    }

    /// An absent primary falls through to the first INSTALLED family in the
    /// declared chain, rather than jumping to the generic.
    ///
    /// This is what a Cupertino run hits on any host without
    /// `CupertinoSystemText`: the theme declares
    /// `["-apple-system", "system-ui", "Segoe UI", "Helvetica Neue", "Arial", "sans-serif"]`
    /// and, before this, every one of them was skipped in favour of the
    /// sans-serif generic even when the host carried one.
    #[test]
    fn an_absent_primary_falls_through_to_an_installed_fallback() {
        let mut system = font_system(database(&[ARIAL]));
        let style = styled_with_fallback(Some("CupertinoSystemText"), &["Helvetica Neue", "Arial"]);
        assert_eq!(
            resolved(&mut system, &style),
            "Name(\"Arial\")",
            "the primary is absent and so is the first fallback, but Arial is \
             installed — before #928 this resolved to SansSerif with Arial \
             sitting right there in the database"
        );
    }

    /// The chain is ordered: an earlier installed entry beats a later one.
    ///
    /// Without this, a walk that happened to return the LAST match, or that
    /// searched the database rather than the chain, would pass the test above.
    #[test]
    fn the_chain_is_walked_in_declared_order() {
        let mut system = font_system(database(&[ARIAL, ROBOTO]));
        let earlier_first = styled_with_fallback(Some("Absent Primary"), &["Roboto", "Arial"]);
        assert_eq!(
            resolved(&mut system, &earlier_first),
            "Name(\"Roboto\")",
            "both are installed, so the DECLARED order decides"
        );

        let mut system = font_system(database(&[ARIAL, ROBOTO]));
        let reversed = styled_with_fallback(Some("Absent Primary"), &["Arial", "Roboto"]);
        assert_eq!(
            resolved(&mut system, &reversed),
            "Name(\"Arial\")",
            "reversing the chain reverses the answer — a resolver ignoring \
             order would return the same family for both"
        );
    }

    /// An installed primary still wins: the chain is a fallback, not a
    /// preference list that overrides.
    #[test]
    fn an_installed_primary_is_not_displaced_by_the_chain() {
        let mut system = font_system(database(&[ARIAL, ROBOTO]));
        let style = styled_with_fallback(Some("Arial"), &["Roboto"]);
        assert_eq!(resolved(&mut system, &style), "Name(\"Arial\")");
    }

    /// A generic in the chain resolves as that generic and ends the walk.
    ///
    /// That is what gives every real chain a defined stop — Cupertino's own
    /// ends in "sans-serif" — rather than relying on the degrade below it.
    #[test]
    fn a_generic_in_the_chain_resolves_and_terminates_it() {
        let mut system = font_system(database(&[ARIAL]));
        // "Arial" is installed and sits AFTER the generic, so reaching it
        // would prove the walk did not stop.
        let style = styled_with_fallback(Some("Absent"), &["monospace", "Arial"]);
        assert_eq!(resolved(&mut system, &style), "Monospace");
    }

    /// A chain of entirely absent names still degrades to the generic.
    #[test]
    fn an_all_absent_chain_still_degrades_to_the_generic() {
        let mut system = font_system(database(&[ARIAL]));
        let style = styled_with_fallback(Some("Absent Primary"), &["Also Absent", "Still Absent"]);
        assert_eq!(resolved(&mut system, &style), "SansSerif");
    }

    #[test]
    fn generic_names_pass_through_as_generics() {
        // Monospace must stay generic: mapping it to a concrete family name
        // would take it out of cosmic-text's `is_mono` weight-filter bypass
        // and its panose-driven monospace candidate set.
        let mut system = font_system(database(&[ROBOTO]));
        let mut installed = InstalledFamilies::default();
        for (written, expected) in [
            ("monospace", Family::Monospace),
            ("Monospace", Family::Monospace),
            ("mono", Family::Monospace),
            ("serif", Family::Serif),
            ("sans-serif", Family::SansSerif),
            ("cursive", Family::Cursive),
            ("fantasy", Family::Fantasy),
        ] {
            let style = styled(Some(written));
            assert_eq!(
                resolve_family(Some(&style), &mut system, &mut installed, 0),
                expected,
                "{written:?} must resolve to its generic, not to a concrete family"
            );
        }
    }

    #[test]
    fn a_present_family_is_used_verbatim() {
        let mut system = font_system(database(&[ROBOTO, ARIAL]));
        assert_eq!(
            resolved(&mut system, &styled(Some("Arial"))),
            r#"Name("Arial")"#
        );
    }

    #[test]
    fn an_absent_family_degrades_to_the_sans_serif_generic() {
        let mut system = font_system(database(&[ROBOTO]));
        assert_eq!(
            resolved(&mut system, &styled(Some("CupertinoSystemText"))),
            "SansSerif"
        );
    }

    #[test]
    fn a_style_naming_no_family_resolves_to_the_sans_serif_generic() {
        let mut system = font_system(database(&[ROBOTO]));
        let mut installed = InstalledFamilies::default();
        assert_eq!(
            resolve_family(None, &mut system, &mut installed, 0),
            Family::SansSerif
        );
        assert_eq!(resolved(&mut system, &styled(None)), "SansSerif");
    }

    #[test]
    fn binding_points_generics_at_a_family_the_database_carries() {
        let mut db = database(&[ROBOTO]);
        // cosmic-text's own defaults name families no test database carries.
        assert!(!database_carries(&db, db.family_name(&Family::SansSerif)));

        bind_generic_families(&mut db);

        for generic in [
            Family::SansSerif,
            Family::Serif,
            Family::Monospace,
            Family::Cursive,
            Family::Fantasy,
        ] {
            let bound = db.family_name(&generic).to_owned();
            assert!(
                database_carries(&db, &bound),
                "{generic:?} was bound to {bound:?}, which this database does not carry"
            );
        }
    }

    #[test]
    fn binding_leaves_an_already_resolvable_generic_alone() {
        let mut db = database(&[ROBOTO, ARIAL]);
        db.set_sans_serif_family("Arial");

        bind_generic_families(&mut db);

        assert_eq!(
            db.family_name(&Family::SansSerif),
            "Arial",
            "a generic the database already resolves must not be re-pointed"
        );
    }

    /// A face that cannot draw letters must never become a generic family,
    /// however early it sits in the database.
    ///
    /// The candidate list this binding draws from is cosmic-text's
    /// `common_fallback()`, which is a *glyph-coverage* list whose tail is
    /// symbols and emoji faces — so "first carried entry" is not a safe rule,
    /// and neither is "first face in the database". Material Icons stands in
    /// for that class here: it is loaded first, and it carries neither `'A'`
    /// nor `' '`.
    #[test]
    fn binding_skips_a_face_that_cannot_render_latin() {
        let mut db = database(&[MATERIAL_ICONS, ROBOTO]);
        assert_eq!(
            db.faces()
                .next()
                .and_then(|face| face.families.first().map(|(name, _)| name.as_str())),
            Some("Material Icons"),
            "fixture control: the icon font must be the first face, or this \
             test cannot observe the rule it exists for"
        );

        bind_generic_families(&mut db);

        for generic in [Family::SansSerif, Family::Monospace, Family::Serif] {
            assert_eq!(
                db.family_name(&generic),
                "Roboto",
                "{generic:?} must skip the icon face and bind to the one \
                 family that can render text"
            );
        }
    }

    /// Shapes `"Ao Bo"` through the same steps the production call sites take
    /// — resolve the family, build attrs, shape — and reports the family name
    /// behind the first LETTER glyph, the family behind the SPACE glyph, and
    /// the space advance in em.
    ///
    /// All three are reported because the defect this module exists for
    /// splits the letters from the space: a test reading only the first glyph
    /// would not see it.
    fn shape_probe(
        db: Database,
        style: &TextStyle,
        weight: u16,
        resolve: bool,
    ) -> (String, String, f32) {
        use cosmic_text::{Attrs, AttrsOwned, Buffer, Metrics, Shaping, Weight};

        const TEXT: &str = "Ao Bo";
        const SIZE: f32 = 17.0;

        let mut font_system = font_system(db);
        let family = if resolve {
            let mut installed = InstalledFamilies::default();
            resolve_family(Some(style), &mut font_system, &mut installed, 0)
        } else {
            // The pre-fix path: the style's family goes to the shaper unchecked.
            style
                .font_family
                .as_deref()
                .map_or(Family::SansSerif, Family::Name)
        };
        let attrs = Attrs::new()
            .family(family)
            .weight(Weight(weight))
            .metrics(Metrics::new(SIZE, SIZE * 1.2));
        let owned = AttrsOwned::new(&attrs);

        let mut buffer = Buffer::new(&mut font_system, Metrics::new(SIZE, SIZE * 1.2));
        buffer.set_size(Some(f32::MAX), None);
        buffer.set_rich_text(
            std::iter::once((TEXT, owned.as_attrs())),
            &Attrs::new(),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);

        let name_of = |id| {
            font_system
                .db()
                .face(id)
                .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "<unknown face>".to_owned())
        };
        let run = buffer
            .layout_runs()
            .next()
            .expect("one line of shaped text");
        let letter = run.glyphs.first().expect("a letter glyph");
        let space = run
            .glyphs
            .iter()
            .find(|glyph| &TEXT[glyph.start..glyph.end] == " ")
            .expect("a space glyph");
        (
            name_of(letter.font_id),
            name_of(space.font_id),
            space.w / SIZE,
        )
    }

    /// A style naming an uninstalled family must shape in the family the
    /// sans-serif generic points at — asserted in BOTH directions, because a
    /// single direction is satisfied by fixture load order alone.
    ///
    /// Without resolution the shaper is handed the uninstalled name,
    /// `Database::query` finds nothing to move to the front of the candidate
    /// list, and the choice falls to the unfiltered tail — which walks
    /// candidates by face id after a six-field tie, i.e. by the order the
    /// fixture happened to load them. Each row's `wrong_answer` is that, and
    /// no load order produces both right answers.
    ///
    /// This pins family selection only. The oversized-space *symptom* needs a
    /// face that carries `' '` but no letters, which no in-tree font does —
    /// `oversized_space_from_an_emoji_face_is_closed` covers it against a host
    /// emoji font instead.
    #[test]
    fn an_uninstalled_family_shapes_in_the_bound_generic_both_ways() {
        let style = styled(Some("CupertinoSystemText"));

        for (faces, generic_target, wrong_answer) in [
            (&[ARIAL, ROBOTO][..], "Roboto", "Arial"),
            (&[ROBOTO, ARIAL][..], "Arial", "Roboto"),
        ] {
            let mut db = database(faces);

            // Fixture control. The `wrong_answer` above is whatever the
            // unfiltered tail picks, and the tail breaks a six-field tie by
            // face id — so it is the first-loaded face only while both faces
            // tie on weight, stretch and style. Assert that tie directly:
            // swapping a fixture for a Bold or variable cut would otherwise
            // move the tie-break silently and leave this test green for the
            // wrong reason.
            let mut fixture: Vec<_> = db
                .faces()
                .map(|face| {
                    (
                        face.families
                            .first()
                            .map(|(name, _)| name.clone())
                            .unwrap_or_default(),
                        face.weight.0,
                        face.style,
                        face.monospaced,
                    )
                })
                .collect();
            assert_eq!(fixture.len(), 2, "fixture control: exactly two faces");
            assert_eq!(
                fixture.remove(0),
                (
                    wrong_answer.to_owned(),
                    400,
                    cosmic_text::Style::Normal,
                    false
                ),
                "fixture control: the lowest-id face decides the unresolved \
                 answer, and only while the faces tie on weight/style"
            );
            assert_eq!(
                (fixture[0].1, fixture[0].2, fixture[0].3),
                (400, cosmic_text::Style::Normal, false),
                "fixture control: both faces must tie, or the tie-break is not id"
            );

            db.set_sans_serif_family(generic_target);

            let (letter, space, _) = shape_probe(db, &style, 600, true);
            assert_eq!(
                letter, generic_target,
                "letters must shape in the family sans-serif points at, not \
                 the face that happens to sort first"
            );
            assert_eq!(
                space, generic_target,
                "the space must shape in the same family as the letters"
            );
        }
    }

    /// The reported symptom, against a real emoji face: an uninstalled family
    /// hands the SPACE to the emoji font at roughly 1.24 em while the letters
    /// shape correctly elsewhere, and resolution closes it.
    ///
    /// The emoji face comes from the host because no in-tree font can play the
    /// part — a decoy has to carry `' '` but no letters, and both shipped icon
    /// fonts carry neither.
    ///
    /// Where the host has none, the test degrades instead of failing — but a
    /// Rust test that returns early is reported **PASSED**, so a silent
    /// degradation would inflate the pass count with coverage that did not
    /// run. `FLUI_REQUIRE_EMOJI_FONT` makes the absent-font branch a hard
    /// failure. CI exports it from the same step that installs the font
    /// package, so the requirement and its guarantee share one condition and
    /// a runner without the install can never be told to require it. The skip
    /// is a developer-machine convenience, never a hole in the gate. Its
    /// hermetic counterpart, `an_uninstalled_family_shapes_in_the_bound_generic_both_ways`,
    /// pins the same fix through family selection and needs no fixture.
    /// Reports a missing fixture precondition: loud where the environment
    /// promises one (CI), quiet where it cannot (a developer machine without
    /// an emoji font installed).
    fn skip_or_fail(reason: &str) {
        assert!(
            std::env::var_os("FLUI_REQUIRE_EMOJI_FONT").is_none(),
            "FLUI_REQUIRE_EMOJI_FONT is set, so this fixture must be available: {reason}"
        );
        eprintln!("skipped: {reason}");
    }

    #[test]
    fn oversized_space_from_an_emoji_face_is_closed() {
        let mut host = Database::new();
        host.load_system_fonts();
        let Some(emoji) = host
            .faces()
            .find(|face| face.post_script_name.contains("Emoji"))
            .map(|face| face.source.clone())
        else {
            return skip_or_fail("this host has no emoji font to build the fixture from");
        };
        let fontdb::Source::File(path) = emoji else {
            return skip_or_fail("the host's emoji face is not a plain file");
        };
        let Ok(emoji_bytes) = std::fs::read(&path) else {
            return skip_or_fail(&format!("could not read {}", path.display()));
        };

        let style = styled(Some("CupertinoSystemText"));
        let fixture = || {
            let mut db = database(&[ROBOTO]);
            db.load_font_data(emoji_bytes.clone());
            db
        };

        // Red state: the family reaches the shaper unchecked.
        let (unresolved_letter, unresolved_space, unresolved_em) =
            shape_probe(fixture(), &style, 400, false);
        assert!(
            unresolved_em > 1.0,
            "precondition: without resolution the space must come from the \
             emoji face at about 1.24 em, got {unresolved_em} em (letters in \
             {unresolved_letter}, space in {unresolved_space})"
        );
        assert_ne!(
            unresolved_letter, unresolved_space,
            "precondition: the defect is letters and space landing on \
             different faces"
        );

        let (letter, space, em) = shape_probe(fixture(), &style, 400, true);
        assert_eq!(
            letter, "Roboto",
            "letters must shape in the only text family present"
        );
        assert_eq!(
            space, letter,
            "the space must shape in the same face as the letters"
        );
        assert!(
            em < 0.5,
            "a space of {em} em is a foreign face's advance, not a text face's"
        );
    }

    #[test]
    fn a_face_loaded_after_the_first_resolve_is_picked_up() {
        // The shared database really does grow after text has been measured:
        // `TextRenderer::new` loads Roboto and both icon fonts into it at
        // renderer construction, which happens after layout has run.
        let mut system = font_system(database(&[ROBOTO]));
        let mut installed = InstalledFamilies::default();
        let style = styled(Some("Material Icons"));
        // The generation stands in for `SharedFontSystem::with_mut`, which
        // bumps it on every call that could touch the database.
        let mut db_generation = 0;
        assert_eq!(
            resolve_family(Some(&style), &mut system, &mut installed, db_generation),
            Family::SansSerif,
            "precondition: the icon family is absent to begin with"
        );

        system.db_mut().load_font_data(MATERIAL_ICONS.to_vec());
        db_generation += 1;

        assert_eq!(
            resolve_family(Some(&style), &mut system, &mut installed, db_generation),
            Family::Name("Material Icons"),
            "a face loaded after the first resolve must be seen"
        );
    }

    /// A remove-then-add through the database leaves the face COUNT unchanged
    /// and the index stale — which is why the index is keyed on a mutation
    /// generation and not on that count.
    ///
    /// `SharedFontSystem::with_mut` hands out `&mut FontSystem`, so this is
    /// reachable from outside the crate today, not only from a hypothetical
    /// future caller: swap one family for another and every later style
    /// resolves against a database that no longer exists.
    #[test]
    fn swapping_one_face_for_another_invalidates_the_index() {
        let mut system = font_system(database(&[ROBOTO]));
        let mut installed = InstalledFamilies::default();
        let arial = styled(Some("Arial"));

        let mut db_generation = 0;
        assert_eq!(
            resolve_family(Some(&arial), &mut system, &mut installed, db_generation),
            Family::SansSerif,
            "precondition: Arial is absent to begin with"
        );

        // Remove one face, add one face: the count is what it was.
        let roboto_id = system
            .db()
            .faces()
            .next()
            .expect("the fixture has a face")
            .id;
        let faces_before = system.db().len();
        system.db_mut().remove_face(roboto_id);
        system.db_mut().load_font_data(ARIAL.to_vec());
        assert_eq!(
            system.db().len(),
            faces_before,
            "the fixture must leave the count unchanged, or this test does not \
             exercise what it is named for"
        );
        db_generation += 1;

        assert_eq!(
            resolve_family(Some(&arial), &mut system, &mut installed, db_generation),
            Family::Name("Arial"),
            "the family that arrived must be seen even though the face count \
             never moved"
        );
    }

    /// The generic bindings must survive a database that was EMPTY when the
    /// font system was built — the headless and CI path, where
    /// `FontSystem::new()` finds nothing and `TextRenderer::new` loads the
    /// embedded faces afterwards.
    ///
    /// Construction-time binding has nothing to choose from there, so without
    /// a rebind every later run resolves through a generic naming a family the
    /// database still does not carry.
    #[test]
    fn generics_bind_when_the_database_was_empty_at_construction() {
        let mut db = Database::new();
        bind_generic_families(&mut db);
        let mut system = font_system(db);
        assert!(
            !database_carries(system.db(), system.db().family_name(&Family::SansSerif)),
            "precondition: an empty database cannot bind any generic"
        );

        system.db_mut().load_font_data(ROBOTO.to_vec());

        let mut installed = InstalledFamilies::default();
        let style = styled(Some("CupertinoSystemText"));
        assert_eq!(
            resolve_family(Some(&style), &mut system, &mut installed, 1),
            Family::SansSerif
        );
        assert_eq!(
            system.db().family_name(&Family::SansSerif),
            "Roboto",
            "resolving must have re-pointed the generic at the face that \
             arrived after construction"
        );
    }
}
