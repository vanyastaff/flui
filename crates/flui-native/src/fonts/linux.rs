//! fontconfig's answer on Linux and the BSDs (ADR-0271 §3): for each script,
//! `FcFontSort` over the script's sample characters and the locale's
//! language, kept to the files that cover the whole sample, in fontconfig's
//! order, so the user's and the distribution's preferences apply.
//!
//! The library is opened with `dlopen` for the query and closed after it:
//! a host without `libfontconfig.so.1`, or one missing a symbol, gives no
//! answer and one debug log, never a panic and never a link-time dependency.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::ptr::NonNull;

use flui_painting::{FaceRef, SystemFallback};

/// How many faces each script's list keeps: the first few fontconfig ranks,
/// enough to cover a face that lacks a glyph the sample did not ask about.
const FACES_PER_SCRIPT: usize = 4;

/// The host's answer for `locale`, or none where fontconfig cannot be opened.
pub(crate) fn system_fallback(locale: &str) -> SystemFallback {
    let Some(fc) = Fontconfig::open() else {
        return SystemFallback::none(locale);
    };
    let mut answer = SystemFallback::none(locale);
    for (script, sample) in SystemFallback::queries() {
        let faces = fc.sort(sample, locale, false);
        if !faces.is_empty() {
            answer = answer.with_script(script, faces);
        }
    }
    answer.with_emoji(fc.sort(SystemFallback::EMOJI_SAMPLE, locale, true))
}

/// fontconfig's opaque types.
type FcConfig = c_void;
type FcPattern = c_void;
type FcCharSet = c_void;
type FcLangSet = c_void;
type FcBool = c_int;
type FcResult = c_int;

/// `FcResultMatch`.
const FC_RESULT_MATCH: FcResult = 0;
/// `FcMatchPattern`.
const FC_MATCH_PATTERN: c_int = 0;

/// `FcFontSet`, as fontconfig lays it out.
#[repr(C)]
struct FcFontSet {
    nfont: c_int,
    sfont: c_int,
    fonts: *mut *mut FcPattern,
}

/// The C signatures of the functions the query calls, as fontconfig.h
/// declares them (2.11 and later).
type ConfigInit = unsafe extern "C" fn() -> *mut FcConfig;
type ConfigDestroy = unsafe extern "C" fn(*mut FcConfig);
type PatternCreate = unsafe extern "C" fn() -> *mut FcPattern;
type PatternDestroy = unsafe extern "C" fn(*mut FcPattern);
type PatternAddBool = unsafe extern "C" fn(*mut FcPattern, *const c_char, FcBool) -> FcBool;
type PatternAddCharSet =
    unsafe extern "C" fn(*mut FcPattern, *const c_char, *const FcCharSet) -> FcBool;
type PatternAddLangSet =
    unsafe extern "C" fn(*mut FcPattern, *const c_char, *const FcLangSet) -> FcBool;
type PatternGetString =
    unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut *mut u8) -> FcResult;
type PatternGetInteger =
    unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut c_int) -> FcResult;
type PatternGetCharSet =
    unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut *mut FcCharSet) -> FcResult;
type CharSetCreate = unsafe extern "C" fn() -> *mut FcCharSet;
type CharSetDestroy = unsafe extern "C" fn(*mut FcCharSet);
type CharSetAddChar = unsafe extern "C" fn(*mut FcCharSet, u32) -> FcBool;
type CharSetHasChar = unsafe extern "C" fn(*const FcCharSet, u32) -> FcBool;
type LangSetCreate = unsafe extern "C" fn() -> *mut FcLangSet;
type LangSetDestroy = unsafe extern "C" fn(*mut FcLangSet);
type LangSetAdd = unsafe extern "C" fn(*mut FcLangSet, *const u8) -> FcBool;
type ConfigSubstitute = unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, c_int) -> FcBool;
type DefaultSubstitute = unsafe extern "C" fn(*mut FcPattern);
type FontSort = unsafe extern "C" fn(
    *mut FcConfig,
    *mut FcPattern,
    FcBool,
    *mut *mut FcCharSet,
    *mut FcResult,
) -> *mut FcFontSet;
type FontSetDestroy = unsafe extern "C" fn(*mut FcFontSet);

