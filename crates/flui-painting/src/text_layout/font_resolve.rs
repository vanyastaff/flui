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
//! * [`resolve_family_name`] degrades a named family the fonts lack to
//!   `Family::SansSerif`; the binding above points that generic at a carried
//!   family whenever the fonts hold any Latin-capable face. This is the
//!   Cupertino path, whose roles all name `CupertinoSystemText` — a family
//!   the platform aliases to San Francisco and that exists nowhere else.
//!
//! Nothing shapes on the process font system any more. The routes above are
//! how cosmic-text reached the emoji face; Parley has the first one too, since
//! a family the collection lacks sends every cluster down the collection's
//! fallback order, which a host feed copies from the process font system's
//! lists. The family rule ([`resolve_family_name`]) closes it on the
//! collection: the Parley path is handed a held family or a generic, never an
//! absent name. The process side keeps the generic binding
//! ([`bind_generic_families`]), whose names the host feed copies.

use crate::typography::TextStyle;
use cosmic_text::fontdb::{self, Database, Family};
use cosmic_text::{Fallback as _, PlatformFallback};

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

/// Points every generic family name at a family this database carries.
///
/// Called on a freshly built [`Database`]: generic names live in the
/// database and are read at query time, so a later call would take effect on
/// a live `FontSystem` too.
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
/// Reached only while binding the generics, never per shaped run — but it is
/// not cheap: `fontdb::with_face_data` opens, maps and parses the file on
/// every call with no cache of its own, and the cost is candidates × faces.
/// That is why [`pick_family`] puts every cheaper discriminator ahead of it.
fn can_render_latin(db: &Database, id: fontdb::ID) -> bool {
    use cosmic_text::skrifa::{self, MetadataProvider as _};

    db.with_face_data(id, |data, index| {
        let font = skrifa::FontRef::from_index(data, index).ok()?;
        let charmap = font.charmap();
        Some(charmap.map('A').is_some() && charmap.map(' ').is_some())
    }) == Some(Some(true))
}

/// Keeps emoji families out of cosmic-text's UNFILTERED fallback tail.
///
/// # The defect this works around
///
/// `FontMatchKey` derives `Ord` with `not_emoji: bool` as its **first** field
/// (`font/system.rs:20-30`) and `font_match_keys.sort()` is ascending, so
/// `false` — the emoji faces — sorts first. The comment on that very sort says
/// *"Sort so we get the keys with weight_offset=0 first"*, which the field
/// order contradicts: it reads as unintended rather than designed. The tail
/// itself (`font/fallback/mod.rs:468-480`) is unordered by weight distance, so
/// once it is reached the choice is arbitrary and emoji-first.
///
/// Issue #927 made the requested family resolve, which puts a real text face at
/// index 0 — but that relies on the tail's FIRST ENTRY being right rather than
/// on the tail being safe. The tail is still reachable on a path resolution
/// does not cover: a word whose script the resolved family lacks meets the same
/// exact-weight filter in the SCRIPT fallback list, and on a host whose CJK
/// face is installed at a non-400 weight it drops through with nothing useful
/// at index 0 (issue #930).
///
/// # Why this is safe for real emoji
///
/// `forbidden_fallback` is consulted in exactly ONE place — the tail loop at
/// `font/fallback/mod.rs:469` — verified by grep across the crate: the other
/// three sites are the trait declaration and `Fallbacks`' own list
/// construction. Nothing on the `common_fallback` or `script_fallback` path
/// reads it, and `next_item` walks both of those loops to exhaustion *before*
/// it reaches the tail.
///
/// The route genuine emoji actually take is `common_fallback()`, whose unix
/// list **ends** in `"Noto Color Emoji"` (`font/fallback/unix.rs`) — not
/// `script_fallback()`, which an earlier revision of this doc claimed. Emoji
/// codepoints are `Script::Common`, and the unix script table has no entry for
/// it: that lookup falls to `_ => &[]`. Naming the wrong route mattered
/// because the two differ exactly where this type has to be careful — see
/// [`EmojiForbiddenFallback::new`] on the targets where `common_fallback()` is
/// empty and the tail is the only route there is.
///
/// # Known gap: the list is a construction-time snapshot
///
/// cosmic-text reads `forbidden_fallback()` once, inside `Fallbacks::new`,
/// which runs in the `FontSystem` constructor, and exposes no setter —
/// `Fallbacks::extend` refreshes only the per-script lists. So an emoji face
/// added to the database *after* construction is never forbidden. Nothing
/// adds one today: a registration loads the collection alone.
///
/// It is not hypothetical: on a host where `FontSystem::new()` finds no faces at
/// all — headless, CI, a minimal container — this scan sees an empty database
/// and forbids nothing for the life of the process. The asymmetry is stated
/// rather than fixed because closing it means rebuilding the font system, and
/// a rebuild discards every shaping cache it holds. What is lost is a
/// mitigation, not a correctness guarantee: without it the tail behaves as it
/// did before this type existed.
///
/// # Why the list is scanned rather than hard-coded
///
/// The trait returns `&[&'static str]` borrowed from `&self`, not
/// `&'static [&'static str]` — so the LIST may be computed, and only the names
/// need be `'static`. A per-platform hard-coded list would be a guess about the
/// host: it misses an emoji font not on it, and wrongly forbids a text family
/// whose name resembles one. Scanning the database with cosmic-text's own
/// predicate — `post_script_name.contains("Emoji")` (`font/system.rs:35`), the
/// very one that produces the `not_emoji` sort key — cannot be less accurate
/// about the host than cosmic-text is about itself. The names are leaked once,
/// for a font system that lives as long as the process.
pub(crate) struct EmojiForbiddenFallback {
    inner: cosmic_text::PlatformFallback,
    forbidden: Vec<&'static str>,
}

