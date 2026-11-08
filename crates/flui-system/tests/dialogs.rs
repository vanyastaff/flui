//! The file dialogs: a widget's dialog reaching the system modal to its
//! window, one dialog per window, and requests refused at the seam.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{acquire, outcome_slot, turn};
use flui_platform::capability::{
    Abandoned, CapabilityRegistrar, Completion, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_system::dialog::{
    DialogError, FileDialogBackend, FileDialogCapability, FileDialogs, OpenRequest, OpenResult,
    SaveRequest, SaveResult,
};

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

#[test]
fn a_dialog_absence_carries_its_unsupported_value() {
    use std::error::Error;

    use flui_platform::{Unsupported, UnsupportedReason};

    let unsupported = Unsupported::of::<FileDialogCapability>(UnsupportedReason::Unavailable);
    let error = DialogError::from(unsupported);
    assert_eq!(error.unsupported(), Some(unsupported));
    assert_eq!(
        error.to_string(),
        "the dialog service is unavailable: unsupported: file-dialog: unavailable for now"
    );
    assert_eq!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<Unsupported>()),
        Some(&unsupported)
    );
    assert_eq!(DialogError::Busy.unsupported(), None);
}

/// A backend that drops every resolver unanswered, as one whose OS call
/// failed before it could answer does.
#[derive(Clone)]
struct ForgetfulBackend;

impl FileDialogBackend for ForgetfulBackend {
    fn open(&self, _window: &WindowRef, _request: OpenRequest, answer: Resolver<OpenResult>) {
        drop(answer);
    }

    fn save(&self, _window: &WindowRef, _request: SaveRequest, answer: Resolver<SaveResult>) {
        drop(answer);
    }
}

struct ForgetfulPlugin;

impl Plugin for ForgetfulPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<FileDialogCapability>(|cx: &ProviderContext<'_>| {
            FileDialogs::try_new(ForgetfulBackend, cx)
        });
    }
}

#[test]
fn a_backend_that_drops_its_answer_ends_the_dialog_and_frees_the_window() {
    let (mut host, handle) = acquire::<FileDialogCapability>(&ForgetfulPlugin);
    let handle = handle.expect("the plugin provides dialogs");

    let first = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(*first.borrow(), Some(Err(Abandoned::ProviderDropped)));

    let second = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(
        *second.borrow(),
        Some(Err(Abandoned::ProviderDropped)),
        "not Busy: the dropped answer freed the window"
    );
}
