//! Autostart as the application's own login item: ServiceManagement's
//! `SMAppService.mainAppService`, macOS 13 and later, which System Settings'
//! Login Items lists and the user switches on and off there.
//!
//! The user's switch-off is never overwritten: the service then reports
//! `requiresApproval`, and a request to enable answers
//! [`AutostartState::Disabled`], what the system grants, without registering
//! again. Registering asks nobody (the system posts its own notice), so a
//! request's reason is not shown. Below macOS 13 the service is not there
//! and every request answers `NotOnThisDevice`. Each request runs off the
//! main thread (`queue::WorkQueue`), since the service waits on its daemon.

use flui_platform::{Unsupported, UnsupportedReason};

use super::{ErrorCode, failure};
use crate::autostart::{
    AutostartCapability, AutostartError, AutostartRequest, AutostartResult, AutostartState,
};

/// What `SMAppService.status` reports of the login item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemStatus {
    /// Not registered.
    NotRegistered,
    /// Registered and on: the system starts the application at login.
    Enabled,
    /// Registered, and switched off by the user in System Settings.
    RequiresApproval,
    /// The system finds no such service: the application is not a bundle
    /// the service can name.
    NotFound,
}

/// The login item as the provider reaches it, off the main thread. A
/// failure is the `NSError`'s code.
pub(super) trait LoginItem {
    fn status(&self) -> ItemStatus;
    fn register(&self) -> Result<(), ErrorCode>;
    fn unregister(&self) -> Result<(), ErrorCode>;
}

/// ServiceManagement's codes the provider names (`SMErrors.h`).
mod code {
    /// The service is not registered.
    pub(super) const JOB_NOT_FOUND: isize = 6;
    /// The user refused to let the application run at login.
    pub(super) const LAUNCH_DENIED_BY_USER: isize = 11;
    /// The service is registered already.
    pub(super) const ALREADY_REGISTERED: isize = 12;
}

/// The refusal of a request the system cannot carry out here, for `reason`.
fn unsupported(reason: UnsupportedReason) -> AutostartError {
    AutostartError::Unsupported(Unsupported::of::<AutostartCapability>(reason))
}

/// The error a register or unregister call's `code` means.
fn item_error(code: ErrorCode) -> AutostartError {
    match code {
        ErrorCode::Other(code::LAUNCH_DENIED_BY_USER) => AutostartError::Denied,
        code => AutostartError::Failed(failure(code)),
    }
}

/// The state the system grants with the item at `status`.
fn granted(status: ItemStatus) -> AutostartState {
    match status {
        ItemStatus::Enabled => AutostartState::Enabled,
        ItemStatus::NotRegistered | ItemStatus::RequiresApproval | ItemStatus::NotFound => {
            AutostartState::Disabled
        }
    }
}

/// Do what `request` asks of `item` and answer the state after it.
pub(super) fn answer(item: &dyn LoginItem, request: &AutostartRequest) -> AutostartResult {
    let before = item.status();
    if before == ItemStatus::NotFound {
        return Err(unsupported(UnsupportedReason::NotDeclared {
            what: "an application bundle",
        }));
    }
    if request.is_enabled() {
        // On already, or switched off by the user: nothing to write, and the
        // user's choice stands.
        if matches!(before, ItemStatus::Enabled | ItemStatus::RequiresApproval) {
            return Ok(granted(before));
        }
        match item.register() {
            Ok(()) | Err(ErrorCode::Other(code::ALREADY_REGISTERED)) => {}
            Err(code) => return Err(item_error(code)),
        }
    } else if before != ItemStatus::NotRegistered {
        match item.unregister() {
            Ok(()) | Err(ErrorCode::Other(code::JOB_NOT_FOUND)) => {}
            Err(code) => return Err(item_error(code)),
        }
    }
    Ok(granted(item.status()))
}

/// `SMAppService`, macOS 13: below it every request answers
/// `NotOnThisDevice`.
pub(super) enum LoginItems {}

impl super::version::Gate for LoginItems {
    const SINCE: (u32, u32, u32) = (13, 0, 0);
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacAutostart;

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "the login item is ServiceManagement's, through objc2"
)]
mod appkit {
    use std::rc::Rc;

    use flui_platform::UnsupportedReason;
    use flui_platform::capability::Resolver;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    use super::super::appkit::error_code;
    use super::super::queue::WorkQueue;
    use super::super::version::{MacOSVersion, Since};
    use super::{ItemStatus, LoginItem, LoginItems, answer, unsupported};
    use crate::autostart::{AutostartBackend, AutostartRequest, AutostartResult};

    /// The application's own login item; made only with the proof that the
    /// class is there.
    struct MainAppService(Since<LoginItems>);

    impl LoginItem for MainAppService {
        fn status(&self) -> ItemStatus {
            // SAFETY: the class exists from macOS 13 on, which the proof
            // this value holds shows; the service is safe on any thread.
            let status = unsafe { SMAppService::mainAppService().status() };
            match status {
                SMAppServiceStatus::Enabled => ItemStatus::Enabled,
                SMAppServiceStatus::RequiresApproval => ItemStatus::RequiresApproval,
                SMAppServiceStatus::NotFound => ItemStatus::NotFound,
                // `NotRegistered`, and a status a later system adds.
                _ => ItemStatus::NotRegistered,
            }
        }

        fn register(&self) -> Result<(), super::ErrorCode> {
            // SAFETY: as in `status`.
            unsafe { SMAppService::mainAppService().registerAndReturnError() }
                .map_err(|error| error_code(&error))
        }

