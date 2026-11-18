//! The file dialogs as `NSOpenPanel` and `NSSavePanel` sheets on the
//! acquiring widget's window.
//!
//! A filter becomes what the panel allows (ADR-0360 §3, §4). From macOS 11
//! that is its allowed content types: an extension the type macOS derives
//! from its last part (`UTType typeWithFilenameExtension:`), a MIME type its
//! type, and `image/*`, `audio/*`, `video/*`, `text/*` and `font/*` the
//! family's type (`public.image`, …); another `type/*` names no family macOS
//! has and is refused with `NotOnThisPlatform` rather than widened. Below
//! macOS 11, on an Intel Mac, it is the allowed file types, the extensions'
//! last parts, and a filter with a MIME type answers `NotOnThisDevice`.
//!
//! macOS admits a file whose type conforms to an allowed one (`.swift` under
//! `txt`, `.docx` under `zip`) and derives a type from the last part of a
//! name, while the contract matches names exactly. So an open panel with a
//! filter checks each item itself through its delegate
//! (`panel:shouldEnableURL:`), which enables a folder to browse or a file
//! the contract's rule admits ([`judge_item`]): by its name first, which
//! reads nothing, then by its type against the MIME patterns. The item
//! judged is the one the panel answers, an alias's or a link's target, and
//! that target is what a pick hands out, so the delegate and the handle,
//! which holds every pick's name to the filters on every host
//! (`FileDialogs::open`), judge one file. The handle cannot tell a type, so
//! under a MIME pattern a pick whose name no extension admits and whose
//! type no pattern does is refused here, with the handle's own
//! `NotInFilter`; a pick whose type cannot be read is not refused for that.
//! A save panel appends the first allowed type's extension to a typed name
//! without one, which for a several-part extension is its last part; its
//! pick is not checked, since it names a file to create. A panel shows no
//! filter menu, so every filter is allowed at once and a save reports no
//! selected filter.
//!
//! The title is the panel's message (a sheet has no title bar) as well as
//! its title, the accept label its prompt. The panel ends with its window
//! and with its caller's dialog: the hold ends the sheet, and the completion
//! handler that ending runs answers nobody.

use std::path::PathBuf;

use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use crate::dialog::{
    DialogError, FileDialogCapability, FileFilter, OpenResult, PickedFile, Picks, SaveLocation,
    SaveResult, name_ends_in,
};

/// What a filter pattern asks the panel to allow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ContentType {
    /// The type of files whose name ends in `.` and this, the last part of
    /// an extension.
    Extension(String),
    /// The type of this MIME type.
    Mime(String),
    /// The system type of this identifier: a MIME type family.
    Identifier(&'static str),
}

/// What a request's filters ask of a panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PanelFilter {
    /// The content types the panel allows, each once.
    pub(super) types: Vec<ContentType>,
    /// Every extension of the request, for the name check.
    pub(super) extensions: Vec<String>,
}

impl PanelFilter {
    /// The file types a panel without content types allows: the
    /// extensions' last parts, or `None` when a MIME pattern needs a type.
    pub(super) fn file_types(&self) -> Option<Vec<String>> {
        self.types
            .iter()
            .map(|content_type| match content_type {
                ContentType::Extension(extension) => Some(extension.clone()),
                ContentType::Mime(_) | ContentType::Identifier(_) => None,
            })
            .collect::<Option<Vec<String>>>()
    }
}

/// The refusal of a request the panel cannot show, for `reason`.
fn unsupported(reason: UnsupportedReason) -> DialogError {
    DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason))
}

/// The content type a MIME type pattern is: the type itself, or a family
/// macOS has a type for.
fn mime_content_type(mime_type: &str) -> Result<ContentType, DialogError> {
    match mime_type.split_once('/') {
        Some((family, "*")) => {
            let identifier = match family {
                "image" => "public.image",
                "audio" => "public.audio",
                "video" => "public.movie",
                "text" => "public.text",
                "font" => "public.font",
                _ => return Err(unsupported(UnsupportedReason::NotOnThisPlatform)),
            };
            Ok(ContentType::Identifier(identifier))
        }
        _ => Ok(ContentType::Mime(mime_type.to_owned())),
    }
}

