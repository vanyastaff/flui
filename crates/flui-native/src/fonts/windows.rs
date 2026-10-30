//! DirectWrite's answer on Windows (ADR-0271 §3): for each script, the face
//! the system font fallback (`IDWriteFontFallback::MapCharacters`) maps the
//! script's sample to in the locale, and the face it maps an emoji to.
//! DirectWrite keeps no common list: a script it does not map falls to
//! FLUI's tables.
//!
//! `IDWriteFactory2` and its system fallback are Windows 8.1, below FLUI's
//! floor, so no gate is needed. The text DirectWrite reads is handed over
//! through an `IDWriteTextAnalysisSource` of FLUI's.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use flui_painting::{FaceRef, SystemFallback};
use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_READING_DIRECTION, DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
    DWriteCreateFactory, IDWriteFactory2, IDWriteFont, IDWriteFontCollection, IDWriteFontFallback,
    IDWriteLocalFontFileLoader, IDWriteNumberSubstitution, IDWriteTextAnalysisSource,
    IDWriteTextAnalysisSource_Impl,
};
use windows_core::{Interface as _, OutRef, PCWSTR, w};

use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// The host's answer for `locale`, or none where DirectWrite cannot be
/// reached (logged at debug).
pub(crate) fn system_fallback(locale: &str) -> SystemFallback {
    match ask(locale) {
        Ok(answer) => answer,
        Err(error) => {
            tracing::debug!(%error, "DirectWrite gave no fallback; the order is FLUI's tables alone");
            SystemFallback::none(locale)
        }
    }
}

/// The system fallback's answer for every script's sample and an emoji.
fn ask(locale: &str) -> windows_core::Result<SystemFallback> {
    // SAFETY: a shared factory, as DirectWrite documents; the interface is
    // one the factory implements on Windows 8.1 and later.
    let factory: IDWriteFactory2 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
    // SAFETY: a call on a live factory.
    let fallback = unsafe { factory.GetSystemFontFallback() }?;
    let mut collection = None;
    // SAFETY: the out-pointer is a live `Option`; no update check.
    unsafe { factory.GetSystemFontCollection(&raw mut collection, false) }?;
    let collection = collection.ok_or_else(|| windows_core::Error::from(E_POINTER))?;

    let mut answer = SystemFallback::none(locale);
    for (script, sample) in SystemFallback::queries() {
        if let Some(face) = map(&fallback, &collection, sample, locale) {
            answer = answer.with_script(script, vec![face]);
        }
    }
    let emoji = map(&fallback, &collection, SystemFallback::EMOJI_SAMPLE, locale);
    Ok(answer.with_emoji(emoji.into_iter().collect::<Vec<FaceRef>>()))
}

/// The face the fallback maps the start of `sample` to in `locale`, if any
/// system face covers it.
fn map(
    fallback: &IDWriteFontFallback,
    collection: &IDWriteFontCollection,
    sample: &str,
    locale: &str,
) -> Option<FaceRef> {
    let text: Vec<u16> = sample.encode_utf16().collect();
    let length = u32::try_from(text.len()).ok()?;
    let source: IDWriteTextAnalysisSource = Sample {
        text,
        locale: locale.encode_utf16().chain([0]).collect::<Vec<u16>>(),
    }
    .into();
    let mut mapped = 0;
    let mut font: Option<IDWriteFont> = None;
    let mut scale = 1.0;
    // SAFETY: the source, the collection and the family name are live for
    // the call; the out-pointers are live locals.
    unsafe {
        fallback.MapCharacters(
            &source,
            0,
            length,
            collection,
            w!(""),
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            &raw mut mapped,
            &raw mut font,
            &raw mut scale,
        )
    }
    .ok()?;
    face_of(&font?)
}

/// The face `font` is: its local file and index, or its family where the
/// file is not local (a downloadable or memory font).
fn face_of(font: &IDWriteFont) -> Option<FaceRef> {
    file_of(font).or_else(|| family_of(font).map(FaceRef::Family))
}

/// The local file `font` is read from, and its index in the file.
fn file_of(font: &IDWriteFont) -> Option<FaceRef> {
    // SAFETY: every call below is on a live interface DirectWrite returned;
    // `GetFiles` is asked for its count first and then given room for that
    // many; the reference key is read while `file` holds it.
    unsafe {
        let face = font.CreateFontFace().ok()?;
        let mut count = 0;
        face.GetFiles(&raw mut count, None).ok()?;
        let mut files = vec![None; usize::try_from(count).ok()?];
        face.GetFiles(&raw mut count, Some(files.as_mut_ptr()))
            .ok()?;
        let file = files.into_iter().next().flatten()?;
        let loader: IDWriteLocalFontFileLoader = file.GetLoader().ok()?.cast().ok()?;
        let mut key = std::ptr::null_mut();
        let mut key_size = 0;
        file.GetReferenceKey(&raw mut key, &raw mut key_size).ok()?;
        let length = loader.GetFilePathLengthFromKey(key, key_size).ok()?;
        let mut path = vec![0_u16; usize::try_from(length).ok()?.checked_add(1)?];
        loader.GetFilePathFromKey(key, key_size, &mut path).ok()?;
        path.truncate(path.len() - 1);
        Some(FaceRef::File {
            path: PathBuf::from(OsString::from_wide(&path)),
            index: face.GetIndex(),
        })
    }
}