/// The fontconfig functions the query calls, from one `dlopen` handle, and
/// the configuration they query. Dropping it destroys the configuration and
/// closes the handle.
struct Fontconfig {
    handle: NonNull<c_void>,
    config: NonNull<FcConfig>,
    config_destroy: ConfigDestroy,
    pattern_create: PatternCreate,
    pattern_destroy: PatternDestroy,
    pattern_add_bool: PatternAddBool,
    pattern_add_char_set: PatternAddCharSet,
    pattern_add_lang_set: PatternAddLangSet,
    pattern_get_string: PatternGetString,
    pattern_get_integer: PatternGetInteger,
    pattern_get_char_set: PatternGetCharSet,
    char_set_create: CharSetCreate,
    char_set_destroy: CharSetDestroy,
    char_set_add_char: CharSetAddChar,
    char_set_has_char: CharSetHasChar,
    lang_set_create: LangSetCreate,
    lang_set_destroy: LangSetDestroy,
    lang_set_add: LangSetAdd,
    config_substitute: ConfigSubstitute,
    default_substitute: DefaultSubstitute,
    font_sort: FontSort,
    font_set_destroy: FontSetDestroy,
}

impl Fontconfig {
    /// Opens `libfontconfig.so.1` and loads its configuration, or `None`
    /// (logged once at debug) when the library, a symbol or the
    /// configuration is missing.
    fn open() -> Option<Self> {
        // SAFETY: the name is NUL-terminated; the handle is closed only in
        // `Drop`, or below when the load fails.
        let handle = NonNull::new(unsafe {
            libc::dlopen(
                c"libfontconfig.so.1".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        });
        let Some(handle) = handle else {
            tracing::debug!("no libfontconfig.so.1; the fallback order is FLUI's tables alone");
            return None;
        };
        let opened = Self::load(handle);
        if opened.is_none() {
            // SAFETY: `handle` came from `dlopen` above, and a failed load
            // keeps no pointer taken from it and builds no `Fontconfig`
            // whose `Drop` would close it again.
            unsafe { libc::dlclose(handle.as_ptr()) };
            tracing::debug!("libfontconfig.so.1 lacks a symbol or a configuration");
        }
        opened
    }

    /// The functions from `handle`, and the loaded configuration; `None`
    /// before any value that owns `handle` exists.
    fn load(handle: NonNull<c_void>) -> Option<Self> {
        /// The function `name` in `handle`, as the signature `ty`.
        macro_rules! symbol {
            ($name:literal as $ty:ty) => {{
                // SAFETY: `handle` is a live `dlopen` handle and the name is
                // NUL-terminated.
                let address = unsafe { libc::dlsym(handle.as_ptr(), $name.as_ptr()) };
                if address.is_null() {
                    return None;
                }
                // SAFETY: fontconfig exports `$name` with the C signature
                // `$ty` (fontconfig.h, 2.11 and later); a data pointer and a
                // function pointer have one size on every target this module
                // builds for.
                unsafe { std::mem::transmute::<*mut c_void, $ty>(address) }
            }};
        }

        let init = symbol!(c"FcInitLoadConfigAndFonts" as ConfigInit);
        let config_destroy = symbol!(c"FcConfigDestroy" as ConfigDestroy);
        let mut fc = Self {
            handle,
            config: NonNull::dangling(),
            config_destroy,
            pattern_create: symbol!(c"FcPatternCreate" as PatternCreate),
            pattern_destroy: symbol!(c"FcPatternDestroy" as PatternDestroy),
            pattern_add_bool: symbol!(c"FcPatternAddBool" as PatternAddBool),
            pattern_add_char_set: symbol!(c"FcPatternAddCharSet" as PatternAddCharSet),
            pattern_add_lang_set: symbol!(c"FcPatternAddLangSet" as PatternAddLangSet),
            pattern_get_string: symbol!(c"FcPatternGetString" as PatternGetString),
            pattern_get_integer: symbol!(c"FcPatternGetInteger" as PatternGetInteger),
            pattern_get_char_set: symbol!(c"FcPatternGetCharSet" as PatternGetCharSet),
            char_set_create: symbol!(c"FcCharSetCreate" as CharSetCreate),
            char_set_destroy: symbol!(c"FcCharSetDestroy" as CharSetDestroy),
            char_set_add_char: symbol!(c"FcCharSetAddChar" as CharSetAddChar),
            char_set_has_char: symbol!(c"FcCharSetHasChar" as CharSetHasChar),
            lang_set_create: symbol!(c"FcLangSetCreate" as LangSetCreate),
            lang_set_destroy: symbol!(c"FcLangSetDestroy" as LangSetDestroy),
            lang_set_add: symbol!(c"FcLangSetAdd" as LangSetAdd),
            config_substitute: symbol!(c"FcConfigSubstitute" as ConfigSubstitute),
            default_substitute: symbol!(c"FcDefaultSubstitute" as DefaultSubstitute),
            font_sort: symbol!(c"FcFontSort" as FontSort),
            font_set_destroy: symbol!(c"FcFontSetDestroy" as FontSetDestroy),
        };
        // SAFETY: `FcInitLoadConfigAndFonts` takes no argument and returns
        // a configuration the caller owns, or null.
        let Some(config) = NonNull::new(unsafe { init() }) else {
            // Not dropped: its `Drop` would destroy the dangling
            // configuration and close the handle `open` closes.
            std::mem::forget(fc);
            return None;
        };
        fc.config = config;
        Some(fc)
    }

    /// The files fontconfig ranks for `sample` in `locale`, best first, kept
    /// to those whose character set covers every character of `sample`, at
    /// most [`FACES_PER_SCRIPT`]. With `colour`, the pattern asks for colour
    /// faces, as an emoji query does.
    fn sort(&self, sample: &str, locale: &str, colour: bool) -> Vec<FaceRef> {
        let mut faces = Vec::new();
        // SAFETY: every pointer passed below is one fontconfig returned and
        // this function has not yet destroyed (each is destroyed once, at the
        // end), or a NUL-terminated string that outlives the call; the font
        // set's `fonts` array holds `nfont` live patterns until it is
        // destroyed.
        unsafe {
            let pattern = (self.pattern_create)();
            if pattern.is_null() {
                return faces;
            }
            let chars = (self.char_set_create)();
            if !chars.is_null() {
                for c in sample.chars() {
                    (self.char_set_add_char)(chars, u32::from(c));
                }
                (self.pattern_add_char_set)(pattern, c"charset".as_ptr(), chars);
                (self.char_set_destroy)(chars);
            }
            let language = fontconfig_language(locale);
            let langs = (self.lang_set_create)();
            if !langs.is_null() {
                (self.lang_set_add)(langs, language.as_ptr().cast());
                (self.pattern_add_lang_set)(pattern, c"lang".as_ptr(), langs);
                (self.lang_set_destroy)(langs);
            }
            if colour {
                (self.pattern_add_bool)(pattern, c"color".as_ptr(), 1);
            }
            (self.config_substitute)(self.config.as_ptr(), pattern, FC_MATCH_PATTERN);
            (self.default_substitute)(pattern);
            let mut result: FcResult = 0;
            let set = (self.font_sort)(
                self.config.as_ptr(),
                pattern,
                1,
                std::ptr::null_mut(),
                &raw mut result,
            );
            (self.pattern_destroy)(pattern);
            if set.is_null() {
                return faces;
            }
            let count = usize::try_from((*set).nfont).unwrap_or(0);
            for slot in 0..count {
                if faces.len() == FACES_PER_SCRIPT {
                    break;
                }
                let font = *(*set).fonts.add(slot);
                if let Some(face) = self.covering_file(font, sample)
                    && !faces.contains(&face)
                {
                    faces.push(face);
                }
            }
            (self.font_set_destroy)(set);
        }
        faces
    }

    /// The file and face index of `font`, if its character set covers every
    /// character of `sample`.
    ///
    /// # Safety
    ///
    /// `font` is a live pattern of a font set this value's library returned.
    unsafe fn covering_file(&self, font: *mut FcPattern, sample: &str) -> Option<FaceRef> {
        // SAFETY: the caller's contract; the out-pointers are valid for one
        // write, and a string fontconfig returns lives as long as `font`.
        unsafe {
            let mut chars: *mut FcCharSet = std::ptr::null_mut();
            if (self.pattern_get_char_set)(font, c"charset".as_ptr(), 0, &raw mut chars)
                != FC_RESULT_MATCH
                || !sample
                    .chars()
                    .all(|c| (self.char_set_has_char)(chars, u32::from(c)) != 0)
            {
                return None;
            }
            let mut file: *mut u8 = std::ptr::null_mut();
            if (self.pattern_get_string)(font, c"file".as_ptr(), 0, &raw mut file)
                != FC_RESULT_MATCH
                || file.is_null()
            {
                return None;
            }
            let path = CStr::from_ptr(file.cast()).to_str().ok()?;
            let mut index: c_int = 0;
            if (self.pattern_get_integer)(font, c"index".as_ptr(), 0, &raw mut index)
                != FC_RESULT_MATCH
            {
                index = 0;
            }
            Some(FaceRef::File {
                path: PathBuf::from(path),
                index: u32::try_from(index).unwrap_or(0),
            })
        }
    }
}

impl Drop for Fontconfig {
    fn drop(&mut self) {
        // SAFETY: the configuration came from `FcInitLoadConfigAndFonts` and
        // is destroyed once; the handle came from `dlopen` and is closed once,
        // after the last call through a pointer taken from it.
        unsafe {
            (self.config_destroy)(self.config.as_ptr());
            libc::dlclose(self.handle.as_ptr());
        }
    }
}

/// `locale` as fontconfig spells a language: lower case, `-` between the
/// language and its region (`ja-jp`), and `und` for an empty tag.
fn fontconfig_language(locale: &str) -> CString {
    let tag = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('_', "-");
    let mut subtags = tag.split('-').filter(|subtag| !subtag.is_empty());
    let language = subtags.next().unwrap_or("und").to_ascii_lowercase();
    let region =
        subtags.find(|subtag| subtag.len() == 2 && subtag.chars().all(char::is_alphabetic));
    let spelled = match region {
        Some(region) => format!("{language}-{}", region.to_ascii_lowercase()),
        None => language,
    };
    CString::new(spelled).unwrap_or_else(|_| c"und".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{fontconfig_language, system_fallback};

    /// A locale reaches fontconfig as a language and region, whatever
    /// spelling the host reported it in.
    #[test]
    fn locales_are_spelled_as_fontconfig_languages() {
        for (locale, expected) in [
            ("en-US", "en-us"),
            ("ja_JP.UTF-8", "ja-jp"),
            ("zh-Hant-TW", "zh-tw"),
            ("sr-Latn", "sr"),
            ("", "und"),
        ] {
            assert_eq!(
                fontconfig_language(locale).to_str().expect("ASCII"),
                expected,
                "{locale:?}"
            );
        }
    }

    /// On a host with fontconfig and a Latin face (the CI image), Latin gets
    /// a list of files that exist; a host without fontconfig gives no answer.
    #[test]
    fn fontconfig_answers_latin() {
        use flui_painting::{FaceRef, ScriptCode};

        let answer = system_fallback("en-US");
        assert_eq!(answer.locale, "en-US");
        let Some(fc) = super::Fontconfig::open() else {
            assert!(answer.is_empty(), "no fontconfig, no answer");
            return;
        };
        drop(fc);
        let latin = answer.script(ScriptCode::from_bytes(*b"Latn"));
        assert!(!latin.is_empty(), "fontconfig names a Latin face");
        for face in latin {
            let FaceRef::File { path, .. } = face else {
                panic!("fontconfig names files, got {face:?}");
            };
            assert!(path.is_file(), "{} exists", path.display());
        }
    }
}
