//! What a dialog is asked for, and what it answers.

use std::path::{Path, PathBuf};

/// A named set of files an open or save dialog offers to filter by: "Images"
/// with `png` and `jpg`.
///
/// Built without checks; [`FileDialogs`](super::FileDialogs) refuses a
/// request whose filter is malformed with
/// [`DialogError::InvalidRequest`](super::DialogError::InvalidRequest),
/// before the system is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileFilter {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
}

impl FileFilter {
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

    /// Also match files ending in `.extension`; a leading `.` is dropped.
    #[must_use]
    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        let extension: String = extension.into();
        let extension = extension
            .strip_prefix('.')
            .map_or(extension.clone(), str::to_owned);
        self.extensions.push(extension);
        self
    }

    /// Also match files of the MIME type `mime_type`, such as `image/png`.
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_types.push(mime_type.into());
        self
    }

    /// The name the dialog shows.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The extensions it matches, without their `.`.
    #[must_use]
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// The MIME types it matches.
    #[must_use]
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// Whether the filter has a name, matches something, and every
    /// extension and MIME type is well formed.
    pub(super) fn is_valid(&self) -> bool {
        let extension_ok = |extension: &String| {
            !extension.is_empty()
                && !extension.chars().any(|c| {
                    c.is_control() || c.is_whitespace() || matches!(c, '.' | '/' | '\\' | '*')
                })
        };
        let token_ok = |token: &str| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
        };
        let mime_ok = |mime: &String| {
            mime.split_once('/')
                .is_some_and(|(kind, sub)| token_ok(kind) && token_ok(sub))
        };
        !self.name.trim().is_empty()
            && (!self.extensions.is_empty() || !self.mime_types.is_empty())
            && self.extensions.iter().all(extension_ok)
            && self.mime_types.iter().all(mime_ok)
    }
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
    title: Option<String>,
    directory: Option<PathBuf>,
    filters: Vec<FileFilter>,
    accept_label: Option<String>,
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
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The folder the dialog opens in.
    #[must_use]
    pub fn with_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Offer `filter`; the first filter added is selected. Only a file
    /// dialog takes filters.
    #[must_use]
    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// The label of the button that accepts the choice.
    #[must_use]
    pub fn with_accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
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

    /// The dialog's title, if set.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The folder the dialog opens in, if set.
    #[must_use]
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    /// The filters offered, the selected one first.
    #[must_use]
    pub fn filters(&self) -> &[FileFilter] {
        &self.filters
    }

    /// The accept button's label, if set.
    #[must_use]
    pub fn accept_label(&self) -> Option<&str> {
        self.accept_label.as_deref()
    }

    pub(super) fn is_valid(&self) -> bool {
        self.filters.iter().all(FileFilter::is_valid)
            && (!self.picks_folders() || self.filters.is_empty())
            && self.directory.as_deref().is_none_or(Path::is_absolute)
    }
}

/// A save dialog: the user picks a new or existing file to write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaveRequest {
    title: Option<String>,
    directory: Option<PathBuf>,
    suggested_name: Option<String>,
    filters: Vec<FileFilter>,
    accept_label: Option<String>,
}

impl SaveRequest {
    /// A save dialog with the system's defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialog's title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The folder the dialog opens in.
    #[must_use]
    pub fn with_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// The file name the dialog proposes: a name, not a path.
    #[must_use]
    pub fn with_suggested_name(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }

    /// Offer `filter`; the first filter added is selected.
    #[must_use]
    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// The label of the button that accepts the choice.
    #[must_use]
    pub fn with_accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
        self
    }

    /// The dialog's title, if set.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The folder the dialog opens in, if set.
    #[must_use]
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    /// The proposed file name, if set.
    #[must_use]
    pub fn suggested_name(&self) -> Option<&str> {
        self.suggested_name.as_deref()
    }

    /// The filters offered, the selected one first.
    #[must_use]
    pub fn filters(&self) -> &[FileFilter] {
        &self.filters
    }

    /// The accept button's label, if set.
    #[must_use]
    pub fn accept_label(&self) -> Option<&str> {
        self.accept_label.as_deref()
    }

    pub(super) fn is_valid(&self) -> bool {
        let name_ok = |name: &String| {
            !name.trim().is_empty()
                && !name
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\'))
                && name != "."
                && name != ".."
        };
        self.filters.iter().all(FileFilter::is_valid)
            && self.suggested_name.as_ref().is_none_or(name_ok)
            && self.directory.as_deref().is_none_or(Path::is_absolute)
    }
}

/// A file or folder the user picked.
///
/// Its own type rather than a path, so that a later backend can carry the
/// access a sandbox grants with it (a security-scoped bookmark, a document
/// portal entry, a content URI) without changing what callers receive.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PickedFile {
    path: PathBuf,
}

impl PickedFile {
    /// The pick at `path`, as a backend reports it.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
