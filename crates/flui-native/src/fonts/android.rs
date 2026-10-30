//! Android's answer below API 29 (ADR-0271 §3): the system's own fallback
//! order, read from `/system/etc/fonts.xml`.
//!
//! The file lists the fallback families in the order Android itself tries
//! them, each tagged with the languages it serves. That format is frozen for
//! API 23 to 28; from API 29 the platform warns against parsing it and
//! `AFontMatcher` answers instead. The parse is pure, so its tests run on
//! the Linux CI host over files recorded from AOSP.

use std::path::Path;

use flui_painting::{FaceRef, ScriptCode, SystemFallback};

/// Where the system's font configuration and its fonts live.
#[cfg(target_os = "android")]
const FONTS_XML: &str = "/system/etc/fonts.xml";
#[cfg(target_os = "android")]
const FONTS_DIR: &str = "/system/fonts";

/// The host's answer for `locale`: below API 29 the system's fallback
/// order, or none where the file cannot be read or parsed (logged at
/// debug). From API 29 none until `AFontMatcher` answers (`docs/gaps.md`,
/// entry 35), and none where the level cannot be read.
#[cfg(target_os = "android")]
pub(crate) fn system_fallback(locale: &str) -> SystemFallback {
    use crate::owner::android::ndk::LoadedLibraries;
    use crate::version::gate::android::{AndroidApi, FontMatcher};

    let api = AndroidApi::detect(&LoadedLibraries::open());
    if api.is_none_or(|api| api.since::<FontMatcher>().is_some()) {
        tracing::debug!(
            ?api,
            "no fonts.xml from API 29; the fallback order is FLUI's tables alone"
        );
        return SystemFallback::none(locale);
    }
    match std::fs::read_to_string(FONTS_XML) {
        Ok(xml) => parse(&xml, Path::new(FONTS_DIR), locale),
        Err(error) => {
            tracing::debug!(%error, "no {FONTS_XML}; the fallback order is FLUI's tables alone");
            SystemFallback::none(locale)
        }
    }
}

/// One fallback family of the file: the face it draws with and the
/// languages it is tagged with.
struct Fallback<'a> {
    face: FaceRef,
    langs: Vec<&'a str>,
}

