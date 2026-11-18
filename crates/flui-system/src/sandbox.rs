//! What the process may open without a grant.
//!
//! A sandboxed process opens files under its own folders (a macOS App
//! Sandbox's container, a Flatpak's `~/.var/app/<id>`, the document portal's
//! folder) and, outside them, only what the system granted it: a file the
//! user picked in the system's dialog, or one a [`Grant`](crate::grant::Grant)
//! reacquires. [`Sandbox`] states the first part, so the layer never hands
//! out a path the process cannot open: a reacquired path outside it must
//! come with the access that opens it ([`Grants`](crate::grant::Grants)).
//!
//! Each platform's provider reads its sandbox from the process's
//! environment, which the system sets for a sandboxed process before it
//! starts, so nothing here touches the file system.

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", test))]
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The kind of sandbox a process runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SandboxKind {
    /// The macOS App Sandbox: the application's container
    /// (`~/Library/Containers/<bundle id>/Data`, its `HOME`) and its
    /// temporary folder.
    AppSandbox,
    /// An iOS application's container: its home and its temporary folder.
    IosApp,
    /// A Flatpak: the application's folders under `~/.var/app/<id>`, its
    /// temporary folder and the document portal's folders. What its
    /// manifest's `filesystems` grant beyond them is not stated here.
    Flatpak,
    /// An Android application: a pick and a reacquired document are handed
    /// over as their contents there, never as a path, so no path is
    /// reached.
    AndroidApp,
}

/// What the process may open without a grant: everything the user can, or
/// the folders of its sandbox.
///
/// A path is judged by its components, without the file system: an
/// absolute path with no `..` under one of the sandbox's folders. A link
/// inside a folder that leads out of it is the sandbox's to refuse when the
/// path is opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sandbox(Option<Confinement>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Confinement {
    kind: SandboxKind,
    roots: Box<[PathBuf]>,
}

impl Sandbox {
    /// No sandbox: the process opens whatever the user may.
    #[must_use]
    pub const fn unconfined() -> Self {
        Self(None)
    }

    /// A sandbox of `kind` that reaches what is under `roots` and nothing
    /// else; `None` when a root is relative or holds `..`, which names no
    /// folder of its own.
    #[must_use]
    pub fn try_confined(
        kind: SandboxKind,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> Option<Self> {
        let roots = roots.into_iter().collect::<Box<[PathBuf]>>();
        roots
            .iter()
            .all(|root| is_plain(root))
            .then_some(Self(Some(Confinement { kind, roots })))
    }

    /// The kind of sandbox; `None` for a process that has none.
    #[must_use]
    pub fn kind(&self) -> Option<SandboxKind> {
        self.0.as_ref().map(|confinement| confinement.kind)
    }

    /// The folders it reaches; empty for a process that has no sandbox, which
    /// reaches every path.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        self.0
            .as_ref()
            .map_or(&[], |confinement| &confinement.roots)
    }

    /// Whether the process may open `path` without a grant: always without
    /// a sandbox; inside one, when `path` is absolute, holds no `..` and is
    /// under one of its folders.
    #[must_use]
    pub fn reaches(&self, path: &Path) -> bool {
        match &self.0 {
            None => true,
            Some(confinement) => {
                is_plain(path) && confinement.roots.iter().any(|root| path.starts_with(root))
            }
        }
    }
}

/// Whether `path` is absolute and names its folders without climbing.
fn is_plain(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| component != Component::ParentDir)
}

/// What the detection reads of the process's environment.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", test))]
pub(crate) type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// A sandbox of `kind` over the `roots` that are absolute and plain; the
/// others name no folder of the host's.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", test))]
fn confined(kind: SandboxKind, roots: impl IntoIterator<Item = PathBuf>) -> Sandbox {
    Sandbox(Some(Confinement {
        kind,
        roots: roots.into_iter().filter(|root| is_plain(root)).collect(),
    }))
}

/// An absolute path an environment variable names, if it names one.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", test))]
fn folder(env: Environment<'_>, name: &str) -> Option<PathBuf> {
    env(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// The macOS App Sandbox, when the process runs in it: the system sets
/// `APP_SANDBOX_CONTAINER_ID` for a sandboxed process, and its `HOME` and
/// `TMPDIR` to the container's folders.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn mac_app_sandbox(env: Environment<'_>) -> Sandbox {
    if env("APP_SANDBOX_CONTAINER_ID").is_none() {
        return Sandbox::unconfined();
    }
    container(SandboxKind::AppSandbox, env)
}

/// An iOS application's container: every iOS application is sandboxed, its
/// `HOME` and `TMPDIR` its container's.
#[cfg(any(target_os = "ios", test))]
pub(crate) fn ios_app(env: Environment<'_>) -> Sandbox {
    container(SandboxKind::IosApp, env)
}

/// A container whose folders are the process's `HOME` and `TMPDIR`.
#[cfg(any(target_os = "macos", target_os = "ios", test))]
fn container(kind: SandboxKind, env: Environment<'_>) -> Sandbox {
    confined(
        kind,
        ["HOME", "TMPDIR"]
            .into_iter()
            .filter_map(|name| folder(env, name)),
    )
}

/// A Flatpak, when the process runs in one (`FLATPAK_ID` is set): the
/// application's folders (`~/.var/app/<id>`, where its `XDG_DATA_HOME`,
/// `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` are), its temporary folder (the
/// sandbox's own `/tmp`) and the document portal's folders, inside the
/// sandbox (`/run/flatpak/doc`) and under the user's runtime folder
/// (`$XDG_RUNTIME_DIR/doc`), where the portal grants each picked file.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn flatpak(env: Environment<'_>) -> Sandbox {
    let Some(id) = env("FLATPAK_ID").filter(|id| is_one_name(Path::new(id))) else {
        return Sandbox::unconfined();
    };
    let own = folder(env, "HOME").map(|home| home.join(".var").join("app").join(id));
    let temporary = folder(env, "TMPDIR").unwrap_or_else(|| PathBuf::from("/tmp"));
    let documents = folder(env, "XDG_RUNTIME_DIR").map(|runtime| runtime.join("doc"));
    let roots = own
        .into_iter()
        .chain([temporary, PathBuf::from("/run/flatpak/doc")])
        .chain(documents);
    confined(SandboxKind::Flatpak, roots)
}

