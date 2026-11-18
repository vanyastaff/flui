//! The rules of a pick its system hands over without a path: a browser's
//! file, an Android document, an iOS document read under its scoped access.
//! The provider reads the pick's contents itself, so the checks a path would
//! leave to the application are made here, one rule for every such host: the
//! picks are held to the request's filters by name and type, their size is
//! bounded before a byte is read and again as the bytes arrive, and a save's
//! file is named after the first filter.

use super::{DialogError, FileFilter};

/// Admit picks, each a name and the MIME type its system gives it, against
/// the request's `filters`: every pick must match one where there are any
/// ([`FileFilter::matches`]), since such a system's picker only suggests
/// them (ADR-0360 §4); [`DialogError::NotInFilter`] otherwise.
#[cfg(any(test, target_arch = "wasm32", target_os = "android"))]
pub(crate) fn admit_picks<'a>(
    picks: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    filters: &[FileFilter],
) -> Result<(), DialogError> {
    if filters.is_empty() {
        return Ok(());
    }
    let admitted = picks
        .into_iter()
        .all(|(name, mime_type)| filters.iter().any(|filter| filter.matches(name, mime_type)));
    if admitted {
        Ok(())
    } else {
        Err(DialogError::NotInFilter)
    }
}

/// The size of the picks, summed from the sizes their system declares for
/// them, refused past `limit` before anything is read. A size declared
/// wrongly, or not at all (counted as nothing), is caught by the bytes
/// actually read ([`admit_read`]).
#[cfg(any(test, target_arch = "wasm32", target_os = "android"))]
pub(crate) fn admit_declared(
    sizes: impl IntoIterator<Item = u64>,
    limit: u64,
) -> Result<u64, DialogError> {
    let total = sizes.into_iter().fold(0_u64, u64::saturating_add);
    admit_read(total, limit).map(|()| total)
}

/// Refuse `read` bytes of picks past `limit`, an open's
/// [`max_bytes`](super::OpenRequest::max_bytes), with
/// [`DialogError::TooLarge`]: what a provider that hands over a pick's
/// contents checks before it reads, by the sizes the system declares, and
/// again by the bytes it read.
pub(crate) fn admit_read(read: u64, limit: u64) -> Result<(), DialogError> {
    if read > limit {
        Err(DialogError::TooLarge {
            actual: read,
            limit,
        })
    } else {
        Ok(())
    }
}

/// The name a save's file is given where the provider names it from the
/// request rather than from what the user typed into a dialog (a browser's
/// save picker or download, an export to iOS's Files): `suggested`, or
/// `unnamed` when there is none, [named after the first
/// filter](named_after_first_filter).
#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
pub(crate) fn save_file_name(
    suggested: Option<&str>,
    filters: &[FileFilter],
    unnamed: &str,
) -> String {
    named_after_first_filter(suggested.unwrap_or(unnamed), filters)
}

/// `name` completed by the first filter, as a desktop save dialog completes
/// a typed name by the selected one (the rule of `FileFilter::completes`).
pub(crate) fn named_after_first_filter(name: &str, filters: &[FileFilter]) -> String {
    match filters
        .first()
        .and_then(|filter| filter.completion(name.as_bytes()))
    {
        // The stem ends before ASCII dots, on a character boundary.
        Some((stem, extension)) => format!("{}.{extension}", &name[..stem]),
        None => name.to_owned(),
    }
}
