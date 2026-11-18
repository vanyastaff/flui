//! What the web providers decide without a page: how a dialog's filters
//! become the browser's, what a download is named, where a launched URI
//! goes, which answer a message box's button is, and what a browser's
//! exception means. Kept apart from the DOM so the rules are tested on every
//! host.

use flui_platform::{BrowserException, Unsupported, UnsupportedReason};

use crate::dialog::{DialogError, FileDialogCapability, FileFilter, MessageAnswer, MessageButtons};
use crate::share::{ShareCapability, ShareError, ShareResult};

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

/// How a picker of the File System Access API ended without picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickerEnd {
    /// The user dismissed it (`AbortError`, which the API also answers for a
    /// picker the browser closed).
    Cancelled,
    /// The document may not show the API's picker (`SecurityError`: a
    /// cross-origin frame) while the page still handles the gesture: the
    /// file input, which such a frame may show, takes over.
    FallBack,
    /// The answer: a refusal of the gesture is
    /// [`Unavailable`](UnsupportedReason::Unavailable), as the provider's
    /// own check answers it, and anything else the failure the browser
    /// named.
    Refused(DialogError),
}

/// Whether the browser refused a call for want of a user's gesture, or of a
/// document allowed to make it: `NotAllowedError`, `SecurityError`.
pub(crate) fn is_activation_refusal(exception: BrowserException) -> bool {
    matches!(
        exception,
        BrowserException::NotAllowed | BrowserException::Security
    )
}

/// How an open picker ended on `exception`, the page still handling the
/// gesture when `activated`.
pub(crate) fn picker_rejection(exception: BrowserException, activated: bool) -> PickerEnd {
    match exception {
        BrowserException::Abort => PickerEnd::Cancelled,
        BrowserException::Security if activated => PickerEnd::FallBack,
        refusal if is_activation_refusal(refusal) => PickerEnd::Refused(unavailable()),
        other => PickerEnd::Refused(DialogError::Failed(other.into())),
    }
}

/// The dialog's answer to a call made, or refused, outside a gesture.
pub(crate) fn unavailable() -> DialogError {
    DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(
        UnsupportedReason::Unavailable,
    ))
}

/// Admit picks, each a name and the MIME type the browser gives it, against
/// the request's `filters`: every pick must match one where there are any
/// ([`FileFilter::matches`]), since a file input only suggests them
/// (ADR-0360 §4); [`DialogError::NotInFilter`] otherwise.
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

/// Where a launched URI goes in the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchRoute {
    /// A new browsing context with no opener and no referrer: a page of
    /// `http` or `https`, which would otherwise replace the application.
    NewContext,
    /// Handed to the system's handler by a link the page follows: a scheme
    /// a browser does not fetch (`mailto`, `tel`, an application's own),
    /// which the HTML standard hands off without unloading the page.
    HandOff,
    /// Nothing a browser opens for the user: `ftp`, which browsers no longer
    /// fetch, and `ws` and `wss`, which only a page's script connects to.
    NoHandler,
}

/// Where a URI of `scheme` goes. The seam admits no other fetch scheme
/// (`about`, `blob`, `data` and `file` are refused there), so `http` and
/// `https` are the only pages.
pub(crate) fn launch_route(scheme: &str) -> LaunchRoute {
    let is = |name: &str| scheme.eq_ignore_ascii_case(name);
    if is("https") || is("http") {
        LaunchRoute::NewContext
    } else if is("ftp") || is("ws") || is("wss") {
        LaunchRoute::NoHandler
    } else {
        LaunchRoute::HandOff
    }
}

/// What the page's frame lacks when the browser refuses a share made while
/// it said the page handled a gesture: the Web Share permissions policy,
/// which a cross-origin frame has only when its embedder grants it.
pub(crate) const SHARE_POLICY: &str = "the `web-share` permissions policy (`allow=\"web-share\"`)";

/// Whether the Web Share API takes a link of `scheme`: one of the web.
pub(crate) fn is_shareable_link(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
}

