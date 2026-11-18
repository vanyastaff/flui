//! The user's folders as `Environment`'s public directories on shared
//! storage: Documents, Download, Music, Pictures and Movies.
//!
//! Android gives a user no home folder, desktop, templates or public share,
//! so those answer none. `Environment` computes a folder's path without
//! reading storage, so the answer is made on the owner thread; whether the
//! application may read or write there is the storage permissions' and the
//! Android version's to say, not this service's.

use std::path::PathBuf;

use flui_platform::SystemFailure;

use crate::user_dirs::{UserDir, UserDirError, UserDirResult};

/// The `Environment.DIRECTORY_*` value of `dir`, `None` for a folder
/// Android has no form of.
pub(crate) const fn environment_directory(dir: UserDir) -> Option<&'static str> {
    match dir {
        UserDir::Documents => Some("Documents"),
        UserDir::Downloads => Some("Download"),
        UserDir::Music => Some("Music"),
        UserDir::Pictures => Some("Pictures"),
        UserDir::Videos => Some("Movies"),
        UserDir::Home | UserDir::Desktop | UserDir::Templates | UserDir::PublicShare => None,
    }
}

/// The answer for the path `Environment` named: the folder, none when it
/// named none, and a malformed answer for a relative path, which no public
/// directory has. Absolute as Android reads a path: from the root.
pub(crate) fn folder(named: Option<String>) -> UserDirResult {
    match named {
        None => Ok(None),
        Some(path) if path.starts_with('/') => Ok(Some(PathBuf::from(path))),
        Some(_) => Err(UserDirError::Failed(SystemFailure::MalformedAnswer)),
    }
}

#[cfg(target_os = "android")]
pub(crate) use provider::AndroidUserDirs;

#[cfg(target_os = "android")]
mod provider {
    use flui_platform::capability::{Activity, Resolver};
    use flui_platform::{Unsupported, UnsupportedReason};

    use super::super::java::public_folder;
    use super::{environment_directory, folder};
    use crate::user_dirs::{
        UserDir, UserDirBackend, UserDirCapability, UserDirError, UserDirResult,
    };

    /// [`UserDirBackend`] over `Environment`, called in the frame the
    /// Activity lends on the owner thread.
    pub(crate) struct AndroidUserDirs {
        activity: Activity,
    }

    impl AndroidUserDirs {
        pub(crate) fn new(activity: Activity) -> Self {
            Self { activity }
        }
    }

    impl UserDirBackend for AndroidUserDirs {
        fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
            let unavailable = || {
                UserDirError::Unsupported(Unsupported::of::<UserDirCapability>(
                    UnsupportedReason::Unavailable,
                ))
            };
            let located = match environment_directory(dir) {
                None => Ok(None),
                // An Activity that ended lends nothing.
                Some(kind) => match self.activity.with_java(|frame| public_folder(frame, kind)) {
                    Some(Ok(named)) => folder(named),
                    Some(Err(_)) | None => Err(unavailable()),
                },
            };
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(located);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_platform::SystemFailure;

    use super::{environment_directory, folder};
    use crate::user_dirs::{UserDir, UserDirError};

    #[test]
    fn a_folder_is_androids_public_directory_or_none() {
        for (dir, expected) in [
            (UserDir::Documents, Some("Documents")),
            (UserDir::Downloads, Some("Download")),
            (UserDir::Music, Some("Music")),
            (UserDir::Pictures, Some("Pictures")),
            (UserDir::Videos, Some("Movies")),
            (UserDir::Home, None),
            (UserDir::Desktop, None),
            (UserDir::Templates, None),
            (UserDir::PublicShare, None),
        ] {
            assert_eq!(environment_directory(dir), expected, "{dir:?}");
        }
        assert_eq!(folder(None), Ok(None));
        assert_eq!(
            folder(Some("/storage/emulated/0/Documents".to_owned())),
            Ok(Some(PathBuf::from("/storage/emulated/0/Documents")))
        );
        assert_eq!(
            folder(Some("Documents".to_owned())),
            Err(UserDirError::Failed(SystemFailure::MalformedAnswer))
        );
    }
}