        fn unregister(&self) -> Result<(), super::ErrorCode> {
            // SAFETY: as in `status`.
            unsafe { SMAppService::mainAppService().unregisterAndReturnError() }
                .map_err(|error| error_code(&error))
        }
    }

    /// [`AutostartBackend`] over the application's login item.
    pub(crate) struct MacAutostart {
        version: MacOSVersion,
        queue: Rc<WorkQueue>,
    }

    impl MacAutostart {
        pub(crate) fn new(version: MacOSVersion, queue: Rc<WorkQueue>) -> Self {
            Self { version, queue }
        }
    }

    impl AutostartBackend for MacAutostart {
        fn request(&self, request: AutostartRequest, answer_to: Resolver<AutostartResult>) {
            let Some(since) = self.version.since::<LoginItems>() else {
                // The owner retiring first leaves nothing waiting.
                let _ = answer_to.resolve(Err(unsupported(UnsupportedReason::NotOnThisDevice)));
                return;
            };
            let busy = Err(unsupported(UnsupportedReason::Unavailable));
            self.queue.run(answer_to, busy, move || {
                answer(&MainAppService(since), &request)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::super::ErrorCode;
    use super::{ItemStatus, LoginItem, answer};
    use crate::autostart::{AutostartCapability, AutostartError, AutostartRequest, AutostartState};

    /// A login item that moves as ServiceManagement's does: registering
    /// turns it on unless the user switched it off, unregistering removes it.
    struct Item {
        status: Cell<ItemStatus>,
        failure: Option<ErrorCode>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Item {
        fn at(status: ItemStatus) -> Self {
            Self {
                status: Cell::new(status),
                failure: None,
                calls: RefCell::default(),
            }
        }
    }

    impl LoginItem for Item {
        fn status(&self) -> ItemStatus {
            self.status.get()
        }

        fn register(&self) -> Result<(), ErrorCode> {
            self.calls.borrow_mut().push("register");
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            self.status.set(ItemStatus::Enabled);
            Ok(())
        }

        fn unregister(&self) -> Result<(), ErrorCode> {
            self.calls.borrow_mut().push("unregister");
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            self.status.set(ItemStatus::NotRegistered);
            Ok(())
        }
    }

    #[test]
    fn the_login_item_is_there_from_macos_13_on() {
        use super::super::version::MacOSVersion;
        use super::LoginItems;

        assert!(MacOSVersion::new(12, 7, 6).since::<LoginItems>().is_none());
        assert!(MacOSVersion::new(13, 0, 0).since::<LoginItems>().is_some());
    }

    #[test]
    fn enabling_registers_once_and_the_users_switch_off_stands() {
        let off = Item::at(ItemStatus::NotRegistered);
        assert_eq!(
            answer(&off, &AutostartRequest::enable()),
            Ok(AutostartState::Enabled)
        );
        assert_eq!(*off.calls.borrow(), ["register"]);
        assert_eq!(
            answer(&off, &AutostartRequest::enable()),
            Ok(AutostartState::Enabled),
            "an item on already is not registered again"
        );
        assert_eq!(*off.calls.borrow(), ["register"]);

        let switched_off = Item::at(ItemStatus::RequiresApproval);
        assert_eq!(
            answer(&switched_off, &AutostartRequest::enable()),
            Ok(AutostartState::Disabled),
            "what the system grants"
        );
        assert!(
            switched_off.calls.borrow().is_empty(),
            "the user's switch-off is never written over"
        );
    }

    #[test]
    fn disabling_unregisters_what_is_registered_and_nothing_else() {
        for before in [ItemStatus::Enabled, ItemStatus::RequiresApproval] {
            let item = Item::at(before);
            assert_eq!(
                answer(&item, &AutostartRequest::disable()),
                Ok(AutostartState::Disabled),
                "{before:?}"
            );
            assert_eq!(*item.calls.borrow(), ["unregister"], "{before:?}");
        }
        let never = Item::at(ItemStatus::NotRegistered);
        assert_eq!(
            answer(&never, &AutostartRequest::disable()),
            Ok(AutostartState::Disabled)
        );
        assert!(never.calls.borrow().is_empty());
    }

    #[test]
    fn a_refusal_and_a_failure_are_named_and_an_unbundled_application_has_no_item() {
        let mut refused = Item::at(ItemStatus::NotRegistered);
        refused.failure = Some(ErrorCode::Other(11));
        assert_eq!(
            answer(&refused, &AutostartRequest::enable()),
            Err(AutostartError::Denied)
        );
        let mut broken = Item::at(ItemStatus::NotRegistered);
        broken.failure = Some(ErrorCode::Other(2));
        assert_eq!(
            answer(&broken, &AutostartRequest::enable()),
            Err(AutostartError::Failed(SystemFailure::Framework(2)))
        );
        let mut registered = Item::at(ItemStatus::NotRegistered);
        registered.failure = Some(ErrorCode::Other(12));
        assert_eq!(
            answer(&registered, &AutostartRequest::enable()),
            Ok(AutostartState::Disabled),
            "registered already: the status after it is the answer"
        );
        let unbundled = Item::at(ItemStatus::NotFound);
        assert_eq!(
            answer(&unbundled, &AutostartRequest::enable()),
            Err(AutostartError::Unsupported(Unsupported::of::<
                AutostartCapability,
            >(
                UnsupportedReason::NotDeclared {
                    what: "an application bundle"
                }
            )))
        );
        assert!(unbundled.calls.borrow().is_empty());
    }
}
