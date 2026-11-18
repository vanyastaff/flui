//! What a dialog is asked for, and what it answers.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flui_platform::data_transfer::TransferLimits;

use crate::grant::Grant;

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
/// with `png` and `jpg`, "Build files" with `Makefile`, or "All files".
///
/// One meaning on every host (ADR-0360, ADR-0374): a file matches when its
/// name ends in `.` and one of the extensions (`PHOTO.PNG` matches `png`,
/// `a.tar.gz` matches `tar.gz`), when its whole name is one of the file
/// names (`makefile` matches `Makefile`), each compared ignoring ASCII case,
/// or when its type is one of the MIME types; a filter of
/// [every file](Self::all_files) matches each one. A host whose dialog
/// matches otherwise maps each pattern to its own, or refuses the request.
/// The portal is the one host that falls short: its backends match globs
/// case-sensitively, so it sends each extension in lower and in upper case,
/// and a name in mixed case (`Photo.Png`) on a case-sensitive file system is
/// not shown.
///
/// Patterns are kept as the dialog will compare them: an extension without
/// its leading `.` and a MIME type in ASCII lower case, a file name as it
/// was written, and one added twice is kept once. The rest is checked when
/// the request is shown, before the system is asked or anything is copied
/// for it: [`FileDialogs`](super::FileDialogs) refuses a malformed filter,
/// or too many filters or patterns, with
/// [`DialogError::InvalidRequest`](super::DialogError::InvalidRequest)
/// naming what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileFilter {
    name: String,
    kind: Kind,
}

/// What a filter matches by.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// Every file ([`FileFilter::all_files`]), by no pattern. A pattern
    /// added to it is not kept: that it was added is, and the request is
    /// refused for it ([`MalformedRequest::PatternOnAllFiles`]), since the
    /// filter would mean something other than what its author wrote.
    EveryFile { given_pattern: bool },
    /// The files these patterns name.
    Patterns {
        extensions: Vec<String>,
        file_names: Vec<String>,
        mime_types: Vec<String>,
    },
}

impl FileFilter {
    /// The longest extension, file name or MIME type a filter takes, in
    /// bytes: a file name is at most 255 bytes on the systems FLUI supports,
    /// and RFC 6838 §4.2 bounds a MIME type's two names at 127 bytes each.
    pub const MAX_PATTERN_LEN: usize = 255;

    /// The longest name a filter shows, in bytes: a dialog shows one line.
    pub const MAX_NAME_LEN: usize = 255;

    /// The most extensions, file names and MIME types one filter holds,
    /// together.
    pub const MAX_PATTERNS: usize = 64;

    /// The most filters one request offers.
    pub const MAX_PER_REQUEST: usize = 64;

