//! Where a sandboxed process keeps its data: the container the system gives
//! it, never a folder its sandbox keeps it from opening.
//!
//! The sandbox is `flui_platform::sandbox`'s, the one `flui-system`'s grants
//! read too: a Flatpak by its `/.flatpak-info`, the macOS App Sandbox by the
//! environment its system sets. On top of it:
//!
//! - A Flatpak keeps its data under `~/.var/app/<id>/data`, its
//!   `XDG_DATA_HOME`; the host's `~/.local/share`, where a process with no
//!   `XDG_DATA_HOME` would fall back to, is not in the sandbox.
//! - The App Sandbox keeps it in the container,
//!   `~/Library/Containers/<id>/Data/Library/Application Support`, even when
//!   `HOME` is gone.
//! - A packaged Windows application (an MSIX package identity) keeps it in
//!   its package's folders, `%LOCALAPPDATA%\Packages\<family name>\RoamingState`
//!   and `LocalState`, which the system removes with the package, instead of
//!   the private copy Windows redirects a write under `%APPDATA%` into. A
//!   store it kept there before is copied over once ([`migrate`]); a store
//!   that was not copied stays where it is, and stays the store
//!   ([`roots_after_migration`]).
//!
//! Reading `/.flatpak-info` (on Linux alone) and copying a store touch the
//! file system: this
//! runs on the store's IO thread, at its first call.

use std::path::{Path, PathBuf};

use flui_platform::sandbox::Confinement;

use super::DataDirs;

/// Where a process with no container keeps its data, as the platform names
/// it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BaseDirs {
    /// The user's roaming data folder (`dirs::data_dir`).
    pub(crate) data: Option<PathBuf>,
    /// The user's local data folder (`dirs::data_local_dir`).
    pub(crate) local: Option<PathBuf>,
}

/// The process's container: its sandbox, and on Windows its package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Container {
    /// A sandbox `flui_platform::sandbox` detected, or none.
    Sandbox(Confinement),
    /// A Windows package of the family `family_name`.
    #[cfg(any(windows, test))]
    Package { family_name: std::ffi::OsString },
}

impl Container {
    /// The process's container. Blocks on the file system.
    ///
    /// # Errors
    ///
    /// A Flatpak's description that could not be read: the container is
    /// unknown this time, and the caller asks again later.
    pub(crate) fn of_process() -> std::io::Result<Self> {
        let confinement = Confinement::of_process(
            &flui_platform::sandbox::read_flatpak_info()?,
            &user_database_home,
            // A data folder reads no manifest entry.
            &|_| None,
        );
        #[cfg(windows)]
        if confinement == Confinement::Unconfined
            && let Some(family_name) = package_family_name()
        {
            return Ok(Self::Package { family_name });
        }
        Ok(Self::Sandbox(confinement))
    }
}

/// The user's home as the system's user database records it
/// (`getpwuid_r`), whatever `HOME` says: what a container rebuilt under the
/// user's home is rebuilt under. `None` when the database has no record of
/// the user, or one that is not an absolute path.
#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "getpwuid_r writes the user's record into buffers it was given"
)]
fn user_database_home() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt as _;

    /// The largest buffer a record is read into: names and a shell path.
    const MAX_RECORD: usize = 64 * 1024;
    // SAFETY: `getuid` takes nothing and always succeeds.
    let uid = unsafe { libc::getuid() };
    let mut buffer = vec![0_u8; 1024];
    loop {
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `record`, `buffer` and `found` outlive the call, which
        // writes the record into `record`, its strings into `buffer` within
        // the length it is given, and `record`'s address, or null, into
        // `found`.
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &raw mut found,
            )
        };
        if status == libc::ERANGE && buffer.len() < MAX_RECORD {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 || found.is_null() {
            return None;
        }
        // SAFETY: a non-null `found` is `record`, which the call wrote whole.
        let directory = unsafe { (*found).pw_dir };
        if directory.is_null() {
            return None;
        }
        // SAFETY: a non-null `pw_dir` is a NUL-terminated string the call
        // wrote inside `buffer`, which is alive and unchanged here.
        let directory = unsafe { CStr::from_ptr(directory) };
        let home = PathBuf::from(OsStr::from_bytes(directory.to_bytes()));
        return flui_platform::sandbox::is_plain(&home).then_some(home);
    }
}

