//! The file dialogs over the `FileChooser` portal.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::future::Future;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::Rc;

use flui_platform::SystemFailure;
use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
use zbus::{OwnedVariant, Variant};

use super::request::{self, Response};
use super::{Dismissal, Interface, Portal, PortalError, with_parent};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileFilter, OpenRequest, OpenResult, PickedFile,
    Picks, SaveLocation, SaveRequest, SaveResult,
};

/// `FileChooser` at the version a dialog needs: picking folders
/// (`directory`) came with 3.
const FILE_CHOOSER: Interface = Interface::new("org.freedesktop.portal.FileChooser", 1);
const FOLDER_CHOOSER: Interface = Interface::new("org.freedesktop.portal.FileChooser", 3);

/// [`FileDialogBackend`] over the desktop portal.
pub(crate) struct PortalDialogs {
    portal: Rc<Portal>,
    completions: CompletionSource,
}

impl PortalDialogs {
    pub(crate) fn new(portal: Rc<Portal>, completions: CompletionSource) -> Self {
        Self {
            portal,
            completions,
        }
    }

    /// Show `method`'s dialog of `interface` with `options`, modal to
    /// `window`, and answer `answer` with what `picked` makes of the files
    /// the user chose and the rest of the portal's results, on the portal's
    /// thread, where it awaits what it reads of the file system. The dialog
    /// stays up while the returned hold is held; dropping it closes the
    /// portal's request. The request holds the window's parent until it
    /// ends.
    #[expect(
        clippy::too_many_arguments,
        reason = "one dialog's parts, each its own type; a struct would only rename them"
    )]
    fn show<T, P, F>(
        &self,
        window: &WindowRef,
        interface: Interface,
        method: &'static str,
        title: Option<&str>,
        mut options: Options,
        answer: Resolver<Result<Option<T>, DialogError>>,
        picked: P,
    ) -> DialogHold
    where
        T: Send + 'static,
        P: FnOnce(Picks, Results) -> F + Send + 'static,
        F: Future<Output = Result<T, DialogError>> + Send,
    {
        if let Err(error) = self.portal.start() {
            let _ = answer.resolve(Err(dialog_error(error)));
            return DialogHold::new(());
        }
        let title = title.unwrap_or_default().to_owned();
        let (hold, dismissal) = Dismissal::pair();
        let portal = Rc::clone(&self.portal);
        with_parent(Some(window), &self.completions, move |parent| {
            if dismissal.is_dismissed() {
                // The dialog ended (its caller, its window) before its
                // window was named: nothing is shown.
                return;
            }
            let token = portal.token();
            options.insert("handle_token", Variant::from(token.clone()));
            let body = (parent.as_str().to_owned(), title, options);
            portal.spawn(move |bus| async move {
                let result = async {
                    let desktop = bus.desktop().await?;
                    request::call(&desktop, interface, method, &token, &body, Some(&dismissal))
                        .await
                }
                .await;
                // The window's export lasts until the request ended.
                drop(parent);
                let answered = match result {
                    Ok(Response::Answered(results)) => match picks(&results) {
                        Ok(picks) => picked(picks, results).await.map(Some),
                        Err(error) => Err(error),
                    },
                    Ok(Response::Cancelled) => Ok(None),
                    Ok(Response::Ended(code)) => Err(SystemFailure::PortalResponse(code).into()),
                    // Nobody waits: the resolver drops unanswered.
                    Ok(Response::Dismissed) => return,
                    Err(error) => Err(dialog_error(error)),
                };
                let _ = answer.resolve(answered);
            });
        });
        DialogHold::new(hold)
    }
}

type Options = HashMap<&'static str, Variant<'static>>;

/// A dialog's results, by name.
type Results = HashMap<String, OwnedVariant>;

/// One filter as the portal's `(sa(us))` carries it: the name, then each
/// pattern with its kind, `0` for a glob and `1` for a MIME type.
type PortalFilter = (String, Vec<(u32, String)>);

fn dialog_error(error: PortalError) -> DialogError {
    match error {
        PortalError::Unsupported(unsupported) => DialogError::Unsupported(unsupported),
        PortalError::Failed(failure) => DialogError::Failed(failure),
    }
}

