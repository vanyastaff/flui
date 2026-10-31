//! The host's own font fallback answer (ADR-0271 §3): which faces the
//! operating system draws each script and emoji with, for a locale, as the
//! [`SystemFallback`] the text stack merges ahead of FLUI's tables.
//!
//! The composition root hands [`system_fallback`] to the font feed, which
//! calls it off the owner thread. Each platform file asks its system once
//! per script; one that cannot answer gives [`SystemFallback::none`], so
//! FLUI's tables alone, never a panic.

use flui_painting::SystemFallback;

#[cfg(any(target_os = "android", test))]
mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(all(
    unix,
    not(any(target_os = "android", target_os = "macos", target_os = "ios"))
))]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
use android::system_fallback as host_answer;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple::system_fallback as host_answer;
#[cfg(all(
    unix,
    not(any(target_os = "android", target_os = "macos", target_os = "ios"))
))]
use linux::system_fallback as host_answer;
#[cfg(target_os = "windows")]
use windows::system_fallback as host_answer;

/// No answer where FLUI does not ask the host.
#[cfg(not(any(unix, target_os = "windows")))]
fn host_answer(locale: &str) -> SystemFallback {
    SystemFallback::none(locale)
}

/// The faces the operating system falls back on for `locale`, a BCP 47
/// tag such as `ja-JP`: DirectWrite's system fallback on Windows, CoreText's
/// on macOS and iOS, fontconfig's order on Linux and the BSDs, and the
/// system's `fonts.xml` on Android below API 29. Android from API 29 and the
/// web give no answer yet (`docs/gaps.md`, entry 35).
///
/// Blocking: it reads the system's configuration and may scan its fonts,
/// so it belongs on the font feed's thread, not the owner's.
#[must_use]
pub fn system_fallback(locale: &str) -> SystemFallback {
    host_answer(locale)
}

/// A face a platform names by its file where the file holds one face, else
/// by `family`, since a collection file (`.ttc`, `.otc`) needs an index the
/// platform may not give. None for a nameless face and for the last-resort
/// face, which draws every character as a box.
#[cfg(any(target_os = "macos", target_os = "ios", test))]
fn face_by_file(path: Option<std::path::PathBuf>, family: &str) -> Option<flui_painting::FaceRef> {
    use flui_painting::FaceRef;

    if family.is_empty() || family.trim_start_matches('.') == "LastResort" {
        return None;
    }
    let collection = |path: &std::path::Path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("ttc") || extension.eq_ignore_ascii_case("otc")
            })
    };
    Some(match path {
        Some(path) if !collection(&path) => FaceRef::File { path, index: 0 },
        _ => FaceRef::Family(family.to_owned()),
    })
}

/// The UTF-16 `text` from `position` to its end; empty past the end.
#[cfg(any(target_os = "windows", test))]
fn text_after(text: &[u16], position: u32) -> &[u16] {
    usize::try_from(position)
        .ok()
        .and_then(|at| text.get(at..))
        .unwrap_or_default()
}

/// The UTF-16 `text` before `position`; empty past the end.
#[cfg(any(target_os = "windows", test))]
fn text_before(text: &[u16], position: u32) -> &[u16] {
    usize::try_from(position)
        .ok()
        .and_then(|at| text.get(..at))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_painting::FaceRef;

    use super::{face_by_file, text_after, text_before};

    /// A single-face file is named by its path, a collection or a face with
    /// no file by its family, and the last-resort face not at all.
    #[test]
    fn a_platform_face_is_named_by_file_or_family() {
        let file = |path: &str| Some(PathBuf::from(path));
        for (path, family, expected) in [
            (
                file("/System/Library/Fonts/Supplemental/Arial.ttf"),
                "Arial",
                Some(FaceRef::File {
                    path: PathBuf::from("/System/Library/Fonts/Supplemental/Arial.ttf"),
                    index: 0,
                }),
            ),
            (
                file("/System/Library/Fonts/PingFang.TTC"),
                "PingFang SC",
                Some(FaceRef::Family("PingFang SC".to_owned())),
            ),
            (
                None,
                "Apple Color Emoji",
                Some(FaceRef::Family("Apple Color Emoji".to_owned())),
            ),
            (
                file("/System/Library/Fonts/LastResort.otf"),
                ".LastResort",
                None,
            ),
            (file("/System/Library/Fonts/Unnamed.ttf"), "", None),
        ] {
            assert_eq!(
                face_by_file(path.clone(), family),
                expected,
                "{path:?} {family:?}"
            );
        }
    }

    /// DirectWrite reads a sample from a position and before it; a position
    /// past the end reads nothing on either side of an empty text.
    #[test]
    fn a_sample_is_read_around_a_position() {
        const NOTHING: &[u16] = &[];
        let text: Vec<u16> = "ab😀".encode_utf16().collect();
        assert_eq!(text_after(&text, 0), &text[..]);
        assert_eq!(text_after(&text, 2), &text[2..]);
        assert_eq!(text_after(&text, 4), NOTHING);
        assert_eq!(text_after(&text, u32::MAX), NOTHING);
        assert_eq!(text_before(&text, 0), NOTHING);
        assert_eq!(text_before(&text, 2), &text[..2]);
        assert_eq!(text_before(&text, 4), &text[..]);
        assert_eq!(text_before(&text, 5), NOTHING);
    }
}
