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
//! * The requested **weight**, snapped to one the resolved family can serve
//!   ([`snap_weight`]). Not snapping was tried first, on the reasoning that
//!   `Database::query` already applies CSS font matching and that a snap
//!   strips the requested instance off a variable face. The first half is
//!   true and irrelevant — `query` picks the best face *within* a family,
//!   while the abandonment happens a layer up, in
//!   `FontFallbackIter::default_font_match_key`, whose filter drops every
//!   face whose weight differs and is not a variable match, and whose empty
//!   result makes `next_item` leave the family entirely. The second half is
//!   why [`family_accepts_weight`] probes the variable axis first and only
//!   snaps when no face — static or variable — can serve the request.

use std::collections::HashSet;

use crate::typography::TextStyle;
use cosmic_text::fontdb::{self, Database, Family};
use cosmic_text::{Fallback as _, FontSystem, PlatformFallback};

/// Maps a family name written in a [`TextStyle`] to a CSS generic, if it names
/// one.
///
/// The spellings mirror what the two `style_to_attrs` conversions accepted
/// before this module existed, so no style that resolved to a generic stops
/// doing so.
pub(crate) fn generic_family(name: &str) -> Option<Family<'static>> {
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
/// registered *after* construction, through
/// `SharedFontSystem::register_font` or any other `SharedFontSystem::with_mut`, is
/// never forbidden.
///
/// That is the same growth [`InstalledFamilies::sync`] exists to track, and it
/// is not hypothetical: on a host where `FontSystem::new()` finds no faces at
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

/// Whether any face in `family` is one cosmic-text would accept at `weight`.
///
/// Mirrors the filter in `FontFallbackIter::default_font_match_key`
/// (`font/fallback/mod.rs`) exactly:
/// `font_weight_diff == 0 || variable_weight_match`. When that filter finds
/// nothing, `next_item`'s `(false, None)` arm logs "No default font match"
/// and `break`s out of the family loop — cosmic-text abandons the family and
/// takes a `common_fallback()` family that happens to own the exact weight, so
/// a `Roboto` run at W600 renders in Noto Sans SemiBold on a host with the
/// full Noto weight set (issue #929).
///
/// # There is no monospace term here, and there was one
///
/// An earlier revision accepted any face with `face.monospaced == true`,
/// citing this same function. That citation was wrong:
/// `default_font_match_key`'s filter has no mono term at all. The `|| is_mono`
/// that does exist lives in `next_item`'s `font_match_keys_iter`, and
/// `is_mono` there is `default_families[i] == &Family::Monospace` — a property
/// of the **request**, never of the face. The consequence was measured: a
/// `Fira Code` request at W900 answered "acceptable", was not snapped, and
/// then shaped in Noto Sans. [`snap_weight`] handles the real mono rule where
/// it belongs, on the request.
///
/// # Uncertainty resolves the way cosmic-text resolves it
///
/// A face whose data will not parse is *not* a variable match — cosmic-text's
/// `variable_weight_match` compares against `Some(Some(true))`, so every other
/// outcome is `false`. This mirrors that. An earlier revision erred the other
/// way, to avoid stripping a variable instance; that reasoning does not hold,
/// because a face whose data will not parse cannot be instanced by
/// `FontSystem::get_font` either. Predicting "the family is fine" for a face
/// cosmic-text will discard is precisely the failure this function exists to
/// prevent.
///
/// # What pins each arm
///
/// No shipped font asset discriminates either arm — `Roboto-Regular`,
/// `FLUI Probe Sans` and `MaterialIcons-Regular` are all single-weight,
/// non-monospaced, static faces, so both arms could be deleted without
/// turning the suite red. The generated fixtures exist for exactly this:
/// `probe-mono-{100,600}.ttf` is one monospaced family at two weights, and
/// `probe-variable-wght.ttf` carries an `fvar` `wght` axis over a
/// `usWeightClass` of 400. See `tools/decoy-face/generate.py`.
fn family_accepts_weight(db: &Database, family: &str, weight: u16) -> bool {
    db.faces()
        .filter(|face| face.families.iter().any(|(name, _)| name == family))
        .any(|face| face.weight.0 == weight || variable_weight_covers(db, face.id, weight))
}