/// The local files a dialog's `uris` result names. A success names one at
/// least: one that names none, or holds something other than a list of
/// strings, is outside the portal's contract, and a URI that names no local
/// file fails the whole answer, since the handle hands out paths only.
fn picks(results: &Results) -> Result<Picks, DialogError> {
    let malformed = DialogError::Failed(SystemFailure::MalformedAnswer);
    let uris = results
        .get("uris")
        .map(|uris| Vec::<String>::try_from(uris.clone()))
        .transpose()
        .map_err(|_| malformed)?
        .unwrap_or_default();
    let files = uris
        .iter()
        .map(|uri| {
            let path = request::file_path(uri).ok_or(DialogError::NotAFile)?;
            // A path with a NUL byte (`%00`) names no file: outside the contract.
            PickedFile::try_new(path).ok_or(malformed)
        })
        .collect::<Result<Vec<PickedFile>, DialogError>>()?;
    Picks::try_new(files).ok_or(malformed)
}

/// Where a save dialog answers: its one file, and the filter the portal
/// reports as selected (`current_filter`), found in `names` by its name,
/// which a request holds once. Several files are outside the portal's
/// contract; a selected filter the request did not offer, or one the portal
/// does not report, is no filter, since the file the user chose stands
/// without it.
fn save_location(
    picks: Picks,
    results: &Results,
    names: &[String],
) -> Result<SaveLocation, DialogError> {
    if picks.len() > 1 {
        return Err(SystemFailure::MalformedAnswer.into());
    }
    let selected = results
        .get("current_filter")
        .and_then(|filter| PortalFilter::try_from(filter.clone()).ok())
        .and_then(|(name, _)| names.iter().position(|offered| *offered == name));
    Ok(SaveLocation::new(picks.first().clone(), selected))
}

/// The kind of a filter pattern that is a glob.
const GLOB: u32 = 0;
/// The kind of a filter pattern that is a MIME type.
const MIME_TYPE: u32 = 1;

/// One filter as the portal takes it: each extension as a glob in lower
/// case and one in upper case (`*.png`, `*.PNG`), each file name as a glob
/// of itself as written, in lower and in upper case (`Makefile`,
/// `makefile`, `MAKEFILE`), then each MIME type; a filter of every file is
/// the one glob `*`.
///
/// The portal's backends match a glob case-sensitively, while the contract,
/// as Windows and macOS, ignores case. A bracket per letter (`*.[pP][nN][gG]`)
/// would match every case, but a save dialog derives the extension it
/// appends from the selected glob and would meet the brackets, so the portal
/// gets the plain spellings GTK applications send, and a name in any other
/// mix of cases is not matched on a case-sensitive file system. A checked
/// extension or file name holds no glob character, so each spelling stands
/// for itself; a spelling that is another's is sent once.
fn portal_filter(filter: &FileFilter) -> PortalFilter {
    if filter.is_all_files() {
        return (filter.name().to_owned(), vec![(GLOB, "*".to_owned())]);
    }
    let mut patterns: Vec<(u32, String)> = Vec::with_capacity(
        filter.extensions().len() * 2 + filter.file_names().len() * 3 + filter.mime_types().len(),
    );
    let mut glob = |glob: String| {
        let glob = (GLOB, glob);
        if !patterns.contains(&glob) {
            patterns.push(glob);
        }
    };
    for extension in filter.extensions() {
        // Extensions are kept in lower case.
        glob(format!("*.{extension}"));
        glob(format!("*.{}", extension.to_ascii_uppercase()));
    }
    for file_name in filter.file_names() {
        glob(file_name.clone());
        glob(file_name.to_ascii_lowercase());
        glob(file_name.to_ascii_uppercase());
    }
    patterns.extend(
        filter
            .mime_types()
            .iter()
            .map(|mime_type| (MIME_TYPE, mime_type.clone())),
    );
    (filter.name().to_owned(), patterns)
}

