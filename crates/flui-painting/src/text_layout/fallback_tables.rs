// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The lists are ported from cosmic-text 0.19 (`src/font/fallback/{windows,
// unix,macos,other}.rs`, Copyright System76), which is MIT OR Apache-2.0.

//! Each platform's font fallback lists: the families tried after a style's
//! own, per script, then a common list (ADR-0092 §7).
//!
//! Plain data, keyed by ISO 15924 script code. Every platform's table is
//! compiled on every target, so a test checks all four on any host
//! (`fallback_chain`'s `platform_tables_match_the_recorded_lists`); only
//! [`HOST`] picks the one a host scan uses.

/// Whose font lists a table holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    /// Windows.
    Windows,
    /// A unix other than Android and macOS: Linux and the BSDs, and iOS,
    /// which the table serves as cosmic-text's did.
    Unix,
    /// macOS.
    MacOs,
    /// Android and wasm: no list, so the fallback is the collection's
    /// sans-serif family alone.
    Other,
}

/// The platform this build runs on.
pub(crate) const HOST: Platform = if cfg!(target_os = "windows") {
    Platform::Windows
} else if cfg!(target_os = "macos") {
    Platform::MacOs
} else if cfg!(all(unix, not(target_os = "android"))) {
    Platform::Unix
} else {
    Platform::Other
};

/// The families tried after a script's own list, in order.
pub(crate) const fn common(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Windows => &[
            "Segoe UI",
            "Segoe UI Emoji",
            "Segoe UI Symbol",
            "Segoe UI Historic",
        ],
        Platform::Unix => &[
            "Noto Sans",
            "DejaVu Sans",
            "FreeSans",
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "FreeMono",
            "Noto Sans Symbols",
            "Noto Sans Symbols2",
            "Noto Color Emoji",
        ],
        Platform::MacOs => &[
            ".SF NS",
            "Menlo",
            "Apple Color Emoji",
            "Geneva",
            "Arial Unicode MS",
        ],
        Platform::Other => &[],
    }
}

/// The families tried first for the script whose ISO 15924 code is
/// `script`, for `locale`. `locale` picks the Han list (Japanese, Korean,
/// Hong Kong and Taiwan faces; Simplified Chinese otherwise) by its
/// subtags ([`Han::of`]).
pub(crate) fn script(platform: Platform, script: &str, locale: &str) -> &'static [&'static str] {
    let han = Han::of(locale);
    match platform {
        Platform::Windows => windows(script, han),
        Platform::Unix => unix(script, han),
        Platform::MacOs => macos(script, han),
        Platform::Other => &[],
    }
}

/// Which Han faces a locale reads: the glyph forms differ between
/// Japanese, Korean, Hong Kong, Taiwan and Simplified Chinese text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Han {
    /// Japanese.
    Jp,
    /// Korean.
    Kr,
    /// Traditional Chinese as written in Hong Kong and Macau.
    Hk,
    /// Traditional Chinese as written in Taiwan.
    Tw,
    /// Simplified Chinese, and every locale that names no other.
    Sc,
}

impl Han {
    /// The Han faces for a BCP-47 tag or a POSIX locale (`ja-JP`,
    /// `ja_JP.UTF-8`, `zh-Hant-HK`), read case-insensitively from its
    /// language, script and region subtags. A Chinese tag's script subtag
    /// wins over its region: `zh-Hans-TW` is Simplified, `zh-Hant` with no
    /// region is Taiwan's.
    fn of(locale: &str) -> Self {
        let tag = locale.split(['.', '@']).next().unwrap_or_default();
        let mut subtags = tag.split(['-', '_']);
        let language = subtags.next().unwrap_or_default();
        if language.eq_ignore_ascii_case("ja") {
            return Self::Jp;
        }
        if language.eq_ignore_ascii_case("ko") {
            return Self::Kr;
        }
        if !language.eq_ignore_ascii_case("zh") {
            return Self::Sc;
        }
        let mut script = None;
        let mut hong_kong = false;
        let mut taiwan = false;
        for subtag in subtags {
            if subtag.eq_ignore_ascii_case("hant") || subtag.eq_ignore_ascii_case("hans") {
                script.get_or_insert(subtag.eq_ignore_ascii_case("hant"));
            } else if subtag.eq_ignore_ascii_case("hk") || subtag.eq_ignore_ascii_case("mo") {
                hong_kong = true;
            } else if subtag.eq_ignore_ascii_case("tw") {
                taiwan = true;
            }
        }
        match (script, hong_kong, taiwan) {
            (Some(false), ..) => Self::Sc,
            (_, true, _) => Self::Hk,
            (Some(true), ..) | (None, _, true) => Self::Tw,
            (None, false, false) => Self::Sc,
        }
    }
}

