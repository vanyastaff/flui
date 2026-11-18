//! The file dialogs as `NSOpenPanel` and `NSSavePanel` sheets on the
//! acquiring widget's window.
//!
//! A filter becomes the panel's allowed content types (ADR-0360 §3, §4): an
//! extension the type macOS derives from it (`UTType typeWithFilenameExtension:`,
//! which matches ignoring case), a MIME type its type, and `image/*`,
//! `audio/*`, `video/*`, `text/*` and `font/*` the family's type
//! (`public.image`, …); another `type/*` names no family macOS has and is
//! refused with `NotOnThisPlatform` rather than widened. macOS derives a
//! file's type from the last part of its name, so `tar.gz` is filtered on
//! `gz`, and an open panel then checks each name itself through its
//! delegate (`panel:shouldEnableURL:`), which enables only a folder or a
//! file the contract's rule admits. A save panel appends the first allowed
//! type's extension to a typed name without one, which for a several-part
//! extension is its last part. A panel shows no filter menu, so every
//! filter's types are allowed at once and a save reports no selected
//! filter. Content types are macOS 11; below it a request with filters
//! answers `NotOnThisDevice`.
//!
//! The title is the panel's message (a sheet has no title bar) as well as
//! its title, the accept label its prompt. The panel ends with its window
//! and with its caller's dialog: the hold ends the sheet, and the completion
//! handler that ending runs answers nobody.

use std::path::PathBuf;

