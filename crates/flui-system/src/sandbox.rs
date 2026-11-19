//! What the process may open without a grant.
//!
//! A sandboxed process opens files under its own folders (a macOS App
//! Sandbox's container, a Flatpak's `~/.var/app/<id>`, the document portal's
//! folder, what a Flatpak's manifest exposes) and, outside them, only what
//! the system granted it: a file the user picked in the system's dialog, or
//! one a [`Grant`](crate::grant::Grant) reacquires. [`Sandbox`] states the
//! first part, so the layer never hands out a path the process cannot open:
//! a reacquired path outside it must come with the access that opens it
//! ([`Grants`](crate::grant::Grants)).
//!
//! The sandbox is the one `flui_platform::sandbox` detects, for this crate
//! and for the data folders alike: a Flatpak by its `/.flatpak-info`, read
//! off the owner thread, the App Sandbox by the environment its system sets.

use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux", test))]
use flui_platform::sandbox::Confinement;
use flui_platform::sandbox::is_plain;

/// The kind of sandbox a process runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SandboxKind {
    /// The macOS App Sandbox: the application's container
    /// (`~/Library/Containers/<bundle id>/Data`) and its temporary folder.
    AppSandbox,
    /// An iOS application's container: its home and its temporary folder.
    IosApp,
    /// A Flatpak: the application's folders under `~/.var/app/<id>`, its
    /// temporary folder, the document portal's folders and what its
    /// manifest's `filesystems` expose.
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
pub struct Sandbox(Option<Confined>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Confined {
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
            .then_some(Self(Some(Confined { kind, roots })))
    }

    /// The kind of sandbox; `None` for a process that has none.
    #[must_use]
    pub fn kind(&self) -> Option<SandboxKind> {
        self.0.as_ref().map(|confined| confined.kind)
    }

    /// The folders it reaches; empty for a process that has no sandbox, which
    /// reaches every path.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        self.0.as_ref().map_or(&[], |confined| &confined.roots)
    }

    /// Whether the process may open `path` without a grant: always without
    /// a sandbox; inside one, when `path` is absolute, holds no `..` and is
    /// under one of its folders.
    #[must_use]
    pub fn reaches(&self, path: &Path) -> bool {
        match &self.0 {
            None => true,
            Some(confined) => {
                is_plain(path) && confined.roots.iter().any(|root| path.starts_with(root))
            }
        }
    }
}

/// A sandbox of `kind` over the `roots` that are absolute and plain.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", test))]
fn confined(kind: SandboxKind, roots: impl IntoIterator<Item = PathBuf>) -> Sandbox {
    Sandbox(Some(Confined {
        kind,
        roots: roots.into_iter().filter(|root| is_plain(root)).collect(),
    }))
}

/// The sandbox a detected confinement reaches: a Flatpak's own folders, its
/// private `/tmp`, the document portal's folders and its manifest's
/// filesystems; the App Sandbox's container and temporary folder.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub(crate) fn of_confinement(confinement: &Confinement) -> Sandbox {
    match confinement {
        Confinement::Flatpak(flatpak) => confined(
            SandboxKind::Flatpak,
            flatpak.reached().map(Path::to_path_buf),
        ),
        Confinement::AppSandbox(container) => confined(
            SandboxKind::AppSandbox,
            [Some(container.home()), container.temporary()]
                .into_iter()
                .flatten()
                .map(Path::to_path_buf),
        ),
        _ => Sandbox::unconfined(),
    }
}

/// The sandbox a description read off the owner thread describes: `None`
/// when it could not be read, which no handle keeps, so the next resolve
/// asks again. The user's home is read through `user_home` only when the
/// sandbox needs it.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn of_description(
    read: std::io::Result<flui_platform::sandbox::FlatpakInfo>,
    user_home: &dyn Fn() -> Option<PathBuf>,
    user_dir: &dyn Fn(flui_platform::sandbox::UserFolder) -> Option<PathBuf>,
) -> Option<Sandbox> {
    let info = read.ok()?;
    Some(of_confinement(&Confinement::of_process(
        &info, user_home, user_dir,
    )))
}