/// The fallback order `xml` (a `fonts.xml`) states, with its font files in
/// `fonts_dir`, for `locale`.
///
/// Each unnamed, not ignored family is one fallback, drawn with its first
/// regular sans face (not one marked `fallbackFor="serif"`). A family tagged
/// with a script (`und-Arab`, `zh-Hans`) or a language that implies one
/// (`ja`, `ko`) joins that script's list; `und-Zsye` is the emoji list; an
/// untagged family, or one tagged only with symbols (`und-Zsym`), joins the
/// common list. Lists keep the file's order, except that among the Han
/// families those tagged with the locale's language come first. A file that
/// does not parse gives no answer.
pub(crate) fn parse(xml: &str, fonts_dir: &Path, locale: &str) -> SystemFallback {
    let document = match roxmltree::Document::parse(xml) {
        Ok(document) => document,
        Err(error) => {
            tracing::debug!(%error, "fonts.xml does not parse; no fallback answer");
            return SystemFallback::none(locale);
        }
    };
    let fallbacks: Vec<Fallback<'_>> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("family"))
        .filter(|family| family.attribute("name").is_none())
        .filter(|family| family.attribute("ignore") != Some("true"))
        .filter_map(|family| {
            let face = family
                .children()
                .filter(|font| font.has_tag_name("font"))
                .filter(|font| font.attribute("fallbackFor").is_none())
                .min_by_key(|font| {
                    (
                        font.attribute("style") != Some("normal"),
                        font.attribute("weight") != Some("400"),
                    )
                })?;
            let file = face
                .children()
                .filter_map(|child| child.text())
                .collect::<String>();
            let file = file.trim();
            if file.is_empty() {
                return None;
            }
            let index = face
                .attribute("index")
                .and_then(|index| index.parse::<u32>().ok())
                .unwrap_or(0);
            Some(Fallback {
                face: FaceRef::File {
                    path: fonts_dir.join(file),
                    index,
                },
                langs: family
                    .attribute("lang")
                    .map(|langs| {
                        langs
                            .split([' ', ','])
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<&str>>()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();

    let mut answer = SystemFallback::none(locale);
    let mut scripts: Vec<(ScriptCode, Vec<(bool, FaceRef)>)> = Vec::new();
    let preferred = han_preference(locale);
    for fallback in &fallbacks {
        let mut tagged = false;
        for lang in &fallback.langs {
            for script in scripts_of(lang) {
                tagged = true;
                if script == EMOJI {
                    if !answer.emoji.contains(&fallback.face) {
                        answer.emoji.push(fallback.face.clone());
                    }
                    continue;
                }
                let first =
                    script == HAN && preferred.is_some_and(|tag| lang.eq_ignore_ascii_case(tag));
                let at = scripts
                    .iter()
                    .position(|(held, _)| *held == script)
                    .unwrap_or_else(|| {
                        scripts.push((script, Vec::new()));
                        scripts.len() - 1
                    });
                let list = &mut scripts[at].1;
                if !list.iter().any(|(_, face)| *face == fallback.face) {
                    list.push((first, fallback.face.clone()));
                }
            }
        }
        if !tagged && !answer.common.contains(&fallback.face) {
            answer.common.push(fallback.face.clone());
        }
    }
    for (script, mut list) in scripts {
        // Stable: the locale's own Han family first, the file's order kept
        // otherwise.
        list.sort_by_key(|(first, _)| !first);
        answer = answer.with_script(
            script,
            list.into_iter()
                .map(|(_, face)| face)
                .collect::<Vec<FaceRef>>(),
        );
    }
    answer
}

const HAN: ScriptCode = ScriptCode::from_bytes(*b"Hani");
const EMOJI: ScriptCode = ScriptCode::from_bytes(*b"Zsye");

/// The scripts a `fonts.xml` language tag serves: its script subtag (Han
/// for `Hans` and `Hant`, with kana for `Jpan` and Hangul for `Kore`), else
/// the script its language implies, else none. `Zsye` marks emoji; `Zsym`
/// (symbols) and a bare `und` serve no one script.
fn scripts_of(lang: &str) -> Vec<ScriptCode> {
    let mut subtags = lang.split(['-', '_']);
    let language = subtags.next().unwrap_or("").to_ascii_lowercase();
    let script =
        subtags.find(|subtag| subtag.len() == 4 && subtag.chars().all(char::is_alphabetic));
    let code = |tag: &[u8; 4]| ScriptCode::from_bytes(*tag);
    match script.map(title_case) {
        Some(script) => match &script {
            b"Hans" | b"Hant" => vec![HAN],
            b"Jpan" => vec![HAN, code(b"Hira"), code(b"Kana")],
            b"Kore" => vec![code(b"Hang"), HAN],
            b"Zsym" => Vec::new(),
            // Estrangela, Eastern and Western Syriac are styles of the one
            // script text is shaped as.
            b"Syre" | b"Syrn" | b"Syrj" => vec![code(b"Syrc")],
            other => vec![code(other)],
        },
        None => match language.as_str() {
            "ja" => vec![HAN, code(b"Hira"), code(b"Kana")],
            "ko" => vec![code(b"Hang"), HAN],
            "zh" => vec![HAN],
            _ => Vec::new(),
        },
    }
}

/// `subtag` as a four-letter script code, `Arab` for `arab`.
fn title_case(subtag: &str) -> [u8; 4] {
    let mut code = [b'Z'; 4];
    for (slot, byte) in code.iter_mut().zip(subtag.bytes()) {
        *slot = byte.to_ascii_lowercase();
    }
    code[0] = code[0].to_ascii_uppercase();
    code
}

/// The `fonts.xml` language tag whose Han family `locale` prefers: Japanese,
/// Korean, Traditional or Simplified Chinese; none for any other language.
fn han_preference(locale: &str) -> Option<&'static str> {
    let tag = locale.replace('_', "-").to_ascii_lowercase();
    let mut subtags = tag.split('-');
    match subtags.next()? {
        "ja" => Some("ja"),
        "ko" => Some("ko"),
        "zh" => {
            // The script subtag wins over the region: `zh-Hans-TW` is
            // Simplified (as `fallback_tables::Han::of` reads it).
            let rest: Vec<&str> = subtags.collect();
            let traditional = if rest.contains(&"hant") {
                true
            } else if rest.contains(&"hans") {
                false
            } else {
                rest.iter()
                    .any(|subtag| matches!(*subtag, "tw" | "hk" | "mo"))
            };
            Some(if traditional { "zh-Hant" } else { "zh-Hans" })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use flui_painting::{FaceRef, ScriptCode, SystemFallback};

    use super::parse;

    const API_23: &str = include_str!("testdata/android-6.0.1_r81-fonts.xml");
    const API_28: &str = include_str!("testdata/android-9.0.0_r61-fonts.xml");

    fn file(name: &str, index: u32) -> FaceRef {
        FaceRef::File {
            path: PathBuf::from("/system/fonts").join(name),
            index,
        }
    }

    fn first(answer: &SystemFallback, script: &[u8; 4]) -> Option<FaceRef> {
        answer
            .script(ScriptCode::from_bytes(*script))
            .first()
            .cloned()
    }

    /// The recorded files of API 23 and API 28 read as Android orders them:
    /// a script's tagged family first, Han by the locale's language, emoji
    /// from `und-Zsye`, untagged families in the common list in file order,
    /// serif faces and named families never. Fails if a family's language
    /// tag is misread or the file's order is lost.
    #[test]
    fn fonts_xml_orders_fallback_by_language() {
        let dir = Path::new("/system/fonts");
        let mut failures = Vec::new();
        let mut check = |row: &str, found: Option<FaceRef>, expected: Option<FaceRef>| {
            if found != expected {
                failures.push(format!("{row}: {found:?}, expected {expected:?}"));
            }
        };

        let api28 = parse(API_28, dir, "en-US");
        check(
            "api28 Arab",
            first(&api28, b"Arab"),
            Some(file("NotoNaskhArabic-Regular.ttf", 0)),
        );
        check(
            "api28 Hebr",
            first(&api28, b"Hebr"),
            Some(file("NotoSansHebrew-Regular.ttf", 0)),
        );
        check(
            "api28 emoji",
            api28.emoji.first().cloned(),
            Some(file("NotoColorEmoji.ttf", 0)),
        );
        check(
            "api28 Hani en",
            first(&api28, b"Hani"),
            Some(file("NotoSansCJK-Regular.ttc", 2)),
        );
        check(
            "api28 Hani ja",
            first(&parse(API_28, dir, "ja-JP"), b"Hani"),
            Some(file("NotoSansCJK-Regular.ttc", 0)),
        );
        check(
            "api28 Hani zh-TW",
            first(&parse(API_28, dir, "zh-TW"), b"Hani"),
            Some(file("NotoSansCJK-Regular.ttc", 3)),
        );
        check(
            "api28 Hani zh-Hans-TW (script over region)",
            first(&parse(API_28, dir, "zh-Hans-TW"), b"Hani"),
            Some(file("NotoSansCJK-Regular.ttc", 2)),
        );
        check(
            "api28 Hani zh-Hant-CN",
            first(&parse(API_28, dir, "zh-Hant-CN"), b"Hani"),
            Some(file("NotoSansCJK-Regular.ttc", 3)),
        );
        check(
            "api28 Syrc keeps Estrangela first",
            first(&api28, b"Syrc"),
            Some(file("NotoSansSyriacEstrangela-Regular.ttf", 0)),
        );
        check(
            "api28 Hang ko",
            first(&parse(API_28, dir, "ko-KR"), b"Hang"),
            Some(file("NotoSansCJK-Regular.ttc", 1)),
        );
        check(
            "api28 named families stay out",
            api28
                .common
                .iter()
                .find(|face| matches!(face, FaceRef::File { path, .. } if path.ends_with("Roboto-Regular.ttf")))
                .cloned(),
            None,
        );

        let api23 = parse(API_23, dir, "en-US");
        check(
            "api23 untagged Arabic is common, first",
            api23.common.first().cloned(),
            Some(file("NotoNaskhArabic-Regular.ttf", 0)),
        );
        check(
            "api23 Hani en",
            first(&api23, b"Hani"),
            Some(file("NotoSansSC-Regular.otf", 0)),
        );
        check(
            "api23 Hani ja",
            first(&parse(API_23, dir, "ja-JP"), b"Hani"),
            Some(file("NotoSansJP-Regular.otf", 0)),
        );
        check(
            "api23 no serif face",
            api23
                .common
                .iter()
                .find(|face| matches!(face, FaceRef::File { path, .. } if path.to_string_lossy().contains("Serif")))
                .cloned(),
            None,
        );
        check(
            "unparsable file",
            parse("<familyset", dir, "en-US").common.first().cloned(),
            None,
        );
        assert!(failures.is_empty(), "{failures:#?}");
        assert_eq!(
            api28.script(ScriptCode::from_bytes(*b"Syrc")).len(),
            3,
            "the Estrangela, Eastern and Western faces share Syriac's list"
        );
    }
}