/// `location` with its file named after the filter selected when the user
/// accepted, where the name typed has no extension and that filter has one
/// ([`FileFilter::completes`]): what Windows' and macOS's dialogs do
/// themselves, and GTK's chooser, behind the portal on GNOME, does not.
///
/// The portal's dialog asked the user about replacing the name as typed. A
/// file that already has the completed name was never asked about, so the
/// name then stands as typed rather than name that file: the one case
/// where the extension is not added. The portal reports no filter where its
/// backend does not say which was selected, and the name stands there too.
async fn named_after_its_filter(location: SaveLocation, filters: Vec<FileFilter>) -> SaveLocation {
    let completed = location
        .filter()
        .and_then(|selected| filters.get(selected))
        .zip(location.file().path())
        .and_then(|(filter, path)| {
            let completed = filter.completes(path.file_name()?.as_bytes())?;
            Some(path.with_file_name(OsStr::from_bytes(&completed)))
        });
    let Some(completed) = completed else {
        return location;
    };
    let examined = completed.clone();
    // Anything but "nothing is there" keeps the name the user was asked
    // about: a file, a link that leads nowhere, a folder that cannot be read.
    let free = zruntime::unblock(move || {
        std::fs::symlink_metadata(&examined)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
    .await;
    match PickedFile::try_new(completed) {
        Some(file) if free => SaveLocation::new(file, location.filter()),
        _ => location,
    }
}

/// A path as the portal's `ay` options take it: its bytes and a NUL.
fn bytes(path: &Path) -> Vec<u8> {
    let mut bytes = path.as_os_str().as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn common(
    options: &mut Options,
    accept_label: Option<&str>,
    directory: Option<&Path>,
    filter_list: &[FileFilter],
) {
    if let Some(label) = accept_label {
        options.insert("accept_label", Variant::from(label.to_owned()));
    }
    if let Some(directory) = directory {
        options.insert("current_folder", Variant::from(bytes(directory)));
    }
    // The first filter is the selected one: said in `current_filter` as well
    // as by its place, since a backend need not select the first.
    if let Some(first) = filter_list.first() {
        let filters = filter_list
            .iter()
            .map(portal_filter)
            .collect::<Vec<PortalFilter>>();
        options.insert("current_filter", Variant::from(portal_filter(first)));
        options.insert("filters", Variant::from(filters));
    }
}

impl FileDialogBackend for PortalDialogs {
    fn open(
        &self,
        window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let mut options = Options::new();
        options.insert("multiple", Variant::from(request.is_multiple()));
        options.insert("directory", Variant::from(request.picks_folders()));
        common(
            &mut options,
            request.accept_label.as_deref(),
            request.directory.as_deref(),
            &request.filters,
        );
        let interface = if request.picks_folders() {
            FOLDER_CHOOSER
        } else {
            FILE_CHOOSER
        };
        self.show(
            window,
            interface,
            "OpenFile",
            request.title.as_deref(),
            options,
            answer,
            |picks, _| std::future::ready(Ok(picks)),
        )
    }

    fn save(
        &self,
        window: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let mut options = Options::new();
        if let Some(name) = request.suggested_name {
            options.insert("current_name", Variant::from(name));
        }
        common(
            &mut options,
            request.accept_label.as_deref(),
            request.directory.as_deref(),
            &request.filters,
        );
        let names = request
            .filters
            .iter()
            .map(|filter| filter.name().to_owned())
            .collect::<Vec<String>>();
        let filters = request.filters;
        self.show(
            window,
            FILE_CHOOSER,
            "SaveFile",
            request.title.as_deref(),
            options,
            answer,
            move |picks, results| async move {
                let location = save_location(picks, &results, &names)?;
                Ok(named_after_its_filter(location, filters).await)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::portal_filter;
    use crate::dialog::FileFilter;

    #[test]
    fn a_filter_becomes_globs_then_mime_types() {
        let filter = FileFilter::new("Images")
            .with_extension("png")
            .with_extension("JPG")
            .with_extension("tar.gz")
            .with_extension("c++")
            .with_extension("7z")
            .with_extension("123")
            .with_mime_type("image/webp");
        assert_eq!(
            portal_filter(&filter),
            (
                "Images".to_owned(),
                vec![
                    // Lower and upper case, as GTK applications send them:
                    // plain globs a save dialog can append an extension from.
                    (0, "*.png".to_owned()),
                    (0, "*.PNG".to_owned()),
                    (0, "*.jpg".to_owned()),
                    (0, "*.JPG".to_owned()),
                    (0, "*.tar.gz".to_owned()),
                    (0, "*.TAR.GZ".to_owned()),
                    (0, "*.c++".to_owned()),
                    (0, "*.C++".to_owned()),
                    (0, "*.7z".to_owned()),
                    (0, "*.7Z".to_owned()),
                    // No letter: one spelling.
                    (0, "*.123".to_owned()),
                    (1, "image/webp".to_owned()),
                ]
            )
        );
    }

    #[test]
    fn a_file_name_is_its_own_glob_and_every_file_is_one_star() {
        let build = FileFilter::new("Build files")
            .with_file_name("Makefile")
            .with_file_name(".env")
            .with_extension("mk");
        assert_eq!(
            portal_filter(&build),
            (
                "Build files".to_owned(),
                vec![
                    (0, "*.mk".to_owned()),
                    (0, "*.MK".to_owned()),
                    // As written, then the two spellings a case-sensitive
                    // glob would otherwise miss.
                    (0, "Makefile".to_owned()),
                    (0, "makefile".to_owned()),
                    (0, "MAKEFILE".to_owned()),
                    // Written in lower case already: two spellings.
                    (0, ".env".to_owned()),
                    (0, ".ENV".to_owned()),
                ]
            )
        );
        assert_eq!(
            portal_filter(&FileFilter::all_files("All files")),
            ("All files".to_owned(), vec![(0, "*".to_owned())])
        );
    }
}