/// The user's home as the system's user database records it
/// (`getpwuid_r`), whatever `HOME` says: what a container rebuilt under the
/// user's home is rebuilt under. `None` when the database has no record of
/// the user, or one that is not an absolute path.
///
/// `flui-native`'s data folders keep the same lookup
/// (`storage::container::user_database_home`): a capability crate does not
/// depend on the host, and `flui-platform`, which both reach, has no
/// `unsafe`. Each copy's test reads the record from a buffer too small for
/// it, so the two keep the same retry.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn user_database_home() -> Option<PathBuf> {
    user_database_home_from(1024)
}

/// [`user_database_home`], starting from a buffer of `initial` bytes and
/// doubling it while the record does not fit, up to 64 KiB.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[expect(
    unsafe_code,
    reason = "getpwuid_r writes the user's record into buffers it was given"
)]
fn user_database_home_from(initial: usize) -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt as _;

    /// The largest buffer a record is read into: names and a shell path.
    const MAX_RECORD: usize = 64 * 1024;
    // SAFETY: `getuid` takes nothing and always succeeds.
    let uid = unsafe { libc::getuid() };
    let mut buffer = vec![0_u8; initial.max(1)];
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
        return is_plain(&home).then_some(home);
    }
}

/// An iOS application's container: every iOS application is sandboxed, its
/// `HOME` its container and a `TMPDIR` inside it its temporary folder.
#[cfg(any(target_os = "ios", test))]
pub(crate) fn ios_app(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Sandbox {
    let home = env("HOME").map(PathBuf::from).filter(|home| is_plain(home));
    let temporary = home
        .as_deref()
        .and_then(|home| flui_platform::sandbox::usable_temporary(env, home));
    confined(SandboxKind::IosApp, home.into_iter().chain(temporary))
}

/// An Android application: its picks and reacquired documents are their
/// contents, so it reaches no path.
#[cfg(any(target_os = "android", test))]
pub(crate) fn android_app() -> Sandbox {
    Sandbox(Some(Confined {
        kind: SandboxKind::AndroidApp,
        roots: Box::default(),
    }))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use flui_platform::sandbox::{Confinement, FlatpakInfo, UserFolder};

    use super::{Sandbox, SandboxKind, android_app, ios_app, of_confinement, of_description};

    /// An absolute path on the host the test runs on, under no folder a
    /// sandbox reaches of itself (a Flatpak reaches `/tmp`).
    fn absolute(path: &str) -> PathBuf {
        std::env::temp_dir()
            .ancestors()
            .last()
            .expect("a path has a root")
            .join("flui-sandbox-test")
            .join(path)
    }

    fn environment(vars: Vec<(&'static str, OsString)>) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn no_user_dirs(_: UserFolder) -> Option<PathBuf> {
        None
    }

    fn no_home() -> Option<PathBuf> {
        None
    }

    #[test]
    fn a_description_that_could_not_be_read_answers_no_sandbox() {
        let unread = of_description(
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            &no_home,
            &no_user_dirs,
        );
        assert_eq!(unread, None, "no answer, which the next resolve asks again");
        assert_eq!(
            of_description(Ok(FlatpakInfo::Absent), &no_home, &no_user_dirs),
            Some(Sandbox::unconfined()),
            "an absent description is an answer"
        );
    }

    /// The lookup starts from a buffer of one byte, which no record fits:
    /// the answer is the one the default buffer gives, whatever it is (a
    /// test user with no record answers none from both).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_user_database_record_is_read_whole_from_a_buffer_too_small() {
        assert_eq!(
            super::user_database_home_from(1),
            super::user_database_home_from(1024)
        );
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
        assert!(
            !android_app().reaches(&container),
            "Android reaches no path"
        );
    }

    /// Each row: the environment and description a process has, a path,
    /// and whether its sandbox reaches it.
    #[test]
    fn each_sandbox_reaches_its_own_folders_and_what_its_system_exposes() {
        let home = absolute("home");
        let runtime = absolute("run");
        let container = absolute("Library/Containers/com.example.app/Data");
        let flatpak_env = || {
            environment(vec![
                ("FLATPAK_ID", "org.example.App".into()),
                ("HOME", home.clone().into()),
                ("XDG_RUNTIME_DIR", runtime.clone().into()),
                ("TMPDIR", absolute("sandbox-tmp").into()),
            ])
        };
        let info = |filesystems: &str| {
            FlatpakInfo::Present(format!(
                "[Application]\nname=org.example.App\n[Context]\nfilesystems={filesystems}\n"
            ))
        };
        let reaches = |env: &dyn Fn(&str) -> Option<OsString>, info: &FlatpakInfo, path: &Path| {
            of_confinement(&Confinement::detect(env, info, &no_home, &no_user_dirs)).reaches(path)
        };
        let documents = home.join("Documents").join("notes.txt");
        assert!(reaches(
            &flatpak_env(),
            &info(""),
            &home.join(".var/app/org.example.App/data/a")
        ));
        assert!(reaches(
            &flatpak_env(),
            &info(""),
            &runtime.join("doc/1a2b/notes.txt")
        ));
        assert!(
            !reaches(&flatpak_env(), &info(""), &documents),
            "not exposed"
        );
        assert!(
            reaches(&flatpak_env(), &info("home;"), &documents),
            "a manifest with --filesystem=home reaches the real path the portal answers"
        );
        assert!(
            !reaches(&flatpak_env(), &info("xdg-documents:ro;"), &documents),
            "a user folder `user-dirs.dirs` does not configure is ignored, as Flatpak ignores it"
        );
        let configured =
            |folder: UserFolder| (folder == UserFolder::Documents).then(|| home.join("Documents"));
        assert!(
            of_confinement(&Confinement::detect(
                &flatpak_env(),
                &info("xdg-documents:ro;"),
                &no_home,
                &configured
            ))
            .reaches(&documents),
            "a configured xdg folder, its mode dropped"
        );
        assert!(
            !reaches(
                &flatpak_env(),
                &info(""),
                &absolute("sandbox-tmp").join("a")
            ),
            "Flatpak unsets TMPDIR: an inherited one is not the sandbox's"
        );
        assert_eq!(
            Confinement::detect(
                &flatpak_env(),
                &FlatpakInfo::Absent,
                &no_home,
                &no_user_dirs
            ),
            Confinement::Unconfined,
            "FLATPAK_ID without the description is no Flatpak"
        );

        let mac = environment(vec![
            ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
            ("HOME", container.clone().into()),
            ("TMPDIR", container.join("tmp").into()),
        ]);
        assert!(reaches(
            &mac,
            &FlatpakInfo::Absent,
            &container.join("Library/a")
        ));
        assert!(!reaches(&mac, &FlatpakInfo::Absent, &home.join("a")));
        let root = std::env::temp_dir()
            .ancestors()
            .last()
            .expect("a path has a root")
            .to_path_buf();
        let wide = environment(vec![
            ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
            ("HOME", container.clone().into()),
            ("TMPDIR", root.into()),
        ]);
        assert!(
            !reaches(&wide, &FlatpakInfo::Absent, &home.join("a")),
            "a TMPDIR of a root would reach everything: ignored"
        );

        let ios = environment(vec![
            ("HOME", container.clone().into()),
            ("TMPDIR", absolute("elsewhere").into()),
        ]);
        let sandbox = ios_app(&ios);
        assert_eq!(sandbox.kind(), Some(SandboxKind::IosApp));
        assert_eq!(
            sandbox.roots(),
            std::slice::from_ref(&container),
            "a TMPDIR outside the container is ignored"
        );
    }
}
