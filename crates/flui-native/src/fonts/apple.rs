//! CoreText's answer on macOS and iOS (ADR-0271 §3): for each script, the
//! face `CTFontCreateForStringWithLanguage` picks for the script's sample in
//! the locale's language, the system cascade list for that language as the
//! common list, and the face it picks for an emoji.
//!
//! `CTFontCreateForStringWithLanguage` is macOS 10.15 and iOS 13; FLUI's
//! macOS floor is 10.13, so it is looked up by name at run time and, where
//! the system lacks it, `CTFontCreateForString` (no language) answers. A
//! font in a collection file (`.ttc`) is named by its family, since CoreText
//! gives a face's file but not its index in the file.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::ffi::c_void;

use std::ptr::NonNull;

use flui_painting::{FaceRef, SystemFallback};
use objc2_core_foundation::{CFArray, CFRange, CFRetained, CFString, CFURL};
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontUIFontType, kCTFontFamilyNameAttribute, kCTFontURLAttribute,
};

/// `CTFontCreateForStringWithLanguage`, as `<CoreText/CTFont.h>` declares it.
type ForStringWithLanguage = unsafe extern "C-unwind" fn(
    current_font: &CTFont,
    string: &CFString,
    range: CFRange,
    language: Option<&CFString>,
) -> Option<NonNull<CTFont>>;

/// The host's answer for `locale`, or none where CoreText gives no base font.
pub(crate) fn system_fallback(locale: &str) -> SystemFallback {
    let language = CFString::from_str(locale);
    // SAFETY: a UI font type, a size (0 picks the default) and a language
    // string; CoreText returns an owned font or null.
    let Some(base) = (unsafe {
        CTFont::new_ui_font_for_language(CTFontUIFontType::System, 0.0, Some(&language))
    }) else {
        tracing::debug!("CoreText gave no system font; the fallback order is FLUI's tables alone");
        return SystemFallback::none(locale);
    };
    let with_language = for_string_with_language();
    let pick = |sample: &str| -> CFRetained<CTFont> {
        let text = CFString::from_str(sample);
        let range = CFRange::new(0, text.length());
        match with_language {
            // SAFETY: the function has the declared signature (looked up by
            // its exported name), and every argument is a live CoreFoundation
            // object or a range inside `text`; it returns an owned font.
            Some(create) => match unsafe { create(&base, &text, range, Some(&language)) } {
                // SAFETY: a `Create` function's result is owned by the caller.
                Some(font) => unsafe { CFRetained::from_raw(font) },
                None => base.clone(),
            },
            // SAFETY: as above, without the language.
            None => unsafe { base.for_string(&text, range) },
        }
    };

    let mut answer = SystemFallback::none(locale);
    for (script, sample) in SystemFallback::queries() {
        if let Some(face) = face_of(&pick(sample)) {
            answer = answer.with_script(script, vec![face]);
        }
    }
    answer = answer.with_emoji(
        face_of(&pick(SystemFallback::EMOJI_SAMPLE))
            .into_iter()
            .collect(),
    );

    let languages = CFArray::from_objects(&[&*language]);
    // SAFETY: the list is an array of language strings, as the function
    // requires; it returns an owned array of font descriptors, or null.
    let cascade = unsafe { base.default_cascade_list_for_languages(Some(languages.as_opaque())) };
    if let Some(cascade) = cascade {
        // SAFETY: CoreText documents the cascade list as an array of
        // `CTFontDescriptorRef`.
        let descriptors = unsafe { cascade.cast_unchecked::<CTFontDescriptor>() };
        let common = descriptors
            .iter()
            .filter_map(|descriptor| descriptor_face(&descriptor))
            .collect();
        answer = answer.with_common(common);
    }
    answer
}

/// `CTFontCreateForStringWithLanguage` where the running system exports it.
fn for_string_with_language() -> Option<ForStringWithLanguage> {
    // SAFETY: `RTLD_DEFAULT` searches the images the process loaded, which
    // include CoreText; the name is NUL-terminated.
    let address = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"CTFontCreateForStringWithLanguage".as_ptr(),
        )
    };
    (!address.is_null()).then(|| {
        // SAFETY: CoreText exports the symbol with the signature
        // `ForStringWithLanguage` declares, and a function pointer and a
        // data pointer have one size on Apple's targets.
        unsafe { std::mem::transmute::<*mut c_void, ForStringWithLanguage>(address) }
    })
}

/// The face `font` draws with: its file, or its family where the file is a
/// collection or CoreText names none.
fn face_of(font: &CTFont) -> Option<FaceRef> {
    // SAFETY: the attribute key is CoreText's own constant; the result is
    // an owned value of the attribute's type, or null.
    let url = unsafe { font.attribute(kCTFontURLAttribute) }
        .and_then(|value| value.downcast::<CFURL>().ok());
    // SAFETY: `CTFontCopyFamilyName` returns an owned string.
    let family = unsafe { font.family_name() }.to_string();
    super::face_by_file(url.and_then(|url| url.to_file_path()), &family)
}

/// The face a cascade-list `descriptor` names, as [`face_of`] does.
fn descriptor_face(descriptor: &CTFontDescriptor) -> Option<FaceRef> {
    // SAFETY: as for `face_of`; both keys are CoreText's constants.
    let url = unsafe { descriptor.attribute(kCTFontURLAttribute) }
        .and_then(|value| value.downcast::<CFURL>().ok());
    // SAFETY: as above.
    let family = unsafe { descriptor.attribute(kCTFontFamilyNameAttribute) }
        .and_then(|value| value.downcast::<CFString>().ok())
        .map(|name| name.to_string())
        .unwrap_or_default();
    super::face_by_file(url.and_then(|url| url.to_file_path()), &family)
}
