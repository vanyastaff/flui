//! What a dialog is asked for, and what it answers.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flui_platform::data_transfer::TransferLimits;

/// The longest title, button label or message a dialog shows, in bytes.
///
/// Checked with the rest of a request, before the system is asked, so one
/// limit holds on every host: Windows takes text up to its first NUL and
/// D-Bus refuses one, so a NUL in any of them is refused as well.
pub const MAX_TEXT_LEN: usize = 4096;

/// Whether `text` is a line a dialog can show as written: at most
/// [`MAX_TEXT_LEN`] bytes and no control character (a title, a label).
pub(super) fn is_line(text: &str) -> bool {
    text.len() <= MAX_TEXT_LEN && !text.chars().any(char::is_control)
}

/// Whether `text` is a body a dialog can show as written: at most
/// [`MAX_TEXT_LEN`] bytes and no NUL; line breaks are kept.
pub(super) fn is_body(text: &str) -> bool {
    text.len() <= MAX_TEXT_LEN && !text.contains('\0')
}

/// The first title or label of a request that is not a line.
fn check_lines<'a>(
    lines: impl IntoIterator<Item = Option<&'a str>>,
) -> Result<(), MalformedRequest> {
    if lines.into_iter().flatten().all(is_line) {
        Ok(())
    } else {
        Err(MalformedRequest::Text)
    }
}

/// A named set of files an open or save dialog offers to filter by: "Images"
/// with `png` and `jpg`.
///
/// One meaning on every host (ADR-0360): a file matches when its name ends
/// in `.` and one of the extensions, compared ignoring ASCII case
/// (`PHOTO.PNG` matches `png`, `a.tar.gz` matches `tar.gz`), or when its type
/// is one of the MIME types. A host whose dialog matches otherwise maps each
/// pattern to its own, or refuses the request. The portal is the one host
/// that falls short: its backends match globs case-sensitively, so it sends
/// each extension in lower and in upper case, and a name in mixed case
/// (`Photo.Png`) on a case-sensitive file system is not shown.
///
/// Patterns are kept as the dialog will compare them: an extension without
/// its leading `.` and a MIME type in ASCII lower case, and one added twice
/// is kept once. The rest is checked when the request is shown, before the
/// system is asked or anything is copied for it:
/// [`FileDialogs`](super::FileDialogs) refuses a malformed filter, or too
/// many filters or patterns, with
/// [`DialogError::InvalidRequest`](super::DialogError::InvalidRequest)
/// naming what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileFilter {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
}

impl FileFilter {
    /// The longest extension or MIME type a filter takes, in bytes: a file
    /// name is at most 255 bytes on the systems FLUI supports, and RFC 6838
    /// §4.2 bounds a MIME type's two names at 127 bytes each.
    pub const MAX_PATTERN_LEN: usize = 255;

    /// The longest name a filter shows, in bytes: a dialog shows one line.
    pub const MAX_NAME_LEN: usize = 255;

    /// The most extensions and MIME types one filter holds, together.
    pub const MAX_PATTERNS: usize = 64;

    /// The most filters one request offers.
    pub const MAX_PER_REQUEST: usize = 64;

