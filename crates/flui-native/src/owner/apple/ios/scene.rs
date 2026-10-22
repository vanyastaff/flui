//! UIKit session identities, and the consumer of scene sessions as the loop
//! calls it.
//!
//! The application registers a handler of `IOSSceneEvent`s, which carry the
//! window's installation acknowledgement; the window feature
//! (`window::apple::ios`) adapts that handler to [`SceneConsumer`], so the
//! loop drives sessions without naming the installation it hands over.

use std::sync::Arc;

use crate::{BootstrapError, HostWindow, PlatformProxy};

/// UIKit's persistent logical scene-session identity, independent of attachments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IOSSceneSessionId(pub(super) String);
impl IOSSceneSessionId {
    /// The UIKit persistent identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One connection of a retained logical session to a native scene. The loop
/// numbers connections in the order they are made, so a later one compares
/// greater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IOSSceneAttachmentId(pub(super) u64);

/// The application's consumer of scene sessions, which the loop calls on the
/// owner thread through its call point.
pub(crate) trait SceneConsumer {
    /// `window` connected to `session` as `attachment`; `reconnect` says the
    /// session installed its application before. Returns what the loop
    /// watches of the window's installation, whose acknowledgement wakes the
    /// owner through `wake`: an accepted connection always carries one, so
    /// the loop publishes the window or rolls it back. The application may
    /// have completed or dropped the installation before this returns.
    ///
    /// # Errors
    /// What the application's handler reports; the loop rolls the
    /// connection back.
    fn connected(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
        window: Arc<dyn HostWindow>,
        reconnect: bool,
        wake: PlatformProxy,
    ) -> Result<Box<dyn SceneInstallation>, BootstrapError>;

    /// `attachment` of `session` was lost; the session may connect again.
    fn disconnected(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
    ) -> Result<(), BootstrapError>;

    /// `attachment` of `session` failed to install and was retired; the
    /// session may connect again.
    fn installation_aborted(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
    ) -> Result<(), BootstrapError>;

    /// `session` ended for good.
    fn discarded(&mut self, session: IOSSceneSessionId) -> Result<(), BootstrapError>;
}

/// What the loop watches of a connected window's installation, which the
/// application completes or drops.
pub(crate) trait SceneInstallation {
    /// `None` until the application completed or dropped the installation;
    /// then whether it completed.
    fn installed(&self) -> Option<bool>;
}
