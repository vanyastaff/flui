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
/// Hong Kong and Taiwan faces; Simplified Chinese otherwise) and is matched
/// whole, as cosmic-text matched it.
pub(crate) fn script(platform: Platform, script: &str, locale: &str) -> &'static [&'static str] {
    match platform {
        Platform::Windows => windows(script, locale),
        Platform::Unix => unix(script, locale),
        Platform::MacOs => macos(script, locale),
        Platform::Other => &[],
    }
}

fn windows_han(locale: &str) -> &'static [&'static str] {
    match locale {
        "ja" => &["Yu Gothic"],
        "ko" => &["Malgun Gothic"],
        "zh-HK" => &["MingLiU_HKSCS"],
        "zh-TW" => &["Microsoft JhengHei UI"],
        _ => &["Microsoft YaHei UI"],
    }
}

fn windows(script: &str, locale: &str) -> &'static [&'static str] {
    match script {
        "Adlm" | "Ethi" | "Tfng" | "Vaii" => &["Ebrima"],
        "Beng" | "Cakm" | "Deva" | "Gujr" | "Guru" | "Knda" | "Mlym" | "Orya" | "Sinh" | "Taml"
        | "Telu" => &["Nirmala UI"],
        "Cans" | "Cher" => &["Gadugi"],
        "Hani" => windows_han(locale),
        "Hang" => windows_han("ko"),
        "Hira" | "Kana" => windows_han("ja"),
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

fn unix_han(locale: &str) -> &'static [&'static str] {
    match locale {
        "ja" => &["Noto Sans CJK JP"],
        "ko" => &["Noto Sans CJK KR"],
        "zh-HK" => &["Noto Sans CJK HK"],
        "zh-TW" => &["Noto Sans CJK TC"],
        _ => &["Noto Sans CJK SC"],
    }
}

fn unix(script: &str, locale: &str) -> &'static [&'static str] {
    match script {
        "Adlm" => &["Noto Sans Adlam", "Noto Sans Adlam Unjoined"],
        "Arab" => &["Noto Sans Arabic"],
        "Armn" => &["Noto Sans Armenian"],
        "Beng" => &["Noto Sans Bengali"],
        "Bopo" | "Hani" => unix_han(locale),
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
        "Hang" => unix_han("ko"),
        "Hano" => &["Noto Sans Hanunoo"],
        "Hebr" => &["Noto Sans Hebrew"],
        "Hira" | "Kana" => unix_han("ja"),
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

fn macos_han(locale: &str) -> &'static [&'static str] {
    match locale {
        "ja" => &["Hiragino Sans"],
        "ko" => &["Apple SD Gothic Neo"],
        "zh-HK" => &["PingFang HK"],
        "zh-TW" => &["PingFang TC"],
        _ => &["PingFang SC"],
    }
}

fn macos(script: &str, locale: &str) -> &'static [&'static str] {
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
        "Hani" => macos_han(locale),
        "Hang" => macos_han("ko"),
        "Hano" => &["Noto Sans Hanunoo"],
        "Hebr" => &["Arial"],
        "Hira" | "Kana" => macos_han("ja"),
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