    /// A filter named `name` that matches nothing until extensions or MIME
    /// types are added.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            extensions: Vec::new(),
            mime_types: Vec::new(),
        }
    }

    /// Also match files whose name ends in `.extension`, ignoring ASCII
    /// case. A leading `.` is dropped; an extension may have several parts
    /// (`tar.gz`), each of ASCII letters, digits, `-`, `_` and `+`, so no
    /// host reads one of its characters as a wildcard or a separator.
    #[must_use]
    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        let mut extension: String = extension.into();
        if extension.starts_with('.') {
            extension.remove(0);
        }
        extension.make_ascii_lowercase();
        if !self.extensions.contains(&extension) {
            self.extensions.push(extension);
        }
        self
    }

    /// Also match files of the MIME type `mime_type`, such as `image/png`,
    /// or of every subtype of a type, such as `image/*`. Compared ignoring
    /// ASCII case, as RFC 2045 §5.1 compares them; no parameters.
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        let mut mime_type: String = mime_type.into();
        mime_type.make_ascii_lowercase();
        if !self.mime_types.contains(&mime_type) {
            self.mime_types.push(mime_type);
        }
        self
    }

    /// The name the dialog shows.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The extensions it matches, without their `.`, in ASCII lower case.
    #[must_use]
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// The MIME types it matches, in ASCII lower case.
    #[must_use]
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// Whether a file named `name`, of the MIME type `mime_type` where the
    /// system says, is one this filter means: its name is longer than and
    /// ends in `.` and one of the extensions, ignoring ASCII case, or its
    /// type (without parameters, ignoring ASCII case) is one of the MIME
    /// types, `image/*` meaning every image type.
    ///
    /// What a host checks a pick against when its dialog only suggests the
    /// filters (ADR-0360 §4): a browser's file input offers every file
    /// beside them.
    #[must_use]
    pub fn matches(&self, name: &str, mime_type: Option<&str>) -> bool {
        let by_name = self.names(name.as_bytes());
        let by_type = mime_type
            .map(|mime_type| mime_type.split(';').next().unwrap_or_default().trim())
            .filter(|mime_type| !mime_type.is_empty())
            .is_some_and(|mime_type| {
                self.mime_types
                    .iter()
                    .any(|accepted| match accepted.strip_suffix("/*") {
                        Some(kind) => mime_type
                            .split_once('/')
                            .is_some_and(|(given, _)| given.eq_ignore_ascii_case(kind)),
                        None => mime_type.eq_ignore_ascii_case(accepted),
                    })
            });
        by_name || by_type
    }

    /// Whether a file named `name`, in the bytes its system keeps names in,
    /// is one this filter means by its name: the name half of
    /// [`matches`](Self::matches), for a name that need not be UTF-8.
    pub(crate) fn names(&self, name: &[u8]) -> bool {
        self.extensions
            .iter()
            .any(|extension| name_ends_in(name, extension))
    }

    /// The first thing wrong with the filter: its name, matching nothing, or
    /// an extension or MIME type that is not one.
    fn check(&self) -> Result<(), MalformedRequest> {
        if self.name.len() > Self::MAX_NAME_LEN
            || self.name.trim().is_empty()
            || self.name.chars().any(char::is_control)
        {
            return Err(MalformedRequest::FilterName);
        }
        if self.extensions.is_empty() && self.mime_types.is_empty() {
            return Err(MalformedRequest::EmptyFilter);
        }
        if self.extensions.len() + self.mime_types.len() > Self::MAX_PATTERNS {
            return Err(MalformedRequest::TooManyPatterns);
        }
        if !self
            .extensions
            .iter()
            .all(|extension| is_extension(extension))
        {
            return Err(MalformedRequest::Extension);
        }
        if !self
            .mime_types
            .iter()
            .all(|mime_type| is_mime_type(mime_type))
        {
            return Err(MalformedRequest::MimeType);
        }
        Ok(())
    }
}

/// Whether `name` is longer than, and ends in, `.` and `extension`, ignoring
/// ASCII case: the one rule a name is matched by (ADR-0360 §3). `name` is a
/// file name in the bytes its system keeps it in; an extension is ASCII, so
/// the rule reads no character of a name that is not UTF-8.
fn name_ends_in(name: &[u8], extension: &str) -> bool {
    let extension = extension.as_bytes();
    name.len()
        .checked_sub(extension.len())
        .filter(|tail| *tail > 1)
        .is_some_and(|tail| name[tail - 1] == b'.' && name[tail..].eq_ignore_ascii_case(extension))
}

