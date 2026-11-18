//! The sandbox a process runs in, read once for every crate that needs it:
//! where a sandboxed process keeps its data (`flui-native`'s data folders) and
//! what it may open without a grant (`flui-system`'s grants).
//!
//! - A **Flatpak** is a process that has `/.flatpak-info`, the file
//!   `bubblewrap` mounts into every Flatpak sandbox: `FLATPAK_ID` alone is not
//!   one, since a host process started from a Flatpak's terminal (an IDE's)
//!   inherits it. The file's `[Context] filesystems=` names what the
//!   application's manifest exposes beyond its own folders.
//! - The **macOS App Sandbox** is a process the system started with
//!   `APP_SANDBOX_CONTAINER_ID`, whose container
//!   (`~/Library/Containers/<id>/Data`) the system makes its `HOME`.
//!
//! [`read_flatpak_info`] touches the file system and runs off the owner
//! thread; [`Confinement::detect`] is a pure function of what it is given.

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

/// What [`Confinement::detect`] reads of the process's environment.
pub type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Where Flatpak mounts its description of the sandbox.
pub const FLATPAK_INFO: &str = "/.flatpak-info";

/// The longest `/.flatpak-info` read, in bytes: it names an application, its
/// runtime and its permissions; one larger is not that file.
pub const MAX_FLATPAK_INFO_LEN: u64 = 64 * 1024;

/// What the process found at [`FLATPAK_INFO`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlatpakInfo {
    /// No such file: the process is no Flatpak, whatever its environment
    /// says.
    Absent,
    /// The file is there, and holds this text: empty when it was larger than
    /// [`MAX_FLATPAK_INFO_LEN`] or could not be read, which leaves the
    /// sandbox its own folders alone.
    Present(String),
}

/// Read [`FLATPAK_INFO`], its size checked before a byte is read and the read
/// bounded again. Blocks on the file system: off the owner thread only.
#[must_use]
pub fn read_flatpak_info() -> FlatpakInfo {
    read_info_at(Path::new(FLATPAK_INFO))
}

fn read_info_at(path: &Path) -> FlatpakInfo {
    let Ok(metadata) = std::fs::metadata(path) else {
        return FlatpakInfo::Absent;
    };
    if !metadata.is_file() || metadata.len() > MAX_FLATPAK_INFO_LEN {
        return FlatpakInfo::Present(String::new());
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(MAX_FLATPAK_INFO_LEN + 1).read_to_end(&mut bytes));
    let fits = u64::try_from(bytes.len()).is_ok_and(|len| len <= MAX_FLATPAK_INFO_LEN);
    match (read, fits) {
        (Ok(_), true) => FlatpakInfo::Present(String::from_utf8_lossy(&bytes).into_owned()),
        _ => FlatpakInfo::Present(String::new()),
    }
}

/// A Flatpak sandbox, as its description and environment say.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlatpakSandbox {
    /// The application's id (`[Application] name=`, else `FLATPAK_ID`), one
    /// plain name; `None` when neither names one.
    pub id: Option<OsString>,
    /// The application's own folder, `~/.var/app/<id>`.
    pub own: Option<PathBuf>,
    /// Its data folder: `XDG_DATA_HOME`, else `~/.var/app/<id>/data`.
    pub data_home: Option<PathBuf>,
    /// Its temporary folder: a usable `TMPDIR`, else the sandbox's own `/tmp`.
    pub temporary: PathBuf,
    /// The document portal's folders: Flatpak's mount inside the sandbox and
    /// `$XDG_RUNTIME_DIR/doc`.
    pub documents: Vec<PathBuf>,
    /// What the manifest's `filesystems` expose, each entry resolved to a
    /// folder, an entry with `!` taking back an earlier one.
    pub filesystems: Vec<PathBuf>,
}

/// The macOS App Sandbox's container.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppSandbox {
    /// The container's id (`APP_SANDBOX_CONTAINER_ID`).
    pub id: OsString,
    /// The container, `~/Library/Containers/<id>/Data`: `HOME` when it is
    /// the container's, else derived from the user's home.
    pub home: PathBuf,
    /// The container's temporary folder: a `TMPDIR` inside the container.
    pub temporary: Option<PathBuf>,
}

/// The sandbox a process runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Confinement {
    /// None: the process opens what the user can.
    Unconfined,
    /// A Flatpak.
    Flatpak(FlatpakSandbox),
    /// The macOS App Sandbox.
    AppSandbox(AppSandbox),
}

/// Whether `path` is absolute and names its folders without climbing.
#[must_use]
pub fn is_plain(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| component != Component::ParentDir)
}