    /// A filter named `name` that matches nothing until extensions, file
    /// names or MIME types are added.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: Kind::Patterns {
                extensions: Vec::new(),
                file_names: Vec::new(),
                mime_types: Vec::new(),
            },
        }
    }

    /// A filter named `name` (the application's word for "All files") that
    /// matches every file: what a request offers beside its specific
    /// filters so the user can reach a file none of them names.
    ///
    /// It takes no pattern: a request whose filter of every file was given
    /// an extension, a file name or a MIME type is refused with
    /// [`MalformedRequest::PatternOnAllFiles`]. A request with such a filter
    /// admits every pick, whichever filter was selected when the user
    /// accepted.
    #[must_use]
    pub fn all_files(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: Kind::EveryFile {
                given_pattern: false,
            },
        }
    }

    /// Add `pattern` to the list `list` picks out of the patterns, unless
    /// one that is the `same` is there already; a filter of every file
    /// records that it was given one.
    fn add(
        mut self,
        pattern: String,
        list: fn(&mut Kind) -> Option<&mut Vec<String>>,
        same: fn(&str, &str) -> bool,
    ) -> Self {
        match list(&mut self.kind) {
            Some(list) => {
                if !list.iter().any(|kept| same(kept, &pattern)) {
                    list.push(pattern);
                }
            }
            None => {
                self.kind = Kind::EveryFile {
                    given_pattern: true,
                };
            }
        }
        self
    }

    /// Also match files whose name ends in `.extension`, ignoring ASCII
    /// case. A leading `.` is dropped; an extension may have several parts
    /// (`tar.gz`), each of ASCII letters, digits, `-`, `_` and `+`, so no
    /// host reads one of its characters as a wildcard or a separator.
    #[must_use]
    pub fn with_extension(self, extension: impl Into<String>) -> Self {
        let mut extension: String = extension.into();
        if extension.starts_with('.') {
            extension.remove(0);
        }
        extension.make_ascii_lowercase();
        self.add(
            extension,
            |kind| match kind {
                Kind::Patterns { extensions, .. } => Some(extensions),
                Kind::EveryFile { .. } => None,
            },
            |kept, added| kept == added,
        )
    }

    /// Also match the file named exactly `file_name`, ignoring ASCII case:
    /// a name with no extension to match by (`Makefile`, `LICENSE`), or one
    /// that is nothing but a dot and a word (`.env`, `.gitignore`), which no
    /// extension matches. A name of ASCII letters, digits, `-`, `_`, `+` and
    /// `.`, not `.` or `..` and not ending in a dot, so no host reads one of
    /// its characters as a wildcard or a separator.
    ///
    /// Kept as written, since a host that compares names case-sensitively is
    /// sent that spelling; one added twice, in whatever case, is kept once.
    #[must_use]
    pub fn with_file_name(self, file_name: impl Into<String>) -> Self {
        self.add(
            file_name.into(),
            |kind| match kind {
                Kind::Patterns { file_names, .. } => Some(file_names),
                Kind::EveryFile { .. } => None,
            },
            str::eq_ignore_ascii_case,
        )
    }

    /// Also match files of the MIME type `mime_type`, such as `image/png`,
    /// or of every subtype of a type, such as `image/*`. Compared ignoring
    /// ASCII case, as RFC 2045 §5.1 compares them; no parameters.
    #[must_use]
    pub fn with_mime_type(self, mime_type: impl Into<String>) -> Self {
        let mut mime_type: String = mime_type.into();
        mime_type.make_ascii_lowercase();
        self.add(
            mime_type,
            |kind| match kind {
                Kind::Patterns { mime_types, .. } => Some(mime_types),
                Kind::EveryFile { .. } => None,
            },
            |kept, added| kept == added,
        )
    }

    /// The name the dialog shows.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether it matches every file ([`all_files`](Self::all_files)): the
    /// entry a host maps to its own "all files" (`*.*` on Windows, `*` for
    /// the portal) or to no restriction at all.
    #[must_use]
    pub fn is_all_files(&self) -> bool {
        matches!(self.kind, Kind::EveryFile { .. })
    }

    /// The extensions it matches, without their `.`, in ASCII lower case;
    /// none for a filter of every file.
    #[must_use]
    pub fn extensions(&self) -> &[String] {
        match &self.kind {
            Kind::Patterns { extensions, .. } => extensions,
            Kind::EveryFile { .. } => &[],
        }
    }

    /// The file names it matches exactly, as they were written; none for a
    /// filter of every file.
    #[must_use]
    pub fn file_names(&self) -> &[String] {
        match &self.kind {
            Kind::Patterns { file_names, .. } => file_names,
            Kind::EveryFile { .. } => &[],
        }
    }

    /// The MIME types it matches, in ASCII lower case; none for a filter of
    /// every file.
    #[must_use]
    pub fn mime_types(&self) -> &[String] {
        match &self.kind {
            Kind::Patterns { mime_types, .. } => mime_types,
            Kind::EveryFile { .. } => &[],
        }
    }

    /// Whether a file named `name`, of the MIME type `mime_type` where the
    /// system says, is one this filter means: the filter is one of
    /// [every file](Self::all_files); or its name is longer than and ends in
    /// `.` and one of the extensions, or is one of the file names, ignoring
    /// ASCII case; or its type (without parameters, ignoring ASCII case) is
    /// one of the MIME types, `image/*` meaning every image type.
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
                self.mime_types()
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
        match &self.kind {
            Kind::EveryFile { .. } => true,
            Kind::Patterns {
                extensions,
                file_names,
                ..
            } => {
                extensions
                    .iter()
                    .any(|extension| name_ends_in(name, extension))
                    || file_names
                        .iter()
                        .any(|file_name| name.eq_ignore_ascii_case(file_name.as_bytes()))
            }
        }
    }

    /// How a save completes a typed `name` with this filter selected and the
    /// dialog left it as typed: the length of the stem the filter's first
    /// extension follows, and that extension. `None` when the name stands:
    /// the filter has no extension, or names `name` already, or `name` has a
    /// dot of its own past any trailing ones, an extension (`report.v2`) or
    /// the leading dot of a hidden file's name (`.env`, as Windows' dialog
    /// leaves it). Trailing dots are dropped (`report.` is `report.txt`), and
    /// a name of dots alone has no stem.
    #[cfg(any(
        target_os = "linux",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(super) fn completion(&self, name: &[u8]) -> Option<(usize, &str)> {
        let extension = self.extensions().first()?;
        if self.names(name) {
            return None;
        }
        let trailing_dots = name.iter().rev().take_while(|byte| **byte == b'.').count();
        let stem = &name[..name.len() - trailing_dots];
        if stem.is_empty() || stem.contains(&b'.') {
            return None;
        }
        Some((stem.len(), extension))
    }

    /// The name a save writes when the user typed `name` with this filter
    /// selected and the dialog left it as typed: `name`'s stem with `.` and
    /// the filter's first extension, as `completion`
    /// says (`report` and `report.` become `report.txt`; `.env`, `report.v2`
    /// and a name the filter matches stay). `None` when the name stands.
    ///
    /// The rule a host applies itself when its dialog does not (the portal):
    /// Windows' and macOS's dialogs complete the name before they ask about
    /// replacing a file, which a rule applied after the answer cannot do.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn completes(&self, name: &[u8]) -> Option<Vec<u8>> {
        let (stem, extension) = self.completion(name)?;
        let mut completed = Vec::with_capacity(stem + 1 + extension.len());
        completed.extend_from_slice(&name[..stem]);
        completed.push(b'.');
        completed.extend_from_slice(extension.as_bytes());
        Some(completed)
    }

    /// The first thing wrong with the filter: its name, a pattern given to a
    /// filter of every file, matching nothing, or an extension, file name or
    /// MIME type that is not one.
    fn check(&self) -> Result<(), MalformedRequest> {
        if self.name.len() > Self::MAX_NAME_LEN
            || self.name.trim().is_empty()
            || self.name.chars().any(char::is_control)
        {
            return Err(MalformedRequest::FilterName);
        }
        let (extensions, file_names, mime_types) = match &self.kind {
            Kind::EveryFile {
                given_pattern: false,
            } => return Ok(()),
            Kind::EveryFile {
                given_pattern: true,
            } => return Err(MalformedRequest::PatternOnAllFiles),
            Kind::Patterns {
                extensions,
                file_names,
                mime_types,
            } => (extensions, file_names, mime_types),
        };
        let patterns = extensions.len() + file_names.len() + mime_types.len();
        if patterns == 0 {
            return Err(MalformedRequest::EmptyFilter);
        }
        if patterns > Self::MAX_PATTERNS {
            return Err(MalformedRequest::TooManyPatterns);
        }
        if !extensions.iter().all(|extension| is_extension(extension)) {
            return Err(MalformedRequest::Extension);
        }
        if !file_names
            .iter()
            .all(|file_name| is_file_name_pattern(file_name))
        {
            return Err(MalformedRequest::FileName);
        }
        if !mime_types.iter().all(|mime_type| is_mime_type(mime_type)) {
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
/// A request without filters admits every pick, and so does one that offers
/// a filter of [every file](FileFilter::all_files). So does one with a MIME
/// type among its filters: a pick's type is its system's to tell, and the
/// backend that mapped the pattern judges it. Otherwise each pick's name
/// must be one a filter [names](FileFilter::names), and a pick with no name
/// (a root) is one none does.
pub(super) fn names_admit(filters: &[FileFilter], picks: &Picks) -> bool {
    let unjudged_by_name =
        |filter: &FileFilter| filter.is_all_files() || !filter.mime_types().is_empty();
    if filters.is_empty() || filters.iter().any(unjudged_by_name) {
        return true;
    }
    picks.iter().all(|pick| {
        pick.file_name().is_some_and(|name| {
            let name = name.as_encoded_bytes();
            filters.iter().any(|filter| filter.names(name))
        })
    })
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

/// Whether `file_name` is a name a filter matches exactly: at most
/// [`FileFilter::MAX_PATTERN_LEN`] bytes of ASCII letters, digits, `-`, `_`,
/// `+` and `.`, not `.` or `..`, and not ending in a dot, which Windows
/// drops from a name.
fn is_file_name_pattern(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.len() <= FileFilter::MAX_PATTERN_LEN
        && !file_name.ends_with('.')
        && file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.'))
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
    /// A filter has no extension, file name or MIME type, and is not one of
    /// [every file](FileFilter::all_files).
    #[error("a filter matches nothing")]
    EmptyFilter,
    /// A filter of [every file](FileFilter::all_files) was given an
    /// extension, a file name or a MIME type, which it would not keep.
    #[error("a filter of every file takes no pattern")]
    PatternOnAllFiles,
    /// A filter holds more than [`FileFilter::MAX_PATTERNS`] extensions,
    /// file names and MIME types.
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
    /// A file name a filter matches exactly is empty, `.` or `..`, longer
    /// than [`FileFilter::MAX_PATTERN_LEN`], ends in a dot, or holds a
    /// character outside ASCII letters, digits, `-`, `_`, `+` and `.` (a
    /// wildcard, a separator, a space).
    #[error("a filter's file name is not one a dialog matches exactly")]
    FileName,
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
    /// control character, or a message or a message box's detail is longer
    /// than [`MAX_TEXT_LEN`] or holds a NUL: a host would cut it short or
    /// refuse it. A detail that is blank is refused the same way.
    #[error("a title, label or message cannot be shown as written")]
    Text,
    /// A message box has no message to show.
    #[error("the message box has no message")]
    EmptyMessage,
    /// A message box's default button is one its buttons do not offer.
    #[error("the default button is not one the message box offers")]
    DefaultButton,
    /// A message box labels a button its buttons do not offer, a label is
    /// blank, or two buttons are given one label, which the user could not
    /// tell apart.
    #[error("a button's label names no button of the box, is blank, or is another button's")]
    ButtonLabel,
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
    /// Whether the dialog shows the files its system hides
    /// ([`show_hidden`](Self::show_hidden)).
    pub show_hidden: bool,
    /// The most files a picked folder may hold where the system hands over
    /// the folder's files rather than its path (a browser), as
    /// [`max_entries`](Self::max_entries) sets it.
    pub max_entries: usize,
    /// Whether each pick carries access that outlasts the process
    /// ([`keep_access`](Self::keep_access)).
    pub keep_access: bool,
}

impl OpenRequest {
    /// The [`max_entries`](Self::max_entries) of a request that sets none:
    /// room for a project's own files. A request that expects a folder of
    /// dependencies or a photo library opts into more.
    pub const DEFAULT_MAX_ENTRIES: usize = 10_000;

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
            show_hidden: false,
            max_entries: Self::DEFAULT_MAX_ENTRIES,
            keep_access: false,
        }
    }

    /// Let the user pick several. A system whose chooser takes one at a
    /// time (a browser's folder chooser) still answers one.
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

    /// The folder the dialog opens in: an absolute path. The dialog opens
    /// there each time it is asked to, over the folder the system remembers
    /// from the user's last dialog; a request that sets none opens where the
    /// system would.
    ///
    /// A folder that is not there (a remembered folder deleted since, a
    /// drive unplugged, a share that does not answer) is left out on every
    /// host, and the dialog opens where the system would. Windows names the
    /// folder to its dialog, so one its shell cannot name for another reason
    /// answers [`DialogError::Failed`](super::DialogError::Failed) with the
    /// system's code and no dialog is shown; a verbatim path (`\\?\C:\…`,
    /// `\\?\UNC\server\share\…`, what `std::fs::canonicalize` answers) is
    /// named in the form the shell reads. The portal and the Apple pickers
    /// take the folder as a hint and open where the system would whenever
    /// they cannot use it.
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

    /// Show the files the system hides (dotfiles, files with the hidden
    /// attribute), whatever the user's own setting: for a dialog that picks
    /// a configuration file or a hidden folder. Without it the dialog shows
    /// what the user's setting says.
    ///
    /// Honoured where the system's dialog takes it: Windows
    /// (`FOS_FORCESHOWHIDDEN`) and macOS (`showsHiddenFiles`). The portal's
    /// file chooser and a browser's picker have no such option and keep the
    /// user's own toggle.
    #[must_use]
    pub fn show_hidden(mut self) -> Self {
        self.show_hidden = true;
        self
    }

    /// Take at most `max_bytes` of picks the system hands over by their
    /// contents rather than their path (a browser, iOS), together, a picked
    /// folder's files among them: more is answered with
    /// [`DialogError::TooLarge`](super::DialogError::TooLarge). The handle
    /// holds every backend's answer to it, and a backend that reads the
    /// picks checks the sizes the system declares before it reads anything.
    /// The default is [`TransferLimits`]' default, the bound a drop's
    /// payload has; a request that expects larger files opts in here. A
    /// pick at a path is not read by the dialog, so this bounds nothing
    /// there.
    #[must_use]
    pub fn max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Take a picked folder of at most `max_entries` files, at any depth,
    /// where the system hands over the folder's files rather than its path
    /// (a browser): a folder that holds more is answered with
    /// [`DialogError::TooManyEntries`](super::DialogError::TooManyEntries).
    /// The handle holds every backend's answer to it. A backend that walks
    /// the folder itself counts the folders it meets as well, so that the
    /// walk ends at the bound, and refuses before it reads a file. The
    /// default is [`DEFAULT_MAX_ENTRIES`](Self::DEFAULT_MAX_ENTRIES); the
    /// files' bytes are bounded by [`max_bytes`](Self::max_bytes). A folder
    /// at a path is not walked by the dialog, so this bounds nothing there.
    #[must_use]
    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Hand each pick over with a [`Grant`]: access the application stores
    /// and, after a restart, resolves through
    /// [`Grants::resolve`](crate::grant::Grants::resolve)
    /// ([`PickedFile::grant`]).
    ///
    /// What the grant is, each system's own form of it:
    ///
    /// - macOS: a security-scoped bookmark of the pick, made off the main
    ///   thread once the user accepted, which outlasts an App Sandbox's
    ///   access to the pick (that lasts for the process alone).
    /// - iOS: a bookmark made while the picker's access is held, as the
    ///   document is read.
    /// - Android: the content URI, whose read permission is persisted once
    ///   the picks were read; Android keeps a bounded number of them per
    ///   application and drops the oldest past it.
    /// - Linux: the path, which a Flatpak's document portal keeps for the
    ///   application across its runs.
    /// - Windows: the path, which a desktop process opens again, packaged
    ///   or not.
    ///
    /// A pick the system could not grant lasting access to (a bookmark that
    /// could not be made, a provider that offers no persistable permission)
    /// is handed over without one. A browser keeps nothing a page could
    /// store as bytes, so a web pick never carries a grant.
    #[must_use]
    pub fn keep_access(mut self) -> Self {
        self.keep_access = true;
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
    /// Whether the dialog shows the files its system hides
    /// ([`show_hidden`](Self::show_hidden)).
    pub show_hidden: bool,
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

    /// The folder the dialog opens in: an absolute path, used or refused as
    /// [`OpenRequest::directory`] says for each host.
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
    ///
    /// A name the user types without an extension is given one, by a rule
    /// each host has its own form of:
    ///
    /// - Windows: the dialog gives it the first filter's first extension
    ///   (its default extension), before it asks about replacing a file;
    ///   whether it follows a filter the user selected instead has not been
    ///   run.
    /// - macOS: the panel has no filter menu, so no filter is selected; it
    ///   gives the name the first allowed type's extension, the last part of
    ///   a several-part one (`gz` of `tar.gz`), and none when a filter names
    ///   a file exactly or admits every file, since the panel is then given
    ///   no types.
    /// - The portal: its provider gives it the first extension of the filter
    ///   the portal reports as selected, after the dialog answered
    ///   (the rule of `FileFilter::completes`), unless a file of the completed
    ///   name is already there, which the user was not asked about, or the
    ///   path is in the document portal's folder, where the portal granted
    ///   the name it answered.
    /// - A browser and iOS name the file from the request, the suggested
    ///   name completed by the first filter by the same rule.
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

    /// Show the files the system hides, whatever the user's own setting, as
    /// [`OpenRequest::show_hidden`] does and on the same hosts.
    #[must_use]
    pub fn show_hidden(mut self) -> Self {
        self.show_hidden = true;
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
            .field("show_hidden", &self.show_hidden)
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

/// Whether `name` names a file or a folder inside a folder: not empty, not
/// `.` or `..`, and without `/`, `\` or a NUL. The one rule a
/// [`FolderPath`]'s names and a handed-over folder's own name are held to,
/// the same on every host.
///
/// Wider than [`is_file_name`], which judges a name a dialog shows or
/// suggests: a folder holds what its file system allows, a blank name and a
/// control character among them.
fn is_name_in_folder(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Where a file is under a folder a system handed over without a path
/// ([`PickedForm::Folder`]): the names from the folder down to the file.
///
/// Each name is as the system gave it, held to one rule on every host: not
/// empty, not `.` or `..`, and without `/`, `\` or a NUL. So the path has
/// one name per folder passed and none that climbs out. Any other name a
/// file system allows is kept, a blank one or one with a control character
/// among them (macOS keeps a folder's own icon in a file named `Icon` and a
/// carriage return).
///
/// It is not a path of this host, and promises nothing about one made from
/// it: Windows reads `NUL` as a device and `C:` as a drive, and drops a
/// name's trailing dot or space. An application that writes a folder's
/// files to disk maps the names for its host first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPath {
    /// The names, joined by `/`; at least one.
    names: String,
}

impl FolderPath {
    /// The path `path` spells, its names separated by `/` as a browser
    /// reports them (a file's `webkitRelativePath` after its first name,
    /// the folder's); `None` when a name is not one a folder holds, as the
    /// type says.
    #[must_use]
    pub fn try_new(path: &str) -> Option<Self> {
        path.split('/').all(is_name_in_folder).then(|| Self {
            names: path.to_owned(),
        })
    }

    /// The path of the entry named `name` in the handed-over folder itself;
    /// `None` when `name` is not one name a folder holds.
    #[must_use]
    pub fn try_from_name(name: &str) -> Option<Self> {
        is_name_in_folder(name).then(|| Self {
            names: name.to_owned(),
        })
    }

    /// The path of the entry named `name` in the folder at this path; `None`
    /// when `name` is not one name a folder holds.
    #[must_use]
    pub fn try_child(&self, name: &str) -> Option<Self> {
        is_name_in_folder(name).then(|| Self {
            names: format!("{}/{name}", self.names),
        })
    }

    /// The names, joined by `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.names
    }

    /// The names, from the one in the handed-over folder down to the
    /// file's; at least one.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.names.split('/')
    }

    /// The last name: the file's.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.names
            .rsplit_once('/')
            .map_or(self.names.as_str(), |(_, name)| name)
    }

    /// The order a folder's files are kept in: name by name from the folder
    /// down, so a folder's files come before those of the folder after it.
    fn cmp_by_names(&self, other: &Self) -> std::cmp::Ordering {
        self.components().cmp(other.components())
    }
}

/// A folder a system handed over without a path, as it is listed: its name,
/// and for each file under it the file's path and `F`, which is what the
/// file is read from until it is read and what it holds afterwards.
///
/// Checked once, when it is made, and kept so while `F` changes (`try_map`,
/// which the web provider reads the files through): the name is one a
/// folder holds, no two files are at one path, and the files are in the
/// order of their paths. What a folder pick is made of (`into_pick`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FolderListing<F> {
    name: String,
    files: Vec<(FolderPath, F)>,
}

