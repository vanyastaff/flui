//! The file dialogs as `NSOpenPanel` and `NSSavePanel` sheets on the
//! acquiring widget's window.
//!
//! A filter becomes what the panel allows (ADR-0360 §3, §4), as
//! `apple::content_types` maps it. From macOS 11 that is its allowed
//! content types; below macOS 11, on an Intel Mac, it is the allowed file
//! types, the extensions' last parts, and a filter with a MIME type answers
//! `NotOnThisDevice`.
//!
//! macOS admits a file whose type conforms to an allowed one (`.swift` under
//! `txt`, `.docx` under `zip`) and derives a type from the last part of a
//! name, while the contract matches names exactly. So an open panel with a
//! filter checks each item itself through its delegate
//! (`panel:shouldEnableURL:`), which enables a folder to browse or a file
//! the contract's rule admits (`apple::content_types::judge_item`): by its
//! name first, which reads nothing, then by its type against the MIME
//! patterns. The item judged is the one the panel answers, an alias's or a
//! link's target, and that target is what a pick hands out, so the delegate
//! and the handle, which holds every pick's name to the filters on every
//! host (`FileDialogs::open`), judge one file. The handle cannot tell a
//! type, so under a MIME pattern a pick whose name no extension admits and
//! whose type no pattern does is refused here, with the handle's own
//! `NotInFilter`; a pick whose type cannot be read is not refused for that.
//! A save panel appends the first allowed type's extension to a typed name
//! without one, which for a several-part extension is its last part; its
//! pick is not checked, since it names a file to create. A panel shows no
//! filter menu, so every filter is allowed at once and a save reports no
//! selected filter.
//!
//! A filter of every file leaves the panel as one with no filter: with no
//! menu to select it in, offering it beside others allows every file. A
//! filter that names a file exactly (`Makefile`) has no content type to
//! allow, so the panel is given no allowed types at all (ADR-0372): an open
//! panel's delegate then enables only what the request's filters name or
//! type, as it does under any filter, and a save panel takes any name and
//! appends nothing.
//!
//! The title is the panel's message (a sheet has no title bar) as well as
//! its title, the accept label its prompt. The panel ends with its window
//! and with its caller's dialog: the hold ends the sheet, and the completion
//! handler that ending runs answers nobody. AppKit answers no panel at all
//! where the process may not show one (an App Sandbox without the
//! user-selected-file entitlement, the panel service failing): the request
//! then answers `Unavailable`.

use std::path::PathBuf;

use flui_platform::{SystemFailure, UnsupportedReason};

use crate::apple::content_types::unsupported;
use crate::dialog::{DialogError, OpenResult, PickedFile, Picks, SaveLocation, SaveResult};

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

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacDialogs;

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "the panel's content types and its delegate")]
mod appkit {
    use block2::RcBlock;
    use flui_platform::UnsupportedReason;
    use flui_platform::capability::{Resolver, WindowRef};
    use std::rc::Rc;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{
        ClassType as _, DefinedClass, MainThreadMarker, MainThreadOnly, Message as _, define_class,
        msg_send,
    };
    use objc2_app_kit::{
        NSModalResponse, NSOpenPanel, NSOpenSavePanelDelegate, NSSavePanel, NSWindow,
    };
    use objc2_foundation::{NSArray, NSNumber, NSString, NSURL, NSURLBookmarkResolutionOptions};

    use super::super::appkit::ns_window;
    use super::{ABORTED, PanelPick, open_answer, save_answer};
    use crate::apple::content_types::{
        ContentFilter, HasContentTypes, Judged, NameRule, content_filter, unsupported, ut_type,
    };
    use crate::apple::modal::Pending;
    use crate::apple::version::MacOSVersion;
    use crate::apple::{file_url, url_path};
    use crate::dialog::{
        DialogError, DialogHold, FileDialogBackend, OpenRequest, OpenResult, SaveRequest,
        SaveResult,
    };
    use crate::owner::panic_boundary::{AfterPanic, ContainedIvars, os_entry};

    /// Where [`MacDialogs`] gets its panels: AppKit's own, or, in a test, a
    /// source that makes none.
    ///
    /// AppKit answers no panel at all where the process may not show one
    /// (an App Sandbox without the user-selected-file entitlement) or the
    /// panel service did not answer, which a later request may find
    /// changed: the request then answers `Unavailable`.
    #[derive(Clone, Copy)]
    struct Panels {
        open: fn(MainThreadMarker) -> Option<Retained<NSOpenPanel>>,
        save: fn(MainThreadMarker) -> Option<Retained<NSSavePanel>>,
    }

    /// AppKit's panels.
    const APPKIT_PANELS: Panels = Panels {
        open: open_panel,
        save: save_panel,
    };

