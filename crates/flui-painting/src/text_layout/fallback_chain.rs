//! The one fallback order both shapers walk (ADR-0092 §7).
//!
//! Past a style's resolved family, cosmic-text tries the script's platform
//! list, then the platform's common list, then any face not forbidden
//! (`font/fallback/mod.rs`, `FontFallbackIter::next_item`). Parley tries the
//! collection's fallback families for the item's script, then the Han
//! fallback (fontique's `Query::set_fallbacks`), and adds the emoji generic
//! after the style's families for an emoji cluster. A [`FallbackChain`] is
//! built once, beside the process font system, and is the source of both:
//! the font system is constructed over it ([`ChainFallback`]) and
//! `install_into` writes the same lists into a fontique collection, so a
//! character that falls back lands on the same family when it is measured
//! and when it is painted.
//!
//! What has no Parley counterpart is cosmic-text's last resort, the walk
//! over every face not forbidden (flui-painting `ARCHITECTURE.md`, mapping
//! decision 17).

use std::fmt;
use std::sync::Arc;

use cosmic_text::Fallback;

/// The fallback lists the process font system was built with, and the
/// locale it picks script lists for.
pub(crate) struct FallbackChain {
    locale: String,
    fallback: Box<dyn Fallback>,
}

impl FallbackChain {
    /// A chain over `fallback`'s lists, picking script lists for `locale`:
    /// the locale the font system is constructed with, so Han unification
    /// agrees between the two shapers.
    pub(crate) fn new(locale: String, fallback: impl Fallback + 'static) -> Self {
        Self {
            locale,
            fallback: Box::new(fallback),
        }
    }

    /// The families tried after a script's own list, in order.
    pub(crate) fn common(&self) -> &[&'static str] {
        self.fallback.common_fallback()
    }

    /// The families tried first for `script`, in order.
    pub(crate) fn script(&self, script: unicode_script::Script) -> &[&'static str] {
        self.fallback.script_fallback(script, &self.locale)
    }
}

impl fmt::Debug for FallbackChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FallbackChain")
            .field("locale", &self.locale)
            .field("common", &self.fallback.common_fallback())
            .field("forbidden", &self.fallback.forbidden_fallback())
            .finish_non_exhaustive()
    }
}

/// A [`FallbackChain`] as cosmic-text's `Fallback`: the font system is built
/// over this, so it reads the very lists the collection is given.
pub(crate) struct ChainFallback(pub(crate) Arc<FallbackChain>);

impl Fallback for ChainFallback {
    fn common_fallback(&self) -> &[&'static str] {
        self.0.fallback.common_fallback()
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        self.0.fallback.forbidden_fallback()
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        self.0.fallback.script_fallback(script, locale)
    }
}