/// The family name of `font`, in US English where the font has one, else
/// its first name.
fn family_of(font: &IDWriteFont) -> Option<String> {
    // SAFETY: calls on live interfaces; the buffer has room for the name
    // and its terminator.
    unsafe {
        let names = font.GetFontFamily().ok()?.GetFamilyNames().ok()?;
        let mut index = 0;
        let mut exists = windows_core::BOOL(0);
        names
            .FindLocaleName(w!("en-us"), &raw mut index, &raw mut exists)
            .ok()?;
        if !exists.as_bool() {
            index = 0;
        }
        let length = names.GetStringLength(index).ok()?;
        let mut name = vec![0_u16; usize::try_from(length).ok()?.checked_add(1)?];
        names.GetString(index, &mut name).ok()?;
        name.truncate(name.len() - 1);
        Some(String::from_utf16_lossy(&name))
    }
}

/// The `#[implement]` expansion, in a module of its own so the lints its
/// generated code trips are expected here and nowhere else.
#[expect(
    clippy::ref_as_ptr,
    clippy::inline_always,
    raw_borrows_via_references,
    reason = "code generated by windows-core's #[implement]"
)]
mod com_object {
    use windows::Win32::Graphics::DirectWrite::IDWriteTextAnalysisSource;
    use windows_core::implement;

    /// One sample text in one locale, as DirectWrite reads text to map.
    #[implement(IDWriteTextAnalysisSource)]
    pub(super) struct Sample {
        /// The sample, in UTF-16.
        pub(super) text: Vec<u16>,
        /// The locale's BCP 47 name, in UTF-16 with its terminator.
        pub(super) locale: Vec<u16>,
    }
}
use com_object::{Sample, Sample_Impl};

/// The boundary every vtable method of [`Sample`] crosses: a panic answers
/// `E_UNEXPECTED`.
fn com_method<R>(body: impl FnOnce() -> windows_core::Result<R>) -> windows_core::Result<R> {
    os_entry(
        "the font fallback's sample",
        (),
        AfterPanic::Answer(|()| Err(E_UNEXPECTED.into())),
        body,
    )
}

impl IDWriteTextAnalysisSource_Impl for Sample_Impl {
    fn GetTextAtPosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows_core::Result<()> {
        com_method(|| {
            if textstring.is_null() || textlength.is_null() {
                return Err(E_POINTER.into());
            }
            let (start, length) = after(&self.text, textposition);
            // SAFETY: both out-pointers were checked; the text lives as long
            // as this object, which DirectWrite holds for the call.
            unsafe {
                textstring.write(start.cast_mut());
                textlength.write(length);
            }
            Ok(())
        })
    }

    fn GetTextBeforePosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows_core::Result<()> {
        com_method(|| {
            if textstring.is_null() || textlength.is_null() {
                return Err(E_POINTER.into());
            }
            let (start, length) = before(&self.text, textposition);
            // SAFETY: as for `GetTextAtPosition`.
            unsafe {
                textstring.write(start.cast_mut());
                textlength.write(length);
            }
            Ok(())
        })
    }

    fn GetParagraphReadingDirection(&self) -> DWRITE_READING_DIRECTION {
        os_entry(
            "the font fallback's sample",
            (),
            AfterPanic::Answer(|()| DWRITE_READING_DIRECTION_LEFT_TO_RIGHT),
            || DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
        )
    }

    fn GetLocaleName(
        &self,
        textposition: u32,
        textlength: *mut u32,
        localename: *mut *mut u16,
    ) -> windows_core::Result<()> {
        com_method(|| {
            if textlength.is_null() || localename.is_null() {
                return Err(E_POINTER.into());
            }
            let (_, length) = after(&self.text, textposition);
            // SAFETY: both out-pointers were checked; the locale is
            // NUL-terminated and lives as long as this object.
            unsafe {
                textlength.write(length);
                localename.write(PCWSTR(self.locale.as_ptr()).0.cast_mut());
            }
            Ok(())
        })
    }

    fn GetNumberSubstitution(
        &self,
        textposition: u32,
        textlength: *mut u32,
        numbersubstitution: OutRef<'_, IDWriteNumberSubstitution>,
    ) -> windows_core::Result<()> {
        com_method(|| {
            if textlength.is_null() {
                return Err(E_POINTER.into());
            }
            let (_, length) = after(&self.text, textposition);
            // SAFETY: the out-pointer was checked.
            unsafe { textlength.write(length) };
            numbersubstitution.write(None)
        })
    }
}

/// The text from `position` to the end, as DirectWrite reads it: a pointer
/// and a length, null and 0 past the end.
fn after(text: &[u16], position: u32) -> (*const u16, u32) {
    raw(super::text_after(text, position))
}

/// The text before `position`, as DirectWrite reads it: a pointer and a
/// length, null and 0 at the start.
fn before(text: &[u16], position: u32) -> (*const u16, u32) {
    raw(super::text_before(text, position))
}

/// `run` as a pointer and a length, null for an empty run.
fn raw(run: &[u16]) -> (*const u16, u32) {
    if run.is_empty() {
        return (std::ptr::null(), 0);
    }
    let length = u32::try_from(run.len()).expect("BUG: a sample is a few characters");
    (run.as_ptr(), length)
}