/// Whether `filters` admit every one of `picks` as far as its name says:
/// the check [`FileDialogs::open`](super::FileDialogs::open) makes of every
/// backend's answer.
///
/// A request without filters admits every pick. So does one with a MIME
/// type among its filters: a pick's type is its system's to tell, and the
/// backend that mapped the pattern judges it. Otherwise each pick's name
/// must be one a filter [names](FileFilter::names), and a pick with no name
/// (a root) is one none does.
pub(super) fn names_admit(filters: &[FileFilter], picks: &Picks) -> bool {
    if filters.is_empty() || filters.iter().any(|filter| !filter.mime_types.is_empty()) {
        return true;
    }
    picks.iter().all(|pick| {
        pick.file_name().is_some_and(|name| {
            let name = name.as_encoded_bytes();
            filters.iter().any(|filter| filter.names(name))
        })
    })
}

/// The name a save's file is given where the provider names it from the
/// request rather than from what the user typed into a dialog (a browser's
/// save picker or download, an export to iOS's Files): `suggested`, or
/// `unnamed` when there is none, with the first filter's first extension
/// added when the name has none, as a desktop save dialog names a typed file
/// after the selected filter.
#[cfg(any(target_arch = "wasm32", target_os = "ios", test))]
pub(crate) fn save_file_name(
    suggested: Option<&str>,
    filters: &[FileFilter],
    unnamed: &str,
) -> String {
    let name = suggested.unwrap_or(unnamed);
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

/// Refuse `read` bytes of picks past `limit`, an open's
/// [`max_bytes`](OpenRequest::max_bytes), with
/// [`DialogError::TooLarge`](super::DialogError::TooLarge): what a provider
/// that hands over a pick's contents checks before it reads, by the sizes
/// the system declares, and again by the bytes it read.
#[cfg(any(target_arch = "wasm32", target_os = "ios", test))]
pub(crate) fn admit_read(read: u64, limit: u64) -> Result<(), super::DialogError> {
    if read > limit {
        Err(super::DialogError::TooLarge {
            actual: read,
            limit,
        })
    } else {
        Ok(())
    }
}

/// Whether `extension` is one: at most [`FileFilter::MAX_PATTERN_LEN`]
/// bytes, parts of ASCII letters, digits, `-`, `_` and `+` joined by single
/// dots.
fn is_extension(extension: &str) -> bool {
    extension.len() <= FileFilter::MAX_PATTERN_LEN
        && extension.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
        })
}

/// Whether `mime_type` is one: at most [`FileFilter::MAX_PATTERN_LEN`]
/// bytes, a type and a subtype that are RFC 6838 §4.2 names, or a type and
/// `*`.
fn is_mime_type(mime_type: &str) -> bool {
    let is_name = |name: &str| {
        let mut characters = name.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
            && characters.all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
    };
    mime_type.len() <= FileFilter::MAX_PATTERN_LEN
        && mime_type
            .split_once('/')
            .is_some_and(|(kind, subtype)| is_name(kind) && (subtype == "*" || is_name(subtype)))
}

/// The first thing wrong with `filters`: a malformed filter, or two that
/// show one name, which the user could not tell apart and which a system
/// that reports the selected filter by its value (the portal) could not
/// tell apart either.
fn check_filters(filters: &[FileFilter]) -> Result<(), MalformedRequest> {
    if filters.len() > FileFilter::MAX_PER_REQUEST {
        return Err(MalformedRequest::TooManyFilters);
    }
    for (index, filter) in filters.iter().enumerate() {
        filter.check()?;
        let name = filter.name.trim();
        if filters[..index]
            .iter()
            .any(|earlier| earlier.name.trim() == name)
        {
            return Err(MalformedRequest::DuplicateFilterName);
        }
    }
    Ok(())
}