impl<F> FolderListing<F> {
    /// The folder named `name` with `files`, put in the order of their
    /// paths; `None` when `name` is not one name a folder holds (empty, `.`
    /// or `..`, or holding `/`, `\` or a NUL), or two of the files are at
    /// one path.
    pub(crate) fn try_new(
        name: impl Into<String>,
        mut files: Vec<(FolderPath, F)>,
    ) -> Option<Self> {
        let name: String = name.into();
        if !is_name_in_folder(&name) {
            return None;
        }
        files.sort_by(|(earlier, _), (later, _)| earlier.cmp_by_names(later));
        if files.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        Some(Self { name, files })
    }

    /// What is held for each file, in the files' order.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn held(&self) -> impl Iterator<Item = &F> {
        self.files.iter().map(|(_, held)| held)
    }

    /// The same folder with what `each` makes of what is held for each
    /// file, in the files' order; the first failure ends it.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn try_map<G, E>(
        self,
        mut each: impl AsyncFnMut(F) -> Result<G, E>,
    ) -> Result<FolderListing<G>, E> {
        let mut files = Vec::with_capacity(self.files.len());
        for (path, held) in self.files {
            files.push((path, each(held).await?));
        }
        Ok(FolderListing {
            name: self.name,
            files,
        })
    }
}

