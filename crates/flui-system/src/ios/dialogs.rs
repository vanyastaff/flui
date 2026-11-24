//! The file dialogs as `UIDocumentPickerViewController`, presented from
//! the requesting window's top controller (`presenter`).
//!
//! **Open.** The picker opens documents in place, filtered by content type
//! as `apple::content_types` maps the request's filters (ADR-0360 §3, §4;
//! a type the application's bundle declares is found by its extension,
//! ADR-0361). The system admits a file whose type conforms to an allowed
//! one and matches a several-part extension by its last part, so every
//! pick is held to the contract's rule before any is read, and one the
//! request does not mean refuses the answer (`NotInFilter`). A filter of
//! every file shows every document, and so does a request with a filter
//! that names a file exactly, which no type expresses (ADR-0374); hidden
//! files are the picker's own to show or not. The picks are
//! then read under their security-scoped access, off the main thread and
//! within the request's `max_bytes` (`document_access`), and handed over as
//! name and contents. A folder dialog answers `NotYetOnThisHost`: a
//! folder's contents cannot be handed over, and a path to it would be one
//! the application cannot open once the access stops.
//!
//! **Save.** iOS has no dialog that names a file for the application to
//! write later: the user picks where an existing file goes. So a save
//! carries its contents, written off the main thread to a file of the name
//! the request gives it (`export`), and the picker exports that file,
//! moving it where the user chose; the answer is the file it wrote, by the
//! name it has there and its contents, with no selected filter (the picker
//! has no filter menu). A save without contents answers
//! `NotOnThisPlatform`, as on the web.
//!
//! The picker shows no title and no accept label. It answers exactly once:
//! a pick, a cancel, or a swipe that dismisses it, which is a cancel (its
//! own delegate and its presentation's both report an end, and the first
//! takes the caller). A picker whose dialog ends first (the caller let go,
//! its window closed) is dismissed by the hold and answers nobody; one that
//! other code dismisses is released by UIKit with its delegate, and the
//! caller's resolver with it, so the dialog ends unanswered rather than
//! waiting for good. `UTType` and the initializers that take content types
//! are iOS 14: below it an open without filters shows the picker of every
//! document, one with filters answers `NotOnThisDevice`, and an export is
//! shown by the initializer iOS 13 has.

use std::sync::Arc;

use flui_platform::{SystemFailure, UnsupportedReason};

use crate::apple::content_types::unsupported;
use crate::dialog::{
    DialogError, OpenRequest, PickedFile, SaveLocation, SaveRequest, SaveResult, save_file_name,
};

/// What a save's file is named when the request suggests no name.
const UNNAMED_EXPORT: &str = "Untitled";

/// Why an open is refused before the picker is asked: a folder dialog,
/// which the picker cannot answer with a folder the application can use.
pub(super) fn refused_open(request: &OpenRequest) -> Option<DialogError> {
    request
        .picks_folders()
        .then(|| unsupported(UnsupportedReason::NotYetOnThisHost))
}

/// What a save exports: the file's name and the contents written to it; a
/// save without contents, which iOS cannot answer, is refused.
pub(super) fn export_of(request: &SaveRequest) -> Result<(String, Arc<[u8]>), DialogError> {
    let contents = request
        .contents
        .clone()
        .ok_or_else(|| unsupported(UnsupportedReason::NotOnThisPlatform))?;
    let name = save_file_name(
        request.suggested_name.as_deref(),
        &request.filters,
        UNNAMED_EXPORT,
    );
    Ok((name, contents))
}

/// What an export the user placed answers: the file it wrote, by the name
/// it has where the user put it (`placed_as`, `None` when the picker named
/// no place) and the contents written there, with no selected filter.
pub(super) fn exported(placed_as: Option<&str>, contents: &Arc<[u8]>) -> SaveResult {
    let malformed = || DialogError::Failed(SystemFailure::MalformedAnswer);
    let name = placed_as.ok_or_else(malformed)?;
    let file = PickedFile::try_from_contents(name, Arc::clone(contents)).ok_or_else(malformed)?;
    Ok(Some(SaveLocation::new(file, None)))
}