/// No user database: Windows keeps no sandbox whose container is rebuilt
/// under the user's home.
#[cfg(not(unix))]
fn user_database_home() -> Option<PathBuf> {
    None
}

/// The data folders of the application `app` in `container`; `None` when the
/// platform names none.
pub(crate) fn data_dirs_in(container: &Container, app: &str, base: &BaseDirs) -> Option<DataDirs> {
    let (roaming, local) = match container {
        Container::Sandbox(Confinement::Flatpak(flatpak)) => {
            let data = flatpak
                .data_home()
                .map(Path::to_path_buf)
                .or_else(|| base.data.clone())?;
            (data.clone(), data)
        }
        Container::Sandbox(Confinement::AppSandbox(sandbox)) => {
            let support = sandbox.home().join("Library").join("Application Support");
            (support.clone(), support)
        }
        Container::Sandbox(_) => (base.data.clone()?, base.local.clone()?),
        #[cfg(any(windows, test))]
        Container::Package { family_name } => {
            let package = base.local.clone()?.join("Packages").join(family_name);
            (package.join("RoamingState"), package.join("LocalState"))
        }
    };
    Some(DataDirs {
        roaming: roaming.join(app),
        local: local.join(app),
    })
}

/// The most files a store copied into a package's folders holds; a larger
/// store is left where it is, never copied in part.
#[cfg(any(windows, test))]
pub(crate) const MAX_MIGRATED_FILES: usize = 4096;

/// The most bytes a store copied into a package's folders holds together.
#[cfg(any(windows, test))]
pub(crate) const MAX_MIGRATED_BYTES: u64 = 256 * 1024 * 1024;

/// What [`migrate`] did with one root.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Migrated {
    /// The new root holds a store already, or the old one holds none.
    Nothing,
    /// The old store was copied, this many files.
    Copied(usize),
    /// The old store is past [`MAX_MIGRATED_FILES`] or
    /// [`MAX_MIGRATED_BYTES`], or holds something but files and folders:
    /// left where it was, and it stays the store
    /// ([`roots_after_migration`]).
    TooLarge,
}

/// Copy the store under `from` to `to` once: only when `to` holds no store
/// (it is missing or empty) and `from` holds one, every file and folder of
/// it within the bounds. The copy is made in a staging folder beside `to`
/// and renamed into place whole, so a copy that fails midway, or a process
/// killed during it, leaves no root behind and the next first call copies
/// again; a root another instance renamed into place first is kept, and the
/// staging copy discarded. Staging folders an earlier copy left (a process
/// killed during one) are removed first. A link, or anything but a file or
/// a folder, leaves the store where it was. Blocks on the file system.
#[cfg(any(windows, test))]
pub(crate) fn migrate(from: &Path, to: &Path) -> std::io::Result<Migrated> {
    let moment = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    migrate_with(
        from,
        to,
        moment,
        &mut |source, target| {
            let mut source = std::fs::File::open(source)?;
            let mut copy = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            std::io::copy(&mut source, &mut copy)?;
            copy.sync_all()
        },
        &mut |staging, root| std::fs::rename(staging, root),
    )
}

/// One step of [`migrate_with`] on two paths: a file copied (source,
/// target), or the staging folder put in the root's place (staging, root).
#[cfg(any(windows, test))]
type FileStep<'a> = &'a mut dyn FnMut(&Path, &Path) -> std::io::Result<()>;