impl FolderListing<Arc<[u8]>> {
    /// The pick of the folder, its files read.
    pub(crate) fn into_pick(self) -> PickedFile {
        PickedFile {
            place: Place::Folder {
                name: self.name,
                files: self
                    .files
                    .into_iter()
                    .map(|(path, contents)| FolderFile::new(path, contents))
                    .collect::<Arc<[FolderFile]>>(),
            },
            grant: None,
        }
    }
}

/// A file or folder the user picked.
///
/// Its own type rather than a path, since not every system answers one: a
/// desktop dialog answers where the file or folder is ([`path`](Self::path)),
/// while a browser answers a file a page may read but never a path to it,
/// and iOS a document the application may reach only while it holds the
/// access the picker granted; both are carried as the file's name and its
/// contents ([`contents`](Self::contents)), and a browser's folder as its
/// name and the files under it ([`files`](Self::files)). A pick of a
/// request that [keeps access](OpenRequest::keep_access) carries the
/// system's lasting grant to it ([`grant`](Self::grant)).
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PickedFile {
    place: Place,
    grant: Option<Grant>,
}

/// Where a pick is: what the system answered for it.
#[derive(Clone, PartialEq, Eq)]
enum Place {
    /// A file or folder at a path the application opens itself.
    Path(PathBuf),
    /// A file the system handed over: its name, and what it holds, read
    /// when the dialog answered (an open) or written before it did (a save).
    Contents { name: String, contents: Arc<[u8]> },
    /// A folder the browser handed over: its name, and the files under it in
    /// the order of their paths, no two at one path, read when the dialog
    /// answered.
    Folder {
        name: String,
        files: Arc<[FolderFile]>,
    },
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
    /// A folder a browser handed over without a path: its name, and the
    /// files under it, read when the open dialog answered. Files are all it
    /// carries, since a browser's file input reports nothing else: a folder
    /// that holds no file, at any depth under the picked one, leaves no
    /// trace here.
    Folder {
        /// The name the browser gave it.
        name: &'a str,
        /// The files under it, at any depth, in the order of their paths
        /// (name by name from the folder down), no two at one path.
        files: &'a [FolderFile],
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
            grant: None,
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
            grant: None,
        })
    }

    /// The folder named `name` holding `files`, as a system that hands over
    /// no path reports it (a browser), its files put in the order of their
    /// paths; `None` when `name` is not one name a folder holds (empty, `.`
    /// or `..`, or holding `/`, `\` or a NUL), as a name of a [`FolderPath`]
    /// is judged, or when two of the files are at one path.
    #[must_use]
    pub fn try_from_folder(name: impl Into<String>, files: Vec<FolderFile>) -> Option<Self> {
        let files = files
            .into_iter()
            .map(|file| (file.path, file.contents))
            .collect::<Vec<(FolderPath, Arc<[u8]>)>>();
        FolderListing::try_new(name, files).map(FolderListing::into_pick)
    }

    /// A save's contents handed to the browser as a download offered as
    /// `name` ([`PickedForm::Download`]); `None` when `name` is not a file
    /// name, as [`try_from_contents`](Self::try_from_contents) judges it.
    #[must_use]
    pub fn try_download(name: impl Into<String>) -> Option<Self> {
        let name: String = name.into();
        is_file_name(&name).then_some(Self {
            place: Place::Download { name },
            grant: None,
        })
    }

    /// The same pick carrying `grant`, the system's lasting access to it, as
    /// a backend hands a pick of a request that
    /// [keeps access](OpenRequest::keep_access) over.
    #[must_use]
    pub fn granted(mut self, grant: Grant) -> Self {
        self.grant = Some(grant);
        self
    }

    /// The system's lasting access to it, which the application stores and
    /// resolves after a restart ([`Grants::resolve`](crate::grant::Grants::resolve));
    /// `None` unless the request [kept access](OpenRequest::keep_access) and
    /// the system granted it.
    #[must_use]
    pub fn grant(&self) -> Option<&Grant> {
        self.grant.as_ref()
    }

    /// What it is, as the system answered it.
    #[must_use]
    pub fn form(&self) -> PickedForm<'_> {
        match &self.place {
            Place::Path(path) => PickedForm::Path(path),
            Place::Contents { name, contents } => PickedForm::Contents { name, contents },
            Place::Folder { name, files } => PickedForm::Folder { name, files },
            Place::Download { name } => PickedForm::Download { name },
        }
    }

    /// Where it is; `None` for a file or folder the system handed over
    /// without a path (a browser's, iOS's), whose
    /// [`contents`](Self::contents) or [`files`](Self::files) the pick holds
    /// instead, and for a download.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self.form() {
            PickedForm::Path(path) => Some(path),
            PickedForm::Contents { .. }
            | PickedForm::Folder { .. }
            | PickedForm::Download { .. } => None,
        }
    }

    /// Its name: the last component of its [`path`](Self::path), the name
    /// the system handed it over with, or the name a download was offered
    /// under. `None` for a path that ends in `..` or is a root.
    #[must_use]
    pub fn file_name(&self) -> Option<&OsStr> {
        match self.form() {
            PickedForm::Path(path) => path.file_name(),
            PickedForm::Contents { name, .. }
            | PickedForm::Folder { name, .. }
            | PickedForm::Download { name } => Some(OsStr::new(name)),
        }
    }

    /// What the file holds, for a file the system handed over without a
    /// path: read when an open dialog answered, within the request's
    /// [`max_bytes`](OpenRequest::max_bytes), or written before a save
    /// answered. `None` for a pick at a path, which the application reads
    /// itself, for a folder, whose [`files`](Self::files) hold theirs, and
    /// for a download, which nobody saw written.
    #[must_use]
    pub fn contents(&self) -> Option<&[u8]> {
        match self.form() {
            PickedForm::Contents { contents, .. } => Some(contents),
            PickedForm::Path(_) | PickedForm::Folder { .. } | PickedForm::Download { .. } => None,
        }
    }

    /// The files under a folder the system handed over without a path, at
    /// any depth, in the order of their paths: read when the open dialog
    /// answered, within the request's
    /// [`max_entries`](OpenRequest::max_entries) and
    /// [`max_bytes`](OpenRequest::max_bytes). `None` for any other pick: a
    /// folder at a path is the application's to read.
    #[must_use]
    pub fn files(&self) -> Option<&[FolderFile]> {
        match self.form() {
            PickedForm::Folder { files, .. } => Some(files),
            PickedForm::Path(_) | PickedForm::Contents { .. } | PickedForm::Download { .. } => None,
        }
    }
}