impl EmojiForbiddenFallback {
    /// Scan `db` for the families cosmic-text would classify as emoji, and add
    /// them to the platform's own forbidden list.
    ///
    /// # Why the platform list is EXTENDED, never replaced
    ///
    /// `PlatformFallback::forbidden_fallback()` is not empty everywhere:
    /// on macOS it is `[".LastResort"]`, the system's tofu face, which exists
    /// precisely so it never wins a fallback. Returning only the scanned names
    /// would drop that entry and let `.LastResort` serve a run — and no CI job
    /// would catch it, since macOS is lint-only here.
    ///
    /// # Why this can be a no-op
    ///
    /// On any target that is neither unix-not-Android, Windows nor macOS —
    /// **Android and wasm** — `common_fallback()` is empty
    /// (`font/fallback/other.rs`), and the unfiltered tail is therefore the
    /// *only* route to any fallback face at all. Forbidding emoji families
    /// there does not redirect a Latin run to a text face; it makes genuine
    /// emoji unrenderable, because nothing else can reach them. So the scan is
    /// skipped wherever the platform offers no curated list to carry emoji
    /// instead. Expressed as a runtime check on that list rather than as a
    /// `cfg`, because the property that matters is "is there another route",
    /// and a new target answers it correctly without being enumerated here.
    pub(crate) fn new(db: &Database) -> Self {
        let inner = cosmic_text::PlatformFallback;
        let forbidden = forbidden_list(
            inner.forbidden_fallback(),
            !inner.common_fallback().is_empty(),
            db,
        );
        Self { inner, forbidden }
    }
}

/// The forbidden list [`EmojiForbiddenFallback::new`] installs, as a function
/// of the platform's own list and whether the platform offers a curated
/// `common_fallback()`.
///
/// Extracted so both properties are testable on any host: on Linux the
/// platform's forbidden list is empty and its common list is not, so a test
/// written against `PlatformFallback` directly could only ever exercise one
/// corner of this — and it is the OTHER corners (macOS's `.LastResort`,
/// Android's empty common list) that carry the risk, on the two platforms CI
/// never executes.
fn forbidden_list(
    platform_forbidden: &[&'static str],
    platform_has_common_fallback: bool,
    db: &Database,
) -> Vec<&'static str> {
    let mut forbidden: Vec<&'static str> = platform_forbidden.to_vec();
    if !platform_has_common_fallback {
        return forbidden;
    }
    for face in db.faces() {
        if !face.post_script_name.contains("Emoji") {
            continue;
        }
        for (family, _) in &face.families {
            if forbidden.contains(&family.as_str()) {
                continue;
            }
            // Leaked deliberately: `Fallback` hands out `&'static str` and the
            // names are only known after a runtime scan, so there is no borrow
            // that satisfies it. Bounded by the host's emoji family count —
            // single digits — and paid once per font system, which production
            // constructs once per process.
            forbidden.push(Box::leak(family.clone().into_boxed_str()));
        }
    }
    forbidden
}

impl cosmic_text::Fallback for EmojiForbiddenFallback {
    fn common_fallback(&self) -> &[&'static str] {
        self.inner.common_fallback()
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &self.forbidden
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        self.inner.script_fallback(script, locale)
    }
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

/// The family to shape `style` with, given which families the fonts carry.
///
/// Returns the style's own family when `carries` says so, the matching
/// generic when the style names one, then the first entry of the style's
/// declared fallback chain that is a generic or is carried, and
/// `Family::SansSerif` when nothing is. The generic binding points that
/// fallback at a carried family whenever the fonts hold a Latin-capable face.
/// A generic is returned *as a generic*, so the collection's binding for it
/// applies.
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
        // Matches what `Attrs::new()` has always defaulted to; a style naming
        // no family is unchanged by this module beyond the generic binding.
        return Family::SansSerif;
    };

    if let Some(generic) = generic_family(requested) {
        return generic;
    }

    if carries(requested) {
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
        if carries(candidate) {
            return Family::Name(candidate);
        }
    }

    Family::SansSerif
}