/// [`migrate`] at `moment` (nanoseconds since the epoch, which names its
/// staging folder), each file copied by `copy` and the copy put in place by
/// `place`.
#[cfg(any(windows, test))]
fn migrate_with(
    from: &Path,
    to: &Path,
    moment: u128,
    copy: FileStep<'_>,
    place: FileStep<'_>,
) -> std::io::Result<Migrated> {
    if holds_store(to)? || !holds_store(from)? {
        return Ok(Migrated::Nothing);
    }
    let Some(entries) = listed(from)? else {
        return Ok(Migrated::TooLarge);
    };
    let staging = Staging::beside(to)?;
    staging.sweep();
    let folder = staging.create(moment)?;
    let copied = entries.iter().try_for_each(|(relative, is_folder)| {
        let target = folder.join(relative);
        if *is_folder {
            std::fs::create_dir(&target)
        } else {
            copy(&from.join(relative), &target)
        }
    });
    let failed = match copied {
        Ok(()) => match place(&folder, to) {
            Ok(()) => {
                return Ok(Migrated::Copied(
                    entries.iter().filter(|(_, is_folder)| !is_folder).count(),
                ));
            }
            Err(error) => error,
        },
        Err(error) => error,
    };
    // Best effort: a staging folder left behind is never a root, and the
    // next copy sweeps it.
    let _ = std::fs::remove_dir_all(&folder);
    // Another instance put its root in place first: it is the store.
    if holds_store(to).unwrap_or(false) {
        return Ok(Migrated::Nothing);
    }
    Err(failed)
}

/// Where a copy into `to` stages: folders beside it named
/// `.<name>.migrating-<process id>-<moment>`.
#[cfg(any(windows, test))]
struct Staging<'a> {
    parent: &'a Path,
    /// `.<name>.migrating-`: what every staging folder of `to` begins with.
    prefix: String,
}

/// The most staging folders one sweep removes.
#[cfg(any(windows, test))]
const MAX_SWEPT: usize = 64;

/// The most entries of the root's parent one sweep looks at.
#[cfg(any(windows, test))]
const MAX_SWEEP_LOOKED_AT: usize = 4096;

#[cfg(any(windows, test))]
impl<'a> Staging<'a> {
    /// The staging of `to`, whose parent is made.
    fn beside(to: &'a Path) -> std::io::Result<Self> {
        let parent = to.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(parent)?;
        let name = to.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
        Ok(Self {
            parent,
            prefix: format!(".{}.migrating-", name.to_string_lossy()),
        })
    }

    /// Make this process's staging folder at `moment`: made here, never
    /// found, since a folder already at the name is another copy's, and this
    /// one stops (`AlreadyExists`) rather than share it.
    fn create(&self, moment: u128) -> std::io::Result<PathBuf> {
        let folder = self
            .parent
            .join(format!("{}{}-{moment}", self.prefix, std::process::id()));
        std::fs::create_dir(&folder)?;
        Ok(folder)
    }

    /// Remove the staging folders an earlier copy left, as far as it can:
    /// only folders (a link is not followed, and stays), only under this
    /// root's exact name, at most [`MAX_SWEPT`] of the first
    /// [`MAX_SWEEP_LOOKED_AT`] entries. One it cannot remove stays for the
    /// next sweep; the copy goes on in a folder of its own.
    fn sweep(&self) {
        let Ok(entries) = std::fs::read_dir(self.parent) else {
            return;
        };
        let leftovers = entries
            .take(MAX_SWEEP_LOOKED_AT)
            .filter_map(Result::ok)
            .filter(|entry| {
                // The entry's own type: a link to a folder is a link.
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && self.names_a_folder(&entry.file_name())
            })
            .take(MAX_SWEPT);
        for leftover in leftovers {
            let _ = std::fs::remove_dir_all(leftover.path());
        }
    }

    /// Whether `name` is a staging folder's: the prefix, a process id and a
    /// moment, both in digits.
    fn names_a_folder(&self, name: &std::ffi::OsStr) -> bool {
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(&self.prefix))
        else {
            return false;
        };
        let is_number = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        rest.split_once('-')
            .is_some_and(|(process, moment)| is_number(process) && is_number(moment))
    }
}