/// What is wrong with a dialog request that
/// [`FileDialogs`](super::FileDialogs) refuses before the system is asked:
/// the reason [`DialogError::InvalidRequest`](super::DialogError::InvalidRequest)
/// carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedRequest {
    /// A filter's name is blank, longer than [`FileFilter::MAX_NAME_LEN`],
    /// or holds a control character.
    #[error("a filter has no name to show")]
    FilterName,
    /// A filter has neither an extension nor a MIME type.
    #[error("a filter matches nothing")]
    EmptyFilter,
    /// A filter holds more than [`FileFilter::MAX_PATTERNS`] extensions and
    /// MIME types.
    #[error("a filter holds too many patterns")]
    TooManyPatterns,
    /// A request offers more than [`FileFilter::MAX_PER_REQUEST`] filters.
    #[error("the request offers too many filters")]
    TooManyFilters,
    /// An extension is empty, longer than [`FileFilter::MAX_PATTERN_LEN`],
    /// or holds a character outside ASCII letters, digits, `-`, `_`, `+` and
    /// the single dots between its parts (a wildcard, a separator).
    #[error("an extension is not one")]
    Extension,
    /// A MIME type is not a type and a subtype (or `*`) of RFC 6838 names,
    /// or is longer than [`FileFilter::MAX_PATTERN_LEN`].
    #[error("a MIME type is not one")]
    MimeType,
    /// Two filters of one request show the same name.
    #[error("two filters show the same name")]
    DuplicateFilterName,
    /// A folder dialog was given filters: it shows no files to filter.
    #[error("a folder dialog takes no filters")]
    FiltersOnFolders,
    /// The suggested name is blank, `.` or `..`, or holds a path separator
    /// or a control character: a name, not a path.
    #[error("the suggested name is not a file name")]
    SuggestedName,
    /// The folder the dialog opens in is relative.
    #[error("the dialog's folder is not absolute")]
    RelativeDirectory,
    /// A title or a button label is longer than [`MAX_TEXT_LEN`] or holds a
    /// control character, or a message is longer than [`MAX_TEXT_LEN`] or
    /// holds a NUL: a host would cut it short or refuse it.
    #[error("a title, label or message cannot be shown as written")]
    Text,
    /// A message box has no message to show.
    #[error("the message box has no message")]
    EmptyMessage,
}

/// What an open dialog picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Files,
    Folders,
}

/// An open dialog: files or folders, one or several.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenRequest {
    pick: Pick,
    multiple: bool,
    /// The most bytes the picks may hold together where the system hands
    /// over a pick's contents rather than its path (a browser, iOS), as
    /// [`max_bytes`](Self::max_bytes) sets it.
    pub max_bytes: u64,
    /// The dialog's title, if set.
    pub title: Option<String>,
    /// The folder the dialog opens in, if set; it must be absolute.
    pub directory: Option<PathBuf>,
    /// The filters offered, the selected one first. Only a file dialog takes filters.
    pub filters: Vec<FileFilter>,
    /// The accept button's label, if set.
    pub accept_label: Option<String>,
}

impl OpenRequest {
    /// Pick one existing file.
    #[must_use]
    pub fn files() -> Self {
        Self::picking(Pick::Files)
    }

    /// Pick one existing folder.
    #[must_use]
    pub fn folders() -> Self {
        Self::picking(Pick::Folders)
    }

    fn picking(pick: Pick) -> Self {
        Self {
            pick,
            multiple: false,
            max_bytes: TransferLimits::default().max_bytes,
            title: None,
            directory: None,
            filters: Vec::new(),
            accept_label: None,
        }
    }

    /// Let the user pick several.
    #[must_use]
    pub fn allow_multiple(mut self) -> Self {
        self.multiple = true;
        self
    }