/// The picker's export initializer, iOS 14: below it an export is shown by
/// the initializer that takes a mode, which copies the file instead of
/// moving it.
pub(super) enum ExportInitializer {}

impl crate::apple::version::Gate for ExportInitializer {
    type System = crate::apple::version::OnIOS;
    const SINCE: (u32, u32, u32) = (14, 0, 0);
}

#[cfg(target_os = "ios")]
pub(crate) use uikit::IOSDialogs;

#[cfg(target_os = "ios")]
#[expect(
    unsafe_code,
    reason = "the picker's content types, its delegates and their association"
)]
mod uikit {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
    use flui_platform::{SystemFailure, UnsupportedReason};
    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_foundation::{NSArray, NSString, NSURL};
    use objc2_ui_kit::{
        UIAdaptivePresentationControllerDelegate, UIDocumentPickerDelegate,
        UIDocumentPickerViewController, UIPresentationController, UIViewController,
    };

    use super::super::document_access::read_picks;
    use super::super::export::{ExportFile, keep_while_shown};
    use super::super::presenter::{Refusal, dismiss, guarded, present, when_presentable};
    use super::{ExportInitializer, export_of, exported, refused_open};
    use crate::apple::content_types::{
        ContentFilter, ContentType, HasContentTypes, content_filter, unsupported, ut_type,
    };
    use crate::apple::file_url;
    use crate::apple::modal::{Pending, WeakPending};
    use crate::apple::version::{IOSVersion, Since};
    use crate::dialog::{
        DialogError, DialogHold, FileDialogBackend, FileDialogCapability, OpenRequest, OpenResult,
        SaveRequest, SaveResult,
    };
    use crate::off_owner::OffOwner;
    use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};

    /// The uniform type identifier every document conforms to.
    const EVERY_DOCUMENT: &str = "public.item";

    /// How a picker ended.
    enum PickerEnd {
        /// The user picked these documents, or the export's destination.
        Picked(Vec<Retained<NSURL>>),
        /// The user cancelled, or dismissed the picker with a swipe.
        Cancelled,
    }

    /// What a picker's delegate hands each end it hears to.
    struct OnEnd {
        handler: Box<dyn Fn(PickerEnd)>,
    }

    define_class!(
        /// A picker's delegate and its presentation's: hands how the picker
        /// ended to its handler, whichever of the two heard it.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FluiSystemDocumentPickerEnd"]
        #[ivars = ContainedIvars<OnEnd>]
        struct PickerDelegate;

        unsafe impl NSObjectProtocol for PickerDelegate {}

        unsafe impl UIDocumentPickerDelegate for PickerDelegate {
            #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
            fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
                Self::picked(self, urls);
            }

            #[unsafe(method(documentPickerWasCancelled:))]
            fn was_cancelled(&self, _picker: &UIDocumentPickerViewController) {
                Self::cancelled(self);
            }
        }

        unsafe impl UIAdaptivePresentationControllerDelegate for PickerDelegate {
            #[unsafe(method(presentationControllerDidDismiss:))]
            fn did_dismiss(&self, _presentation: &UIPresentationController) {
                Self::cancelled(self);
            }
        }
    );

    impl PickerDelegate {
        fn new(mtm: MainThreadMarker, on_end: impl Fn(PickerEnd) + 'static) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(OnEnd {
                handler: Box::new(on_end),
            }));
            // SAFETY: `NSObject`'s `init`, on an instance whose ivars are set.
            unsafe { msg_send![super(this), init] }
        }

        fn picked(&self, urls: &NSArray<NSURL>) {
            os_entry(
                "documentPicker:didPickDocumentsAtURLs:",
                (),
                AfterPanic::Answer(|()| ()),
                || (self.ivars().handler)(PickerEnd::Picked(urls.to_vec())),
            );
        }

        fn cancelled(&self) {
            os_entry(
                "documentPickerWasCancelled: or presentationControllerDidDismiss:",
                (),
                AfterPanic::Answer(|()| ()),
                || (self.ivars().handler)(PickerEnd::Cancelled),
            );
        }

        /// Make `self` what `picker` reports a pick and a cancel to, and
        /// `picker` what keeps it. The delegate property is weak, so the
        /// picker also holds the delegate as an associated object: it lives
        /// exactly as long as the picker, and a picker UIKit releases
        /// unanswered, or never took, releases its handler with it.
        fn attach(&self, picker: &UIDocumentPickerViewController) {
            picker.setDelegate(Some(ProtocolObject::from_ref(self)));
            let picker: *const UIDocumentPickerViewController = picker;
            let delegate: *const Self = self;
            // SAFETY: both are live objects on the main thread; the key is
            // where the runtime keeps a selector's name, an address that is
            // the same for the process's life and is no other association's
            // key; the policy retains the delegate until the picker is
            // deallocated.
            unsafe {
                objc2::ffi::objc_setAssociatedObject(
                    picker.cast_mut().cast::<AnyObject>(),
                    sel!(fluiSystemDocumentPickerEnd)
                        .name()
                        .as_ptr()
                        .cast::<std::ffi::c_void>(),
                    delegate.cast_mut().cast::<AnyObject>(),
                    objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
                );
            }
        }

        /// Hear a swipe that dismisses `picker`, which only its
        /// presentation reports. Asked once UIKit took the picker: a
        /// controller that is never presented keeps the presentation made
        /// for it, and is kept by it, for good.
        fn hear_dismissal(&self, picker: &UIDocumentPickerViewController) {
            if let Some(presentation) = picker.presentationController() {
                // SAFETY: the property is weak, and the delegate lives as
                // long as the picker whose presentation this is.
                unsafe { presentation.setDelegate(Some(ProtocolObject::from_ref(self))) };
            }
        }
    }

    /// Present `picker`, which reports to `delegate`, from `from`. After a
    /// refusal nothing keeps the picker but its caller.
    fn present_picker(
        from: &UIViewController,
        picker: &UIDocumentPickerViewController,
        delegate: &PickerDelegate,
    ) -> Result<(), Refusal> {
        delegate.attach(picker);
        present(from, picker)?;
        delegate.hear_dismissal(picker);
        Ok(())
    }

    /// The picker a dialog shows, once it shows one: a save shows its
    /// picker only after its file was written. UIKit keeps the picker while
    /// it is up, and the picker its delegate; this keeps neither.
    type ShownPicker = Rc<Cell<Option<Weak<UIDocumentPickerViewController>>>>;

    /// Dismisses a picker when its dialog ends before the user answered.
    struct PickerHold<T: 'static> {
        pending: WeakPending<T>,
        shown: ShownPicker,
    }

    impl<T: 'static> Drop for PickerHold<T> {
        fn drop(&mut self) {
            if !self.pending.dismiss() {
                return;
            }
            let Some(picker) = self.shown.take().and_then(|picker| picker.load()) else {
                return;
            };
            // Nothing reports to the delegate after this, whatever the
            // picker still does on its way out.
            picker.setDelegate(None);
            if let Some(presentation) = picker.presentationController() {
                // SAFETY: clears a weak property.
                unsafe { presentation.setDelegate(None) };
            }
            dismiss(&picker);
        }
    }

    /// [`FileDialogBackend`] over the document picker.
    pub(crate) struct IOSDialogs {
        mtm: MainThreadMarker,
        content_types: Option<HasContentTypes>,
        export_initializer: Option<Since<ExportInitializer>>,
        queue: Rc<OffOwner>,
        completions: CompletionSource,
    }

    /// What a file dialog answers when nothing was presented.
    fn not_presented(refusal: Refusal) -> DialogError {
        refusal.dialog_error::<FileDialogCapability>()
    }

    /// Answer `answer` with `error` at once, the dialog showing nothing.
    fn refuse<T: 'static>(
        answer: Resolver<Result<T, DialogError>>,
        error: DialogError,
    ) -> DialogHold {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(Err(error));
        DialogHold::new(())
    }

    impl IOSDialogs {
        pub(crate) fn new(
            mtm: MainThreadMarker,
            version: IOSVersion,
            queue: Rc<OffOwner>,
            completions: CompletionSource,
        ) -> Self {
            Self {
                mtm,
                content_types: HasContentTypes::on(version),
                export_initializer: version.since::<ExportInitializer>(),
                queue,
                completions,
            }
        }

        /// A picker that opens documents in place, of `filter`'s types or
        /// of every type.
        #[expect(
            deprecated,
            reason = "iOS 13 has only the picker that takes type identifiers and a mode"
        )]
        fn opening_picker(
            &self,
            filter: Option<&ContentFilter>,
        ) -> Result<Retained<UIDocumentPickerViewController>, DialogError> {
            let not_here = || unsupported(UnsupportedReason::NotOnThisDevice);
            let Some(proof) = self.content_types else {
                if filter.is_some() {
                    return Err(not_here());
                }
                let every = NSArray::from_retained_slice(&[NSString::from_str(EVERY_DOCUMENT)]);
                return guarded("making a document picker", || {
                    UIDocumentPickerViewController::initWithDocumentTypes_inMode(
                        UIDocumentPickerViewController::alloc(self.mtm),
                        &every,
                        objc2_ui_kit::UIDocumentPickerMode::Open,
                    )
                })
                .map_err(not_presented);
            };
            let every = [ContentType::Identifier(EVERY_DOCUMENT)];
            let mut types = filter
                .map_or(&every[..], |filter| filter.types.as_slice())
                .iter()
                .map(|content_type| ut_type(proof, content_type))
                .collect::<Option<Vec<Retained<AnyObject>>>>()
                .ok_or_else(not_here)?;
            if filter.is_some_and(|filter| !filter.restricts_by_type()) {
                // A file named exactly has no type: every document is
                // shown, and each pick is held to the request's rule.
                types = vec![ut_type(proof, &every[0]).ok_or_else(not_here)?];
            }
            let types = NSArray::from_retained_slice(&types);
            let picker: Option<Retained<UIDocumentPickerViewController>> =
                guarded("making a document picker", || {
                    // SAFETY: `initForOpeningContentTypes:asCopy:` takes an array
                    // of `UTType`s, which `types` is, on a system that has it, as
                    // the proof says, and answers the picker or nil; the picker
                    // is made on the main thread.
                    unsafe {
                        msg_send![
                            UIDocumentPickerViewController::alloc(self.mtm),
                            initForOpeningContentTypes: &*types,
                            asCopy: false
                        ]
                    }
                })
                .map_err(not_presented)?;
            // A picker the system did not make is a failure it answered,
            // not one to panic on.
            picker.ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
        }
    }

    /// A picker that puts the file at `file` where the user chooses: moved
    /// there by the export initializer where the system has it, copied by
    /// the export mode below it.
    #[expect(
        deprecated,
        reason = "iOS 13 has only the picker that exports by a mode"
    )]
    fn exporting_picker(
        mtm: MainThreadMarker,
        export_initializer: Option<Since<ExportInitializer>>,
        file: &NSURL,
    ) -> Result<Retained<UIDocumentPickerViewController>, Refusal> {
        let files = NSArray::from_slice(&[file]);
        guarded("making an export picker", || {
            let picker = UIDocumentPickerViewController::alloc(mtm);
            match export_initializer {
                Some(_proof) => UIDocumentPickerViewController::initForExportingURLs_asCopy(
                    picker, &files, false,
                ),
                None => UIDocumentPickerViewController::initWithURLs_inMode(
                    picker,
                    &files,
                    objc2_ui_kit::UIDocumentPickerMode::ExportToService,
                ),
            }
        })
    }

    /// The name of the place the picker put an export at.
    fn placed_as(urls: &[Retained<NSURL>]) -> Option<String> {
        urls.first()
            .and_then(|url| url.lastPathComponent())
            .map(|name| name.to_string())
    }

    impl FileDialogBackend for IOSDialogs {
        fn open(
            &self,
            window: &WindowRef,
            request: OpenRequest,
            answer: Resolver<OpenResult>,
        ) -> DialogHold {
            if let Some(refusal) = refused_open(&request) {
                return refuse(answer, refusal);
            }
            let filter = match content_filter(&request.filters) {
                Ok(filter) => filter,
                Err(refusal) => return refuse(answer, refusal),
            };
            let picker = match self.opening_picker(filter.as_ref()) {
                Ok(picker) => picker,
                Err(refusal) => return refuse(answer, refusal),
            };
            picker.setAllowsMultipleSelection(request.is_multiple());
            picker.setShouldShowFileExtensions(true);
            let directory = request
                .directory
                .as_deref()
                .and_then(|directory| file_url(directory, true));
            if let Some(directory) = directory {
                picker.setDirectoryURL(Some(&directory));
            }
            let pending = Pending::new(answer);
            let ended = pending.clone();
            let queue = Rc::clone(&self.queue);
            let content_types = self.content_types;
            let limit = request.max_bytes;
            let keep_access = request.keep_access;
            let delegate = PickerDelegate::new(self.mtm, move |end| match end {
                PickerEnd::Cancelled => ended.answer(|| Ok(None)),
                PickerEnd::Picked(urls) => {
                    // The read answers the caller; a cancel or a dismissal
                    // that comes meanwhile answers nobody.
                    let Some(answer) = ended.take() else {
                        return;
                    };
                    let busy = Err(unsupported(UnsupportedReason::Unavailable));
                    let filter = filter.clone();
                    queue.dispatch(answer, busy, move || {
                        read_picks(urls, filter.as_ref(), content_types, limit, keep_access)
                    });
                }
            });
            let hold = PickerHold {
                pending: pending.downgrade(),
                shown: ShownPicker::default(),
            };
            let shown = Rc::clone(&hold.shown);
            // Until the picker is up, this closure is what keeps the
            // caller's resolver; then the picker's delegate is.
            when_presentable(window, self.mtm, move |from| {
                if !pending.is_waiting() {
                    // The dialog ended while a controller was leaving.
                    return;
                }
                match from.and_then(|from| present_picker(&from, &picker, &delegate)) {
                    Ok(()) => shown.set(Some(Weak::from_retained(&picker))),
                    // UIKit took nothing: no delegate method will run.
                    Err(refusal) => pending.answer(|| Err(not_presented(refusal))),
                }
            });
            DialogHold::new(hold)
        }

        fn save(
            &self,
            window: &WindowRef,
            request: SaveRequest,
            answer: Resolver<SaveResult>,
        ) -> DialogHold {
            let (name, contents) = match export_of(&request) {
                Ok(export) => export,
                Err(refusal) => return refuse(answer, refusal),
            };
            let (written, write) = self
                .completions
                .pending::<Result<ExportFile, DialogError>>();
            let busy = Err(unsupported(UnsupportedReason::Unavailable));
            let to_write = Arc::clone(&contents);
            self.queue
                .dispatch(write, busy, move || ExportFile::write(&name, &to_write));

            let pending = Pending::new(answer);
            let hold = PickerHold {
                pending: pending.downgrade(),
                shown: ShownPicker::default(),
            };
            let shown = Rc::clone(&hold.shown);
            let mtm = self.mtm;
            let export_initializer = self.export_initializer;
            let window = window.clone();
            let directory = request.directory;
            // Until the picker is up, this closure is what keeps the
            // caller's resolver; then the picker's delegate is.
            let mut waiting = Some((pending, contents));
            written.on_complete(move |outcome| {
                let Some((pending, contents)) = waiting.take() else {
                    return;
                };
                let export = match outcome {
                    Ok(Ok(export)) => export,
                    Ok(Err(refusal)) => return pending.answer(|| Err(refusal)),
                    // The write's work item panicked: the resolver drops
                    // here, and the caller's completion ends as the write's
                    // did.
                    Err(_) => return,
                };
                if !pending.is_waiting() {
                    // The dialog ended while the file was written; dropping
                    // the file removes it.
                    return;
                }
                let Some(file) = file_url(export.path(), false) else {
                    return pending
                        .answer(|| Err(DialogError::Failed(SystemFailure::MalformedAnswer)));
                };
                let picker = match exporting_picker(mtm, export_initializer, &file) {
                    Ok(picker) => picker,
                    Err(refusal) => return pending.answer(|| Err(not_presented(refusal))),
                };
                let directory = directory
                    .as_deref()
                    .and_then(|directory| file_url(directory, true));
                if let Some(directory) = directory {
                    picker.setDirectoryURL(Some(&directory));
                }
                // The file lives as long as the picker that is up, kept by
                // the picker's delegate, whichever way the export ends.
                let kept: Rc<Cell<Option<ExportFile>>> = Rc::default();
                let keeps = Rc::clone(&kept);
                let ended = pending.clone();
                let delegate = PickerDelegate::new(mtm, move |end| {
                    let _keeps = &keeps;
                    match end {
                        PickerEnd::Cancelled => ended.answer(|| Ok(None)),
                        PickerEnd::Picked(urls) => {
                            ended.answer(|| exported(placed_as(&urls).as_deref(), &contents));
                        }
                    }
                });
                let shown = Rc::clone(&shown);
                when_presentable(&window, mtm, move |from| {
                    if !pending.is_waiting() {
                        // The dialog ended while a controller was leaving;
                        // dropping the file removes it.
                        return;
                    }
                    let presented = from.and_then(|from| present_picker(&from, &picker, &delegate));
                    match keep_while_shown(export, &kept, presented) {
                        Ok(()) => shown.set(Some(Weak::from_retained(&picker))),
                        // UIKit took nothing: no delegate method will run,
                        // and the file is gone already.
                        Err(refusal) => pending.answer(|| Err(not_presented(refusal))),
                    }
                });
            });
            DialogHold::new(hold)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::{ExportInitializer, export_of, exported, refused_open};
    use crate::apple::version::IOSVersion;
    use crate::dialog::{
        DialogError, FileDialogCapability, FileFilter, OpenRequest, PickedForm, SaveRequest,
    };

    fn refusal(reason: UnsupportedReason) -> DialogError {
        DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason))
    }

    #[test]
    fn a_folder_is_not_picked_and_a_save_carries_what_it_exports() {
        assert_eq!(
            refused_open(&OpenRequest::folders()),
            Some(refusal(UnsupportedReason::NotYetOnThisHost))
        );
        assert_eq!(refused_open(&OpenRequest::files().allow_multiple()), None);

        assert_eq!(
            export_of(&SaveRequest::new().suggested_name("notes")),
            Err(refusal(UnsupportedReason::NotOnThisPlatform)),
            "a save with nothing to export"
        );
        let contents: Arc<[u8]> = Arc::from(&b"hello"[..]);
        let request = SaveRequest::new()
            .suggested_name("notes")
            .filter(FileFilter::new("Text").extension("txt"))
            .contents(Arc::clone(&contents));
        assert_eq!(export_of(&request), Ok(("notes.txt".to_owned(), contents)));
        let unnamed = SaveRequest::new().contents(Arc::<[u8]>::from(&b""[..]));
        assert_eq!(
            export_of(&unnamed).map(|(name, _)| name),
            Ok("Untitled".to_owned())
        );
    }

    #[test]
    fn an_export_answers_the_file_it_wrote_by_the_name_it_was_placed_under() {
        let contents: Arc<[u8]> = Arc::from(&b"hello"[..]);
        let saved = exported(Some("notes 2.txt"), &contents)
            .expect("placed")
            .expect("a file");
        assert_eq!(
            saved.file().form(),
            PickedForm::Contents {
                name: "notes 2.txt",
                contents: b"hello"
            },
            "the system renamed it beside a file of its name"
        );
        assert_eq!(saved.filter(), None, "the picker has no filter menu");
        assert_eq!(saved.file().path(), None);
        assert_eq!(
            exported(None, &contents),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            "an export placed nowhere"
        );
    }

    #[test]
    fn the_export_initializer_is_there_from_ios_14_on() {
        assert!(
            IOSVersion::new(13, 7, 0)
                .since::<ExportInitializer>()
                .is_none()
        );
        assert!(
            IOSVersion::new(14, 0, 0)
                .since::<ExportInitializer>()
                .is_some()
        );
    }
}
