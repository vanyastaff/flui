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
//!   store it kept there before is copied over once ([`migrate`]).
//!
//! Reading `/.flatpak-info` and copying a store touch the file system: this
//! runs on the store's IO thread, at its first call.

#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;

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
    pub(crate) fn of_process() -> Self {
        let confinement = Confinement::of_process(
            &flui_platform::sandbox::read_flatpak_info(),
            // A data folder reads no manifest entry.
            &|_| None,
        );
        #[cfg(windows)]
        if confinement == Confinement::Unconfined
            && let Some(family_name) = package_family_name()
        {
            return Self::Package { family_name };
        }
        Self::Sandbox(confinement)
    }
}

/// The data folders of the application `app` in `container`; `None` when the
/// platform names none.
pub(crate) fn data_dirs_in(container: &Container, app: &str, base: &BaseDirs) -> Option<DataDirs> {
    let (roaming, local) = match container {
        Container::Sandbox(Confinement::Flatpak(flatpak)) => {
            let data = flatpak.data_home.clone().or_else(|| base.data.clone())?;
            (data.clone(), data)
        }
        Container::Sandbox(Confinement::AppSandbox(sandbox)) => {
            let support = sandbox.home.join("Library").join("Application Support");
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
    /// left where it was, and the new root starts empty.
    TooLarge,
}

/// Copy the store under `from` to `to` once: only when `to` holds no store
/// (it is missing or empty) and `from` holds one, every file and folder of
/// it within the bounds, each file made new (`create_new`, so nothing at
/// `to` is ever overwritten). A link, or anything but a file or a folder,
/// leaves the store where it was. Blocks on the file system.
#[cfg(any(windows, test))]
pub(crate) fn migrate(from: &Path, to: &Path) -> std::io::Result<Migrated> {
    let holds_store = |root: &Path| -> std::io::Result<bool> {
        match std::fs::read_dir(root) {
            Ok(mut entries) => Ok(entries.next().is_some()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    };
    if holds_store(to)? || !holds_store(from)? {
        return Ok(Migrated::Nothing);
    }
    let Some(entries) = listed(from)? else {
        return Ok(Migrated::TooLarge);
    };
    for (relative, is_folder) in &entries {
        let target = to.join(relative);
        if *is_folder {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut source = std::fs::File::open(from.join(relative))?;
        let mut copy = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::copy(&mut source, &mut copy)?;
        copy.sync_all()?;
    }
    Ok(Migrated::Copied(
        entries.iter().filter(|(_, is_folder)| !is_folder).count(),
    ))
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

    use super::{BaseDirs, Container, MAX_MIGRATED_FILES, Migrated, data_dirs_in, migrate};
    use crate::storage::DataDirs;

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
}
