//! The rules of a pick its system hands over without a path: a browser's
//! file, an Android document. The provider reads the pick's contents itself,
//! so the checks a path would leave to the application are made here, one
//! rule for every such host: the picks are held to the request's filters by
//! name and type, their size is bounded before a byte is read and again as
//! the bytes arrive, and a save's file is named after the first filter.

use super::{DialogError, FileFilter};

/// Admit picks, each a name and the MIME type its system gives it, against
/// the request's `filters`: every pick must match one where there are any
/// ([`FileFilter::matches`]), since such a system's picker only suggests
/// them (ADR-0360 §4); [`DialogError::NotInFilter`] otherwise.
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
pub(crate) fn admit_declared(
    sizes: impl IntoIterator<Item = u64>,
    limit: u64,
) -> Result<u64, DialogError> {
    let total = sizes.into_iter().fold(0_u64, u64::saturating_add);
    admit_read(total, limit).map(|()| total)
}

/// Refuse `read` bytes past `limit` with [`DialogError::TooLarge`].
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

/// `name` with the first filter's first extension added when it has none,
/// as a desktop save dialog names a typed file after the selected filter.
pub(crate) fn named_after_first_filter(name: &str, filters: &[FileFilter]) -> String {
    let has_extension = name
        .rsplit_once('.')
        .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty());
    match filters
        .first()
        .and_then(|filter| filter.extensions().first())
    {
        Some(extension) if !has_extension => {
            // `notes.` is `notes` with an empty extension: the filter's
            // replaces it rather than following a second dot.
            let stem = match name.trim_end_matches('.') {
                "" => name,
                stem => stem,
            };
            format!("{stem}.{extension}")
        }
        _ => name.to_owned(),
    }
}