/// Whether `id` is a variable face whose `wght` axis covers `weight`.
///
/// Uses the `skrifa` re-export cosmic-text already exposes, so no new
/// dependency — the same route [`can_render_latin`] takes.
///
/// Every uncertain outcome answers `false`, because that is what cosmic-text
/// answers: `FontMatchKey::new` computes `variable_weight_match` as
/// `db.with_face_data(..) == Some(Some(true))`, so an unreadable face, an
/// absent `wght` axis and a face missing from the database are all "not a
/// variable match" there. See [`family_accepts_weight`] for why mirroring that
/// direction — rather than erring toward "acceptable" — is the correct one.
fn variable_weight_covers(db: &Database, id: fontdb::ID, weight: u16) -> bool {
    use cosmic_text::skrifa::{self, MetadataProvider as _};

    db.with_face_data(id, |data, index| {
        let font = skrifa::FontRef::from_index(data, index).ok()?;
        let wght = font.axes().get_by_tag(skrifa::Tag::new(b"wght"))?;
        let weight = f32::from(weight);
        Some(wght.min_value() <= weight && weight <= wght.max_value())
    }) == Some(Some(true))
}

/// The weight to actually request for `family`, given the style asked for
/// `requested`.
///
/// Unchanged whenever the family can serve the request. When it cannot, the
/// weight the family DOES carry that CSS font matching would pick — which is
/// what keeps cosmic-text from discarding the family altogether (issue #929).
///
/// # Generics are probed, not skipped
///
/// An earlier revision returned early for every non-`Name` family, on the
/// reasoning that "a generic is resolved by cosmic-text against a family this
/// layer did not choose". That was wrong twice over:
/// [`bind_generic_families`] in this very module is what chose it, and
/// `Database::family_name` reads it back — the same call cosmic-text itself
/// makes at the top of `default_font_match_key`. The measured consequence was
/// that the snap never ran on the busiest path there is: every Material
/// `title_*` and `label_*` style is W500 with **no** family, so it resolves to
/// `Family::SansSerif` and skipped the probe entirely.
///
/// `Family::Monospace` is probed like the rest. A monospace request is never
/// abandoned outright (`next_item`'s `(true, None)` arm does not `break` the
/// family loop), but at a weight the bound family lacks it moves on to another
/// monospaced face that has it: with the generics bound to the bundled Roboto
/// Regular, a bold "monospace" painted in the host's mono bold while the
/// collection measured it in Roboto (painting mapping decision 16). The probe
/// accepts a variable face first, so no instance is stripped for nothing.
pub(crate) fn snap_weight(db: &Database, family: &Family<'_>, requested: u16) -> u16 {
    // Resolves `Family::Name(n)` to `n` and every generic to its bound name.
    let name = db.family_name(family);
    if family_accepts_weight(db, name, requested) {
        return requested;
    }
    let mut carried: Vec<u16> = db
        .faces()
        .filter(|face| face.families.iter().any(|(fam, _)| fam == name))
        .map(|face| face.weight.0)
        .collect();
    if carried.is_empty() {
        return requested;
    }
    carried.sort_unstable();
    carried.dedup();
    // Pinned through this call, not only on the rule:
    // `the_snap_takes_the_css_answer_not_the_nearest_one` drives a family
    // carrying 100 and 600 at a W500 request, which is a case where CSS order
    // (100) and nearest-by-distance (600) disagree.
    css_nearest_weight(&carried, requested).unwrap_or(requested)
}

