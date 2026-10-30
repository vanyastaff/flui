//! The fallback order a host-fed collection walks past a style's family
//! (ADR-0092 §7).
//!
//! Parley tries the collection's fallback families for the item's script,
//! then the Han fallback (fontique's `Query::set_fallbacks`), and adds the
//! emoji generic after the style's families for an emoji cluster. A
//! [`FallbackChain`] holds FLUI's lists for the host (`fallback_tables`) and
//! [`install_into`] writes them into a fontique collection: per script, the
//! script's list, then the common list, then the family the collection's
//! sans-serif generic names; the emoji generic is the platform's colour
//! emoji faces, then the common list. There is no walk over every other
//! face, so a character no listed family covers shapes as `.notdef`
//! (flui-painting `ARCHITECTURE.md`, mapping decision 17).

use parley::fontique::Script;

use super::fallback_tables::{self, Platform};
use super::system_fallback::{FaceRef, SystemFallback};

/// The fallback lists a host feed installs: a common list, the colour emoji
/// faces, and a list per script, chosen for one locale.
#[derive(Clone, Debug)]
pub(crate) struct FallbackChain {
    common: Vec<&'static str>,
    emoji: Vec<&'static str>,
    scripts: Vec<(Script, &'static [&'static str])>,
}

impl FallbackChain {
    /// This platform's lists, with the Han list picked for `locale`.
    pub(crate) fn platform(locale: &str) -> Self {
        Self::of(fallback_tables::HOST, locale)
    }

    /// `platform`'s lists, with the Han list picked for `locale`: an entry
    /// for every script fontique names that has a list of its own.
    fn of(platform: Platform, locale: &str) -> Self {
        Self {
            common: fallback_tables::common(platform).to_vec(),
            emoji: fallback_tables::emoji(platform).to_vec(),
            scripts: fontique_scripts()
                .filter_map(|script| {
                    let list = fallback_tables::script(platform, script.as_str(), locale);
                    (!list.is_empty()).then_some((script, list))
                })
                .collect(),
        }
    }

    /// A chain over the given lists and no emoji faces, for a test.
    #[cfg(test)]
    pub(crate) fn from_lists(
        common: &[&'static str],
        scripts: &[(Script, &'static [&'static str])],
    ) -> Self {
        Self {
            common: common.to_vec(),
            emoji: Vec::new(),
            scripts: scripts.to_vec(),
        }
    }

    /// This chain with `emoji` as its colour emoji faces, for a test.
    #[cfg(test)]
    pub(crate) fn with_emoji(mut self, emoji: &[&'static str]) -> Self {
        self.emoji = emoji.to_vec();
        self
    }

    /// The families tried after a script's own list, in order.
    pub(crate) fn common(&self) -> &[&'static str] {
        &self.common
    }

    /// The colour emoji faces an emoji cluster tries before the common list.
    pub(crate) fn emoji(&self) -> &[&'static str] {
        &self.emoji
    }

    /// The families tried first for `script`, in order.
    pub(crate) fn script(&self, script: Script) -> &[&'static str] {
        self.scripts
            .iter()
            .find(|(held, _)| *held == script)
            .map_or(&[], |(_, list)| list)
    }
}

/// Every script fontique names, and the Common, Inherited and Unknown
/// scripts: the keys a collection's fallbacks are set under.
pub(crate) fn fontique_scripts() -> impl Iterator<Item = Script> {
    use parley::fontique::ScriptExt as _;

    Script::all_samples()
        .iter()
        .map(|(script, _)| *script)
        .chain([Script::COMMON, Script::INHERITED, Script::UNKNOWN])
}

/// The host's own fallback answer (ADR-0271), resolved to families the
/// collection holds: what [`install_into`] puts ahead of the chain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HostOrder {
    /// Per script, the host's families for it, best first.
    pub(crate) scripts: Vec<(Script, Vec<parley::fontique::FamilyId>)>,
    /// The host's colour emoji families, best first.
    pub(crate) emoji: Vec<parley::fontique::FamilyId>,
    /// The host's common families, best first.
    pub(crate) common: Vec<parley::fontique::FamilyId>,
}

