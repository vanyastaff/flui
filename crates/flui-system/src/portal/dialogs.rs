//! The file dialogs over the `FileChooser` portal.

use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::Rc;

use flui_platform::capability::{Resolver, WindowRef};
use zbus::{OwnedVariant, Variant};

use super::request::{self, Response};
use super::{Portal, PortalError, parent_window};
use crate::dialog::{
    DialogError, FileDialogBackend, FileFilter, OpenRequest, OpenResult, PickedFile, SaveRequest,
    SaveResult,
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

    /// Show `method`'s dialog with `options` and answer `answer` with the
    /// picks `picked` makes of the URIs the user chose.
    fn show<T>(
        &self,
        window: &WindowRef,
        method: &'static str,
        title: Option<&str>,
        mut options: Options,
        answer: Resolver<Result<Option<T>, DialogError>>,
        picked: fn(Vec<PickedFile>) -> Option<T>,
    ) where
        T: Send + 'static,
    {
        if let Err(error) = self.portal.start() {
            let _ = answer.resolve(Err(dialog_error(error)));
            return;
        }
        let token = self.portal.token();
        options.insert("handle_token", Variant::from(token.clone()));
        let body = (
            parent_window(window),
            title.unwrap_or_default().to_owned(),
            options,
        );
        self.portal.spawn(move |bus| async move {
            let result = async {
                let connection = bus.connection().await?;
                request::call(&connection, FILE_CHOOSER, method, &token, &body).await
            }
            .await;
            let _ = answer.resolve(match result {
                Ok(Response::Answered(results)) => picks(&results).map(picked),
                Ok(Response::Cancelled) => Ok(None),
                Ok(Response::Ended) => Err(DialogError::Failed { code: 2 }),
                Err(error) => Err(dialog_error(error)),
            });
        });
    }
}

type Options = HashMap<&'static str, Variant<'static>>;

fn dialog_error(error: PortalError) -> DialogError {
    match error {
        PortalError::Unavailable(reason) => DialogError::Unavailable(reason),
        PortalError::Failed => DialogError::Failed { code: 0 },
    }
}

/// The local files a dialog's `uris` result names; a URI that names no local
/// file fails the whole answer, since the handle hands out paths only.
fn picks(results: &HashMap<String, OwnedVariant>) -> Result<Vec<PickedFile>, DialogError> {
    let uris = results
        .get("uris")
        .map(|uris| Vec::<String>::try_from(uris.clone()))
        .transpose()
        .map_err(|_| DialogError::Failed { code: 0 })?
        .unwrap_or_default();
    uris.iter()
        .map(|uri| {
            request::file_path(uri)
                .map(PickedFile::new)
                .ok_or(DialogError::NotAFile)
        })
        .collect::<Result<Vec<PickedFile>, DialogError>>()
}

/// The portal's `a(sa(us))` filters: `0` with a glob, `1` with a MIME type.
fn filters(filters: &[FileFilter]) -> Vec<(String, Vec<(u32, String)>)> {
    filters
        .iter()
        .map(|filter| {
            let globs = filter
                .extensions()
                .iter()
                .map(|extension| (0, format!("*.{extension}")));
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
    fn open(&self, window: &WindowRef, request: OpenRequest, answer: Resolver<OpenResult>) {
        let mut options = Options::new();
        options.insert("multiple", Variant::from(request.is_multiple()));
        options.insert("directory", Variant::from(request.picks_folders()));
        common(
            &mut options,
            request.accept_label(),
            request.directory(),
            request.filters(),
        );
        self.show(
            window,
            "OpenFile",
            request.title(),
            options,
            answer,
            |picks| (!picks.is_empty()).then_some(picks),
        );
    }

    fn save(&self, window: &WindowRef, request: SaveRequest, answer: Resolver<SaveResult>) {
        let mut options = Options::new();
        if let Some(name) = request.suggested_name() {
            options.insert("current_name", Variant::from(name.to_owned()));
        }
        common(
            &mut options,
            request.accept_label(),
            request.directory(),
            request.filters(),
        );
        self.show(
            window,
            "SaveFile",
            request.title(),
            options,
            answer,
            |picks| picks.into_iter().next(),
        );
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
            .with_mime_type("image/webp");
        assert_eq!(
            filters(&[filter]),
            [(
                "Images".to_owned(),
                vec![
                    (0, "*.png".to_owned()),
                    (0, "*.jpg".to_owned()),
                    (1, "image/webp".to_owned()),
                ]
            )]
        );
    }
}
