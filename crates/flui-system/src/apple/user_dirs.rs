//! The user's folders as Foundation names them: `NSFileManager`'s
//! `URLsForDirectory:inDomains:` in the user's domain, and the home folder
//! (`homeDirectoryForCurrentUser` on macOS, `NSHomeDirectory` on iOS, which
//! has no such method), each path as the file system's bytes. A sandboxed
//! application is answered its container's folders, as the system gives
//! them to it: on iOS every application is one, so its home is its
//! container and its documents the container's Documents folder.
//!
//! The systems have no templates folder, so [`UserDir::Templates`] is none;
//! [`UserDir::PublicShare`] is the Public folder file sharing serves, and
//! [`UserDir::Videos`] is Movies. A folder the system names but the user
//! does not have (deleted, never made, or one an iOS container does not
//! hold) is none too. Each lookup runs off the main thread
//! (`queue`, bounded by `off_owner`), since the home folder may be on a network volume.

use crate::user_dirs::UserDir;

/// A folder Foundation's search paths name in the user's domain: each is
/// the `NSSearchPathDirectory` of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchPath {
    Desktop,
    Document,
    Downloads,
    Music,
    Pictures,
    Movies,
    SharedPublic,
}

/// Where Foundation keeps a kind of folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Location {
    /// The home folder.
    Home,
    /// A search path in the user's domain.
    Search(SearchPath),
    /// The systems name no folder of this kind.
    None,
}

/// Where Foundation keeps `dir`: videos are Movies, the public share is
/// the Public folder file sharing serves, and there is no templates folder.
pub(super) fn location(dir: UserDir) -> Location {
    match dir {
        UserDir::Home => Location::Home,
        UserDir::Desktop => Location::Search(SearchPath::Desktop),
        UserDir::Documents => Location::Search(SearchPath::Document),
        UserDir::Downloads => Location::Search(SearchPath::Downloads),
        UserDir::Music => Location::Search(SearchPath::Music),
        UserDir::Pictures => Location::Search(SearchPath::Pictures),
        UserDir::Videos => Location::Search(SearchPath::Movies),
        UserDir::PublicShare => Location::Search(SearchPath::SharedPublic),
        UserDir::Templates => Location::None,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use foundation::AppleUserDirs;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod foundation {
    use std::path::PathBuf;
    use std::rc::Rc;

    use flui_platform::capability::Resolver;
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
    use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};

    use super::super::url_path;
    use super::{Location, SearchPath, location};
    use crate::off_owner::OffOwner;
    use crate::user_dirs::{
        UserDir, UserDirBackend, UserDirCapability, UserDirError, UserDirResult,
    };

    /// [`UserDirBackend`] over Foundation's search paths.
    pub(crate) struct AppleUserDirs {
        queue: Rc<OffOwner>,
    }

    impl AppleUserDirs {
        pub(crate) fn new(queue: Rc<OffOwner>) -> Self {
            Self { queue }
        }
    }

    /// The search path directory `path` is.
    fn search_directory(path: SearchPath) -> NSSearchPathDirectory {
        match path {
            SearchPath::Desktop => NSSearchPathDirectory::DesktopDirectory,
            SearchPath::Document => NSSearchPathDirectory::DocumentDirectory,
            SearchPath::Downloads => NSSearchPathDirectory::DownloadsDirectory,
            SearchPath::Music => NSSearchPathDirectory::MusicDirectory,
            SearchPath::Pictures => NSSearchPathDirectory::PicturesDirectory,
            SearchPath::Movies => NSSearchPathDirectory::MoviesDirectory,
            SearchPath::SharedPublic => NSSearchPathDirectory::SharedPublicDirectory,
        }
    }

    /// The user's home folder.
    #[cfg(target_os = "macos")]
    fn home() -> Option<PathBuf> {
        url_path(&NSFileManager::defaultManager().homeDirectoryForCurrentUser())
    }

    /// The application's container, an iOS application's home: iOS has no
    /// `homeDirectoryForCurrentUser`.
    #[cfg(target_os = "ios")]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the macOS build of the same function reads a URL, which may name no file"
    )]
    fn home() -> Option<PathBuf> {
        Some(PathBuf::from(
            objc2_foundation::NSHomeDirectory().to_string(),
        ))
    }

    /// The folder Foundation names for `dir`, when the user has it.
    fn locate(dir: UserDir) -> UserDirResult {
        let path = match location(dir) {
            Location::None => return Ok(None),
            Location::Home => home(),
            Location::Search(directory) => NSFileManager::defaultManager()
                .URLsForDirectory_inDomains(
                    search_directory(directory),
                    NSSearchPathDomainMask::UserDomainMask,
                )
                .firstObject()
                .and_then(|url| url_path(&url)),
        };
        match path {
            Some(path) if !path.is_absolute() => {
                Err(UserDirError::Failed(SystemFailure::MalformedAnswer))
            }
            Some(path) if path.is_dir() => Ok(Some(path)),
            Some(_) | None => Ok(None),
        }
    }

    impl UserDirBackend for AppleUserDirs {
        fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
            let busy = Err(UserDirError::Unsupported(Unsupported::of::<
                UserDirCapability,
            >(
                UnsupportedReason::Unavailable
            )));
            self.queue.dispatch(answer, busy, move || locate(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Location, SearchPath, location};
    use crate::user_dirs::UserDir;

    #[test]
    fn each_folder_is_the_one_foundation_keeps_and_templates_none() {
        let cases = [
            (UserDir::Home, Location::Home),
            (UserDir::Desktop, Location::Search(SearchPath::Desktop)),
            (UserDir::Documents, Location::Search(SearchPath::Document)),
            (UserDir::Downloads, Location::Search(SearchPath::Downloads)),
            (UserDir::Music, Location::Search(SearchPath::Music)),
            (UserDir::Pictures, Location::Search(SearchPath::Pictures)),
            (UserDir::Videos, Location::Search(SearchPath::Movies)),
            (
                UserDir::PublicShare,
                Location::Search(SearchPath::SharedPublic),
            ),
            (UserDir::Templates, Location::None),
        ];
        for (dir, expected) in cases {
            assert_eq!(location(dir), expected, "{dir:?}");
        }
    }
}
