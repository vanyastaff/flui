//! What the web providers decide without a page: how a dialog's filters
//! become the browser's, what a download is named, where a launched URI
//! goes, and what a browser's exception means. Kept apart from the DOM so
//! the rules are tested on every host.

use flui_platform::BrowserException;

use crate::dialog::{DialogError, FileFilter};

/// The longest extension, with its leading `.`, the File System Access API
/// takes: Chromium refuses a longer one with a `TypeError`
/// (`VerifyIsValidExtension` in `global_file_system_access.cc`).
const MAX_PICKER_EXTENSION_LEN: usize = 16;

/// What a download is named when the request suggests no name.
const UNNAMED_DOWNLOAD: &str = "download";

/// The type a download's blob is given when no filter names a concrete one.
const UNTYPED_DOWNLOAD: &str = "application/octet-stream";

/// The key a filter of extensions only is given in a picker's `accept`
/// record, which takes MIME types as keys: `*/*` passes the API's token
/// check and adds no extension of its own, so the filter matches its
/// extensions alone (as `browser-fs-access` does).
const ANY_TYPE: &str = "*/*";

/// The `accept` attribute of an `<input type=file>` for `filters`: every
/// extension as `.ext` and every MIME type, comma-separated; empty, which
/// accepts every file, when there are none.
///
/// One input offers one filter, so the request's filters become their
/// union: the attribute matches exactly the files one of them matches, and
/// the filters' names are not shown.
pub(crate) fn accept_attribute(filters: &[FileFilter]) -> String {
    let mut patterns: Vec<String> = Vec::new();
    for filter in filters {
        for extension in filter.extensions() {
            let pattern = format!(".{extension}");
            if !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
        for mime_type in filter.mime_types() {
            if !patterns.contains(mime_type) {
                patterns.push(mime_type.clone());
            }
        }
    }
    patterns.join(",")
}

/// One entry of a File System Access picker's `types`: a filter's name, and
/// its `accept` record of MIME types to the extensions under each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PickerType<'a> {
    pub(crate) description: &'a str,
    pub(crate) accept: Vec<(&'a str, Vec<String>)>,
}

/// The `types` of a File System Access picker for `filters`, one per
/// filter in order; `None` when an extension is one the API refuses (a `-`
/// or `_`, or longer than [`MAX_PICKER_EXTENSION_LEN`] with its `.`), so the
/// request goes to `<input type=file>`, whose `accept` takes any.
///
/// A filter's extensions go under its first MIME type, or under `*/*` when
/// it has none; its other MIME types carry none. The picker matches the
/// union of what each key and extension names, which is the filter's
/// meaning (ADR-0360).
pub(crate) fn picker_types(filters: &[FileFilter]) -> Option<Vec<PickerType<'_>>> {
    filters
        .iter()
        .map(|filter| {
            let extensions = filter
                .extensions()
                .iter()
                .map(|extension| {
                    let dotted = format!(".{extension}");
                    is_picker_extension(&dotted).then_some(dotted)
                })
                .collect::<Option<Vec<String>>>()?;
            let mut mime_types = filter.mime_types().iter().map(String::as_str);
            let first = mime_types.next();
            let mut accept: Vec<(&str, Vec<String>)> = Vec::new();
            match first {
                Some(first) => accept.push((first, extensions)),
                None => accept.push((ANY_TYPE, extensions)),
            }
            accept.extend(mime_types.map(|mime_type| (mime_type, Vec::new())));
            Some(PickerType {
                description: filter.name(),
                accept,
            })
        })
        .collect::<Option<Vec<PickerType<'_>>>>()
}

/// Whether the File System Access API takes `dotted` (an extension with its
/// `.`): ASCII letters, digits, `+` and `.`, at most
/// [`MAX_PICKER_EXTENSION_LEN`] characters.
fn is_picker_extension(dotted: &str) -> bool {
    dotted.len() <= MAX_PICKER_EXTENSION_LEN
        && dotted
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.'))
}