/// Whether `name` is one plain name, as an application id is.
#[cfg(any(target_os = "linux", test))]
fn is_one_name(name: &Path) -> bool {
    let mut components = name.components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// An Android application: its picks and reacquired documents are their
/// contents, so it reaches no path.
#[cfg(any(target_os = "android", test))]
pub(crate) fn android_app() -> Sandbox {
    Sandbox(Some(Confinement {
        kind: SandboxKind::AndroidApp,
        roots: Box::default(),
    }))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{Sandbox, SandboxKind, android_app, flatpak, ios_app, mac_app_sandbox};

    /// An absolute path on the host the test runs on.
    fn absolute(path: &str) -> PathBuf {
        std::env::temp_dir().join(path)
    }

    /// An environment of `vars`.
    fn environment(vars: Vec<(&'static str, OsString)>) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn a_sandbox_reaches_what_is_under_its_folders_and_nothing_that_climbs_out() {
        let container = absolute("container");
        let sandbox = Sandbox::try_confined(SandboxKind::AppSandbox, [container.clone()])
            .expect("an absolute root");
        assert!(sandbox.reaches(&container));
        assert!(sandbox.reaches(&container.join("Documents").join("a.txt")));
        assert!(!sandbox.reaches(&absolute("elsewhere.txt")));
        assert!(
            !sandbox.reaches(&container.join("..").join("elsewhere.txt")),
            "a path that climbs out is judged by its components, not its prefix"
        );
        assert!(
            !sandbox.reaches(&absolute("container-of-another")),
            "a folder whose name begins alike is another folder"
        );
        assert!(!sandbox.reaches(Path::new("container/a.txt")));
        assert!(Sandbox::unconfined().reaches(&absolute("anywhere")));
        assert_eq!(
            Sandbox::try_confined(SandboxKind::Flatpak, [PathBuf::from("relative")]),
            None
        );
        assert_eq!(
            Sandbox::try_confined(SandboxKind::Flatpak, [container.join("..")]),
            None
        );
        assert!(
            !android_app().reaches(&container),
            "Android reaches no path"
        );
    }

    #[test]
    fn each_platform_reads_its_sandbox_from_the_environment_it_sets() {
        let home = absolute("home");
        let tmp = absolute("tmp");
        let mac = environment(vec![
            ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
            ("HOME", home.clone().into()),
            ("TMPDIR", tmp.clone().into()),
        ]);
        let sandbox = mac_app_sandbox(&mac);
        assert_eq!(sandbox.kind(), Some(SandboxKind::AppSandbox));
        assert_eq!(sandbox.roots(), [home.clone(), tmp]);
        let unsandboxed = environment(vec![("HOME", home.clone().into())]);
        assert_eq!(mac_app_sandbox(&unsandboxed), Sandbox::unconfined());
        assert_eq!(ios_app(&unsandboxed).kind(), Some(SandboxKind::IosApp));
        assert_eq!(ios_app(&unsandboxed).roots(), std::slice::from_ref(&home));

        let runtime = absolute("run");
        let flatpak_env = environment(vec![
            ("FLATPAK_ID", "org.example.App".into()),
            ("HOME", home.clone().into()),
            ("TMPDIR", absolute("sandbox-tmp").into()),
            ("XDG_RUNTIME_DIR", runtime.clone().into()),
        ]);
        let sandbox = flatpak(&flatpak_env);
        assert_eq!(sandbox.kind(), Some(SandboxKind::Flatpak));
        assert!(sandbox.reaches(&home.join(".var/app/org.example.App/data/notes.txt")));
        assert!(sandbox.reaches(&runtime.join("doc").join("1a2b3c").join("notes.txt")));
        assert!(!sandbox.reaches(&home.join("Documents").join("notes.txt")));
        assert!(!sandbox.reaches(&home.join(".var/app/org.example.Other/data")));
        assert_eq!(flatpak(&unsandboxed), Sandbox::unconfined());
        let climbing = environment(vec![("FLATPAK_ID", "../etc".into())]);
        assert_eq!(
            flatpak(&climbing),
            Sandbox::unconfined(),
            "an id that is no one name is no Flatpak's"
        );
        let relative = environment(vec![
            ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
            ("HOME", "relative".into()),
        ]);
        assert_eq!(
            mac_app_sandbox(&relative).roots(),
            [] as [PathBuf; 0],
            "a relative folder is not one the sandbox reaches"
        );
    }
}