/// The weight CSS font matching picks from `carried` for a `requested` the
/// family does not have, per CSS Fonts 4 §"Matching font styles".
///
/// `carried` must be sorted ascending and deduplicated.
///
/// Ordering by absolute distance — the obvious implementation, and the one
/// this replaces — is both non-deterministic and wrong. Non-deterministic
/// because `min_by_key` keeps the first minimum in iteration order, so a
/// family carrying 400 and 600 answers a W500 request differently depending on
/// which face the database loaded first. Wrong because CSS does not resolve by
/// distance: 500 resolves DOWN to 400 before it looks up, and 400 resolves UP
/// to 500 before it looks down, so the two adjacent text weights prefer each
/// other rather than tying.
fn css_nearest_weight(carried: &[u16], requested: u16) -> Option<u16> {
    let below = || carried.iter().rev().find(|w| **w < requested).copied();
    let above = || carried.iter().find(|w| **w > requested).copied();

    if requested < 400 {
        // Below in descending order, then above in ascending order.
        below().or_else(above)
    } else if requested > 500 {
        // Above in ascending order, then below in descending order.
        above().or_else(below)
    } else {
        // 400..=500: weights at or above the target up to 500 first, then
        // below in descending order, then everything above 500.
        carried
            .iter()
            .find(|w| **w > requested && **w <= 500)
            .copied()
            .or_else(below)
            .or_else(above)
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
    /// hard-coded `"Open Sans"`. `register_font` then loads faces into that
    /// same database (as the bundled fonts once did from the engine). Without a
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

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
    const MATERIAL_ICONS: &[u8] = include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf");
    /// Maps ONLY `U+0020`, at 1.3 em, with "Emoji" in its PostScript name.
    ///
    /// Generated by `tools/decoy-face/generate.py`. This test used to build
    /// its fixture from the HOST's emoji font, because no shipped font can
    /// play the part — a decoy has to carry `U+0020` and no letters, and both
    /// icon fonts carry neither. That made a merge-blocking assertion depend
    /// on a distro package's space advance (issue #932).
    const DECOY_WIDE_SPACE: &[u8] = include_bytes!("../../assets/fonts/decoy-wide-space.ttf");

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
        for glyph in run.glyphs {
            assert_ne!(glyph.glyph_id, 0, "shaping probe must not use .notdef");
            assert!(glyph.w > 0.0, "shaping probe glyphs need positive advances");
        }
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

    /// Issue #927's actual symptom: an emoji face shaping the SPACE of a Latin
    /// run at ~1.3 em while the letters shape elsewhere.
    ///
    /// The fixture is committed (`DECOY_WIDE_SPACE`), not built from the
    /// host's emoji font as it once was. That mattered: the `unresolved_em`
    /// assertion is merge-blocking, and parameterising it on a distro font
    /// package meant a Noto Color Emoji metrics change could turn CI red on a
    /// FONT update, with the cause nowhere near the diff (issue #932). It also
    /// removes the skip branch entirely — a Rust test that returns early is
    /// reported PASSED, so the old absent-font path needed
    /// `FLUI_REQUIRE_EMOJI_FONT` to stay honest, and now needs nothing.
    ///
    /// Its hermetic counterpart,
    /// `an_uninstalled_family_shapes_in_the_bound_generic_both_ways`, pins the
    /// same fix through family selection and needs no fixture at all.
    fn oversized_space_from_an_emoji_face_is_closed() {
        let style = styled(Some("CupertinoSystemText"));
        let fixture = || database(&[ROBOTO, DECOY_WIDE_SPACE]);

        // Red state: the family reaches the shaper unchecked.
        let (unresolved_letter, unresolved_space, unresolved_em) =
            shape_probe(fixture(), &style, 400, false);
        assert!(
            unresolved_em > 1.0,
            "precondition: without resolution the space must come from the \
             decoy at 1.3 em, got {unresolved_em} em (letters in \
             {unresolved_letter}, space in {unresolved_space}). Roboto's own \
             space is ~0.25 em, which is what this reads if the decoy is \
             missing from the fixture"
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

    /// Font resolution rows: each names the family-selection rule it pins.
    #[test]
    fn family_resolution_contract() {
        let cases: [(&str, fn()); 2] = [
            (
                "a_present_but_narrow_family_stops_the_chain",
                a_present_but_narrow_family_stops_the_chain_where_flutter_would_not,
            ),
            (
                "oversized_space_from_an_emoji_face_is_closed",
                oversized_space_from_an_emoji_face_is_closed,
            ),
        ];
        let mut failed = Vec::new();
        for (name, case) in cases {
            if std::panic::catch_unwind(case).is_err() {
                failed.push(name);
            }
        }
        assert!(
            failed.is_empty(),
            "font resolution: failing rows: {failed:?}"
        );
    }
}