/// The name a save's file is given: the suggested name, or
/// [`UNNAMED_DOWNLOAD`], with the first filter's first extension added when
/// the name has none, as a desktop save dialog names a typed file after the
/// selected filter.
pub(crate) fn save_name(suggested: Option<&str>, filters: &[FileFilter]) -> String {
    let name = suggested.unwrap_or(UNNAMED_DOWNLOAD);
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

/// The type a download's blob carries: the first concrete MIME type of the
/// first filter (one without `*`), [`UNTYPED_DOWNLOAD`] otherwise.
pub(crate) fn download_type(filters: &[FileFilter]) -> &str {
    filters
        .first()
        .and_then(|filter| {
            filter
                .mime_types()
                .iter()
                .find(|mime_type| !mime_type.contains('*'))
        })
        .map_or(UNTYPED_DOWNLOAD, String::as_str)
}

/// The size of the picks, summed from the sizes the browser declares for
/// them (`File.size`, a non-negative double, which saturates into `u64`),
/// refused past `limit` before anything is read.
pub(crate) fn admit_sizes(
    sizes: impl IntoIterator<Item = f64>,
    limit: u64,
) -> Result<u64, DialogError> {
    // `as` saturates a double into `u64`, and maps a NaN to 0: a size the
    // browser declares wrongly is caught again by the bytes actually read.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a float-to-int `as` saturates, which is the bound wanted here"
    )]
    let total = sizes
        .into_iter()
        .fold(0_u64, |total, size| total.saturating_add(size as u64));
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

/// What a picker's rejection means: `None` when the user dismissed the
/// picker (`AbortError`, which the API also answers for a picker the
/// browser closed), the failure the browser named otherwise.
pub(crate) fn picker_rejection(exception: BrowserException) -> Option<DialogError> {
    match exception {
        BrowserException::Abort => None,
        other => Some(DialogError::Failed(other.into())),
    }
}

/// Where a launched URI goes in the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// A new browsing context with no opener and no referrer: a page of
    /// `http` or `https`, which would otherwise replace the application.
    NewContext,
    /// The application's own browsing context: a scheme a browser does not
    /// fetch (`mailto`, `tel`, an application's own), which the HTML
    /// standard hands to the system's handler without unloading the page,
    /// and which Safari opens nowhere else.
    HandOff,
}

