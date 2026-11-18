//! The user's folders as Foundation names them: `NSFileManager`'s
//! `URLsForDirectory:inDomains:` in the user's domain, the home folder from
//! `NSHomeDirectory`. A sandboxed application is answered its container's
//! folders, as the system gives them to it.
//!
//! macOS has no templates folder, so [`UserDir::Templates`] is none;
//! [`UserDir::PublicShare`] is the Public folder file sharing serves, and
//! [`UserDir::Videos`] is Movies. A folder the system names but the user
//! does not have (deleted, never made) is none too. Each lookup runs off the
//! main thread (`queue::WorkQueue`), since the home folder may be on a network
//! volume.

use crate::user_dirs::UserDir;

/// Where Foundation keeps a kind of folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Location {
    /// `NSHomeDirectory`.
    Home,
    /// The `NSSearchPathDirectory` of this value, in the user's domain.
    Search(usize),
    /// macOS names no folder of this kind.
    None,
}

/// Where Foundation keeps `dir`.
pub(super) fn location(dir: UserDir) -> Location {
    match dir {
        UserDir::Home => Location::Home,
        // NSDesktopDirectory, NSDocumentDirectory, NSDownloadsDirectory,
        // NSMusicDirectory, NSPicturesDirectory, NSMoviesDirectory,
        // NSSharedPublicDirectory.
        UserDir::Desktop => Location::Search(12),
        UserDir::Documents => Location::Search(9),
        UserDir::Downloads => Location::Search(15),
        UserDir::Music => Location::Search(18),
        UserDir::Pictures => Location::Search(19),
        UserDir::Videos => Location::Search(17),
        UserDir::PublicShare => Location::Search(21),
        UserDir::Templates => Location::None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacUserDirs;

#[cfg(target_os = "macos")]
mod appkit {
    use std::path::PathBuf;
    use std::rc::Rc;

    use flui_platform::capability::Resolver;
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
    use objc2_foundation::{
        NSFileManager, NSHomeDirectory, NSSearchPathDirectory, NSSearchPathDomainMask,
    };

    use super::super::queue::WorkQueue;
    use super::{Location, location};
    use crate::user_dirs::{
        UserDir, UserDirBackend, UserDirCapability, UserDirError, UserDirResult,
    };

    /// [`UserDirBackend`] over Foundation's search paths.
    pub(crate) struct MacUserDirs {
        queue: Rc<WorkQueue>,
    }

    impl MacUserDirs {
        pub(crate) fn new(queue: Rc<WorkQueue>) -> Self {
            Self { queue }
        }
    }

    /// The folder Foundation names for `dir`, when the user has it.
    fn locate(dir: UserDir) -> UserDirResult {
        let path = match location(dir) {
            Location::None => return Ok(None),
            Location::Home => Some(PathBuf::from(NSHomeDirectory().to_string())),
            Location::Search(directory) => NSFileManager::defaultManager()
                .URLsForDirectory_inDomains(
                    NSSearchPathDirectory(directory),
                    NSSearchPathDomainMask::UserDomainMask,
                )
                .firstObject()
                .and_then(|url| url.path())
                .map(|path| PathBuf::from(path.to_string())),
        };
        match path {
            Some(path) if !path.is_absolute() => {
                Err(UserDirError::Failed(SystemFailure::MalformedAnswer))
            }
            Some(path) if path.is_dir() => Ok(Some(path)),
            Some(_) | None => Ok(None),
        }
    }

    impl UserDirBackend for MacUserDirs {
        fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
            let busy = Err(UserDirError::Unsupported(Unsupported::of::<
                UserDirCapability,
            >(
                UnsupportedReason::Unavailable
            )));
            self.queue.run(answer, busy, move || locate(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Location, location};
    use crate::user_dirs::UserDir;

    #[test]
    fn each_folder_is_the_one_foundation_keeps_and_templates_none() {
        let cases = [
            (UserDir::Home, Location::Home),
            (UserDir::Desktop, Location::Search(12)),
            (UserDir::Documents, Location::Search(9)),
            (UserDir::Downloads, Location::Search(15)),
            (UserDir::Music, Location::Search(18)),
            (UserDir::Pictures, Location::Search(19)),
            (UserDir::Videos, Location::Search(17)),
            (UserDir::PublicShare, Location::Search(21)),
            (UserDir::Templates, Location::None),
        ];
        for (dir, expected) in cases {
            assert_eq!(location(dir), expected, "{dir:?}");
        }
    }
}
