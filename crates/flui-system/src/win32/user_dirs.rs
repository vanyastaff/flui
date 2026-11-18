//! The user's folders over `SHGetKnownFolderPath`, each found on its own
//! thread ([`ShellThreads`]), since a redirected folder may reach the
//! network. A folder the user does not have answers none.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Profile, FOLDERID_Public, FOLDERID_Templates, FOLDERID_Videos, KF_FLAG_DEFAULT,
    SHGetKnownFolderPath,
};
use windows::core::GUID;

use super::{ShellThreads, os_code};
use crate::user_dirs::{UserDir, UserDirBackend, UserDirCapability, UserDirError, UserDirResult};

/// The known folder id of `dir`.
pub(super) fn folder_id(dir: UserDir) -> GUID {
    match dir {
        UserDir::Home => FOLDERID_Profile,
        UserDir::Desktop => FOLDERID_Desktop,
        UserDir::Documents => FOLDERID_Documents,
        UserDir::Downloads => FOLDERID_Downloads,
        UserDir::Music => FOLDERID_Music,
        UserDir::Pictures => FOLDERID_Pictures,
        UserDir::Videos => FOLDERID_Videos,
        UserDir::Templates => FOLDERID_Templates,
        // The folder every user of the machine shares, `C:\Users\Public`.
        UserDir::PublicShare => FOLDERID_Public,
    }
}

/// What finding a folder answered, as its code says: a folder the user does
/// not have is none, and a relative path is outside the system's contract.
pub(super) fn folder_answer(found: Result<PathBuf, i32>) -> UserDirResult {
    let missing = |code: i32| {
        [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND]
            .iter()
            .any(|error| i32::try_from(error.0).is_ok_and(|missing| missing == code))
    };
    match found {
        Ok(path) if path.is_absolute() => Ok(Some(path)),
        Ok(_) => Err(UserDirError::Failed(SystemFailure::MalformedAnswer)),
        Err(code) if missing(code) => Ok(None),
        Err(code) => Err(UserDirError::Failed(SystemFailure::Os(code))),
    }
}

/// [`UserDirBackend`] over the shell's known folders.
pub(crate) struct WindowsUserDirs {
    threads: Rc<ShellThreads>,
    /// Finds a known folder, on a shell thread: the system's lookup, or a
    /// test's answers.
    lookup: fn(GUID) -> Result<PathBuf, i32>,
}

impl WindowsUserDirs {
    pub(crate) fn new(threads: Rc<ShellThreads>) -> Self {
        Self::over(threads, known_folder)
    }

    pub(super) fn over(
        threads: Rc<ShellThreads>,
        lookup: fn(GUID) -> Result<PathBuf, i32>,
    ) -> Self {
        Self { threads, lookup }
    }
}

/// The path of the known folder `id`.
fn known_folder(id: GUID) -> Result<PathBuf, i32> {
    // SAFETY: `id` outlives the call; the string it returns is the caller's
    // to free with `CoTaskMemFree`, which happens once, after its code units
    // were copied out.
    unsafe {
        let path = SHGetKnownFolderPath(&raw const id, KF_FLAG_DEFAULT, None)
            .map_err(|error| os_code(error.code()))?;
        let owned = OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.as_ptr().cast_const().cast()));
        Ok(PathBuf::from(owned))
    }
}

impl UserDirBackend for WindowsUserDirs {
    fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
        let id = folder_id(dir);
        let lookup = self.lookup;
        self.threads.answer(
            "flui-known-folder",
            answer,
            || {
                Err(UserDirError::Unsupported(Unsupported::of::<
                    UserDirCapability,
                >(
                    UnsupportedReason::Unavailable
                )))
            },
            move || folder_answer(lookup(id)),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_platform::SystemFailure;

    use super::folder_answer;
    use crate::user_dirs::UserDirError;

    #[test]
    fn a_folder_the_user_does_not_have_is_none_and_a_relative_one_malformed() {
        assert_eq!(folder_answer(Err(2)), Ok(None));
        assert_eq!(folder_answer(Err(3)), Ok(None));
        assert_eq!(
            folder_answer(Err(5)),
            Err(UserDirError::Failed(SystemFailure::Os(5)))
        );
        assert_eq!(
            folder_answer(Ok(PathBuf::from(r"C:\Users\a"))),
            Ok(Some(PathBuf::from(r"C:\Users\a")))
        );
        assert_eq!(
            folder_answer(Ok(PathBuf::from("relative"))),
            Err(UserDirError::Failed(SystemFailure::MalformedAnswer))
        );
    }

    /// The known folders of a system whose user has no videos folder.
    fn lookup(id: windows::core::GUID) -> Result<PathBuf, i32> {
        if id == windows::Win32::UI::Shell::FOLDERID_Videos {
            Err(2)
        } else if id == windows::Win32::UI::Shell::FOLDERID_Profile {
            Ok(PathBuf::from(r"C:\Users\a"))
        } else if id == windows::Win32::UI::Shell::FOLDERID_Templates {
            Ok(PathBuf::from(
                r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Templates",
            ))
        } else {
            Err(5)
        }
    }

    #[test]
    fn a_folder_is_found_on_a_shell_thread_and_answered_as_the_system_said() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use flui_platform::capability::Abandoned;

        use crate::testing::win32::TestOwner;
        use crate::user_dirs::{UserDir, UserDirResult, UserDirs};
        use crate::win32::ShellThreads;

        let owner = TestOwner::new();
        let ctx = owner.ctx(None);
        let dirs = UserDirs::new(
            super::WindowsUserDirs::over(Rc::new(ShellThreads::default()), lookup),
            &ctx,
        );
        let find = |dir| {
            let landed: Rc<RefCell<Option<Result<UserDirResult, Abandoned>>>> = Rc::default();
            let into = Rc::clone(&landed);
            dirs.locate(dir)
                .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
            owner.turn_until(|| landed.borrow().is_some());
            let answer = landed.borrow_mut().take();
            answer.expect("landed").expect("not abandoned")
        };
        assert_eq!(find(UserDir::Home), Ok(Some(PathBuf::from(r"C:\Users\a"))));
        assert_eq!(find(UserDir::Videos), Ok(None));
        assert!(matches!(find(UserDir::Templates), Ok(Some(path)) if path.ends_with("Templates")));
        assert_eq!(
            find(UserDir::Music),
            Err(UserDirError::Failed(SystemFailure::Os(5)))
        );
    }
}