    /// A new open panel; `None` where AppKit makes none.
    ///
    /// Sent by hand: the binding's `openPanel` declares a panel that is
    /// never nil and panics on the nil AppKit answers in an App Sandbox
    /// without the user-selected-file entitlement, or when the panel
    /// service fails.
    fn open_panel(_mtm: MainThreadMarker) -> Option<Retained<NSOpenPanel>> {
        // SAFETY: `+[NSOpenPanel openPanel]` takes no argument and answers an
        // autoreleased `NSOpenPanel` or nil, on the main thread, which the
        // marker proves this is.
        unsafe { msg_send![NSOpenPanel::class(), openPanel] }
    }

    /// A new save panel; `None` where AppKit makes none, as [`open_panel`].
    fn save_panel(_mtm: MainThreadMarker) -> Option<Retained<NSSavePanel>> {
        // SAFETY: `+[NSSavePanel savePanel]` takes no argument and answers an
        // autoreleased `NSSavePanel` or nil, on the main thread, which the
        // marker proves this is.
        unsafe { msg_send![NSSavePanel::class(), savePanel] }
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

    /// What `rule` makes of `item`, with its kind as far as it had to be
    /// read: `None` for an item its name admitted, or whose kind the file
    /// system does not say.
    fn judge(rule: &NameRule, item: &NSURL) -> (Judged, Option<ItemKind>) {
        let mut kind = None;
        let judged = rule.judge(item, || {
            let read = ItemKind::read(item)?;
            let conforms = rule.conforms(read.type_identifier.as_deref());
            kind = Some(read);
            Some(conforms)
        });
        (judged, kind)
    }

    /// Whether the panel shows `url` enabled under `rule`: a file the rule
    /// admits, or a folder to browse. An item whose name does not admit it
    /// and whose kind cannot be read is not shown as one to choose.
    fn rule_enables(rule: &NameRule, url: &NSURL) -> bool {
        let (judged, kind) = judge(rule, &answered_item(url));
        judged == Judged::Admitted || kind.is_some_and(|kind| kind.browsable)
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
                || rule_enables(self.ivars(), url),
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
        panels: Panels,
    }

    impl MacDialogs {
        pub(crate) fn new(mtm: MainThreadMarker, version: MacOSVersion) -> Self {
            Self::with_panels(mtm, version, APPKIT_PANELS)
        }

        fn with_panels(mtm: MainThreadMarker, version: MacOSVersion, panels: Panels) -> Self {
            Self {
                mtm,
                content_types: HasContentTypes::on(version),
                panels,
            }
        }

        /// Allow `filter` on `panel`: its content types from macOS 11, its
        /// extensions' last parts as file types below it. A type the system
        /// does not know, or a MIME pattern below macOS 11, refuses the
        /// request. A filter that names a file exactly is checked the same
        /// and then leaves the panel allowing every file, since no type
        /// expresses a name.
        fn allow(&self, panel: &NSSavePanel, filter: &ContentFilter) -> Result<(), DialogError> {
            let refused = || unsupported(UnsupportedReason::NotOnThisDevice);
            let Some(proof) = self.content_types else {
                let names = filter.file_types().ok_or_else(refused)?;
                if !filter.restricts_by_type() {
                    return Ok(());
                }
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
            if !filter.restricts_by_type() {
                return Ok(());
            }
            let types = NSArray::from_retained_slice(&types);
            // SAFETY: `setAllowedContentTypes:` takes an array of `UTType`s,
            // which `types` is, on a panel of the main thread.
            unsafe {
                let () = msg_send![panel, setAllowedContentTypes: &*types];
            }
            Ok(())
        }
    }

    /// Tell `panel` what both dialogs share. Hidden files are shown when the
    /// request asks, and otherwise as the panel finds the user's choice
    /// (⌘⇧.).
    fn configure(
        panel: &NSSavePanel,
        title: Option<&str>,
        accept_label: Option<&str>,
        directory: Option<&std::path::Path>,
        show_hidden: bool,
    ) {
        if show_hidden {
            panel.setShowsHiddenFiles(true);
        }
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
        if rule.judges_types() && judge(rule, &item).0 == Judged::Refused {
            PanelPick::NotInFilter
        } else {
            PanelPick::Path(path)
        }
    }

    /// Answer `refusal` for a dialog that is not shown: nothing is held.
    fn refuse<T>(answer: Resolver<Result<T, DialogError>>, refusal: DialogError) -> DialogHold
    where
        T: 'static,
    {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(Err(refusal));
        DialogHold::new(())
    }

    /// The refusal of a dialog in a window with no AppKit window.
    fn no_window<T>(answer: Resolver<Result<T, DialogError>>) -> DialogHold
    where
        T: 'static,
    {
        refuse(answer, unsupported(UnsupportedReason::NoWindow))
    }

    impl FileDialogBackend for MacDialogs {
        fn open(
            &self,
            window: &WindowRef,
            request: OpenRequest,
            answer: Resolver<OpenResult>,
        ) -> DialogHold {
            let Some(panel) = (self.panels.open)(self.mtm) else {
                return refuse(answer, unsupported(UnsupportedReason::Unavailable));
            };
            let Some(parent) = ns_window(window, self.mtm) else {
                return no_window(answer);
            };
            configure(
                &panel,
                request.title.as_deref(),
                request.accept_label.as_deref(),
                request.directory.as_deref(),
                request.show_hidden,
            );
            let folders = request.picks_folders();
            panel.setCanChooseFiles(!folders);
            panel.setCanChooseDirectories(folders);
            panel.setCanCreateDirectories(folders);
            panel.setAllowsMultipleSelection(request.is_multiple());
            let mut rule = None;
            let filtered = content_filter(&request.filters).and_then(|filter| match filter {
                Some(filter) => {
                    self.allow(&panel, &filter)?;
                    rule = Some(Rc::new(NameRule::new(self.content_types, &filter)));
                    Ok(())
                }
                None => Ok(()),
            });
            if let Err(refusal) = filtered {
                return refuse(answer, refusal);
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
            let Some(panel) = (self.panels.save)(self.mtm) else {
                return refuse(answer, unsupported(UnsupportedReason::Unavailable));
            };
            let Some(parent) = ns_window(window, self.mtm) else {
                return no_window(answer);
            };
            configure(
                &panel,
                request.title.as_deref(),
                request.accept_label.as_deref(),
                request.directory.as_deref(),
                request.show_hidden,
            );
            panel.setCanCreateDirectories(true);
            if let Some(name) = &request.suggested_name {
                panel.setNameFieldStringValue(&NSString::from_str(name));
            }
            let filtered = content_filter(&request.filters).and_then(|filter| match filter {
                Some(filter) => self.allow(&panel, &filter),
                None => Ok(()),
            });
            if let Err(refusal) = filtered {
                return refuse(answer, refusal);
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

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;
        use std::rc::Rc;

        use flui_platform::capability::Abandoned;
        use flui_platform::{Unsupported, UnsupportedReason};
        use objc2::MainThreadMarker;
        use objc2::rc::Retained;
        use objc2_app_kit::{NSOpenPanel, NSSavePanel};

        use super::{MacDialogs, Panels};
        use crate::apple::version::MacOSVersion;
        use crate::dialog::{
            DialogError, FileDialogCapability, FileDialogs, OpenRequest, OpenResult, SaveRequest,
            SaveResult,
        };
        use crate::testing::owner::TestOwner;
        use crate::testing::window::window;

        fn no_open_panel(_: MainThreadMarker) -> Option<Retained<NSOpenPanel>> {
            None
        }

        fn no_save_panel(_: MainThreadMarker) -> Option<Retained<NSSavePanel>> {
            None
        }

        /// AppKit in an App Sandbox without the user-selected-file
        /// entitlement: no panel at all.
        const NO_PANELS: Panels = Panels {
            open: no_open_panel,
            save: no_save_panel,
        };

        /// Turn `owner` until the completion `into` waits for lands.
        fn landed<T: 'static>(
            owner: &TestOwner,
            into: &Rc<RefCell<Option<Result<T, Abandoned>>>>,
        ) -> T {
            owner.turn_until(|| into.borrow().is_some());
            into.borrow_mut()
                .take()
                .expect("landed")
                .expect("not abandoned")
        }

        #[test]
        fn a_panel_appkit_does_not_make_answers_unavailable_and_frees_the_window() {
            // SAFETY: no AppKit call is made: the panel source answers nil
            // before anything main-thread-only is touched, and the window
            // has no AppKit handle to read.
            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let owner = TestOwner::new();
            let (window, _closer) = window();
            let dialogs = FileDialogs::try_new(
                MacDialogs::with_panels(mtm, MacOSVersion::new(14, 0, 0), NO_PANELS),
                &owner.ctx(Some(window)),
            )
            .expect("a window has dialogs");
            let unavailable = Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::Unavailable
            )));

            let opened: Rc<RefCell<Option<Result<OpenResult, Abandoned>>>> = Rc::default();
            let into = Rc::clone(&opened);
            dialogs
                .open(OpenRequest::files())
                .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
            assert_eq!(
                landed(&owner, &opened),
                unavailable,
                "a refusal, not a panic"
            );

            // The window is free for the next dialog.
            let saved: Rc<RefCell<Option<Result<SaveResult, Abandoned>>>> = Rc::default();
            let into = Rc::clone(&saved);
            dialogs
                .save(SaveRequest::new())
                .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
            assert_eq!(landed(&owner, &saved).map(|_| ()), unavailable.map(|_| ()));
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::{ABORTED, PanelPick, open_answer, save_answer};
    use crate::dialog::{DialogError, FileDialogCapability};

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
}