/// Whether `name` is one plain name, as an application or container id is.
fn is_one_name(name: &OsString) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// An absolute, plain path the variable `name` names.
fn folder(env: Environment<'_>, name: &str) -> Option<PathBuf> {
    env(name).map(PathBuf::from).filter(|path| is_plain(path))
}

/// `TMPDIR`, when it names a temporary folder a sandbox may reach: absolute,
/// plain, not a root (`/` would reach everything), and inside `within` when
/// given (an App Sandbox's container).
#[must_use]
pub fn usable_temporary(env: Environment<'_>, within: Option<&Path>) -> Option<PathBuf> {
    folder(env, "TMPDIR")
        .filter(|path| path.parent().is_some())
        .filter(|path| within.is_none_or(|container| path.starts_with(container)))
}

impl Confinement {
    /// The process's sandbox: its environment, `flatpak_info` as
    /// [`read_flatpak_info`] read it, and the user's home as the standard
    /// library finds it (`HOME`, else the system's user database).
    pub fn of_process(
        flatpak_info: &FlatpakInfo,
        user_dir: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Self {
        let home = std::env::home_dir();
        Self::detect(
            &|name| std::env::var_os(name),
            flatpak_info,
            home.as_deref(),
            user_dir,
        )
    }

    /// The sandbox `env` and `flatpak_info` describe; `user_home` is the
    /// user's home from the system's user database, for a container whose
    /// `HOME` is not set; `user_dir` names a user folder (`"documents"`,
    /// `"download"`, …) as the user's `user-dirs.dirs` does, for a
    /// manifest's `xdg-*` entries.
    pub fn detect(
        env: Environment<'_>,
        flatpak_info: &FlatpakInfo,
        user_home: Option<&Path>,
        user_dir: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Self {
        if let FlatpakInfo::Present(text) = flatpak_info {
            return Self::Flatpak(flatpak(env, text, user_home, user_dir));
        }
        if let Some(id) = env("APP_SANDBOX_CONTAINER_ID").filter(is_one_name) {
            let derived = || {
                user_home.map(|home| {
                    home.join("Library")
                        .join("Containers")
                        .join(&id)
                        .join("Data")
                })
            };
            if let Some(home) = folder(env, "HOME").or_else(derived) {
                let temporary = usable_temporary(env, Some(&home));
                return Self::AppSandbox(AppSandbox {
                    id,
                    home,
                    temporary,
                });
            }
        }
        Self::Unconfined
    }
}

/// The value of `key` in the `[group]` of an INI-style `text`, its last line
/// winning.
fn ini_value<'a>(text: &'a str, group: &str, key: &str) -> Option<&'a str> {
    let mut in_group = false;
    let mut value = None;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_group = name == group;
        } else if in_group
            && let Some((name, rest)) = line.split_once('=')
            && name.trim() == key
        {
            value = Some(rest.trim());
        }
    }
    value
}

