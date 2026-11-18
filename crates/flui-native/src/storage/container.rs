//! Where a sandboxed process keeps its data: the container the system gives
//! it, never a folder its sandbox keeps it from opening.
//!
//! - A Flatpak (`FLATPAK_ID` set) keeps its data under
//!   `~/.var/app/<id>/data`, which the sandbox exposes and names as its
//!   `XDG_DATA_HOME`; the host's `~/.local/share`, where a process with no
//!   `XDG_DATA_HOME` would fall back to, is not in the sandbox.
//! - The macOS App Sandbox (`APP_SANDBOX_CONTAINER_ID` set) keeps it in the
//!   application's container, `~/Library/Containers/<id>/Data`, which the
//!   system makes the process's `HOME`; a process whose `HOME` is gone would
//!   otherwise be answered the user's own home, outside the container.
//! - A packaged Windows application (an MSIX package identity) keeps it in
//!   its package's folders, `%LOCALAPPDATA%\Packages\<family name>\LocalState`
//!   and `RoamingState`, which the system removes with the package: Windows
//!   would otherwise redirect a write under `%APPDATA%` into the package's
//!   private copy, a path no other process sees.
//!
//! The container is read from the environment the system sets and, on
//! Windows, the process's package identity: no file system call.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::DataDirs;

/// The container a process's data lives in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Container {
    /// No sandbox: the user's own data folders.
    Unconfined,
    /// A Flatpak of the application `id`.
    Flatpak { id: OsString },
    /// The macOS App Sandbox, the application's container `id`.
    AppSandbox { id: OsString },
    /// A Windows package of the family `family_name`.
    Package { family_name: OsString },
}

/// The folders a process without a container keeps its data in, as the
/// platform names them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BaseDirs {
    /// The user's roaming data folder (`dirs::data_dir`).
    pub(crate) data: Option<PathBuf>,
    /// The user's local data folder (`dirs::data_local_dir`).
    pub(crate) local: Option<PathBuf>,
    /// The user's home folder (`dirs::home_dir`): `HOME`, or the system's
    /// user database's entry when `HOME` is not set.
    pub(crate) home: Option<PathBuf>,
}