fn windows_han(han: Han) -> &'static [&'static str] {
    match han {
        Han::Jp => &["Yu Gothic"],
        Han::Kr => &["Malgun Gothic"],
        Han::Hk => &["MingLiU_HKSCS"],
        Han::Tw => &["Microsoft JhengHei UI"],
        Han::Sc => &["Microsoft YaHei UI"],
    }
}

fn windows(script: &str, han: Han) -> &'static [&'static str] {
    match script {
        "Adlm" | "Ethi" | "Tfng" | "Vaii" => &["Ebrima"],
        "Beng" | "Cakm" | "Deva" | "Gujr" | "Guru" | "Knda" | "Mlym" | "Orya" | "Sinh" | "Taml"
        | "Telu" => &["Nirmala UI"],
        "Cans" | "Cher" => &["Gadugi"],
        "Hani" => windows_han(han),
        "Hang" => windows_han(Han::Kr),
        "Hira" | "Kana" => windows_han(Han::Jp),
        "Java" => &["Javanese Text"],
        "Khmr" | "Laoo" | "Thai" => &["Leelawadee UI"],
        "Mong" => &["Mongolian Baiti"],
        "Mymr" => &["Myanmar Text"],
        "Thaa" => &["MV Boli"],
        "Tibt" => &["Microsoft Himalaya"],
        "Yiii" => &["Microsoft Yi Baiti"],
        _ => &[],
    }
}

fn unix_han(han: Han) -> &'static [&'static str] {
    match han {
        Han::Jp => &["Noto Sans CJK JP"],
        Han::Kr => &["Noto Sans CJK KR"],
        Han::Hk => &["Noto Sans CJK HK"],
        Han::Tw => &["Noto Sans CJK TC"],
        Han::Sc => &["Noto Sans CJK SC"],
    }
}

fn unix(script: &str, han: Han) -> &'static [&'static str] {
    match script {
        "Adlm" => &["Noto Sans Adlam", "Noto Sans Adlam Unjoined"],
        "Arab" => &["Noto Sans Arabic"],
        "Armn" => &["Noto Sans Armenian"],
        "Beng" => &["Noto Sans Bengali"],
        "Bopo" | "Hani" => unix_han(han),
        // FreeMono before DejaVu Sans keeps braille aligned beside
        // monospaced text.
        "Brai" => &["FreeMono"],
        "Buhd" => &["Noto Sans Buhid"],
        "Cakm" => &["Noto Sans Chakma"],
        "Cher" => &["Noto Sans Cherokee"],
        "Dsrt" => &["Noto Sans Deseret"],
        "Deva" => &["Noto Sans Devanagari"],
        "Ethi" => &["Noto Sans Ethiopic"],
        "Geor" => &["Noto Sans Georgian"],
        "Goth" => &["Noto Sans Gothic"],
        "Gran" => &["Noto Sans Grantha"],
        "Gujr" => &["Noto Sans Gujarati"],
        "Guru" => &["Noto Sans Gurmukhi"],
        "Hang" => unix_han(Han::Kr),
        "Hano" => &["Noto Sans Hanunoo"],
        "Hebr" => &["Noto Sans Hebrew"],
        "Hira" | "Kana" => unix_han(Han::Jp),
        "Java" => &["Noto Sans Javanese"],
        "Knda" => &["Noto Sans Kannada"],
        "Khmr" => &["Noto Sans Khmer"],
        "Laoo" => &["Noto Sans Lao"],
        "Mlym" => &["Noto Sans Malayalam"],
        "Mong" => &["Noto Sans Mongolian"],
        "Mymr" => &["Noto Sans Myanmar"],
        "Orya" => &["Noto Sans Oriya"],
        "Runr" => &["Noto Sans Runic"],
        "Sinh" => &["Noto Sans Sinhala"],
        "Syrc" => &["Noto Sans Syriac"],
        "Tglg" => &["Noto Sans Tagalog"],
        "Tagb" => &["Noto Sans Tagbanwa"],
        "Tale" => &["Noto Sans Tai Le"],
        "Lana" => &["Noto Sans Tai Tham"],
        "Tavt" => &["Noto Sans Tai Viet"],
        "Taml" => &["Noto Sans Tamil"],
        "Telu" => &["Noto Sans Telugu"],
        "Thaa" => &["Noto Sans Thaana"],
        "Thai" => &["Noto Sans Thai"],
        "Tibt" => &["Noto Serif Tibetan"],
        "Tfng" => &["Noto Sans Tifinagh"],
        "Vaii" => &["Noto Sans Vai"],
        "Yiii" => &["Noto Sans Yi", "Noto Sans CJK SC"],
        _ => &[],
    }
}