impl HostOrder {
    /// The host's families for `script`, empty when it named none.
    fn script(&self, script: Script) -> &[parley::fontique::FamilyId] {
        self.scripts
            .iter()
            .find(|(held, _)| *held == script)
            .map_or(&[], |(_, families)| families)
    }
}

/// `system` resolved against `collection`: each named face replaced by the
/// family the collection holds it under, each family once per list, in the
/// host's order.
///
/// A [`FaceRef::File`] is matched by its file and face index (both sides
/// canonicalised, so a path through a symlink still matches), a
/// [`FaceRef::Family`] by the collection's family name. A face the
/// collection does not hold is left out. A face with colour glyph tables
/// (`COLR`, `CBDT`, `sbix`) is left out of the script and common lists and
/// kept in the emoji list only: in a text list it would shape the spaces and
/// digits of ordinary text (the defect `font_resolve` exists for).
pub(crate) fn resolve(
    system: &SystemFallback,
    collection: &mut parley::fontique::Collection,
) -> HostOrder {
    let mut faces = FaceIndex::default();
    let mut text_list = |faces: &mut FaceIndex, refs: &[FaceRef]| {
        let mut ids = Vec::new();
        for face in refs {
            if let Some((id, colour)) = faces.resolve(face, collection)
                && !colour
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        ids
    };
    let scripts = system
        .scripts
        .iter()
        .map(|(script, refs)| (script.to_fontique(), text_list(&mut faces, refs)))
        .filter(|(_, ids)| !ids.is_empty())
        .collect();
    let common = text_list(&mut faces, &system.common);
    let mut emoji = Vec::new();
    for face in &system.emoji {
        if let Some((id, _)) = faces.resolve(face, collection)
            && !emoji.contains(&id)
        {
            emoji.push(id);
        }
    }
    let order = HostOrder {
        scripts,
        emoji,
        common,
    };
    if faces.unresolved > 0 {
        tracing::debug!(
            unresolved = faces.unresolved,
            "faces the host named for fallback are not in the font collection"
        );
    }
    order
}

/// The collection's file-backed faces by canonical file and index, built on
/// the first [`FaceRef::File`] it is asked about.
#[derive(Default)]
struct FaceIndex {
    files: Option<std::collections::HashMap<(std::path::PathBuf, u32), parley::fontique::FamilyId>>,
    unresolved: usize,
}

impl FaceIndex {
    /// The family `face` is held under, and whether that face has colour
    /// glyph tables.
    fn resolve(
        &mut self,
        face: &FaceRef,
        collection: &mut parley::fontique::Collection,
    ) -> Option<(parley::fontique::FamilyId, bool)> {
        let found = match face {
            FaceRef::Family(name) => collection.family_id(name).and_then(|id| {
                let family = collection.family(id)?;
                let colour = family.default_font().is_some_and(has_colour_tables);
                Some((id, colour))
            }),
            FaceRef::File { path, index } => {
                let files = self.files.get_or_insert_with(|| file_index(collection));
                let path = canonical(path);
                let id = files.get(&(path.clone(), *index)).copied();
                id.and_then(|id| {
                    let family = collection.family(id)?;
                    let colour = family
                        .fonts()
                        .iter()
                        .find(|font| {
                            font.index() == *index
                                && font_path(font).is_some_and(|file| canonical(file) == path)
                        })
                        .is_some_and(has_colour_tables);
                    Some((id, colour))
                })
            }
        };
        if found.is_none() {
            self.unresolved += 1;
        }
        found
    }
}

/// Every file-backed face of `collection`, by canonical file and index.
fn file_index(
    collection: &mut parley::fontique::Collection,
) -> std::collections::HashMap<(std::path::PathBuf, u32), parley::fontique::FamilyId> {
    let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    let mut files = std::collections::HashMap::new();
    for name in names {
        let Some(family) = collection.family_by_name(&name) else {
            continue;
        };
        for font in family.fonts() {
            if let Some(path) = font_path(font) {
                files
                    .entry((canonical(path), font.index()))
                    .or_insert(family.id());
            }
        }
    }
    files
}

/// The file `font` is read from, if it is file-backed.
fn font_path(font: &parley::fontique::FontInfo) -> Option<&std::path::Path> {
    match font.source().kind() {
        parley::fontique::SourceKind::Path(path) => Some(path),
        parley::fontique::SourceKind::Memory(_) => None,
    }
}

/// `path` with symlinks resolved, or as given where it cannot be.
fn canonical(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `font` has colour glyph tables. A face that cannot be read
/// answers no.
fn has_colour_tables(font: &parley::fontique::FontInfo) -> bool {
    use swash::tag_from_bytes;

    let Some(blob) = font.load(None) else {
        return false;
    };
    let Ok(index) = usize::try_from(font.index()) else {
        return false;
    };
    swash::FontRef::from_index(blob.as_ref(), index).is_some_and(|face| {
        [b"COLR", b"CBDT", b"sbix"]
            .into_iter()
            .any(|tag| face.table(tag_from_bytes(tag)).is_some())
    })
}

/// Writes `host` and `chain` into `collection`: for every script fontique
/// names, its fallback families are the host's list for that script, then
/// the chain's, then the host's common list, then the chain's, then the
/// family the collection's sans-serif generic names; the emoji generic is
/// the host's colour emoji faces, then the chain's, then the host's and the
/// chain's common lists (ADR-0271 §2).
///
/// Only families the collection holds are written, in that order, each
/// once. The trailing sans-serif family is fontique's last resort, so
/// a glyph no listed family has still reaches the face the collection's
/// sans-serif generic names rather than `.notdef` (a host whose common list
/// is empty, as on Android, falls back there alone). Only the default key
/// of each script is set (no locale): the Parley path shapes with no locale
/// today. A script with no held family at all gets no entry, so it keeps
/// whatever it had.
///
/// Returns whether any list it wrote differs from the one the collection
/// had; a list equal to the one already there is not written again.
pub(crate) fn install_into(
    chain: &FallbackChain,
    host: &HostOrder,
    collection: &mut parley::fontique::Collection,
) -> bool {
    use parley::fontique::{FallbackKey, GenericFamily};

    let mut changed = false;
    let common = held(collection, chain.common(), host.common.clone());
    let sans_serif: Vec<_> = collection
        .generic_families(GenericFamily::SansSerif)
        .collect();
    for script in fontique_scripts() {
        let mut families = held(
            collection,
            chain.script(script),
            host.script(script).to_vec(),
        );
        for id in common.iter().chain(&sans_serif) {
            if !families.contains(id) {
                families.push(*id);
            }
        }
        let key = FallbackKey::new(script, None);
        if !families.is_empty()
            && !collection
                .fallback_families(key)
                .eq(families.iter().copied())
        {
            collection.set_fallbacks(key, families.into_iter());
            changed = true;
        }
    }
    let mut emoji = held(collection, chain.emoji(), host.emoji.clone());
    for id in &common {
        if !emoji.contains(id) {
            emoji.push(*id);
        }
    }
    if !emoji.is_empty()
        && !collection
            .generic_families(GenericFamily::Emoji)
            .eq(emoji.iter().copied())
    {
        collection.set_generic_families(GenericFamily::Emoji, emoji.into_iter());
        changed = true;
    }
    changed
}

/// `into` extended with the ids of the `names` `collection` holds, in
/// order, skipping any already present.
fn held(
    collection: &mut parley::fontique::Collection,
    names: &[&str],
    mut into: Vec<parley::fontique::FamilyId>,
) -> Vec<parley::fontique::FamilyId> {
    for name in names {
        if let Some(id) = collection.family_id(name)
            && !into.contains(&id)
        {
            into.push(id);
        }
    }
    into
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parley::fontique::{
        Blob, Collection, CollectionOptions, FallbackKey, FamilyId, GenericFamily, Script,
    };

    use super::super::fallback_tables::Platform;
    use super::{FallbackChain, HostOrder, fontique_scripts, install_into};

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
    const MATERIAL_ICONS: &[u8] = include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf");
    const CUPERTINO_ICONS: &[u8] = include_bytes!("../../assets/fonts/CupertinoIcons.ttf");

    /// Han gets Material Icons, then Roboto (also in the common list);
    /// every other script gets nothing of its own; the common list starts
    /// with a family nothing carries.
    fn fixture() -> FallbackChain {
        FallbackChain::from_lists(
            &["Nothing Carries This", "Roboto", "CupertinoIcons"],
            &[(
                Script::from_str_unchecked("Hani"),
                &["Material Icons", "Roboto"],
            )],
        )
    }

    fn fixture_collection() -> Collection {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        for bytes in [ROBOTO, MATERIAL_ICONS, CUPERTINO_ICONS] {
            collection.register_fonts(Blob::new(Arc::new(bytes)), None);
        }
        collection
    }

    fn ids(collection: &mut Collection, names: &[&str]) -> Vec<FamilyId> {
        let ids: Vec<_> = names
            .iter()
            .filter_map(|name| collection.family_id(name))
            .collect();
        assert_eq!(
            ids.len(),
            names.len(),
            "the fixture registers every family it names"
        );
        ids
    }

    /// Han's fallback is the chain's Han list, then the common list, held
    /// families only, each once, in order; Latin (no list of its own) and
    /// Common are the common list alone.
    #[test]
    fn fontique_fallbacks_follow_the_chain_in_order() {
        let mut collection = fixture_collection();
        install_into(&fixture(), &HostOrder::default(), &mut collection);

        let han: Vec<_> = collection
            .fallback_families(FallbackKey::new(Script::from_str_unchecked("Hani"), None))
            .collect();
        assert_eq!(
            han,
            ids(
                &mut collection,
                &["Material Icons", "Roboto", "CupertinoIcons"]
            ),
            "Han: the script list, then the common list"
        );
        let common = ids(&mut collection, &["Roboto", "CupertinoIcons"]);
        for script in [Script::from_str_unchecked("Latn"), Script::COMMON] {
            let families: Vec<_> = collection
                .fallback_families(FallbackKey::new(script, None))
                .collect();
            assert_eq!(families, common, "{script:?}: the common list alone");
        }
    }

    /// Past the chain's lists, every script falls back to the family the
    /// collection's sans-serif generic names, once.
    #[test]
    fn the_sans_serif_family_ends_every_script_fallback() {
        let chain =
            FallbackChain::from_lists(&["Nothing Carries This", "Roboto", "CupertinoIcons"], &[]);
        let mut collection = fixture_collection();
        let icons = ids(&mut collection, &["Material Icons"]);
        collection.set_generic_families(GenericFamily::SansSerif, icons.into_iter());
        install_into(&chain, &HostOrder::default(), &mut collection);

        for tag in ["Latn", "Hani"] {
            let families: Vec<_> = collection
                .fallback_families(FallbackKey::new(Script::from_str_unchecked(tag), None))
                .collect();
            assert_eq!(
                families,
                ids(
                    &mut collection,
                    &["Roboto", "CupertinoIcons", "Material Icons"]
                ),
                "{tag}: the common list, then the sans-serif family once"
            );
        }
    }

    /// An emoji cluster walks the style's family, then the emoji generic:
    /// the chain's colour emoji faces the collection holds, then the common
    /// list, each once, in order. A chain with no emoji faces leaves the
    /// common list alone.
    #[test]
    fn the_emoji_generic_starts_with_the_colour_faces() {
        let cases: [(&str, &[&'static str], &[&str]); 3] = [
            ("no emoji faces", &[], &["Roboto", "CupertinoIcons"]),
            (
                "a held emoji face leads",
                &["Nothing Carries This", "CupertinoIcons"],
                &["CupertinoIcons", "Roboto"],
            ),
            (
                "no emoji face held",
                &["Nothing Carries This"],
                &["Roboto", "CupertinoIcons"],
            ),
        ];
        for (name, emoji_faces, expected) in cases {
            let mut collection = fixture_collection();
            install_into(
                &fixture().with_emoji(emoji_faces),
                &HostOrder::default(),
                &mut collection,
            );

            let emoji: Vec<_> = collection.generic_families(GenericFamily::Emoji).collect();
            assert_eq!(emoji, ids(&mut collection, expected), "{name}");
        }
    }

    /// Each platform's emoji generic starts with its own colour face: a
    /// common list that reaches a monochrome text face first (unix's DejaVu
    /// Sans) would otherwise set emoji in black and white.
    #[test]
    fn each_platform_names_its_colour_emoji_face_first() {
        for (platform, face) in [
            (Platform::Unix, Some("Noto Color Emoji")),
            (Platform::Windows, Some("Segoe UI Emoji")),
            (Platform::MacOs, Some("Apple Color Emoji")),
            (Platform::Other, None),
        ] {
            assert_eq!(
                FallbackChain::of(platform, "en-US")
                    .emoji()
                    .first()
                    .copied(),
                face,
                "{platform:?}"
            );
        }
    }

    /// Every platform's tables hold what cosmic-text 0.19's did
    /// (`fallback_recorded`, recorded from it before it left the workspace):
    /// the common list, and for every recorded locale every script's list,
    /// a script it names no list for included. Every table is checked on any
    /// host, so a transcription slip in a platform CI never runs fails here.
    ///
    /// One deliberate divergence: cosmic-text matched the locale whole, so
    /// its `ja-JP` lists were its Simplified Chinese ones. FLUI reads the
    /// language subtag, so `ja-JP` is checked against the recorded `ja` lists.
    #[test]
    fn platform_tables_match_the_recorded_lists() {
        use super::super::fallback_recorded::RECORDED;
        use super::super::fallback_tables::{common, script};

        let mut failures = Vec::new();
        for (name, recorded_common, locales) in RECORDED {
            let platform = match name {
                "windows" => Platform::Windows,
                "unix" => Platform::Unix,
                "macos" => Platform::MacOs,
                _ => Platform::Other,
            };
            if common(platform) != recorded_common {
                failures.push(format!("{name}: common list"));
            }
            for (locale, _) in locales {
                let reference = if *locale == "ja-JP" { "ja" } else { *locale };
                let (_, lists) = locales
                    .iter()
                    .find(|(held, _)| *held == reference)
                    .expect("BUG: the recording holds every bare locale it diverges to");
                let tags = lists
                    .iter()
                    .map(|(tag, _)| (*tag).to_owned())
                    .chain(fontique_scripts().map(|script| script.as_str().to_owned()));
                for tag in tags {
                    let recorded = lists
                        .iter()
                        .find(|(held, _)| *held == tag)
                        .map_or(&[][..], |(_, list)| *list);
                    if script(platform, &tag, locale) != recorded {
                        failures.push(format!("{name} {locale} {tag}"));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "differ from the recording: {failures:?}"
        );
    }

    /// A host chain keeps a list only for a script fontique names, and the
    /// locale picks the Han list by its language, script and region subtags:
    /// a host reports a full tag (`sys_locale` gives `ja-JP`, never `ja`), so
    /// a Japanese, Korean, Hong Kong or Taiwan host gets its own faces, and a
    /// locale naming none of them takes the Simplified Chinese ones.
    #[test]
    fn a_platform_chain_picks_han_by_locale() {
        let hani = Script::from_str_unchecked("Hani");
        let ja = FallbackChain::of(Platform::Unix, "ja");
        let us = FallbackChain::of(Platform::Unix, "en-US");
        assert_eq!(ja.script(hani), ["Noto Sans CJK JP"]);
        assert_eq!(us.script(hani), ["Noto Sans CJK SC"]);
        assert_eq!(
            ja.script(Script::from_str_unchecked("Kana")),
            ["Noto Sans CJK JP"]
        );
        assert_eq!(
            us.script(Script::from_str_unchecked("Latn")),
            [] as [&str; 0]
        );
        assert_eq!(us.common().first(), Some(&"Noto Sans"));
        for (platform, locale, expected) in [
            (Platform::Unix, "ja-JP", "Noto Sans CJK JP"),
            (Platform::Unix, "ja_JP.UTF-8", "Noto Sans CJK JP"),
            (Platform::Unix, "ko-KR", "Noto Sans CJK KR"),
            (Platform::Unix, "zh-TW", "Noto Sans CJK TC"),
            (Platform::Unix, "zh-Hant", "Noto Sans CJK TC"),
            (Platform::Unix, "zh-Hant-HK", "Noto Sans CJK HK"),
            (Platform::Unix, "zh-MO", "Noto Sans CJK HK"),
            (Platform::Unix, "zh-Hans-TW", "Noto Sans CJK SC"),
            (Platform::Unix, "zh-CN", "Noto Sans CJK SC"),
            (Platform::Unix, "JA", "Noto Sans CJK JP"),
            (Platform::Windows, "ja-JP", "Yu Gothic"),
            (Platform::Windows, "zh-HK", "MingLiU_HKSCS"),
            (Platform::MacOs, "ko-KR", "Apple SD Gothic Neo"),
            (Platform::MacOs, "zh-Hant-TW", "PingFang TC"),
        ] {
            assert_eq!(
                FallbackChain::of(platform, locale).script(hani),
                [expected],
                "{platform:?} {locale}"
            );
        }
    }

    /// `bytes` with its first table renamed `COLR`: a face that reads as
    /// one with colour glyph tables. The directory stays sorted only when
    /// that first table sorts after `COLR` (here `OS/2`).
    fn with_colour_table(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        assert_eq!(&out[12..16], b"OS/2", "the fixture's first table");
        out[12..16].copy_from_slice(b"COLR");
        out
    }

    /// The host's answer goes ahead of the chain (ADR-0271 §2): per script,
    /// the host's list, the chain's list, the host's common list, the
    /// chain's common list; the emoji generic starts with the host's emoji
    /// faces. A face named by a path through `..` resolves to the file the
    /// collection read; a face the collection lacks is left out; a colour
    /// face is kept out of every text list and stays in the emoji generic.
    /// Fails if the merge drops or reorders the host's answer, matches files
    /// by spelling, or lets a colour face into a script's list.
    #[test]
    fn the_host_answer_precedes_the_tables() {
        use std::path::Path;

        use super::super::system_fallback::{FaceRef, ScriptCode, SystemFallback};
        use super::resolve;

        const DECOY: &[u8] = include_bytes!("../../assets/fonts/decoy-wide-space.ttf");
        let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
        let mut collection = fixture_collection();
        collection.load_fonts_from_paths([fonts.join("probe-arabic-ligature.ttf")]);
        collection.register_fonts(Blob::new(Arc::new(with_colour_table(DECOY))), None);

        let family = |name: &str| FaceRef::Family(name.to_owned());
        let system = SystemFallback::none("en-US")
            .with_script(
                ScriptCode::from_bytes(*b"Arab"),
                vec![
                    FaceRef::File {
                        path: fonts.join("../fonts/probe-arabic-ligature.ttf"),
                        index: 0,
                    },
                    family("FLUI Decoy Emoji"),
                    family("Not Installed"),
                ],
            )
            .with_script(
                ScriptCode::from_bytes(*b"Hani"),
                vec![family("CupertinoIcons")],
            )
            .with_emoji(vec![family("FLUI Decoy Emoji")])
            .with_common(vec![family("FLUI Decoy Emoji"), family("Material Icons")]);
        let order = resolve(&system, &mut collection);
        install_into(&fixture(), &order, &mut collection);

        let mut fallbacks = |script: &str| -> Vec<FamilyId> {
            collection
                .fallback_families(FallbackKey::new(Script::from_str_unchecked(script), None))
                .collect()
        };
        let arab = fallbacks("Arab");
        let hani = fallbacks("Hani");
        let latn = fallbacks("Latn");
        let emoji: Vec<_> = collection.generic_families(GenericFamily::Emoji).collect();
        let common = ids(
            &mut collection,
            &["Material Icons", "Roboto", "CupertinoIcons"],
        );
        let arabic = ids(&mut collection, &["FLUI Probe Arabic"]);
        let decoy = ids(&mut collection, &["FLUI Decoy Emoji"]);

        assert_eq!(arab, [arabic, common.clone()].concat());
        assert_eq!(
            hani,
            ids(
                &mut collection,
                &["CupertinoIcons", "Material Icons", "Roboto"]
            )
        );
        assert_eq!(latn, common);
        assert_eq!(emoji, [decoy, common].concat());
    }
}