/// What the container's rules read of the process's environment.
pub(crate) type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// An absolute path the variable `name` names.
fn folder(env: Environment<'_>, name: &str) -> Option<PathBuf> {
    env(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Whether `name` is one plain name, as an id or a family name is.
fn is_one_name(name: &OsString) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

impl Container {
    /// The container the environment names, before the platform's package
    /// identity: a Flatpak or the App Sandbox by the variables their systems
    /// set.
    pub(crate) fn of_environment(env: Environment<'_>) -> Self {
        if let Some(id) = env("FLATPAK_ID").filter(is_one_name) {
            return Self::Flatpak { id };
        }
        if let Some(id) = env("APP_SANDBOX_CONTAINER_ID").filter(is_one_name) {
            return Self::AppSandbox { id };
        }
        Self::Unconfined
    }

    /// The process's container.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn of_process() -> Self {
        let container = Self::of_environment(&|name| std::env::var_os(name));
        #[cfg(windows)]
        if container == Self::Unconfined
            && let Some(family_name) = package_family_name()
        {
            return Self::Package { family_name };
        }
        container
    }
}

/// The data folders of the application `app` in `container`; `None` when the
/// platform names none.
pub(crate) fn data_dirs_in(
    container: &Container,
    app: &str,
    env: Environment<'_>,
    base: &BaseDirs,
) -> Option<DataDirs> {
    let (roaming, local) = match container {
        Container::Unconfined => (base.data.clone()?, base.local.clone()?),
        Container::Flatpak { id } => {
            let data = folder(env, "XDG_DATA_HOME").or_else(|| {
                folder(env, "HOME")
                    .or_else(|| base.home.clone())
                    .map(|home| home.join(".var").join("app").join(id).join("data"))
            })?;
            (data.clone(), data)
        }
        Container::AppSandbox { id } => {
            let home = folder(env, "HOME").or_else(|| {
                base.home.clone().map(|home| {
                    home.join("Library")
                        .join("Containers")
                        .join(id)
                        .join("Data")
                })
            })?;
            let support = home.join("Library").join("Application Support");
            (support.clone(), support)
        }
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

/// The process's package family name, when it has a package identity.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "GetCurrentPackageFamilyName writes into a buffer it was given"
)]
fn package_family_name() -> Option<OsString> {
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
    let family_name = OsString::from_wide(units);
    is_one_name(&family_name).then_some(family_name)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{BaseDirs, Container, data_dirs_in};
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

    #[test]
    fn a_sandboxed_process_keeps_its_data_in_its_container() {
        let base = BaseDirs {
            data: Some(absolute("user/data")),
            local: Some(absolute("user/local")),
            home: Some(absolute("user")),
        };
        let nothing = environment(Vec::new());
        assert_eq!(
            data_dirs_in(&Container::Unconfined, "app", &nothing, &base),
            Some(DataDirs {
                roaming: absolute("user/data").join("app"),
                local: absolute("user/local").join("app"),
            })
        );

        let flatpak = Container::Flatpak {
            id: "org.example.App".into(),
        };
        let data_home = absolute("user/.var/app/org.example.App/data");
        let set = environment(vec![("XDG_DATA_HOME", data_home.clone().into())]);
        assert_eq!(
            data_dirs_in(&flatpak, "app", &set, &base).map(|dirs| dirs.local),
            Some(data_home.join("app"))
        );
        assert_eq!(
            data_dirs_in(&flatpak, "app", &nothing, &base).map(|dirs| dirs.roaming),
            Some(
                absolute("user")
                    .join(".var/app/org.example.App/data")
                    .join("app")
            ),
            "without XDG_DATA_HOME, the sandbox's own folder, never ~/.local/share"
        );

        let sandboxed = Container::AppSandbox {
            id: "com.example.app".into(),
        };
        let container = absolute("user/Library/Containers/com.example.app/Data");
        let home = environment(vec![("HOME", container.clone().into())]);
        let in_container = container
            .join("Library")
            .join("Application Support")
            .join("app");
        assert_eq!(
            data_dirs_in(&sandboxed, "app", &home, &base).map(|dirs| dirs.local),
            Some(in_container.clone())
        );
        assert_eq!(
            data_dirs_in(&sandboxed, "app", &nothing, &base).map(|dirs| dirs.local),
            Some(in_container),
            "a process whose HOME is gone keeps to its container"
        );

        let packaged = Container::Package {
            family_name: "Example.App_8wekyb3d8bbwe".into(),
        };
        let package = absolute("user/local")
            .join("Packages")
            .join("Example.App_8wekyb3d8bbwe");
        assert_eq!(
            data_dirs_in(&packaged, "app", &nothing, &base),
            Some(DataDirs {
                roaming: package.join("RoamingState").join("app"),
                local: package.join("LocalState").join("app"),
            })
        );
        assert_eq!(
            data_dirs_in(&packaged, "app", &nothing, &BaseDirs::default()),
            None,
            "no local data folder, no package folder"
        );
    }

    #[test]
    fn the_container_is_the_one_its_system_names_in_the_environment() {
        let flatpak = environment(vec![("FLATPAK_ID", "org.example.App".into())]);
        assert_eq!(
            Container::of_environment(&flatpak),
            Container::Flatpak {
                id: "org.example.App".into()
            }
        );
        let mac = environment(vec![("APP_SANDBOX_CONTAINER_ID", "com.example.app".into())]);
        assert_eq!(
            Container::of_environment(&mac),
            Container::AppSandbox {
                id: "com.example.app".into()
            }
        );
        let climbing = environment(vec![("FLATPAK_ID", "../../etc".into())]);
        assert_eq!(Container::of_environment(&climbing), Container::Unconfined);
        assert_eq!(
            Container::of_environment(&environment(Vec::new())),
            Container::Unconfined
        );
    }
}