/// A file under a folder a browser handed over ([`PickedForm::Folder`]):
/// where it is in the folder, and what it holds.
#[derive(Clone, PartialEq, Eq)]
pub struct FolderFile {
    path: FolderPath,
    contents: Arc<[u8]>,
}

impl FolderFile {
    /// The file at `path` under its folder, holding `contents`.
    #[must_use]
    pub fn new(path: FolderPath, contents: Arc<[u8]>) -> Self {
        Self { path, contents }
    }

    /// Where it is under the folder.
    #[must_use]
    pub fn path(&self) -> &FolderPath {
        &self.path
    }

    /// What it holds.
    #[must_use]
    pub fn contents(&self) -> &[u8] {
        &self.contents
    }
}

impl fmt::Debug for FolderFile {
    /// The path and the length: its contents are the user's.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderFile")
            .field("path", &self.path.as_str())
            .field("len", &self.contents.len())
            .finish()
    }
}

impl fmt::Debug for PickedFile {
    /// The path or the name, a handed-over file's length or a folder's
    /// count of files, and its grant's kind: what they hold is the user's.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("PickedFile");
        match &self.place {
            Place::Path(path) => debug.field("path", path),
            Place::Contents { name, contents } => {
                debug.field("name", name).field("len", &contents.len())
            }
            Place::Folder { name, files } => {
                debug.field("folder", name).field("files", &files.len())
            }
            Place::Download { name } => debug.field("download", name),
        };
        if let Some(grant) = &self.grant {
            debug.field("grant", &grant.kind());
        }
        debug.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::FileFilter;