    /// The dialog's title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The folder the dialog opens in.
    #[must_use]
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Offer `filter`; the first filter added is selected. Only a file
    /// dialog takes filters.
    #[must_use]
    pub fn filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// The label of the button that accepts the choice.
    #[must_use]
    pub fn accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
        self
    }

    /// Read at most `max_bytes` of the picks, together, where the system
    /// hands over a pick's contents rather than its path (a browser, iOS): the
    /// picks' sizes are checked against it before anything is read, and
    /// more is answered with
    /// [`DialogError::TooLarge`](super::DialogError::TooLarge). The default
    /// is [`TransferLimits`]' default, the bound a drop's payload has; a
    /// request that expects larger files opts in here. A pick at a path is
    /// not read by the dialog, so this bounds nothing there.
    #[must_use]
    pub fn max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Whether it picks folders rather than files.
    #[must_use]
    pub fn picks_folders(&self) -> bool {
        self.pick == Pick::Folders
    }

    /// Whether the user may pick several.
    #[must_use]
    pub fn is_multiple(&self) -> bool {
        self.multiple
    }

    /// The first thing wrong with the request, which no dialog is shown
    /// for.
    pub(super) fn check(&self) -> Result<(), MalformedRequest> {
        if self.picks_folders() && !self.filters.is_empty() {
            return Err(MalformedRequest::FiltersOnFolders);
        }
        check_filters(&self.filters)?;
        check_lines([self.title.as_deref(), self.accept_label.as_deref()])?;
        check_directory(self.directory.as_deref())
    }
}

/// A dialog's folder is absolute, when it has one.
fn check_directory(directory: Option<&Path>) -> Result<(), MalformedRequest> {
    if directory.is_none_or(Path::is_absolute) {
        Ok(())
    } else {
        Err(MalformedRequest::RelativeDirectory)
    }
}

/// A save dialog: the user picks a new or existing file to write, and the
/// dialog writes the request's [`contents`](Self::contents) there when it
/// carries them.
#[derive(Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaveRequest {
    /// What the save writes, if set ([`contents`](Self::contents)).
    pub contents: Option<Arc<[u8]>>,
    /// The dialog's title, if set.
    pub title: Option<String>,
    /// The folder the dialog opens in, if set; it must be absolute.
    pub directory: Option<PathBuf>,
    /// The file name the dialog proposes, if set: a name, not a path.
    pub suggested_name: Option<String>,
    /// The filters offered, the selected one first. Only a file dialog takes filters.
    pub filters: Vec<FileFilter>,
    /// The accept button's label, if set.
    pub accept_label: Option<String>,
}

impl SaveRequest {
    /// A save dialog with the system's defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialog's title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The folder the dialog opens in.
    #[must_use]
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// The file name the dialog proposes: a name, not a path.
    #[must_use]
    pub fn suggested_name(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }

    /// Offer `filter`; the first filter added is selected.
    #[must_use]
    pub fn filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// The label of the button that accepts the choice.
    #[must_use]
    pub fn accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
        self
    }

    /// Write `contents` to the file the user chooses before the dialog
    /// answers: the one way to save on a system that hands over no file to
    /// write later (a browser, which writes them through its save picker or
    /// as a download; iOS, whose picker places a file that exists), and the
    /// same save everywhere else, where the file is written off the owner
    /// thread once the user has chosen it. A save that fails to write
    /// answers the failure, not the location.
    #[must_use]
    pub fn contents(mut self, contents: impl Into<Arc<[u8]>>) -> Self {
        self.contents = Some(contents.into());
        self
    }

    /// The first thing wrong with the request, which no dialog is shown
    /// for.
    pub(super) fn check(&self) -> Result<(), MalformedRequest> {
        check_filters(&self.filters)?;
        if !self.suggested_name.as_deref().is_none_or(is_file_name) {
            return Err(MalformedRequest::SuggestedName);
        }
        check_lines([self.title.as_deref(), self.accept_label.as_deref()])?;
        check_directory(self.directory.as_deref())
    }
}

impl fmt::Debug for SaveRequest {
    /// The contents by their length: they are the user's.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveRequest")
            .field("title", &self.title)
            .field("directory", &self.directory)
            .field("suggested_name", &self.suggested_name)
            .field("filters", &self.filters)
            .field("accept_label", &self.accept_label)
            .field(
                "contents_len",
                &self.contents.as_ref().map(|contents| contents.len()),
            )
            .finish()
    }
}

/// Where the user chose to save: the file, and the filter that was
/// selected when they accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveLocation {
    file: PickedFile,
    filter: Option<usize>,
}

