//! What a platform answers for each of the crate's capabilities.
//!
//! A platform states its answers as one [`Providers`] literal, a field per
//! capability, so a capability added to the crate does not compile until
//! every platform says whether it provides it or refuses it and why
//! (ADR-0154 §10: a refusal, never a stand-in). The plugin registers the
//! literal and decides nothing itself.

use flui_platform::capability::{
    Capability, CapabilityProvider, CapabilityRegistrar, ProviderContext,
};
use flui_platform::launch::LauncherCapability;
use flui_platform::{Unsupported, UnsupportedReason};

use crate::autostart::AutostartCapability;
use crate::dialog::{FileDialogCapability, MessageBoxCapability};
use crate::grant::GrantCapability;
use crate::jump_list::JumpListCapability;
use crate::os::OsCapability;
use crate::share::ShareCapability;
use crate::shortcuts::GlobalShortcutCapability;
use crate::taskbar::TaskbarCapability;
use crate::user_dirs::UserDirCapability;

/// A platform's answer for `C`.
pub(crate) enum Provision<C: Capability> {
    /// A provider, asked once per acquisition.
    Provided(Box<dyn CapabilityProvider<C>>),
    /// Every acquisition is refused, for this reason.
    #[cfg_attr(windows, expect(dead_code, reason = "Windows provides every service"))]
    Refused(UnsupportedReason),
}

impl<C: Capability> Provision<C> {
    /// `provider` answers for `C`.
    pub(crate) fn provided(provider: impl CapabilityProvider<C>) -> Self {
        Self::Provided(Box::new(provider))
    }

    fn register(self, registrar: &mut CapabilityRegistrar) {
        match self {
            Self::Provided(provider) => {
                registrar.provide::<C>(move |ctx: &ProviderContext<'_>| provider.provide(ctx));
            }
            Self::Refused(reason) => {
                registrar.provide::<C>(move |_: &ProviderContext<'_>| {
                    Err::<C::Handle, _>(Unsupported::of::<C>(reason))
                });
            }
        }
    }
}

/// A platform's answer for every capability of the crate.
pub(crate) struct Providers {
    pub(crate) launcher: Provision<LauncherCapability>,
    pub(crate) file_dialogs: Provision<FileDialogCapability>,
    pub(crate) message_boxes: Provision<MessageBoxCapability>,
    pub(crate) share: Provision<ShareCapability>,
    pub(crate) autostart: Provision<AutostartCapability>,
    pub(crate) global_shortcuts: Provision<GlobalShortcutCapability>,
    pub(crate) taskbar: Provision<TaskbarCapability>,
    pub(crate) user_dirs: Provision<UserDirCapability>,
    pub(crate) jump_list: Provision<JumpListCapability>,
    pub(crate) os: Provision<OsCapability>,
    pub(crate) grants: Provision<GrantCapability>,
}

impl Providers {
    /// Register every answer.
    pub(crate) fn register(self, registrar: &mut CapabilityRegistrar) {
        // Destructured, so a field added to the struct is a field registered.
        let Self {
            launcher,
            file_dialogs,
            message_boxes,
            share,
            autostart,
            global_shortcuts,
            taskbar,
            user_dirs,
            jump_list,
            os,
            grants,
        } = self;
        launcher.register(registrar);
        file_dialogs.register(registrar);
        message_boxes.register(registrar);
        share.register(registrar);
        autostart.register(registrar);
        global_shortcuts.register(registrar);
        taskbar.register(registrar);
        user_dirs.register(registrar);
        jump_list.register(registrar);
        os.register(registrar);
        grants.register(registrar);
    }
}