/// What `filters` ask of a panel; `None` when there are none, and the
/// panel allows every file.
pub(super) fn panel_filter(filters: &[FileFilter]) -> Result<Option<PanelFilter>, DialogError> {
    if filters.is_empty() {
        return Ok(None);
    }
    let mut filter = PanelFilter {
        types: Vec::new(),
        extensions: Vec::new(),
    };
    let allow = |content_type: ContentType, types: &mut Vec<ContentType>| {
        if !types.contains(&content_type) {
            types.push(content_type);
        }
    };
    for requested in filters {
        for extension in requested.extensions() {
            let (_, last) = extension.rsplit_once('.').unwrap_or(("", extension));
            allow(ContentType::Extension(last.to_owned()), &mut filter.types);
            if !filter.extensions.contains(extension) {
                filter.extensions.push(extension.clone());
            }
        }
        for mime_type in requested.mime_types() {
            allow(mime_content_type(mime_type)?, &mut filter.types);
        }
    }
    Ok(Some(filter))
}

/// Whether a file named `name` is one `extensions` name: the contract's
/// rule for a name (ADR-0360 §3), the one the handle checks a pick by.
pub(super) fn name_matches(name: &[u8], extensions: &[String]) -> bool {
    extensions
        .iter()
        .any(|extension| name_ends_in(name, extension))
}

/// What the contract's rule makes of one item of an open panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Judged {
    /// Its name ends in one of the extensions, or its type is one of the
    /// MIME patterns'.
    Admitted,
    /// Neither, and its type was read.
    Refused,
    /// Its name does not admit it, and its type could not be read.
    Unread,
}

/// Judge an item named `name` (`None` for one with no name, a root) by the
/// request's `extensions`, and by its type only when its name does not
/// admit it: `conforms` reads whether its type is one of the MIME patterns',
/// `None` when the file system does not say. A name that admits the item
/// reads nothing, so a file that cannot be read is never refused for it.
pub(super) fn judge_item(
    name: Option<&[u8]>,
    extensions: &[String],
    conforms: impl FnOnce() -> Option<bool>,
) -> Judged {
    if name.is_some_and(|name| name_matches(name, extensions)) {
        return Judged::Admitted;
    }
    match conforms() {
        Some(true) => Judged::Admitted,
        Some(false) => Judged::Refused,
        None => Judged::Unread,
    }
}

/// One item the user picked, as the panel names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PanelPick {
    /// A file at this path.
    Path(PathBuf),
    /// A file neither an extension nor a MIME pattern of the request
    /// admits, which the panel answered all the same.
    NotInFilter,
    /// A URL that names no file.
    NotAFile,
}

/// `NSModalResponseOK`.
const ACCEPTED: isize = 1;
/// `NSModalResponseCancel`.
const CANCELLED: isize = 0;
/// `NSModalResponseAbort`: the panel failed to show, or its hold ended it.
pub(super) const ABORTED: isize = -1001;

/// A pick as the handle hands it out.
fn picked(pick: PanelPick) -> Result<PickedFile, DialogError> {
    match pick {
        PanelPick::Path(path) => {
            PickedFile::try_new(path).ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
        }
        PanelPick::NotInFilter => Err(DialogError::NotInFilter),
        PanelPick::NotAFile => Err(DialogError::NotAFile),
    }
}

/// What an ended panel's `response` means, with what it picked read only
/// when the user accepted.
fn ended<T>(
    response: isize,
    accepted: impl FnOnce() -> Result<T, DialogError>,
) -> Result<Option<T>, DialogError> {
    match response {
        ACCEPTED => accepted().map(Some),
        CANCELLED => Ok(None),
        ABORTED => Err(unsupported(UnsupportedReason::Unavailable)),
        _ => Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
    }
}