fn macos_han(han: Han) -> &'static [&'static str] {
    match han {
        Han::Jp => &["Hiragino Sans"],
        Han::Kr => &["Apple SD Gothic Neo"],
        Han::Hk => &["PingFang HK"],
        Han::Tw => &["PingFang TC"],
        Han::Sc => &["PingFang SC"],
    }
}

fn macos(script: &str, han: Han) -> &'static [&'static str] {
    match script {
        "Adlm" => &["Noto Sans Adlam"],
        "Arab" => &["Geeza Pro"],
        "Armn" => &["Noto Sans Armenian"],
        "Beng" => &["Bangla Sangam MN"],
        "Buhd" => &["Noto Sans Buhid"],
        "Cans" => &["Euphemia UCAS"],
        "Cakm" => &["Noto Sans Chakma"],
        "Deva" => &["Devanagari Sangam MN"],
        "Ethi" => &["Kefa"],
        "Goth" => &["Noto Sans Gothic"],
        "Gran" => &["Grantha Sangam MN"],
        "Gujr" => &["Gujarati Sangam MN"],
        "Guru" => &["Gurmukhi Sangam MN"],
        "Hani" => macos_han(han),
        "Hang" => macos_han(Han::Kr),
        "Hano" => &["Noto Sans Hanunoo"],
        "Hebr" => &["Arial"],
        "Hira" | "Kana" => macos_han(Han::Jp),
        "Java" => &["Noto Sans Javanese"],
        "Knda" => &["Noto Sans Kannada"],
        "Khmr" => &["Khmer Sangam MN"],
        "Laoo" => &["Lao Sangam MN"],
        "Mlym" => &["Malayalam Sangam MN"],
        "Mong" => &["Noto Sans Mongolian"],
        "Mymr" => &["Noto Sans Myanmar"],
        "Orya" => &["Noto Sans Oriya"],
        "Sinh" => &["Sinhala Sangam MN"],
        "Syrc" => &["Noto Sans Syriac"],
        "Tglg" => &["Noto Sans Tagalog"],
        "Tagb" => &["Noto Sans Tagbanwa"],
        "Tale" => &["Noto Sans Tai Le"],
        "Lana" => &["Noto Sans Tai Tham"],
        "Tavt" => &["Noto Sans Tai Viet"],
        "Taml" => &["InaiMathi"],
        "Telu" => &["Telugu Sangam MN"],
        "Thaa" => &["Noto Sans Thaana"],
        "Thai" => &["Ayuthaya"],
        "Tibt" => &["Kailasa"],
        "Tfng" => &["Noto Sans Tifinagh"],
        "Vaii" => &["Noto Sans Vai"],
        "Yiii" => &["Noto Sans Yi", "PingFang SC"],
        _ => &[],
    }
}
