//! The file dialogs over the `FileChooser` portal.

use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::Rc;

use flui_platform::SystemFailure;
use flui_platform::capability::{Resolver, WindowRef};
use zbus::{OwnedVariant, Variant};

use super::request::{self, Response};
use super::{Dismissal, Portal, PortalError, parent_window};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileFilter, OpenRequest, OpenResult, PickedFile,
    Picks, SaveLocation, SaveRequest, SaveResult,
};

const FILE_CHOOSER: &str = "org.freedesktop.portal.FileChooser";

/// [`FileDialogBackend`] over the desktop portal.
pub(crate) struct PortalDialogs {
    portal: Rc<Portal>,
}

impl PortalDialogs {
    pub(crate) fn new(portal: Rc<Portal>) -> Self {
        Self { portal }
    }

    /// Show `method`'s dialog with `options` and answer `answer` with what
    /// `picked` makes of the files the user chose and the rest of the
    /// portal's results. The dialog stays up while the returned hold is
    /// held; dropping it closes the portal's request.
    fn show<T, P>(
        &self,
        window: &WindowRef,
        method: &'static str,
        title: Option<&str>,
        mut options: Options,
        answer: Resolver<Result<Option<T>, DialogError>>,
        picked: P,
    ) -> DialogHold
    where
        T: Send + 'static,
        P: FnOnce(Picks, &Results) -> Result<T, DialogError> + Send + 'static,
    {
        if let Err(error) = self.portal.start() {
            let _ = answer.resolve(Err(dialog_error(error)));
            return DialogHold::new(());
        }
        let token = self.portal.token();
        options.insert("handle_token", Variant::from(token.clone()));
        let body = (
            parent_window(window),
            title.unwrap_or_default().to_owned(),
            options,
        );
        let (hold, dismissal) = Dismissal::pair();
        self.portal.spawn(move |bus| async move {
            let result = async {
                let connection = bus.connection().await?;
                request::call(
                    &connection,
                    FILE_CHOOSER,
                    method,
                    &token,
                    &body,
                    Some(&dismissal),
                )
                .await
            }
            .await;
            let answered = match result {
                Ok(Response::Answered(results)) => picks(&results)
                    .and_then(|picks| picked(picks, &results))
                    .map(Some),
                Ok(Response::Cancelled) => Ok(None),
                Ok(Response::Ended(code)) => Err(SystemFailure::PortalResponse(code).into()),
                // Nobody waits: the resolver drops unanswered.
                Ok(Response::Dismissed) => return,
                Err(error) => Err(dialog_error(error)),
            };
            let _ = answer.resolve(answered);
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

/// One filter as the portal takes it: each extension as a glob in lower
/// case and one in upper case (`*.png`, `*.PNG`), then each MIME type.
///
/// The portal's backends match a glob case-sensitively, while the contract,
/// as Windows and macOS, ignores case. A bracket per letter (`*.[pP][nN][gG]`)
/// would match every case, but a save dialog derives the extension it
/// appends from the selected glob and would meet the brackets, so the portal
/// gets the two plain spellings GTK applications send, and a name in mixed
/// case is not matched on a case-sensitive file system. A checked extension
/// holds no glob character, so each spelling stands for itself; one with no
/// letter is sent once.
fn portal_filter(filter: &FileFilter) -> PortalFilter {
    let mut patterns =
        Vec::with_capacity(filter.extensions().len() * 2 + filter.mime_types().len());
    for extension in filter.extensions() {
        // Extensions are kept in lower case.
        let upper = extension.to_ascii_uppercase();
        let has_letter = upper != *extension;
        patterns.push((0, format!("*.{extension}")));
        if has_letter {
            patterns.push((0, format!("*.{upper}")));
        }
    }
    patterns.extend(
        filter
            .mime_types()
            .iter()
            .map(|mime_type| (1, mime_type.clone())),
    );
    (filter.name().to_owned(), patterns)
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
        self.show(
            window,
            "OpenFile",
            request.title.as_deref(),
            options,
            answer,
            |picks, _| Ok(picks),
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
        self.show(
            window,
            "SaveFile",
            request.title.as_deref(),
            options,
            answer,
            move |picks, results| save_location(picks, results, &names),
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
}