/// What an open panel that ended with `response` answers.
pub(super) fn open_answer(response: isize, picks: impl FnOnce() -> Vec<PanelPick>) -> OpenResult {
    ended(response, || {
        let files = picks()
            .into_iter()
            .map(picked)
            .collect::<Result<Vec<PickedFile>, DialogError>>()?;
        // An accepted panel with nothing picked is outside its contract.
        Picks::try_new(files).ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
    })
}

/// What a save panel that ended with `response` answers: the file, and no
/// selected filter, which a panel without a filter menu has none of.
pub(super) fn save_answer(response: isize, pick: impl FnOnce() -> Option<PanelPick>) -> SaveResult {
    ended(response, || {
        let pick = pick().ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
        Ok(SaveLocation::new(picked(pick)?, None))
    })
}

/// `UTType` and the panels' content types, macOS 11: below it a filter is
/// the panel's allowed file types, and one with a MIME type answers
/// `NotOnThisDevice`. On arm64 the floor is 11, and the types are always
/// there.
#[cfg(not(target_arch = "aarch64"))]
pub(super) enum ContentTypes {}

#[cfg(not(target_arch = "aarch64"))]
impl super::version::Gate for ContentTypes {
    const SINCE: (u32, u32, u32) = (11, 0, 0);
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacDialogs;

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "UTType through the runtime, the panel's delegate"
)]
mod appkit {
    use block2::RcBlock;
    use flui_platform::UnsupportedReason;
    use flui_platform::capability::{Resolver, WindowRef};
    use std::rc::Rc;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, Message as _, define_class, msg_send,
    };
    use objc2_app_kit::{
        NSModalResponse, NSOpenPanel, NSOpenSavePanelDelegate, NSSavePanel, NSWindow,
    };
    use objc2_foundation::{NSArray, NSNumber, NSString, NSURL, NSURLBookmarkResolutionOptions};

    use super::super::appkit::{file_url, ns_window, url_path};
    use super::super::sheet::Pending;
    use super::super::version::MacOSVersion;
    use super::{
        ABORTED, ContentType, Judged, PanelFilter, PanelPick, judge_item, open_answer,
        panel_filter, save_answer, unsupported,
    };
    use crate::dialog::{
        DialogError, DialogHold, FileDialogBackend, OpenRequest, OpenResult, SaveRequest,
        SaveResult,
    };
    use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};

    /// Evidence that this macOS has `UTType` and the panels' content types.
    #[derive(Clone, Copy)]
    struct HasContentTypes;

    #[cfg(target_arch = "aarch64")]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the x86_64 build of the same function answers None below macOS 11"
    )]
    fn content_types(_version: MacOSVersion) -> Option<HasContentTypes> {
        Some(HasContentTypes)
    }

    #[cfg(not(target_arch = "aarch64"))]
    fn content_types(version: MacOSVersion) -> Option<HasContentTypes> {
        version
            .since::<super::ContentTypes>()
            .map(|_| HasContentTypes)
    }

    /// The `UTType` `content_type` names, looked up at run time: the class
    /// is in UniformTypeIdentifiers, which AppKit loads from macOS 11 on.
    fn ut_type(_: HasContentTypes, content_type: &ContentType) -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"UTType")?;
        // SAFETY: each is a class method of `UTType` taking an `NSString`
        // and returning a nullable, autoreleased `UTType`.
        unsafe {
            match content_type {
                ContentType::Extension(extension) => msg_send![
                    class,
                    typeWithFilenameExtension: &*NSString::from_str(extension)
                ],
                ContentType::Mime(mime_type) => {
                    msg_send![class, typeWithMIMEType: &*NSString::from_str(mime_type)]
                }
                ContentType::Identifier(identifier) => {
                    msg_send![class, typeWithIdentifier: &*NSString::from_str(identifier)]
                }
            }
        }
    }

    /// The contract's rule for an open panel's items: the request's
    /// extensions, matched by name, and the types of its MIME patterns
    /// (none below macOS 11, where such a request is refused).
    struct NameRule {
        extensions: Vec<String>,
        mime_types: Vec<Retained<AnyObject>>,
    }

    /// The item the panel answers for `url`: the target of an alias or a
    /// link, resolved without asking the user or mounting a volume, and
    /// `url` itself when it is neither or does not resolve.
    fn answered_item(url: &NSURL) -> Retained<NSURL> {
        NSURL::URLByResolvingAliasFileAtURL_options_error(
            url,
            NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutMounting,
        )
        .unwrap_or_else(|_| url.retain())
    }

    /// What the file system tells of an item beside its name.
    struct ItemKind {
        /// A folder the panel opens rather than picks: not a package.
        browsable: bool,
        /// Its uniform type identifier, when it has one.
        type_identifier: Option<Retained<NSString>>,
    }

    impl ItemKind {
        /// Read `item`'s kind; `None` when the file system does not say (a
        /// file gone since it was listed, a volume that stopped answering).
        #[expect(
            deprecated,
            reason = "its successor, NSURLContentTypeKey, is macOS 11 and a symbol the floor's Foundation lacks"
        )]
        fn read(item: &NSURL) -> Option<Self> {
            // SAFETY: Foundation's resource key constants live for the
            // process; each of these is in every macOS FLUI supports.
            let (folder_key, package_key, type_key) = unsafe {
                (
                    objc2_foundation::NSURLIsDirectoryKey,
                    objc2_foundation::NSURLIsPackageKey,
                    objc2_foundation::NSURLTypeIdentifierKey,
                )
            };
            let values = item
                .resourceValuesForKeys_error(&NSArray::from_slice(&[
                    folder_key,
                    package_key,
                    type_key,
                ]))
                .ok()?;
            let flag = |key| {
                values
                    .objectForKey(key)
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .is_some_and(|value| value.boolValue())
            };
            Some(Self {
                browsable: flag(folder_key) && !flag(package_key),
                type_identifier: values
                    .objectForKey(type_key)
                    .and_then(|identifier| identifier.downcast::<NSString>().ok()),
            })
        }
    }

    impl NameRule {
        /// What the rule makes of `item`, with its kind as far as it had
        /// to be read: `None` for an item its name admitted, or whose kind
        /// the file system does not say.
        fn judge(&self, item: &NSURL) -> (Judged, Option<ItemKind>) {
            use std::os::unix::ffi::OsStrExt as _;

            let path = url_path(item);
            let name = path
                .as_deref()
                .and_then(std::path::Path::file_name)
                .map(std::ffi::OsStr::as_bytes);
            let mut kind = None;
            let judged = judge_item(name, &self.extensions, || {
                let read = ItemKind::read(item)?;
                let conforms = self.conforms(read.type_identifier.as_deref());
                kind = Some(read);
                Some(conforms)
            });
            (judged, kind)
        }

        /// Whether a file of the type `identifier` is one of the rule's
        /// MIME types.
        fn conforms(&self, identifier: Option<&NSString>) -> bool {
            if self.mime_types.is_empty() {
                return false;
            }
            let (Some(identifier), Some(class)) = (identifier, AnyClass::get(c"UTType")) else {
                return false;
            };
            // SAFETY: a class method of `UTType` taking an `NSString`.
            let file_type: Option<Retained<AnyObject>> =
                unsafe { msg_send![class, typeWithIdentifier: identifier] };
            file_type.is_some_and(|file_type| {
                self.mime_types.iter().any(|allowed| {
                    // SAFETY: `conformsToType:` takes a `UTType` and answers
                    // a `BOOL`; both are `UTType`s.
                    unsafe { msg_send![&*file_type, conformsToType: &**allowed] }
                })
            })
        }

        /// Whether the panel shows `url` enabled: a file the rule admits,
        /// or a folder to browse. An item whose name does not admit it and
        /// whose kind cannot be read is not shown as one to choose.
        fn enables(&self, url: &NSURL) -> bool {
            let (judged, kind) = self.judge(&answered_item(url));
            judged == Judged::Admitted || kind.is_some_and(|kind| kind.browsable)
        }
    }

    define_class!(
        /// An open panel's delegate that enables only a folder to browse or
        /// a file the contract's rule admits.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FluiSystemPanelNameRule"]
        #[ivars = ContainedIvars<Rc<NameRule>>]
        struct NameRuleDelegate;

        unsafe impl NSObjectProtocol for NameRuleDelegate {}

        unsafe impl NSOpenSavePanelDelegate for NameRuleDelegate {
            #[unsafe(method(panel:shouldEnableURL:))]
            fn should_enable(&self, _sender: &AnyObject, url: &NSURL) -> bool {
                Self::enables(self, url)
            }
        }
    );

    impl NameRuleDelegate {
        fn new(mtm: MainThreadMarker, rule: Rc<NameRule>) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(rule));
            // SAFETY: `NSObject`'s `init`, on an instance whose ivars are set.
            unsafe { msg_send![super(this), init] }
        }

        fn enables(&self, url: &NSURL) -> bool {
            os_entry(
                "panel:shouldEnableURL:",
                (),
                // A pick is checked again when the panel answers.
                AfterPanic::Answer(|()| false),
                || self.ivars().enables(url),
            )
        }
    }

    /// Ends a panel's sheet when the dialog ends before the user answered.
    struct PanelHold<T: 'static> {
        pending: Pending<T>,
        panel: Retained<NSSavePanel>,
        parent: Retained<NSWindow>,
        /// The panel's delegate, which the panel holds weakly.
        delegate: Option<Retained<NameRuleDelegate>>,
    }

    impl<T: 'static> Drop for PanelHold<T> {
        fn drop(&mut self) {
            if self.pending.dismiss() {
                self.parent.endSheet_returnCode(&self.panel, ABORTED);
            }
            if self.delegate.is_some() {
                // SAFETY: the weak delegate is cleared while this value still
                // holds it, then released.
                unsafe { self.panel.setDelegate(None) };
                drop(self.delegate.take());
            }
        }
    }

    /// [`FileDialogBackend`] over the open and save panels.
    pub(crate) struct MacDialogs {
        mtm: MainThreadMarker,
        content_types: Option<HasContentTypes>,
    }

    impl MacDialogs {
        pub(crate) fn new(mtm: MainThreadMarker, version: MacOSVersion) -> Self {
            Self {
                mtm,
                content_types: content_types(version),
            }
        }

        /// Allow `filter` on `panel`: its content types from macOS 11, its
        /// extensions' last parts as file types below it. A type the system
        /// does not know, or a MIME pattern below macOS 11, refuses the
        /// request.
        fn allow(&self, panel: &NSSavePanel, filter: &PanelFilter) -> Result<(), DialogError> {
            let refused = || unsupported(UnsupportedReason::NotOnThisDevice);
            let Some(proof) = self.content_types else {
                let names = filter.file_types().ok_or_else(refused)?;
                let names = names
                    .iter()
                    .map(|name| NSString::from_str(name))
                    .collect::<Vec<Retained<NSString>>>();
                #[expect(
                    deprecated,
                    reason = "the content types replacing it are macOS 11, above this Mac"
                )]
                panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&names)));
                return Ok(());
            };
            let types = filter
                .types
                .iter()
                .map(|content_type| ut_type(proof, content_type))
                .collect::<Option<Vec<Retained<AnyObject>>>>()
                .ok_or_else(refused)?;
            let types = NSArray::from_retained_slice(&types);
            // SAFETY: `setAllowedContentTypes:` takes an array of `UTType`s,
            // which `types` is, on a panel of the main thread.
            unsafe {
                let () = msg_send![panel, setAllowedContentTypes: &*types];
            }
            Ok(())
        }

        /// The contract's rule an open panel's delegate and its picks are
        /// held to.
        fn name_rule(&self, filter: &PanelFilter) -> Rc<NameRule> {
            let mime_types = self.content_types.map_or_else(Vec::new, |proof| {
                filter
                    .types
                    .iter()
                    .filter(|content_type| !matches!(content_type, ContentType::Extension(_)))
                    .filter_map(|content_type| ut_type(proof, content_type))
                    .collect::<Vec<Retained<AnyObject>>>()
            });
            Rc::new(NameRule {
                extensions: filter.extensions.clone(),
                mime_types,
            })
        }
    }

    /// Tell `panel` what both dialogs share.
    fn configure(
        panel: &NSSavePanel,
        title: Option<&str>,
        accept_label: Option<&str>,
        directory: Option<&std::path::Path>,
    ) {
        if let Some(title) = title {
            let title = NSString::from_str(title);
            panel.setTitle(Some(&title));
            panel.setMessage(Some(&title));
        }
        if let Some(label) = accept_label {
            panel.setPrompt(Some(&NSString::from_str(label)));
        }
        if let Some(directory) = directory.and_then(|directory| file_url(directory, true)) {
            panel.setDirectoryURL(Some(&directory));
        }
    }

    /// A URL the panel answered, as a pick.
    ///
    /// Under a rule the pick is the item the delegate judged: an alias's or
    /// a link's target. The handle holds its name to the filters, so only
    /// its type is judged here, where a MIME pattern makes the name alone
    /// no answer, and only a type that was read refuses it.
    fn pick(url: &NSURL, rule: Option<&NameRule>) -> PanelPick {
        let Some(rule) = rule else {
            return url_path(url).map_or(PanelPick::NotAFile, PanelPick::Path);
        };
        let item = answered_item(url);
        let Some(path) = url_path(&item) else {
            return PanelPick::NotAFile;
        };
        if !rule.mime_types.is_empty() && rule.judge(&item).0 == Judged::Refused {
            PanelPick::NotInFilter
        } else {
            PanelPick::Path(path)
        }
    }

    /// The refusal of a dialog in a window with no AppKit window.
    fn no_window<T>(answer: Resolver<Result<T, DialogError>>) -> DialogHold
    where
        T: 'static,
    {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(Err(unsupported(UnsupportedReason::NoWindow)));
        DialogHold::new(())
    }

    impl FileDialogBackend for MacDialogs {
        fn open(
            &self,
            window: &WindowRef,
            request: OpenRequest,
            answer: Resolver<OpenResult>,
        ) -> DialogHold {
            let Some(parent) = ns_window(window, self.mtm) else {
                return no_window(answer);
            };
            let panel = NSOpenPanel::openPanel(self.mtm);
            configure(
                &panel,
                request.title.as_deref(),
                request.accept_label.as_deref(),
                request.directory.as_deref(),
            );
            let folders = request.picks_folders();
            panel.setCanChooseFiles(!folders);
            panel.setCanChooseDirectories(folders);
            panel.setCanCreateDirectories(folders);
            panel.setAllowsMultipleSelection(request.is_multiple());
            let mut rule = None;
            let filtered = panel_filter(&request.filters).and_then(|filter| match filter {
                Some(filter) => {
                    self.allow(&panel, &filter)?;
                    rule = Some(self.name_rule(&filter));
                    Ok(())
                }
                None => Ok(()),
            });
            if let Err(refusal) = filtered {
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
            let delegate = rule
                .as_ref()
                .map(|rule| NameRuleDelegate::new(self.mtm, Rc::clone(rule)));
            if let Some(delegate) = &delegate {
                // SAFETY: the delegate is the panel's until the hold clears
                // it, and the hold keeps it alive until then.
                unsafe { panel.setDelegate(Some(ProtocolObject::from_ref(&**delegate))) };
            }
            let pending = Pending::new(answer);
            let handler_pending = pending.clone();
            let answered = Weak::from_retained(&panel);
            let handler = RcBlock::new(move |response: NSModalResponse| {
                os_entry(
                    "NSOpenPanel completion handler",
                    response,
                    AfterPanic::Answer(|_| ()),
                    || {
                        handler_pending.answer(|| {
                            open_answer(response, || {
                                answered.load().map_or_else(Vec::new, |panel| {
                                    panel
                                        .URLs()
                                        .iter()
                                        .map(|url| pick(&url, rule.as_deref()))
                                        .collect::<Vec<PanelPick>>()
                                })
                            })
                        });
                    },
                );
            });
            panel.beginSheetModalForWindow_completionHandler(&parent, &handler);
            DialogHold::new(PanelHold {
                pending,
                panel: Retained::into_super(panel),
                parent,
                delegate,
            })
        }

        fn save(
            &self,
            window: &WindowRef,
            request: SaveRequest,
            answer: Resolver<SaveResult>,
        ) -> DialogHold {
            let Some(parent) = ns_window(window, self.mtm) else {
                return no_window(answer);
            };
            let panel = NSSavePanel::savePanel(self.mtm);
            configure(
                &panel,
                request.title.as_deref(),
                request.accept_label.as_deref(),
                request.directory.as_deref(),
            );
            panel.setCanCreateDirectories(true);
            if let Some(name) = &request.suggested_name {
                panel.setNameFieldStringValue(&NSString::from_str(name));
            }
            let filtered = panel_filter(&request.filters).and_then(|filter| match filter {
                Some(filter) => self.allow(&panel, &filter),
                None => Ok(()),
            });
            if let Err(refusal) = filtered {
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
            let pending = Pending::new(answer);
            let handler_pending = pending.clone();
            let answered = Weak::from_retained(&panel);
            let handler = RcBlock::new(move |response: NSModalResponse| {
                os_entry(
                    "NSSavePanel completion handler",
                    response,
                    AfterPanic::Answer(|_| ()),
                    || {
                        handler_pending.answer(|| {
                            save_answer(response, || {
                                answered
                                    .load()
                                    .and_then(|panel| panel.URL())
                                    .map(|url| pick(&url, None))
                            })
                        });
                    },
                );
            });
            panel.beginSheetModalForWindow_completionHandler(&parent, &handler);
            DialogHold::new(PanelHold {
                pending,
                panel,
                parent,
                delegate: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::{
        ABORTED, ContentType, Judged, PanelPick, judge_item, name_matches, open_answer,
        panel_filter, save_answer,
    };
    use crate::dialog::{DialogError, FileDialogCapability, FileFilter};

    fn extensions(list: &[&str]) -> Vec<String> {
        list.iter()
            .map(|extension| (*extension).to_owned())
            .collect()
    }

    #[test]
    fn a_filter_allows_each_type_once_and_a_several_part_extension_by_its_last_part() {
        let filters = [
            FileFilter::new("Archives")
                .with_extension("tar.gz")
                .with_extension("zip"),
            FileFilter::new("Images")
                .with_extension("PNG")
                .with_extension("zip")
                .with_mime_type("image/*")
                .with_mime_type("image/webp"),
        ];
        let filter = panel_filter(&filters)
            .expect("every pattern maps")
            .expect("filters were given");
        assert_eq!(
            filter.types,
            [
                ContentType::Extension("gz".to_owned()),
                ContentType::Extension("zip".to_owned()),
                ContentType::Extension("png".to_owned()),
                ContentType::Identifier("public.image"),
                ContentType::Mime("image/webp".to_owned()),
            ]
        );
        assert_eq!(filter.extensions, extensions(&["tar.gz", "zip", "png"]));
        assert_eq!(filter.file_types(), None, "a MIME pattern needs a type");

        let plain = panel_filter(&[FileFilter::new("Archives").with_extension("tar.gz")])
            .expect("maps")
            .expect("given");
        assert_eq!(
            plain.file_types(),
            Some(extensions(&["gz"])),
            "below macOS 11 a panel allows the last part, and the delegate checks the name"
        );
        assert_eq!(panel_filter(&[]), Ok(None), "no filter allows every file");
    }

    #[test]
    fn a_mime_family_macos_has_no_type_for_is_refused_rather_than_widened() {
        for family in ["image/*", "audio/*", "video/*", "text/*", "font/*"] {
            assert!(
                panel_filter(&[FileFilter::new("F").with_mime_type(family)]).is_ok(),
                "{family}"
            );
        }
        assert_eq!(
            panel_filter(&[FileFilter::new("Models").with_mime_type("model/*")]),
            Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::NotOnThisPlatform
            )))
        );
    }

    #[test]
    fn a_name_matches_as_the_contract_says() {
        let rule = extensions(&["tar.gz", "png", "txt", "zip"]);
        let cases: [(&[u8], bool); 13] = [
            (b"backup.tar.gz", true),
            (b"BACKUP.TAR.GZ", true),
            (b"photo.Png", true),
            (b"notes.gz", false),
            (b"tar.gz", false),
            // A name is longer than its extension and the dot before it.
            (b".png", false),
            (b"png", false),
            (b"photo.png.txt", true),
            ("é.png".as_bytes(), true),
            // A name that is not UTF-8 still ends in its extension.
            (b"\xff\xfe.png", true),
            // Types that conform to an allowed one are not its names.
            (b"main.swift", false),
            (b"photo.jpeg", false),
            (b"letter.docx", false),
        ];
        for (name, matches) in cases {
            assert_eq!(
                name_matches(name, &rule),
                matches,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn an_items_name_admits_it_before_anything_of_it_is_read() {
        let rule = extensions(&["png", "tar.gz"]);
        assert_eq!(
            judge_item(Some(b"photo.PNG"), &rule, || panic!(
                "a name that admits its file reads nothing of it"
            )),
            Judged::Admitted
        );
        /// An item's name, what its type's read answers, and the verdict.
        type Case<'a> = (Option<&'a [u8]>, Option<bool>, Judged);
        let cases: [Case<'_>; 5] = [
            // A file whose type is one of the MIME patterns'.
            (Some(b"scan"), Some(true), Judged::Admitted),
            (Some(b"main.swift"), Some(false), Judged::Refused),
            // A file gone since it was listed, a volume that stopped
            // answering: not read, so not refused.
            (Some(b"scan"), None, Judged::Unread),
            (Some(b"notes.gz"), None, Judged::Unread),
            (None, Some(false), Judged::Refused),
        ];
        for (name, conforms, judged) in cases {
            assert_eq!(
                judge_item(name, &rule, || conforms),
                judged,
                "{name:?} {conforms:?}"
            );
        }
    }

    #[test]
    fn an_ended_panel_answers_its_picks_a_cancel_or_why_it_could_not() {
        // Absolute on every host the model runs on.
        let file = std::env::temp_dir().join("report.pdf");
        let picks = open_answer(1, || vec![PanelPick::Path(file.clone())])
            .expect("accepted")
            .expect("a pick");
        assert_eq!(picks.first().path(), Some(file.as_path()));
        assert_eq!(
            open_answer(0, || panic!("a cancel reads no picks")),
            Ok(None)
        );
        assert_eq!(
            open_answer(1, Vec::new),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            "accepted with nothing picked"
        );
        assert_eq!(
            open_answer(1, || vec![PanelPick::NotAFile]),
            Err(DialogError::NotAFile)
        );
        assert_eq!(
            open_answer(1, || vec![
                PanelPick::Path(file.clone()),
                PanelPick::NotInFilter
            ]),
            Err(DialogError::NotInFilter),
            "a pick the filters do not admit refuses the answer as the handle does"
        );
        assert_eq!(
            open_answer(ABORTED, Vec::new),
            Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::Unavailable
            )))
        );
        assert_eq!(
            open_answer(7, Vec::new),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer))
        );

        let saved = save_answer(1, || Some(PanelPick::Path(file.clone())))
            .expect("accepted")
            .expect("a file");
        assert_eq!(saved.file().path(), Some(file.as_path()));
        assert_eq!(saved.filter(), None, "a panel has no filter menu to report");
        assert_eq!(
            save_answer(1, || None),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer))
        );
        assert_eq!(save_answer(0, || None), Ok(None));
    }

    #[cfg(not(target_arch = "aarch64"))]
    #[test]
    fn content_types_are_there_from_macos_11_on() {
        use super::super::version::MacOSVersion;
        use super::ContentTypes;

        assert!(
            MacOSVersion::new(10, 15, 7)
                .since::<ContentTypes>()
                .is_none()
        );
        assert!(
            MacOSVersion::new(11, 0, 0)
                .since::<ContentTypes>()
                .is_some()
        );
    }
}