use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use crate::dialog::{
    DialogError, FileDialogCapability, FileFilter, OpenResult, PickedFile, Picks, SaveLocation,
    SaveResult,
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
    /// Whether an extension has several parts, which the content types
    /// match only by the last: an open panel then checks names itself.
    pub(super) checks_names: bool,
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
        checks_names: false,
    };
    let allow = |content_type: ContentType, types: &mut Vec<ContentType>| {
        if !types.contains(&content_type) {
            types.push(content_type);
        }
    };
    for requested in filters {
        for extension in requested.extensions() {
            let (_, last) = extension.rsplit_once('.').unwrap_or(("", extension));
            filter.checks_names |= last.len() != extension.len();
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

/// Whether a file named `name` ends in `.` and one of `extensions`,
/// ignoring ASCII case: the contract's rule for a name (ADR-0360 §3).
pub(super) fn name_matches(name: &str, extensions: &[String]) -> bool {
    let name = name.as_bytes();
    extensions.iter().any(|extension| {
        let extension = extension.as_bytes();
        name.len() > extension.len()
            && name[name.len() - extension.len() - 1] == b'.'
            && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
    })
}

/// One item the user picked, as the panel names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PanelPick {
    /// A file URL's path.
    Path(PathBuf),
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

/// `UTType` and the panels' content types, macOS 11: below it a request
/// with filters answers `NotOnThisDevice`. On arm64 the floor is 11, and
/// the types are always there.
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
    use std::path::PathBuf;

    use block2::RcBlock;
    use flui_platform::UnsupportedReason;
    use flui_platform::capability::{Resolver, WindowRef};
    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSModalResponse, NSOpenPanel, NSOpenSavePanelDelegate, NSSavePanel, NSWindow,
    };
    use objc2_foundation::{NSArray, NSNumber, NSString, NSURL};

    use super::super::appkit::ns_window;
    use super::super::sheet::Pending;
    use super::super::version::MacOSVersion;
    use super::{
        ABORTED, ContentType, PanelFilter, PanelPick, name_matches, open_answer, panel_filter,
        save_answer, unsupported,
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

    /// What the name check of an open panel needs: the request's extensions
    /// and the types of its MIME patterns.
    struct NameRule {
        extensions: Vec<String>,
        mime_types: Vec<Retained<AnyObject>>,
    }

    define_class!(
        /// An open panel's delegate that enables only a folder or a file the
        /// contract's rule admits, for a filter macOS matches by the last
        /// part of an extension alone.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FluiSystemPanelNameRule"]
        #[ivars = ContainedIvars<NameRule>]
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
        fn new(mtm: MainThreadMarker, rule: NameRule) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ContainedIvars::new(rule));
            // SAFETY: `NSObject`'s `init`, on an instance whose ivars are set.
            unsafe { msg_send![super(this), init] }
        }

        fn enables(&self, url: &NSURL) -> bool {
            os_entry(
                "panel:shouldEnableURL:",
                (),
                // A rule that failed hides nothing it cannot judge.
                AfterPanic::Answer(|()| true),
                || self.admits(url),
            )
        }

        /// Whether `url` is a folder to browse or a file the rule admits.
        #[expect(
            deprecated,
            reason = "its successor, NSURLContentTypeKey, is macOS 11 and a symbol the floor's Foundation lacks"
        )]
        fn admits(&self, url: &NSURL) -> bool {
            // SAFETY: Foundation's resource key constants live for the
            // process; each of these is in every macOS FLUI supports.
            let (folder_key, package_key, type_key) = unsafe {
                (
                    objc2_foundation::NSURLIsDirectoryKey,
                    objc2_foundation::NSURLIsPackageKey,
                    objc2_foundation::NSURLTypeIdentifierKey,
                )
            };
            let Ok(values) = url.resourceValuesForKeys_error(&NSArray::from_slice(&[
                folder_key,
                package_key,
                type_key,
            ])) else {
                return false;
            };
            let flag = |key| {
                values
                    .objectForKey(key)
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .is_some_and(|value| value.boolValue())
            };
            if flag(folder_key) && !flag(package_key) {
                return true;
            }
            let rule = &**self.ivars();
            let named = url
                .lastPathComponent()
                .is_some_and(|name| name_matches(&name.to_string(), &rule.extensions));
            named || self.conforms(&values.objectForKey(type_key), rule)
        }

        /// Whether a file of the type `identifier` is one of the rule's
        /// MIME types.
        fn conforms(&self, identifier: &Option<Retained<AnyObject>>, rule: &NameRule) -> bool {
            let Some(identifier) = identifier
                .as_ref()
                .and_then(|identifier| identifier.downcast_ref::<NSString>())
            else {
                return false;
            };
            let Some(class) = AnyClass::get(c"UTType") else {
                return false;
            };
            // SAFETY: a class method of `UTType` taking an `NSString`.
            let file_type: Option<Retained<AnyObject>> =
                unsafe { msg_send![class, typeWithIdentifier: identifier] };
            file_type.is_some_and(|file_type| {
                rule.mime_types.iter().any(|allowed| {
                    // SAFETY: `conformsToType:` takes a `UTType` and answers
                    // a `BOOL`; both are `UTType`s.
                    unsafe { msg_send![&*file_type, conformsToType: &**allowed] }
                })
            })
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
            if self.delegate.take().is_some() {
                // SAFETY: clearing the weak delegate before it is released.
                unsafe { self.panel.setDelegate(None) };
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

        /// Allow `filter`'s types on `panel`; a type the system does not
        /// know, or a macOS without content types, refuses the request.
        fn allow(&self, panel: &NSSavePanel, filter: &PanelFilter) -> Result<(), DialogError> {
            let proof = self
                .content_types
                .ok_or_else(|| unsupported(UnsupportedReason::NotOnThisDevice))?;
            let types = filter
                .types
                .iter()
                .map(|content_type| ut_type(proof, content_type))
                .collect::<Option<Vec<Retained<AnyObject>>>>()
                .ok_or_else(|| unsupported(UnsupportedReason::NotOnThisDevice))?;
            let types = NSArray::from_retained_slice(&types);
            // SAFETY: `setAllowedContentTypes:` takes an array of `UTType`s,
            // which `types` is, on a panel of the main thread.
            unsafe {
                let () = msg_send![panel, setAllowedContentTypes: &*types];
            }
            Ok(())
        }

        /// The delegate that checks names for an open panel whose filter
        /// matches a several-part extension by its last part.
        fn name_rule(&self, filter: &PanelFilter) -> Option<Retained<NameRuleDelegate>> {
            let proof = self.content_types?;
            filter.checks_names.then(|| {
                let mime_types = filter
                    .types
                    .iter()
                    .filter(|content_type| !matches!(content_type, ContentType::Extension(_)))
                    .filter_map(|content_type| ut_type(proof, content_type))
                    .collect::<Vec<Retained<AnyObject>>>();
                NameRuleDelegate::new(
                    self.mtm,
                    NameRule {
                        extensions: filter.extensions.clone(),
                        mime_types,
                    },
                )
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
        if let Some(directory) = directory.and_then(std::path::Path::to_str) {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(directory),
                true,
            )));
        }
    }

    /// A URL the panel answered, as a pick.
    fn pick(url: &NSURL) -> PanelPick {
        match url.path() {
            Some(path) if url.isFileURL() => PanelPick::Path(PathBuf::from(path.to_string())),
            _ => PanelPick::NotAFile,
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
            let mut delegate = None;
            let filtered = panel_filter(&request.filters).and_then(|filter| match filter {
                Some(filter) => {
                    self.allow(&panel, &filter)?;
                    delegate = self.name_rule(&filter);
                    Ok(())
                }
                None => Ok(()),
            });
            if let Err(refusal) = filtered {
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
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
                                    panel.URLs().iter().map(|url| pick(&url)).collect()
                                })
                            })
                        });
                    },
                )
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
                                    .map(|url| pick(&url))
                            })
                        });
                    },
                )
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
        ABORTED, ContentType, PanelPick, name_matches, open_answer, panel_filter, save_answer,
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
        assert!(filter.checks_names, "`gz` alone would admit `notes.gz`");

        let plain = panel_filter(&[FileFilter::new("Text").with_extension("txt")])
            .expect("maps")
            .expect("given");
        assert!(
            !plain.checks_names,
            "a one-part extension is matched exactly"
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
        let rule = extensions(&["tar.gz", "png"]);
        let cases = [
            ("backup.tar.gz", true),
            ("BACKUP.TAR.GZ", true),
            ("photo.Png", true),
            ("notes.gz", false),
            ("tar.gz", false),
            (".png", true),
            ("png", false),
            ("photo.png.txt", false),
            ("é.png", true),
        ];
        for (name, matches) in cases {
            assert_eq!(name_matches(name, &rule), matches, "{name}");
        }
    }

    #[test]
    fn an_ended_panel_answers_its_picks_a_cancel_or_why_it_could_not() {
        // Absolute on every host the model runs on.
        let file = std::env::temp_dir().join("report.pdf");
        let picks = open_answer(1, || vec![PanelPick::Path(file.clone())])
            .expect("accepted")
            .expect("a pick");
        assert_eq!(picks.first().path(), file);
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
        assert_eq!(saved.file().path(), file);
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
