//! The fallback order a host-fed collection walks past a style's family
//! (ADR-0092 §7).
//!
//! Parley tries the collection's fallback families for the item's script,
//! then the Han fallback (fontique's `Query::set_fallbacks`), and adds the
//! emoji generic after the style's families for an emoji cluster. A
//! [`FallbackChain`] holds FLUI's lists for the host (`fallback_tables`) and
//! [`install_into`] writes them into a fontique collection: per script, the
//! script's list, then the common list, then the family the collection's
//! sans-serif generic names. There is no walk over every other face, so a
//! character no listed family covers shapes as `.notdef` (flui-painting
//! `ARCHITECTURE.md`, mapping decision 17).

use parley::fontique::Script;

use super::fallback_tables::{self, Platform};

/// The fallback lists a host feed installs: a common list, and a list per
/// script, chosen for one locale.
#[derive(Clone, Debug)]
pub(crate) struct FallbackChain {
    common: Vec<&'static str>,
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
            scripts: fontique_scripts()
                .filter_map(|script| {
                    let list = fallback_tables::script(platform, script.as_str(), locale);
                    (!list.is_empty()).then_some((script, list))
                })
                .collect(),
        }
    }

    /// A chain over the given lists, for a test.
    #[cfg(test)]
    pub(crate) fn from_lists(
        common: &[&'static str],
        scripts: &[(Script, &'static [&'static str])],
    ) -> Self {
        Self {
            common: common.to_vec(),
            scripts: scripts.to_vec(),
        }
    }

    /// The families tried after a script's own list, in order.
    pub(crate) fn common(&self) -> &[&'static str] {
        &self.common
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

/// Writes `chain` into `collection`: for every script fontique names, its
/// fallback families are the chain's list for that script, then the common
/// list, then the family the collection's sans-serif generic names; the
/// emoji generic is the common list.
///
/// Only families the collection holds are written, in the chain's order,
/// each once. The trailing sans-serif family is fontique's last resort, so
/// a glyph no listed family has still reaches the face the collection's
/// sans-serif generic names rather than `.notdef` (a host whose common list
/// is empty, as on Android, falls back there alone). Only the default key
/// of each script is set (no locale): the Parley path shapes with no locale
/// today. A script with no held family at all gets no entry, so it keeps
/// whatever it had.
pub(crate) fn install_into(chain: &FallbackChain, collection: &mut parley::fontique::Collection) {
    use parley::fontique::{FallbackKey, GenericFamily};

    let common = held(collection, chain.common(), Vec::new());
    let sans_serif: Vec<_> = collection
        .generic_families(GenericFamily::SansSerif)
        .collect();
    for script in fontique_scripts() {
        let mut families = held(collection, chain.script(script), Vec::new());
        for id in common.iter().chain(&sans_serif) {
            if !families.contains(id) {
                families.push(*id);
            }
        }
        if !families.is_empty() {
            collection.set_fallbacks(FallbackKey::new(script, None), families.into_iter());
        }
    }
    if !common.is_empty() {
        collection.set_generic_families(GenericFamily::Emoji, common.into_iter());
    }
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
    use super::{FallbackChain, fontique_scripts, install_into};

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
        install_into(&fixture(), &mut collection);

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
        install_into(&chain, &mut collection);

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
    /// the whole common list, in order.
    #[test]
    fn the_emoji_generic_is_the_common_list() {
        let mut collection = fixture_collection();
        install_into(&fixture(), &mut collection);

        let emoji: Vec<_> = collection.generic_families(GenericFamily::Emoji).collect();
        assert_eq!(emoji, ids(&mut collection, &["Roboto", "CupertinoIcons"]));
    }

    /// Every platform's tables hold what cosmic-text 0.19's did
    /// (`fallback_recorded`, recorded from it before it left the workspace):
    /// the common list, and for every recorded locale every script's list,
    /// a script it names no list for included. Every table is checked on any
    /// host, so a transcription slip in a platform CI never runs fails here.
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
            for (locale, lists) in locales {
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

    /// A host chain keeps a list only for a script fontique names, the
    /// locale picks the Han list, and a locale no table names takes the
    /// Simplified Chinese one.
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
        assert!(us.script(Script::from_str_unchecked("Latn")).is_empty());
        assert_eq!(us.common().first(), Some(&"Noto Sans"));
    }
}