/// Whether `root` holds a store: a folder with an entry.
#[cfg(any(windows, test))]
fn holds_store(root: &Path) -> std::io::Result<bool> {
    match std::fs::read_dir(root) {
        Ok(mut entries) => Ok(entries.next().is_some()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The roots a packaged application's store uses this session: each of
/// `dirs` once `migrate` brought the store kept at `earlier`'s matching root
/// over, or found nothing to bring; else that earlier root, where the store
/// still is. A copy that failed, or a store past the bounds, leaves the
/// session reading and writing where the values are: nothing written now
/// lands in a root the next start would take for the store while the old
/// values stay behind, and the next start copies again.
#[cfg(any(windows, test))]
pub(crate) fn roots_after_migration(
    earlier: DataDirs,
    dirs: DataDirs,
    migrate: &mut dyn FnMut(&Path, &Path) -> std::io::Result<Migrated>,
) -> DataDirs {
    let mut root = |from: PathBuf, to: PathBuf| match migrate(&from, &to) {
        Ok(Migrated::Nothing | Migrated::Copied(_)) => to,
        Ok(Migrated::TooLarge) => {
            tracing::warn!(
                from = %from.display(),
                "the store kept before the application was packaged is past the bound a copy \
                 takes: it stays the store"
            );
            from
        }
        Err(error) => {
            tracing::warn!(
                from = %from.display(),
                %error,
                "the store kept before the application was packaged was not copied: it stays \
                 the store until a later start copies it"
            );
            from
        }
    };
    DataDirs {
        roaming: root(earlier.roaming, dirs.roaming),
        local: root(earlier.local, dirs.local),
    }
}

/// Every file and folder under `root`, relative to it, folders before what
/// they hold; `None` when it is past the bounds or holds a link or anything
/// else, which no store this crate writes holds.
#[cfg(any(windows, test))]
fn listed(root: &Path) -> std::io::Result<Option<Vec<(PathBuf, bool)>>> {
    let mut found = Vec::new();
    let mut folders = vec![PathBuf::new()];
    let mut files = 0_usize;
    let mut bytes = 0_u64;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(root.join(&folder))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let relative = folder.join(entry.file_name());
            if kind.is_dir() {
                found.push((relative.clone(), true));
                folders.push(relative);
            } else if kind.is_file() {
                files += 1;
                bytes = bytes.saturating_add(entry.metadata()?.len());
                if files > MAX_MIGRATED_FILES || bytes > MAX_MIGRATED_BYTES {
                    return Ok(None);
                }
                found.push((relative, false));
            } else {
                return Ok(None);
            }
            if found.len() > MAX_MIGRATED_FILES.saturating_mul(2) {
                return Ok(None);
            }
        }
    }
    Ok(Some(found))
}

/// The process's package family name, when it has a package identity.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "GetCurrentPackageFamilyName writes into a buffer it was given"
)]
fn package_family_name() -> Option<std::ffi::OsString> {
    use std::os::windows::ffi::OsStringExt as _;

    use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    use windows::core::PWSTR;

    let mut length = 0_u32;
    // SAFETY: with no buffer the call writes only the name's length, its
    // terminating NUL included, to the stack-local `length`.
    let asked = unsafe { GetCurrentPackageFamilyName(&raw mut length, None) };
    if asked != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        // APPMODEL_ERROR_NO_PACKAGE: a process with no package identity.
        return None;
    }
    let mut name = vec![0_u16; usize::try_from(length).ok()?];
    // SAFETY: `name` holds `length` units, the length the call asked for,
    // and outlives the call.
    let written =
        unsafe { GetCurrentPackageFamilyName(&raw mut length, Some(PWSTR(name.as_mut_ptr()))) };
    if written.is_err() {
        return None;
    }
    let units = name.split(|unit| *unit == 0).next()?;
    let family_name = std::ffi::OsString::from_wide(units);
    let mut components = Path::new(&family_name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
    .then_some(family_name)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use flui_platform::sandbox::{Confinement, FlatpakInfo};

    use super::{
        BaseDirs, Container, MAX_MIGRATED_FILES, MAX_SWEPT, Migrated, Staging, data_dirs_in,
        migrate, migrate_with, roots_after_migration,
    };
    use crate::storage::DataDirs;
    use crate::storage::FileStore;

    fn absolute(path: &str) -> PathBuf {
        std::env::temp_dir().join(path)
    }

    fn environment(vars: Vec<(&'static str, OsString)>) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn detected(vars: Vec<(&'static str, OsString)>, info: &FlatpakInfo) -> Container {
        let user = absolute("user");
        Container::Sandbox(Confinement::detect(
            &environment(vars),
            info,
            Some(&user),
            &|_| None,
        ))
    }

    #[test]
    fn a_sandboxed_process_keeps_its_data_in_its_container() {
        let base = BaseDirs {
            data: Some(absolute("user/data")),
            local: Some(absolute("user/local")),
        };
        let unconfined = detected(Vec::new(), &FlatpakInfo::Absent);
        assert_eq!(
            data_dirs_in(&unconfined, "app", &base),
            Some(DataDirs {
                roaming: absolute("user/data").join("app"),
                local: absolute("user/local").join("app"),
            })
        );

        let info = FlatpakInfo::Present("[Application]\nname=org.example.App\n".to_owned());
        let data_home = absolute("user/.var/app/org.example.App/data");
        let flatpak = detected(vec![("XDG_DATA_HOME", data_home.clone().into())], &info);
        assert_eq!(
            data_dirs_in(&flatpak, "app", &base).map(|dirs| dirs.local),
            Some(data_home.join("app"))
        );
        let without = detected(Vec::new(), &info);
        assert_eq!(
            data_dirs_in(&without, "app", &base).map(|dirs| dirs.roaming),
            Some(data_home.join("app")),
            "without XDG_DATA_HOME, the sandbox's own folder, never ~/.local/share"
        );
        let inherited = detected(
            vec![("FLATPAK_ID", "org.example.Ide".into())],
            &FlatpakInfo::Absent,
        );
        assert_eq!(
            data_dirs_in(&inherited, "app", &base).map(|dirs| dirs.roaming),
            Some(absolute("user/data").join("app")),
            "a host process that inherited FLATPAK_ID keeps the user's folders"
        );

        let container = absolute("user/Library/Containers/com.example.app/Data");
        let in_container = container
            .join("Library")
            .join("Application Support")
            .join("app");
        for vars in [
            vec![
                ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
                ("HOME", container.into()),
            ],
            vec![("APP_SANDBOX_CONTAINER_ID", "com.example.app".into())],
        ] {
            let sandboxed = detected(vars, &FlatpakInfo::Absent);
            assert_eq!(
                data_dirs_in(&sandboxed, "app", &base).map(|dirs| dirs.local),
                Some(in_container.clone()),
                "with HOME or without it, the container"
            );
        }

        let packaged = Container::Package {
            family_name: "Example.App_8wekyb3d8bbwe".into(),
        };
        let package = absolute("user/local")
            .join("Packages")
            .join("Example.App_8wekyb3d8bbwe");
        assert_eq!(
            data_dirs_in(&packaged, "app", &base),
            Some(DataDirs {
                roaming: package.join("RoamingState").join("app"),
                local: package.join("LocalState").join("app"),
            })
        );
        assert_eq!(
            data_dirs_in(&packaged, "app", &BaseDirs::default()),
            None,
            "no local data folder, no package folder"
        );
    }

    #[test]
    fn a_packaged_applications_store_is_copied_over_once_and_nothing_is_overwritten() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let old = scratch.path().join("old");
        let new = scratch.path().join("new");
        std::fs::create_dir_all(old.join(".machine-local")).expect("the old store");
        std::fs::write(old.join("notes"), b"notes").expect("written");
        std::fs::write(old.join(".machine-local").join("window"), b"window").expect("written");

        assert_eq!(migrate(&old, &new).expect("copied"), Migrated::Copied(2));
        assert_eq!(std::fs::read(new.join("notes")).expect("copied"), b"notes");
        assert_eq!(
            std::fs::read(new.join(".machine-local").join("window")).expect("copied"),
            b"window"
        );
        assert!(old.join("notes").exists(), "the old store stays");

        std::fs::write(old.join("notes"), b"changed").expect("written");
        assert_eq!(
            migrate(&old, &new).expect("judged"),
            Migrated::Nothing,
            "a root that holds a store is never copied into"
        );
        assert_eq!(std::fs::read(new.join("notes")).expect("kept"), b"notes");

        let empty = scratch.path().join("empty");
        assert_eq!(
            migrate(&empty, &scratch.path().join("fresh")).expect("judged"),
            Migrated::Nothing,
            "no old store, nothing to copy"
        );

        let large = scratch.path().join("large");
        std::fs::create_dir_all(&large).expect("a folder");
        for index in 0..=MAX_MIGRATED_FILES {
            std::fs::write(large.join(index.to_string()), b"").expect("written");
        }
        let untouched = scratch.path().join("untouched");
        assert_eq!(
            migrate(&large, &untouched).expect("judged"),
            Migrated::TooLarge
        );
        assert!(
            !untouched.exists(),
            "a store past the bound is not copied in part"
        );
    }

    /// A store of two files under a fresh scratch folder, and where it goes.
    fn two_file_store() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let old = scratch.path().join("old");
        std::fs::create_dir_all(&old).expect("the old store");
        std::fs::write(old.join("a"), b"a").expect("written");
        std::fs::write(old.join("b"), b"b").expect("written");
        let new = scratch.path().join("new");
        (scratch, old, new)
    }

    /// What the scratch folder holds besides the old store and the new root:
    /// a staging folder left behind.
    fn leftovers(scratch: &tempfile::TempDir) -> Vec<String> {
        std::fs::read_dir(scratch.path())
            .expect("listed")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name != "old" && name != "new")
            .collect()
    }

    /// A file copied as a migration copies it.
    fn copy_file(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
        std::fs::copy(source, target).map(drop)
    }

    /// The staging folder put in place as a migration puts it.
    fn rename(staging: &std::path::Path, root: &std::path::Path) -> std::io::Result<()> {
        std::fs::rename(staging, root)
    }

    #[test]
    fn a_copy_that_fails_midway_leaves_no_root_and_the_next_start_copies_again() {
        let (scratch, old, new) = two_file_store();
        let mut copied = 0;
        let failed = migrate_with(
            &old,
            &new,
            1,
            &mut |source, target| {
                copied += 1;
                if copied == 2 {
                    return Err(std::io::Error::other("the volume filled up"));
                }
                copy_file(source, target)
            },
            &mut rename,
        );
        assert!(failed.is_err());
        assert!(
            !new.exists(),
            "no root, which a later start would read as the store"
        );
        assert_eq!(
            leftovers(&scratch),
            Vec::<String>::new(),
            "the staging copy is removed"
        );
        assert_eq!(migrate(&old, &new).expect("copied"), Migrated::Copied(2));
        assert_eq!(std::fs::read(new.join("b")).expect("copied"), b"b");
    }

    #[test]
    fn a_root_another_instance_put_in_place_first_is_kept() {
        let (scratch, old, new) = two_file_store();
        let theirs = new.clone();
        let result = migrate_with(
            &old,
            &new,
            1,
            &mut |source, target| {
                // The other instance finishes its copy while this one copies.
                if !theirs.exists() {
                    std::fs::create_dir_all(&theirs).expect("their root");
                    std::fs::write(theirs.join("theirs"), b"theirs").expect("written");
                }
                copy_file(source, target)
            },
            &mut rename,
        );
        assert_eq!(result.expect("judged"), Migrated::Nothing);
        assert_eq!(
            std::fs::read(new.join("theirs")).expect("kept"),
            b"theirs",
            "the root in place is the store"
        );
        assert!(!new.join("a").exists(), "nothing of this copy reaches it");
        assert_eq!(
            leftovers(&scratch),
            Vec::<String>::new(),
            "the staging copy is discarded"
        );
    }

    #[test]
    fn a_rename_that_fails_removes_its_staging_and_reports_its_own_error() {
        let (scratch, old, new) = two_file_store();
        let failed = migrate_with(&old, &new, 1, &mut copy_file, &mut |_, root| {
            // A file takes the root's place as the rename fails, so reading
            // the root as a folder fails as well.
            std::fs::write(root, b"").expect("written");
            Err(std::io::Error::other("the rename failed"))
        });
        assert_eq!(
            failed.expect_err("the rename failed").to_string(),
            "the rename failed",
            "the rename's error, not the later read's"
        );
        assert_eq!(
            leftovers(&scratch),
            Vec::<String>::new(),
            "the staging copy is removed"
        );
    }

    /// Make `count` folders named as `name`'s staging folders under
    /// `parent`, each holding a file.
    fn leftover_staging(parent: &std::path::Path, name: &str, count: usize) {
        for index in 0..count {
            let folder = parent.join(format!(".{name}.migrating-{index}-{index}"));
            std::fs::create_dir(&folder).expect("a folder");
            std::fs::write(folder.join("a"), b"a").expect("written");
        }
    }

    #[test]
    fn staging_folders_an_earlier_copy_left_are_swept_by_exact_name_within_a_bound() {
        let (scratch, old, new) = two_file_store();
        leftover_staging(scratch.path(), "new", MAX_SWEPT + 1);
        let kept = [
            ".new.migrating-1-x",
            ".new.migrating-1",
            ".newer.migrating-1-1",
            ".other.migrating-1-1",
        ];
        for name in kept {
            std::fs::create_dir(scratch.path().join(name)).expect("a folder");
        }
        std::fs::write(scratch.path().join(".new.migrating-9-9-file"), b"").expect("written");
        assert_eq!(
            migrate_with(&old, &new, 1, &mut copy_file, &mut rename).expect("copied"),
            Migrated::Copied(2)
        );
        let mut left = leftovers(&scratch);
        left.sort();
        let staging_left = left
            .iter()
            .filter(|name| {
                name.strip_prefix(".new.migrating-")
                    .and_then(|rest| rest.split_once('-'))
                    .is_some_and(|(process, moment)| {
                        process.bytes().all(|b| b.is_ascii_digit())
                            && moment.bytes().all(|b| b.is_ascii_digit())
                    })
            })
            .count();
        assert_eq!(staging_left, 1, "one sweep removes at most its bound");
        for name in kept {
            assert!(
                left.iter().any(|left| left == name),
                "{name} is not a staging folder"
            );
        }
        assert!(left.iter().any(|left| left == ".new.migrating-9-9-file"));
    }

    /// A link named as a staging folder is not followed: the sweep removes
    /// at most the link, never what it points at.
    #[cfg(unix)]
    #[test]
    fn a_sweep_follows_no_link() {
        let (scratch, old, new) = two_file_store();
        let elsewhere = scratch.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("a folder");
        std::fs::write(elsewhere.join("precious"), b"p").expect("written");
        std::os::unix::fs::symlink(&elsewhere, scratch.path().join(".new.migrating-1-1"))
            .expect("a link");
        migrate_with(&old, &new, 2, &mut copy_file, &mut rename).expect("copied");
        assert!(elsewhere.join("precious").exists());
    }

    #[test]
    fn a_staging_folder_already_there_is_an_error_not_a_shared_folder() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let to = scratch.path().join("new");
        let staging = Staging::beside(&to).expect("its parent");
        let first = staging.create(7).expect("made");
        std::fs::write(first.join("theirs"), b"theirs").expect("written");
        assert_eq!(
            staging.create(7).map_err(|error| error.kind()),
            Err(std::io::ErrorKind::AlreadyExists),
        );
        assert!(
            first.join("theirs").exists(),
            "the other copy's folder is untouched"
        );
    }

    #[test]
    fn a_store_that_was_not_copied_stays_the_store_and_no_new_root_is_made() {
        let (scratch, old, new) = two_file_store();
        let old_local = scratch.path().join("old-local");
        let new_local = scratch.path().join("new-local");
        let earlier = DataDirs {
            roaming: old.clone(),
            local: old_local,
        };
        let dirs = DataDirs {
            roaming: new.clone(),
            local: new_local.clone(),
        };
        let used = roots_after_migration(earlier, dirs, &mut |from, to| {
            migrate_with(
                from,
                to,
                1,
                &mut |_, _| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
                &mut rename,
            )
        });
        assert_eq!(
            used.roaming, old,
            "the copy failed: the old store stays the store"
        );
        assert_eq!(
            used.local, new_local,
            "no old store at that root: the package's folder"
        );
        let store = FileStore::new(used);
        let name = flui_platform::StorageName::from_static("notes");
        store
            .write(&name, b"written", flui_platform::WriteMode::Replace)
            .expect("written");
        assert!(
            !new.exists(),
            "no new root, which the next start would take for the store"
        );
        assert_eq!(
            store.read(&name, 64).expect("read").bytes,
            Some(b"written".to_vec()),
            "the session reads back what it wrote"
        );
    }
}