/// What a share's rejection means, `saw_activation` saying whether the
/// browser itself reported that the page handled a gesture at the call.
///
/// - The user closing the share sheet (`AbortError`) is not reported, as on
///   every system that does not tell (`ShareResult`).
/// - A refusal for want of a gesture or an allowed document is
///   [`NotDeclared`](UnsupportedReason::NotDeclared) ([`SHARE_POLICY`]) when
///   the browser had said the gesture was there: only the embedder can grant
///   the frame the policy, and no retry changes it. Without that word it is
///   [`Unavailable`](UnsupportedReason::Unavailable), as the provider's own
///   check answers: a later gesture may succeed.
/// - A share while one is still showing (`InvalidStateError`) has no busy
///   answer in the contract; it is the failure the browser named, as is
///   anything else.
pub(crate) fn share_rejection(exception: BrowserException, saw_activation: bool) -> ShareResult {
    let refused = |reason| {
        Err(ShareError::Unsupported(Unsupported::of::<ShareCapability>(
            reason,
        )))
    };
    match exception {
        BrowserException::Abort => Ok(()),
        refusal if is_activation_refusal(refusal) && saw_activation => {
            refused(UnsupportedReason::NotDeclared { what: SHARE_POLICY })
        }
        refusal if is_activation_refusal(refusal) => refused(UnsupportedReason::Unavailable),
        other => Err(ShareError::Failed(other.into())),
    }
}

/// A message box button: the answer it gives, the value its `<button>`
/// returns to the `<dialog>`, and its label.
pub(crate) struct BoxButton {
    pub(crate) answer: MessageAnswer,
    pub(crate) value: &'static str,
    pub(crate) label: &'static str,
}

const OK: BoxButton = BoxButton {
    answer: MessageAnswer::Ok,
    value: "ok",
    label: "OK",
};
const CANCEL: BoxButton = BoxButton {
    answer: MessageAnswer::Cancel,
    value: "cancel",
    label: "Cancel",
};
const YES: BoxButton = BoxButton {
    answer: MessageAnswer::Yes,
    value: "yes",
    label: "Yes",
};
const NO: BoxButton = BoxButton {
    answer: MessageAnswer::No,
    value: "no",
    label: "No",
};

/// The buttons a box of `buttons` shows, in order; the first takes focus.
pub(crate) fn box_buttons(buttons: MessageButtons) -> &'static [BoxButton] {
    match buttons {
        MessageButtons::OkCancel => &[OK, CANCEL],
        MessageButtons::YesNo => &[YES, NO],
        MessageButtons::YesNoCancel => &[YES, NO, CANCEL],
        MessageButtons::Ok => &[OK],
    }
}

/// The answer a box of `buttons` gives when its `<dialog>` closes with
/// `value` as its `returnValue`: the button's, or, closed without one (the
/// Escape key, the page closing it), what closing means for the buttons.
/// `None` for a box with no way out but an answer ([`MessageButtons::YesNo`]),
/// which is shown again.
pub(crate) fn box_answer(buttons: MessageButtons, value: &str) -> Option<MessageAnswer> {
    let shown = box_buttons(buttons);
    if let Some(button) = shown.iter().find(|button| button.value == value) {
        return Some(button.answer);
    }
    if shown
        .iter()
        .any(|button| button.answer == MessageAnswer::Cancel)
    {
        Some(MessageAnswer::Cancel)
    } else if shown
        .iter()
        .all(|button| button.answer == MessageAnswer::Ok)
    {
        Some(MessageAnswer::Ok)
    } else {
        None
    }
}