impl SaveLocation {
    /// The user chose `file` with the request's filter at `filter` selected;
    /// `None` when the request offered no filter or the system does not say.
    #[must_use]
    pub fn new(file: PickedFile, filter: Option<usize>) -> Self {
        Self { file, filter }
    }

    /// The file to write.
    #[must_use]
    pub fn file(&self) -> &PickedFile {
        &self.file
    }

    /// The index, in the request's [`filters`](SaveRequest::filters), of the
    /// filter selected when the user accepted: the format an application
    /// writes when the name the user typed has no extension. `None` when the
    /// request offered none or the system does not report it.
    #[must_use]
    pub fn filter(&self) -> Option<usize> {
        self.filter
    }

    /// The file to write, without the filter.
    #[must_use]
    pub fn into_file(self) -> PickedFile {
        self.file
    }
}

/// What the user picked in an open dialog: one file or folder at least,
/// several only when the request [allowed them](OpenRequest::allow_multiple).
///
/// Never empty, so an answer with nothing picked cannot read as a pick: the
/// user cancelling is `None`, not an empty list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picks {
    first: PickedFile,
    rest: Vec<PickedFile>,
}

impl Picks {
    /// `picks`, in the order the user picked them; `None` when there are
    /// none.
    #[must_use]
    pub fn try_new(picks: Vec<PickedFile>) -> Option<Self> {
        let mut picks = picks.into_iter();
        let first = picks.next()?;
        Some(Self {
            first,
            rest: picks.collect::<Vec<PickedFile>>(),
        })
    }

    /// The first pick: the only one of a dialog that picks one.
    #[must_use]
    pub fn first(&self) -> &PickedFile {
        &self.first
    }

    /// How many were picked; at least one.
    #[must_use]
    pub fn len(&self) -> usize {
        1 + self.rest.len()
    }

    /// Always `false`: there is at least one pick. Here for the `len`
    /// convention.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The picks, in order.
    pub fn iter(&self) -> impl Iterator<Item = &PickedFile> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

/// The picks, in the order the user picked them.
impl IntoIterator for Picks {
    type Item = PickedFile;
    type IntoIter = std::iter::Chain<std::iter::Once<PickedFile>, std::vec::IntoIter<PickedFile>>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(self.first).chain(self.rest)
    }
}

impl<'a> IntoIterator for &'a Picks {
    type Item = &'a PickedFile;
    type IntoIter =
        std::iter::Chain<std::iter::Once<&'a PickedFile>, std::slice::Iter<'a, PickedFile>>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

/// Whether `name` is a file name a dialog can hand out: not blank, not `.`
/// or `..`, no path separator and no control character.
fn is_file_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        && name != "."
        && name != ".."
}

/// A file or folder the user picked.
///
/// Its own type rather than a path, since not every system answers one: a
/// desktop dialog answers where the file is ([`path`](Self::path)), while a
/// browser answers a file a page may read but never a path to it, and iOS a
/// document the application may reach only while it holds the access the
/// picker granted; both are carried as the file's name and its contents
/// ([`contents`](Self::contents)). A later backend can carry the access a
/// sandbox grants with a pick (a security-scoped bookmark, a document portal
/// entry, a content URI) the same way, without changing what callers
/// receive.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PickedFile {
    place: Place,
}

/// Where a pick is: what the system answered for it.
#[derive(Clone, PartialEq, Eq)]
enum Place {
    /// A file at a path the application opens itself.
    Path(PathBuf),
    /// A file the system handed over: its name, and what it holds, read
    /// when the dialog answered (an open) or written before it did (a save).
    Contents { name: String, contents: Arc<[u8]> },
    /// Contents handed to the browser as a download under this name.
    Download { name: String },
}