/// Where a URI of `scheme` goes. The seam admits no other fetch scheme
/// (`about`, `blob`, `data` and `file` are refused there), so `http` and
/// `https` are the only pages.
pub(crate) fn link_target(scheme: &str) -> LinkTarget {
    if scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http") {
        LinkTarget::NewContext
    } else {
        LinkTarget::HandOff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn images() -> FileFilter {
        FileFilter::new("Images")
            .with_extension("png")
            .with_extension("JPG")
            .with_mime_type("image/webp")
    }

    #[test]
    fn the_accept_attribute_is_the_union_of_the_filters() {
        let archives = FileFilter::new("Archives")
            .with_extension("tar.gz")
            .with_extension("png");
        assert_eq!(
            accept_attribute(&[images(), archives]),
            ".png,.jpg,image/webp,.tar.gz"
        );
        assert_eq!(accept_attribute(&[]), "", "no filter accepts every file");
    }

    #[test]
    fn a_picker_type_keeps_each_filter_and_puts_extensions_under_a_type() {
        let images = images();
        let text = FileFilter::new("Text").with_extension("txt");
        let types = picker_types(std::slice::from_ref(&images))
            .into_iter()
            .chain(picker_types(std::slice::from_ref(&text)))
            .flatten()
            .collect::<Vec<PickerType<'_>>>();
        assert_eq!(
            types,
            [
                PickerType {
                    description: "Images",
                    accept: vec![("image/webp", vec![".png".to_owned(), ".jpg".to_owned()])],
                },
                PickerType {
                    description: "Text",
                    accept: vec![("*/*", vec![".txt".to_owned()])],
                },
            ]
        );
        let two_types = FileFilter::new("Pictures")
            .with_mime_type("image/png")
            .with_mime_type("image/*");
        assert_eq!(
            picker_types(std::slice::from_ref(&two_types)).expect("mime types are taken")[0].accept,
            vec![("image/png", Vec::new()), ("image/*", Vec::new())]
        );
    }

    #[test]
    fn an_extension_the_picker_refuses_sends_the_request_to_the_input() {
        for extension in ["my-ext", "my_ext", "abcdefghijklmnop"] {
            let filter = FileFilter::new("Odd").with_extension(extension);
            assert_eq!(
                picker_types(std::slice::from_ref(&filter)),
                None,
                "{extension}"
            );
            assert_eq!(
                accept_attribute(std::slice::from_ref(&filter)),
                format!(".{extension}"),
                "the input takes {extension}"
            );
        }
        let longest = FileFilter::new("Long").with_extension("abcdefghijklmno");
        assert!(picker_types(std::slice::from_ref(&longest)).is_some());
        let parts = FileFilter::new("Parts").with_extension("tar.gz");
        assert!(picker_types(std::slice::from_ref(&parts)).is_some());
    }

    #[test]
    fn a_save_is_named_after_the_first_filter_when_its_name_has_no_extension() {
        let filters = [images()];
        assert_eq!(save_name(Some("photo"), &filters), "photo.png");
        assert_eq!(save_name(Some("photo.jpeg"), &filters), "photo.jpeg");
        assert_eq!(save_name(Some(".profile"), &filters), ".profile.png");
        assert_eq!(save_name(Some("notes."), &filters), "notes.png");
        assert_eq!(save_name(None, &filters), "download.png");
        assert_eq!(save_name(None, &[]), "download");
        assert_eq!(save_name(Some("photo"), &[]), "photo");
        let typed = FileFilter::new("Pictures").with_mime_type("image/png");
        assert_eq!(save_name(Some("photo"), &[typed]), "photo");
    }

    #[test]
    fn a_download_is_typed_after_the_first_concrete_type_of_the_first_filter() {
        assert_eq!(download_type(&[images()]), "image/webp");
        let any_image = FileFilter::new("Any").with_mime_type("image/*");
        assert_eq!(download_type(&[any_image]), "application/octet-stream");
        assert_eq!(download_type(&[]), "application/octet-stream");
    }

    #[test]
    fn the_picks_are_refused_past_the_limit_before_they_are_read() {
        assert_eq!(admit_sizes([10.0, 6.0], 16), Ok(16));
        assert_eq!(
            admit_sizes([10.0, 7.0], 16),
            Err(DialogError::TooLarge {
                actual: 17,
                limit: 16
            })
        );
        assert_eq!(
            admit_sizes([f64::MAX, 1.0], u64::MAX - 1),
            Err(DialogError::TooLarge {
                actual: u64::MAX,
                limit: u64::MAX - 1
            }),
            "a huge declared size saturates instead of wrapping"
        );
        assert_eq!(admit_sizes([], 0), Ok(0));
        assert_eq!(
            admit_read(5, 4),
            Err(DialogError::TooLarge {
                actual: 5,
                limit: 4
            })
        );
    }

    #[test]
    fn a_dismissed_picker_is_a_cancel_and_other_rejections_are_failures() {
        assert_eq!(picker_rejection(BrowserException::Abort), None);
        assert_eq!(
            picker_rejection(BrowserException::Security),
            Some(DialogError::Failed(BrowserException::Security.into()))
        );
    }

    #[test]
    fn a_page_opens_apart_and_any_other_scheme_is_handed_off() {
        assert_eq!(link_target("https"), LinkTarget::NewContext);
        assert_eq!(link_target("HTTP"), LinkTarget::NewContext);
        for scheme in ["mailto", "tel", "sms", "myapp"] {
            assert_eq!(link_target(scheme), LinkTarget::HandOff, "{scheme}");
        }
    }
}