/// Whether a box of `buttons` keeps the Escape key from closing it: one
/// with no way out but an answer.
pub(crate) fn box_holds_escape(buttons: MessageButtons) -> bool {
    box_answer(buttons, "").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_rejection_is_not_reported_retryable_lasting_or_the_browsers_failure() {
        let refused = |reason| {
            Err(ShareError::Unsupported(Unsupported::of::<ShareCapability>(
                reason,
            )))
        };
        let policy = UnsupportedReason::NotDeclared { what: SHARE_POLICY };
        let cases = [
            (BrowserException::Abort, true, Ok(())),
            (BrowserException::Abort, false, Ok(())),
            (BrowserException::NotAllowed, true, refused(policy)),
            (BrowserException::Security, true, refused(policy)),
            (
                BrowserException::NotAllowed,
                false,
                refused(UnsupportedReason::Unavailable),
            ),
            (
                BrowserException::Security,
                false,
                refused(UnsupportedReason::Unavailable),
            ),
            (
                BrowserException::InvalidState,
                true,
                Err(ShareError::Failed(BrowserException::InvalidState.into())),
            ),
            (
                BrowserException::Type,
                true,
                Err(ShareError::Failed(BrowserException::Type.into())),
            ),
        ];
        for (exception, saw_activation, answer) in cases {
            assert_eq!(
                share_rejection(exception, saw_activation),
                answer,
                "{exception} with the gesture reported: {saw_activation}"
            );
        }
    }

    #[test]
    fn a_page_shares_a_link_of_the_web_only() {
        for (scheme, shareable) in [
            ("https", true),
            ("HTTP", true),
            ("mailto", false),
            ("tel", false),
            ("myapp", false),
        ] {
            assert_eq!(is_shareable_link(scheme), shareable, "{scheme}");
        }
    }

    #[test]
    fn a_closed_box_answers_what_closing_means_for_its_buttons() {
        let cases = [
            (MessageButtons::Ok, "ok", Some(MessageAnswer::Ok)),
            (MessageButtons::Ok, "", Some(MessageAnswer::Ok)),
            (MessageButtons::OkCancel, "ok", Some(MessageAnswer::Ok)),
            (MessageButtons::OkCancel, "", Some(MessageAnswer::Cancel)),
            (MessageButtons::YesNo, "yes", Some(MessageAnswer::Yes)),
            (MessageButtons::YesNo, "no", Some(MessageAnswer::No)),
            (MessageButtons::YesNo, "", None),
            (MessageButtons::YesNo, "cancel", None),
            (MessageButtons::YesNoCancel, "no", Some(MessageAnswer::No)),
            (MessageButtons::YesNoCancel, "", Some(MessageAnswer::Cancel)),
            (MessageButtons::Ok, "yes", Some(MessageAnswer::Ok)),
        ];
        for (buttons, value, answer) in cases {
            assert_eq!(box_answer(buttons, value), answer, "{buttons:?} {value:?}");
            if let Some(answer) = answer {
                assert!(buttons.admits(answer), "{buttons:?} admits {answer:?}");
            }
        }
        assert!(box_holds_escape(MessageButtons::YesNo));
        assert!(!box_holds_escape(MessageButtons::YesNoCancel));
        assert_eq!(
            box_buttons(MessageButtons::YesNoCancel)
                .iter()
                .map(|button| button.label)
                .collect::<Vec<&str>>(),
            ["Yes", "No", "Cancel"]
        );
    }

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
    fn a_picker_ends_cancelled_handed_to_the_input_or_with_one_refusal_of_the_gesture() {
        let cases = [
            (BrowserException::Abort, true, PickerEnd::Cancelled),
            (BrowserException::Abort, false, PickerEnd::Cancelled),
            (BrowserException::Security, true, PickerEnd::FallBack),
            (
                BrowserException::Security,
                false,
                PickerEnd::Refused(unavailable()),
            ),
            (
                BrowserException::NotAllowed,
                true,
                PickerEnd::Refused(unavailable()),
            ),
            (
                BrowserException::NotFound,
                true,
                PickerEnd::Refused(DialogError::Failed(BrowserException::NotFound.into())),
            ),
        ];
        for (exception, activated, end) in cases {
            assert_eq!(
                picker_rejection(exception, activated),
                end,
                "{exception} activated: {activated}"
            );
        }
    }

    /// A pick as the browser describes it: its name and its MIME type.
    type Described<'a> = (&'a str, Option<&'a str>);

    #[test]
    fn a_pick_the_filters_do_not_mean_refuses_the_whole_answer() {
        let filters = [images(), FileFilter::new("Text").with_mime_type("text/*")];
        let cases: [(&[Described<'_>], Result<(), DialogError>); 7] = [
            (&[("photo.PNG", None)], Ok(())),
            (&[("photo.jpg", Some(""))], Ok(())),
            (&[("anything", Some("image/webp"))], Ok(())),
            (&[("notes", Some("TEXT/plain; charset=utf-8"))], Ok(())),
            (
                &[("photo.png", None), ("run.exe", None)],
                Err(DialogError::NotInFilter),
            ),
            (&[(".png", None)], Err(DialogError::NotInFilter)),
            (
                &[("archive.png.zip", Some("application/zip"))],
                Err(DialogError::NotInFilter),
            ),
        ];
        for (picks, admitted) in cases {
            assert_eq!(
                admit_picks(picks.iter().copied(), &filters),
                admitted,
                "{picks:?}"
            );
        }
        assert_eq!(
            admit_picks([("run.exe", None)], &[]),
            Ok(()),
            "no filter means every file"
        );
    }

    #[test]
    fn a_page_opens_apart_a_handler_takes_its_scheme_and_a_socket_has_none() {
        let cases = [
            ("https", LaunchRoute::NewContext),
            ("HTTP", LaunchRoute::NewContext),
            ("mailto", LaunchRoute::HandOff),
            ("tel", LaunchRoute::HandOff),
            ("myapp", LaunchRoute::HandOff),
            ("ftp", LaunchRoute::NoHandler),
            ("ws", LaunchRoute::NoHandler),
            ("WSS", LaunchRoute::NoHandler),
        ];
        for (scheme, route) in cases {
            assert_eq!(launch_route(scheme), route, "{scheme}");
        }
    }
}
