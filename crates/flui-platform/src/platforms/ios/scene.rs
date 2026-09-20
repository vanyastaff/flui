//! UIKit session identities and the platform-to-runner installation boundary.
use crate::{BootstrapError, PlatformWindow};
use std::sync::Arc;

/// UIKit's persistent logical scene-session identity, independent of attachments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IOSSceneSessionId(pub(super) String);
impl IOSSceneSessionId {
    /// The UIKit persistent identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One connection of a retained logical session to a native scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IOSSceneAttachmentId(pub(super) u64);

/// Owner-thread session notification. Connection acknowledges initial renderer
/// and tree installation; reconnect keeps the previously installed logical UI.
pub enum IOSSceneEvent {
    /// A native attachment is ready for installation, with display ticks paused.
    Connected {
        /// Persistent session identity.
        session: IOSSceneSessionId,
        /// Origin of this native connection.
        attachment: IOSSceneAttachmentId,
        /// Stable logical window, also retained across reconnects.
        window: Arc<dyn PlatformWindow>,
        /// Whether this session already installed its application tree.
        reconnect: bool,
    },
    /// Reversible loss of a native attachment; retain logical application state.
    Disconnected {
        /// Persistent session identity.
        session: IOSSceneSessionId,
        /// The attachment being retired.
        attachment: IOSSceneAttachmentId,
    },
    /// Failed provisional installation was disposed; the same session may retry.
    InstallationAborted {
        /// Persistent session identity, still eligible for a later connection.
        session: IOSSceneSessionId,
        /// Failed attachment, whose native resources have been retired.
        attachment: IOSSceneAttachmentId,
    },
    /// Terminal session disposal. This persistent identity cannot reconnect.
    Discarded {
        /// Persistent session identity.
        session: IOSSceneSessionId,
    },
}
impl std::fmt::Debug for IOSSceneEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected {
                session,
                attachment,
                reconnect,
                ..
            } => f
                .debug_struct("Connected")
                .field("session", session)
                .field("attachment", attachment)
                .field("reconnect", reconnect)
                .finish_non_exhaustive(),
            Self::Disconnected {
                session,
                attachment,
            } => f
                .debug_struct("Disconnected")
                .field("session", session)
                .field("attachment", attachment)
                .finish(),
            Self::InstallationAborted {
                session,
                attachment,
            } => f
                .debug_struct("InstallationAborted")
                .field("session", session)
                .field("attachment", attachment)
                .finish(),
            Self::Discarded { session } => f
                .debug_struct("Discarded")
                .field("session", session)
                .finish(),
        }
    }
}

pub(super) type SceneHandler = Box<dyn FnMut(IOSSceneEvent) -> Result<(), BootstrapError>>;