    #[test]
    fn a_typed_name_without_an_extension_is_completed_by_the_selected_filters_first() {
        let text = FileFilter::new("Text")
            .with_extension("txt")
            .with_extension("tar.gz")
            .with_file_name("README");
        let cases: [(&[u8], Option<&[u8]>); 13] = [
            (b"report", Some(b"report.txt")),
            // An empty extension is replaced, not followed by a second dot.
            (b"report.", Some(b"report.txt")),
            (b"report..", Some(b"report.txt")),
            // A leading dot starts a hidden file's name, not an extension,
            // and the name stands, as Windows' dialog leaves it.
            (b".profile", None),
            (b".profile.", None),
            // A name that is not UTF-8 is completed by its bytes.
            (b"\xff\xfe", Some(b"\xff\xfe.txt")),
            // Named by the filter already: by an extension, in any case, or
            // as a whole name.
            (b"report.txt", None),
            (b"REPORT.TXT", None),
            (b"backup.tar.gz", None),
            (b"readme", None),
            // An extension of the user's own choosing stands.
            (b"report.v2", None),
            (b".profile.bak", None),
            // No stem to complete.
            (b"...", None),
        ];
        for (typed, completed) in cases {
            assert_eq!(
                text.completes(typed).as_deref(),
                completed,
                "{}",
                String::from_utf8_lossy(typed)
            );
        }

        let no_extension = [
            FileFilter::all_files("All files"),
            FileFilter::new("Images").with_mime_type("image/*"),
            FileFilter::new("Build files").with_file_name("Makefile"),
        ];
        for filter in no_extension {
            assert_eq!(
                filter.completes(b"report"),
                None,
                "{} names no extension to add",
                filter.name()
            );
        }
    }
}