fn flatpak(
    env: Environment<'_>,
    text: &str,
    user_home: Option<&Path>,
    user_dir: &dyn Fn(&str) -> Option<PathBuf>,
) -> FlatpakSandbox {
    let id = ini_value(text, "Application", "name")
        .map(OsString::from)
        .or_else(|| env("FLATPAK_ID"))
        .filter(is_one_name);
    let home = folder(env, "HOME").or_else(|| user_home.map(Path::to_path_buf));
    let own = home
        .as_ref()
        .zip(id.as_ref())
        .map(|(home, id)| home.join(".var").join("app").join(id));
    let data_home =
        folder(env, "XDG_DATA_HOME").or_else(|| own.as_ref().map(|own| own.join("data")));
    let runtime = folder(env, "XDG_RUNTIME_DIR");
    let documents = [
        Some(PathBuf::from("/run/flatpak/doc")),
        runtime.as_ref().map(|run| run.join("doc")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let context = Context {
        home: home.as_deref(),
        runtime: runtime.as_deref(),
        user_dir,
    };
    let filesystems = ini_value(text, "Context", "filesystems")
        .map_or_else(Vec::new, |list| context.filesystems(list));
    FlatpakSandbox {
        id,
        own,
        data_home,
        temporary: usable_temporary(env, None).unwrap_or_else(|| PathBuf::from("/tmp")),
        documents,
        filesystems,
    }
}

/// What a manifest's `filesystems` entries resolve against.
struct Context<'a> {
    home: Option<&'a Path>,
    runtime: Option<&'a Path>,
    user_dir: &'a dyn Fn(&str) -> Option<PathBuf>,
}

/// The user folders a manifest names as `xdg-<name>`, and the English folder
/// a home holds when the user's `user-dirs.dirs` names none.
const USER_FOLDERS: [(&str, &str); 8] = [
    ("desktop", "Desktop"),
    ("documents", "Documents"),
    ("download", "Downloads"),
    ("music", "Music"),
    ("pictures", "Pictures"),
    ("public-share", "Public"),
    ("videos", "Videos"),
    ("templates", "Templates"),
];

/// The folders under the user's home a manifest names as `xdg-<name>`.
const HOME_FOLDERS: [(&str, &str); 4] = [
    ("config", ".config"),
    ("cache", ".cache"),
    ("data", ".local/share"),
    ("state", ".local/state"),
];

impl Context<'_> {
    /// The folders `list` (`;`-separated entries) exposes, in order, an entry
    /// with `!` taking back the folder it names.
    fn filesystems(&self, list: &str) -> Vec<PathBuf> {
        let mut exposed: Vec<PathBuf> = Vec::new();
        for entry in list
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let (negated, entry) = match entry.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, entry),
            };
            // The access mode does not change what is reached.
            let entry = entry
                .strip_suffix(":ro")
                .or_else(|| entry.strip_suffix(":rw"))
                .or_else(|| entry.strip_suffix(":create"))
                .unwrap_or(entry);
            let Some(folder) = self.resolve(entry).filter(|folder| is_plain(folder)) else {
                continue;
            };
            exposed.retain(|known| *known != folder);
            if !negated {
                exposed.push(folder);
            }
        }
        exposed
    }

    /// The folder one entry names; `None` for one this library does not read.
    fn resolve(&self, entry: &str) -> Option<PathBuf> {
        let (head, rest) = entry
            .split_once('/')
            .map_or((entry, None), |(head, rest)| (head, Some(rest)));
        let under = |base: PathBuf| match rest {
            Some(rest) if !rest.is_empty() => base.join(rest),
            _ => base,
        };
        match head {
            "host" if rest.is_none() => Some(PathBuf::from("/")),
            "host-os" if rest.is_none() => Some(PathBuf::from("/run/host")),
            "host-etc" if rest.is_none() => Some(PathBuf::from("/run/host/etc")),
            "home" if rest.is_none() => self.home.map(Path::to_path_buf),
            "~" => self.home.map(|home| under(home.to_path_buf())),
            "" => Some(PathBuf::from(entry)),
            "xdg-run" => self.runtime.map(|run| under(run.to_path_buf())),
            _ => {
                let name = head.strip_prefix("xdg-")?;
                if let Some((_, folder)) = HOME_FOLDERS.iter().find(|(key, _)| *key == name) {
                    return self.home.map(|home| under(home.join(folder)));
                }
                let (_, english) = USER_FOLDERS.iter().find(|(key, _)| *key == name)?;
                (self.user_dir)(name)
                    .or_else(|| self.home.map(|home| home.join(english)))
                    .map(under)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{Confinement, FlatpakInfo, read_info_at, usable_temporary};

    fn environment(vars: Vec<(&'static str, OsString)>) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn absolute(path: &str) -> PathBuf {
        std::env::temp_dir().join(path)
    }

    fn no_user_dirs(_: &str) -> Option<PathBuf> {
        None
    }

    #[test]
    fn a_flatpak_is_a_process_with_its_description_whatever_its_environment_says() {
        let inherited = environment(vec![("FLATPAK_ID", "org.example.Ide".into())]);
        assert_eq!(
            Confinement::detect(&inherited, &FlatpakInfo::Absent, None, &no_user_dirs),
            Confinement::Unconfined,
            "a host process started from a Flatpak's terminal inherits FLATPAK_ID"
        );
        let home = absolute("home");
        let env = environment(vec![
            ("FLATPAK_ID", "org.example.Env".into()),
            ("HOME", home.clone().into()),
        ]);
        let info = FlatpakInfo::Present("[Application]\nname=org.example.App\n".to_owned());
        let Confinement::Flatpak(sandbox) = Confinement::detect(&env, &info, None, &no_user_dirs)
        else {
            panic!("a process with the description is a Flatpak");
        };
        assert_eq!(
            sandbox.id,
            Some("org.example.App".into()),
            "the description names it"
        );
        assert_eq!(sandbox.own, Some(home.join(".var/app/org.example.App")));
        assert_eq!(
            sandbox.data_home,
            Some(home.join(".var/app/org.example.App/data"))
        );
    }

    /// A manifest's entries are Linux paths: the table runs where they are
    /// absolute.
    #[cfg(unix)]
    #[test]
    fn a_manifests_filesystems_name_what_the_flatpak_reaches() {
        let env = environment(vec![
            ("HOME", "/home/ann".into()),
            ("XDG_RUNTIME_DIR", "/run/user/1000".into()),
        ]);
        let user_dir =
            |name: &str| (name == "documents").then(|| PathBuf::from("/home/ann/Dokumente"));
        let text = "[Context]\nshared=network;\nfilesystems=home;xdg-documents/Notes:ro;\
                    xdg-download;xdg-config/app:create;xdg-run/pipewire-0;~/Projects:rw;\
                    /media/usb;!xdg-download;../escape;/a/../b;unknown-thing;\n";
        let Confinement::Flatpak(sandbox) = Confinement::detect(
            &env,
            &FlatpakInfo::Present(text.to_owned()),
            None,
            &user_dir,
        ) else {
            panic!("a Flatpak");
        };
        let folders = [
            "/home/ann",
            "/home/ann/Dokumente/Notes",
            "/home/ann/.config/app",
            "/run/user/1000/pipewire-0",
            "/home/ann/Projects",
            "/media/usb",
        ]
        .map(PathBuf::from);
        assert_eq!(
            sandbox.filesystems, folders,
            "the mode is dropped, a `!` entry takes back its folder, an unknown or climbing one is ignored"
        );
        let host = FlatpakInfo::Present("[Context]\nfilesystems=host;\n".to_owned());
        let Confinement::Flatpak(sandbox) = Confinement::detect(&env, &host, None, &no_user_dirs)
        else {
            panic!("a Flatpak");
        };
        assert_eq!(
            sandbox.filesystems,
            [std::path::Path::new("/")],
            "host is the whole file system"
        );
    }

    #[test]
    fn a_temporary_folder_that_would_reach_everything_is_ignored() {
        let container = absolute("Containers/com.example.app/Data");
        let cases: [(&str, Option<OsString>, Option<PathBuf>); 5] = [
            (
                "inside the container",
                Some(container.join("tmp").into()),
                Some(container.join("tmp")),
            ),
            ("relative", Some("tmp".into()), None),
            (
                "outside the container",
                Some(absolute("elsewhere").into()),
                None,
            ),
            (
                "climbing out",
                Some(container.join("..").join("x").into()),
                None,
            ),
            ("unset", None, None),
        ];
        for (name, tmpdir, usable) in cases {
            let env = environment(tmpdir.into_iter().map(|tmp| ("TMPDIR", tmp)).collect());
            assert_eq!(usable_temporary(&env, Some(&container)), usable, "{name}");
        }
        let root = std::env::temp_dir()
            .ancestors()
            .last()
            .expect("a path has a root")
            .to_path_buf();
        let env = environment(vec![("TMPDIR", root.into())]);
        assert_eq!(
            usable_temporary(&env, None),
            None,
            "a root reaches everything"
        );
    }

    #[test]
    fn an_app_sandbox_keeps_to_its_container_even_without_home() {
        let user = absolute("user");
        let env = environment(vec![("APP_SANDBOX_CONTAINER_ID", "com.example.app".into())]);
        let Confinement::AppSandbox(sandbox) =
            Confinement::detect(&env, &FlatpakInfo::Absent, Some(&user), &no_user_dirs)
        else {
            panic!("the App Sandbox");
        };
        assert_eq!(
            sandbox.home,
            user.join("Library/Containers/com.example.app/Data")
        );
        let climbing = environment(vec![("APP_SANDBOX_CONTAINER_ID", "../x".into())]);
        assert_eq!(
            Confinement::detect(&climbing, &FlatpakInfo::Absent, Some(&user), &no_user_dirs),
            Confinement::Unconfined
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_description_is_read_within_its_bound() {
        let info = std::env::temp_dir().join(format!("flui-flatpak-info-{}", std::process::id()));
        let _ = std::fs::remove_file(&info);
        assert_eq!(read_info_at(&info), FlatpakInfo::Absent);
        std::fs::write(&info, "[Application]\nname=a.b\n").expect("written");
        assert_eq!(
            read_info_at(&info),
            FlatpakInfo::Present("[Application]\nname=a.b\n".to_owned())
        );
        std::fs::write(&info, vec![b'x'; 64 * 1024 + 1]).expect("written");
        assert_eq!(
            read_info_at(&info),
            FlatpakInfo::Present(String::new()),
            "one past the bound is refused by its size, a Flatpak with its own folders alone"
        );
        let _ = std::fs::remove_file(&info);
    }
}
