//! A grant that is the pick's path: what a desktop process keeps on Windows
//! and Linux, where it opens what the user can, and what a Flatpak keeps of
//! the document portal's path, which the portal keeps for it across runs.

use std::io;

use flui_platform::SystemFailure;
#[cfg(any(windows, target_os = "linux"))]
use flui_platform::launch::is_local_path;

use super::{Access, Grant, GrantError, GrantResult};
use crate::dialog::{PickedFile, Picks};

/// `picks`, each at a path carrying the grant of that path; a pick of
/// another form, or at a path longer than a grant holds, carries none.
pub(crate) fn granted(picks: Picks) -> Picks {
    let granted = picks
        .into_iter()
        .map(|pick| match pick.path().and_then(Grant::of_path) {
            Some(grant) => pick.granted(grant),
            None => pick,
        })
        .collect::<Vec<PickedFile>>();
    Picks::try_new(granted).expect("BUG: as many picks as the answer held, one at least")
}

/// Where a path grant's file is, as the platform judges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Granter {
    /// The process opens what the user can: a file that is not there was
    /// moved or deleted.
    User,
    /// The document portal granted it: a file it no longer shows was
    /// forgotten by the portal (the user withdrew it, or the file is gone,
    /// which the portal's folder does not tell apart).
    #[cfg(any(target_os = "linux", test))]
    DocumentPortal,
}

/// The file a path grant names, judged before any file system call: an
/// absolute path with no NUL (a grant this library made names one, so any
/// other is `MalformedAnswer`), on a local drive (ADR-0360 §2: a share, a
/// pipe or a device is `NotLocal`, and the network is never reached).
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn file_of(grant: &Grant) -> Result<PickedFile, GrantError> {
    let malformed = GrantError::Failed(SystemFailure::MalformedAnswer);
    let path = grant.path().ok_or(malformed)?;
    if !is_local_path(&path) {
        return Err(GrantError::NotLocal);
    }
    PickedFile::try_new(path).ok_or(malformed)
}

/// What resolving a path grant answers, once the file system was asked
/// about `file` off the owner thread (`looked`, a `metadata` call).
pub(crate) fn reacquired(
    file: PickedFile,
    looked: io::Result<()>,
    granter: Granter,
) -> GrantResult {
    match looked {
        Ok(()) => Ok(Access::new(file, None)),
        Err(error) => Err(match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => match granter {
                Granter::User => GrantError::Missing,
                #[cfg(any(target_os = "linux", test))]
                Granter::DocumentPortal => GrantError::Revoked,
            },
            io::ErrorKind::PermissionDenied => GrantError::Revoked,
            _ => GrantError::Failed(
                error
                    .raw_os_error()
                    .map_or(SystemFailure::MalformedAnswer, SystemFailure::Os),
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use flui_platform::SystemFailure;

    use super::{Granter, granted, reacquired};
    use crate::dialog::{PickedFile, Picks};
    use crate::grant::{Grant, GrantError};

    #[test]
    fn a_path_grant_answers_the_file_or_why_it_is_not_reached() {
        let path = std::env::temp_dir().join("notes.txt");
        let file = || PickedFile::try_new(&path).expect("absolute");
        let found = reacquired(file(), Ok(()), Granter::User).expect("found");
        assert_eq!(found.file().path(), Some(path.as_path()));
        assert!(!found.is_scoped(), "a path needs no scope");
        let gone = || Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(
            reacquired(file(), gone(), Granter::User).err(),
            Some(GrantError::Missing)
        );
        assert_eq!(
            reacquired(file(), gone(), Granter::DocumentPortal).err(),
            Some(GrantError::Revoked),
            "the portal shows no document it forgot"
        );
        assert_eq!(
            reacquired(
                file(),
                Err(io::Error::from(io::ErrorKind::PermissionDenied)),
                Granter::User
            )
            .err(),
            Some(GrantError::Revoked)
        );
        assert_eq!(
            reacquired(
                file(),
                Err(io::Error::from_raw_os_error(5_000)),
                Granter::User
            )
            .err(),
            Some(GrantError::Failed(SystemFailure::Os(5_000)))
        );
    }

    #[test]
    fn each_pick_at_a_path_carries_the_grant_of_that_path() {
        let path = std::env::temp_dir().join("a.txt");
        let picks = granted(
            Picks::try_new(vec![
                PickedFile::try_new(&path).expect("absolute"),
                PickedFile::try_from_contents("b.txt", Vec::new().into()).expect("a name"),
            ])
            .expect("two picks"),
        );
        let mut picks = picks.iter();
        let first = picks.next().expect("the path");
        assert_eq!(first.grant().and_then(Grant::path), Some(path));
        assert_eq!(picks.next().and_then(PickedFile::grant), None);
    }
}
