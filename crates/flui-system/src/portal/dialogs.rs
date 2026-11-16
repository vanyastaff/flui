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
    Picks, SaveRequest, SaveResult,
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
    /// `picked` makes of the files the user chose. The dialog stays up while
    /// the returned hold is held; dropping it closes the portal's request.
    fn show<T>(
        &self,
        window: &WindowRef,
        method: &'static str,
        title: Option<&str>,
        mut options: Options,
        answer: Resolver<Result<Option<T>, DialogError>>,
        picked: fn(Picks) -> Result<T, DialogError>,
    ) -> DialogHold
    where
        T: Send + 'static,
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
                Ok(Response::Answered(results)) => picks(&results).and_then(picked).map(Some),
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
fn picks(results: &HashMap<String, OwnedVariant>) -> Result<Picks, DialogError> {
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

/// The one file a save dialog answers; several are outside the portal's
/// contract.
fn one_file(picks: Picks) -> Result<PickedFile, DialogError> {
    if picks.len() > 1 {
        return Err(SystemFailure::MalformedAnswer.into());
    }
    Ok(picks.first().clone())
}

/// The portal's `a(sa(us))` filters: `0` with a glob, `1` with a MIME type.
fn filters(filters: &[FileFilter]) -> Vec<(String, Vec<(u32, String)>)> {
    filters
        .iter()
        .map(|filter| {
            let globs = filter
                .extensions()
                .iter()
                .map(|extension| (0, format!("*.{}", literal_glob(extension))));
            let types = filter
                .mime_types()
                .iter()
                .map(|mime_type| (1, mime_type.clone()));
            (
                filter.name().to_owned(),
                globs.chain(types).collect::<Vec<(u32, String)>>(),
            )
        })
        .collect::<Vec<(String, Vec<(u32, String)>)>>()
}

/// `text` as a glob that matches only itself: each glob character in a
/// bracket of its own, which every glob dialect reads as that character.
fn literal_glob(text: &str) -> String {
    let mut glob = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '*' | '?' | '[' | ']') {
            glob.push('[');
            glob.push(character);
            glob.push(']');
        } else {
            glob.push(character);
        }
    }
    glob
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
    if !filter_list.is_empty() {
        options.insert("filters", Variant::from(filters(filter_list)));
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
            Ok,
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
        self.show(
            window,
            "SaveFile",
            request.title.as_deref(),
            options,
            answer,
            one_file,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::filters;
    use crate::dialog::FileFilter;

    #[test]
    fn a_filter_becomes_globs_then_mime_types() {
        let filter = FileFilter::new("Images")
            .with_extension("png")
            .with_extension("jpg")
            .with_extension("a[1]?")
            .with_mime_type("image/webp");
        assert_eq!(
            filters(&[filter]),
            [(
                "Images".to_owned(),
                vec![
                    (0, "*.png".to_owned()),
                    (0, "*.jpg".to_owned()),
                    // Glob characters in an extension match only themselves.
                    (0, "*.a[[]1[]][?]".to_owned()),
                    (1, "image/webp".to_owned()),
                ]
            )]
        );
    }
}