/// What a [`PickedFile`] is, as the system answered it: the form a caller
/// branches on when it handles more than one system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickedForm<'a> {
    /// A file at a path, which the application reads or writes itself.
    Path(&'a Path),
    /// A file a browser or iOS handed over without a path: its name, and
    /// what it holds, read when an open dialog answered or written before a
    /// save answered.
    Contents {
        /// The name the system gave it.
        name: &'a str,
        /// What it holds.
        contents: &'a [u8],
    },
    /// A save's contents handed to the browser as a download under `name`.
    /// Where it was written, under which name, and whether it was written
    /// at all, the browser does not say: the user may cancel it, a setting
    /// may block it, and the browser renames it beside a file of that name.
    Download {
        /// The name the download was offered under.
        name: &'a str,
    },
}

impl PickedFile {
    /// The pick at `path`, as a backend reports it; `None` when `path` is
    /// relative or holds a NUL byte, which no file the system picked has
    /// and no file API accepts.
    #[must_use]
    pub fn try_new(path: impl Into<PathBuf>) -> Option<Self> {
        let path: PathBuf = path.into();
        let nul = path.as_os_str().as_encoded_bytes().contains(&0);
        (path.is_absolute() && !nul).then_some(Self {
            place: Place::Path(path),
        })
    }

    /// The file named `name` holding `contents`, as a system that hands
    /// over no path reports it (a browser, iOS); `None` when `name` is not a file
    /// name: blank, `.` or `..`, or holding a path separator or a control
    /// character.
    #[must_use]
    pub fn try_from_contents(name: impl Into<String>, contents: Arc<[u8]>) -> Option<Self> {
        let name: String = name.into();
        is_file_name(&name).then_some(Self {
            place: Place::Contents { name, contents },
        })
    }

    /// A save's contents handed to the browser as a download offered as
    /// `name` ([`PickedForm::Download`]); `None` when `name` is not a file
    /// name, as [`try_from_contents`](Self::try_from_contents) judges it.
    #[must_use]
    pub fn try_download(name: impl Into<String>) -> Option<Self> {
        let name: String = name.into();
        is_file_name(&name).then_some(Self {
            place: Place::Download { name },
        })
    }

    /// What it is, as the system answered it.
    #[must_use]
    pub fn form(&self) -> PickedForm<'_> {
        match &self.place {
            Place::Path(path) => PickedForm::Path(path),
            Place::Contents { name, contents } => PickedForm::Contents { name, contents },
            Place::Download { name } => PickedForm::Download { name },
        }
    }

    /// Where it is; `None` for a file the system handed over without a
    /// path (a browser's, iOS's), whose [`contents`](Self::contents) the pick
    /// holds instead, and for a download.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self.form() {
            PickedForm::Path(path) => Some(path),
            PickedForm::Contents { .. } | PickedForm::Download { .. } => None,
        }
    }

    /// Its name: the last component of its [`path`](Self::path), the name
    /// the system handed it over with, or the name a download was offered
    /// under. `None` for a path that ends in `..` or is a root.
    #[must_use]
    pub fn file_name(&self) -> Option<&OsStr> {
        match self.form() {
            PickedForm::Path(path) => path.file_name(),
            PickedForm::Contents { name, .. } | PickedForm::Download { name } => {
                Some(OsStr::new(name))
            }
        }
    }

    /// What the file holds, for a file the system handed over without a
    /// path: read when an open dialog answered, within the request's
    /// [`max_bytes`](OpenRequest::max_bytes), or written before a save
    /// answered. `None` for a pick at a path, which the application reads
    /// itself, and for a download, which nobody saw written.
    #[must_use]
    pub fn contents(&self) -> Option<&[u8]> {
        match self.form() {
            PickedForm::Contents { contents, .. } => Some(contents),
            PickedForm::Path(_) | PickedForm::Download { .. } => None,
        }
    }
}

impl fmt::Debug for PickedFile {
    /// The path or the name, and a handed-over file's length: its contents
    /// are the user's.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.place {
            Place::Path(path) => f.debug_struct("PickedFile").field("path", path).finish(),
            Place::Contents { name, contents } => f
                .debug_struct("PickedFile")
                .field("name", name)
                .field("len", &contents.len())
                .finish(),
            Place::Download { name } => f
                .debug_struct("PickedFile")
                .field("download", name)
                .finish(),
        }
    }
}