/// Every script fontique names, and the Common, Inherited and Unknown
/// scripts: the keys a collection's fallbacks are set under.
pub(crate) fn fontique_scripts() -> impl Iterator<Item = parley::fontique::Script> {
    use parley::fontique::{Script, ScriptExt as _};

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
/// each once. The trailing sans-serif family stands in for cosmic-text's
/// last resort, which fontique lacks, so a glyph no listed family has still
/// reaches the face the process font system's sans-serif generic names
/// rather than `.notdef`
/// (a host whose common list is empty, as on Android, falls back there
/// alone). Only the default key of each script is set (no locale): the
/// Parley path shapes with no locale today. A script with no held family at
/// all gets no entry, so it keeps whatever it had.
pub(crate) fn install_into(chain: &FallbackChain, collection: &mut parley::fontique::Collection) {
    use parley::fontique::{FallbackKey, GenericFamily};

    let common = held(collection, chain.common(), Vec::new());
    let sans_serif: Vec<_> = collection
        .generic_families(GenericFamily::SansSerif)
        .collect();
    for script in fontique_scripts() {
        let own = unicode_script::Script::from_short_name(script.as_str())
            .map_or(&[][..], |script| chain.script(script));
        let mut families = held(collection, own, Vec::new());
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

    use super::{ChainFallback, FallbackChain, install_into};

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
    const MATERIAL_ICONS: &[u8] = include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf");
    const CUPERTINO_ICONS: &[u8] = include_bytes!("../../assets/fonts/CupertinoIcons.ttf");

    /// Names fixture families: Han gets an absent family, then Material
    /// Icons, then Roboto (also in the common list); every other script gets
    /// nothing of its own.
    struct Fixture;

    impl cosmic_text::Fallback for Fixture {
        fn common_fallback(&self) -> &[&'static str] {
            &["Nothing Carries This", "Roboto", "CupertinoIcons"]
        }

        fn forbidden_fallback(&self) -> &[&'static str] {
            &[]
        }

        fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
            match (script, locale) {
                (unicode_script::Script::Han, "ja-JP") => &["Material Icons", "Roboto"],
                (unicode_script::Script::Han, _) => &["Roboto"],
                _ => &[],
            }
        }
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

    /// Han's fallback is the chain's Han list for the chain's locale, then
    /// the common list, held families only, each once, in order; Latin (no
    /// list of its own) and Common are the common list alone. The process
    /// font system is built over the same lists (`ChainFallback`).
    #[test]
    fn fontique_fallbacks_follow_the_paint_chain_in_order() {
        let chain = FallbackChain::new("ja-JP".to_owned(), Fixture);
        let mut collection = fixture_collection();
        install_into(&chain, &mut collection);

        let han: Vec<_> = collection
            .fallback_families(FallbackKey::new(Script::from_str_unchecked("Hani"), None))
            .collect();
        assert_eq!(
            han,
            ids(
                &mut collection,
                &["Material Icons", "Roboto", "CupertinoIcons"]
            ),
            "Han: the script list for the chain's locale, then the common list"
        );
        let common = ids(&mut collection, &["Roboto", "CupertinoIcons"]);
        for script in [Script::from_str_unchecked("Latn"), Script::COMMON] {
            let families: Vec<_> = collection
                .fallback_families(FallbackKey::new(script, None))
                .collect();
            assert_eq!(families, common, "{script:?}: the common list alone");
        }

        let paint = ChainFallback(Arc::new(chain));
        assert_eq!(
            cosmic_text::Fallback::script_fallback(&paint, unicode_script::Script::Han, "ja-JP"),
            ["Material Icons", "Roboto"],
            "the process font system reads the same Han list"
        );
    }

    /// Past the chain's lists, every script falls back to the family the
    /// collection's sans-serif generic names, once, standing in for
    /// cosmic-text's last resort.
    #[test]
    fn the_sans_serif_family_ends_every_script_fallback() {
        let chain = FallbackChain::new("en-US".to_owned(), Fixture);
        let mut collection = fixture_collection();
        let icons = ids(&mut collection, &["Material Icons"]);
        collection.set_generic_families(GenericFamily::SansSerif, icons.into_iter());
        install_into(&chain, &mut collection);

        let latin: Vec<_> = collection
            .fallback_families(FallbackKey::new(Script::from_str_unchecked("Latn"), None))
            .collect();
        assert_eq!(
            latin,
            ids(
                &mut collection,
                &["Roboto", "CupertinoIcons", "Material Icons"]
            )
        );
        let han: Vec<_> = collection
            .fallback_families(FallbackKey::new(Script::from_str_unchecked("Hani"), None))
            .collect();
        assert_eq!(
            han,
            ids(
                &mut collection,
                &["Roboto", "CupertinoIcons", "Material Icons"]
            ),
            "Han for en-US: Roboto, the common list, then the sans-serif family once"
        );
    }

    /// An emoji cluster walks the style's family, then the emoji generic:
    /// the whole common list, in order, as cosmic-text walks it.
    #[test]
    fn the_emoji_generic_is_the_common_list() {
        let chain = FallbackChain::new("en-US".to_owned(), Fixture);
        let mut collection = fixture_collection();
        install_into(&chain, &mut collection);

        let emoji: Vec<_> = collection.generic_families(GenericFamily::Emoji).collect();
        assert_eq!(emoji, ids(&mut collection, &["Roboto", "CupertinoIcons"]));
    }
}
