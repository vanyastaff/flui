//! The sandbox a process runs in, read once for every crate that needs it:
//! where a sandboxed process keeps its data (`flui-native`'s data folders) and
//! what it may open without a grant (`flui-system`'s grants).
//!
//! - A **Flatpak** is a Linux process that has `/.flatpak-info`, the file
//!   `bubblewrap` mounts into every Flatpak sandbox: `FLATPAK_ID` alone is not
//!   one, since a host process started from a Flatpak's terminal (an IDE's)
//!   inherits it. The file's `[Context] filesystems=` names what the
//!   application's manifest exposes beyond its own folders, as Flatpak wrote
//!   it: flattened, with no entry that takes one back.
//! - The **macOS App Sandbox** is a process the system started with
//!   `APP_SANDBOX_CONTAINER_ID`, whose container
//!   (`~/Library/Containers/<id>/Data`) the system makes its `HOME`.
//!
//! [`read_flatpak_info`] and [`is_flatpak`] touch the file system and run off
//! the owner thread; [`Confinement::detect`] is a pure function of what it is
//! given, and [`Confinement::of_process`] adds the process's environment.

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

/// What [`Confinement::detect`] reads of the process's environment.
pub(crate) type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Where Flatpak mounts its description of the sandbox.
const FLATPAK_INFO: &str = "/.flatpak-info";

/// The longest `/.flatpak-info` read, in bytes: it names an application, its
/// runtime and its permissions; one larger is not that file.
const MAX_FLATPAK_INFO_LEN: u64 = 64 * 1024;

/// What the process found at `/.flatpak-info`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlatpakInfo {
    /// No such file, or a system with no Flatpak: the process is no
    /// Flatpak, whatever its environment says.
    Absent,
    /// The file is there, and holds this text: empty when it is no file or
    /// larger than a description is, which leaves the sandbox its own
    /// folders alone.
    Present(String),
}

/// Read `/.flatpak-info` on Linux, its size checked before a byte is read
/// and the read bounded again; every other system has no Flatpak, so a file
/// at that name (the root of a Windows drive) means nothing there and is not
/// read. Blocks on the file system: off the owner thread only.
///
/// # Errors
///
/// The file system's error when the file could not be opened for a reason
/// other than its absence, or not read: the process may be a Flatpak whose
/// description is out of reach this time, so the answer is no answer, and a
/// caller that keeps what it detects reads again later.
pub fn read_flatpak_info() -> std::io::Result<FlatpakInfo> {
    read_description(Path::new(FLATPAK_INFO), cfg!(target_os = "linux"))
}

/// Whether the process is a Flatpak as [`Confinement::detect`] judges one: a
/// Linux process with `/.flatpak-info`, one that could not be opened
/// included, since its absence is the only sign of none. Blocks on the file
/// system, without reading the file.
#[must_use]
pub fn is_flatpak() -> bool {
    has_description(Path::new(FLATPAK_INFO), cfg!(target_os = "linux"))
}

/// Whether `path` is there, on a system that has Flatpak (`linux`): found,
/// or out of reach for a reason other than its absence.
fn has_description(path: &Path, linux: bool) -> bool {
    linux
        && !matches!(
            std::fs::metadata(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
}

/// Whether `metadata` is a description's: a file within the bound.
fn is_description(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file() && metadata.len() <= MAX_FLATPAK_INFO_LEN
}

/// The description at `path`, on a system that has Flatpak (`linux`).
///
/// It is judged a file within the bound before it is opened, so a FIFO or a
/// device at the name is never opened; on Unix it is opened without
/// blocking and judged again from the open file, so one swapped in between
/// the two cannot block the reader either.
fn read_description(path: &Path, linux: bool) -> std::io::Result<FlatpakInfo> {
    if !linux {
        return Ok(FlatpakInfo::Absent);
    }
    match std::fs::metadata(path) {
        Ok(metadata) if !is_description(&metadata) => {
            return Ok(FlatpakInfo::Present(String::new()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FlatpakInfo::Absent);
        }
        Err(error) => return Err(error),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FlatpakInfo::Absent);
        }
        Err(error) => return Err(error),
    };
    if !is_description(&file.metadata()?) {
        return Ok(FlatpakInfo::Present(String::new()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FLATPAK_INFO_LEN + 1)
        .read_to_end(&mut bytes)?;
    let fits = u64::try_from(bytes.len()).is_ok_and(|len| len <= MAX_FLATPAK_INFO_LEN);
    Ok(FlatpakInfo::Present(if fits {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::new()
    }))
}

/// A user folder a Flatpak manifest names as `xdg-<name>`, which the user's
/// `user-dirs.dirs` places.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UserFolder {
    /// `xdg-desktop`.
    Desktop,
    /// `xdg-documents`.
    Documents,
    /// `xdg-download`.
    Download,
    /// `xdg-music`.
    Music,
    /// `xdg-pictures`.
    Pictures,
    /// `xdg-public-share`.
    PublicShare,
    /// `xdg-videos`.
    Videos,
    /// `xdg-templates`.
    Templates,
}

impl UserFolder {
    /// The folder a manifest's `xdg-<name>` names.
    fn of_entry(name: &str) -> Option<Self> {
        Some(match name {
            "desktop" => Self::Desktop,
            "documents" => Self::Documents,
            "download" => Self::Download,
            "music" => Self::Music,
            "pictures" => Self::Pictures,
            "public-share" => Self::PublicShare,
            "videos" => Self::Videos,
            "templates" => Self::Templates,
            _ => return None,
        })
    }
}

/// A Flatpak sandbox, as its description and environment say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatpakSandbox {
    /// The application's own folder, `~/.var/app/<id>`.
    own: Option<PathBuf>,
    /// Its data folder: `XDG_DATA_HOME`, else `~/.var/app/<id>/data`.
    data_home: Option<PathBuf>,
    /// The document portal's folders: Flatpak's mount inside the sandbox and
    /// `$XDG_RUNTIME_DIR/doc`.
    documents: Vec<PathBuf>,
    /// What the manifest's `filesystems` expose, each entry resolved.
    filesystems: Vec<PathBuf>,
}

/// The folder every Flatpak keeps private: its own `/tmp`. Flatpak unsets
/// `TMPDIR`, so the variable is never read there.
const FLATPAK_TEMPORARY: &str = "/tmp";

impl FlatpakSandbox {
    /// Its data folder: `XDG_DATA_HOME`, else `~/.var/app/<id>/data`; `None`
    /// when neither names one.
    #[must_use]
    pub fn data_home(&self) -> Option<&Path> {
        self.data_home.as_deref()
    }

    /// Every folder the sandbox reaches: its own, its private `/tmp`, the
    /// document portal's folders and what its manifest exposes. A path its
    /// manifest hides with `--nofilesystem` under an exposed folder is a
    /// tmpfs the description does not show: reached here, and refused by
    /// the sandbox when it is opened.
    pub fn reached(&self) -> impl Iterator<Item = &Path> {
        self.own
            .as_deref()
            .into_iter()
            .chain([Path::new(FLATPAK_TEMPORARY)])
            .chain(self.documents.iter().map(PathBuf::as_path))
            .chain(self.filesystems.iter().map(PathBuf::as_path))
    }
}

/// The macOS App Sandbox's container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSandbox {
    /// The container, `~/Library/Containers/<id>/Data`.
    home: PathBuf,
    /// The container's temporary folder: a `TMPDIR` inside the container.
    temporary: Option<PathBuf>,
}

impl AppSandbox {
    /// The container, `~/Library/Containers/<id>/Data`: `HOME` when it is
    /// that path for the container's id, else rebuilt under the user's home.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The container's temporary folder: a `TMPDIR` inside the container.
    #[must_use]
    pub fn temporary(&self) -> Option<&Path> {
        self.temporary.as_deref()
    }
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

/// `TMPDIR`, when it names a temporary folder inside `container`: absolute,
/// plain and under it. Any other is not the sandbox's, and is not reached.
#[must_use]
pub fn usable_temporary(
    env: &dyn Fn(&str) -> Option<OsString>,
    container: &Path,
) -> Option<PathBuf> {
    folder(env, "TMPDIR").filter(|path| path.starts_with(container) && path != container)
}

impl Confinement {
    /// The process's sandbox: its environment, `flatpak_info` as
    /// [`read_flatpak_info`] read it, the user's home as `user_home` reads it
    /// from the system's user database (never from `HOME`, which a container
    /// rebuilt under it would follow wherever the environment points), and
    /// `user_dir` as [`detect`](Self::detect) takes it.
    pub fn of_process(
        flatpak_info: &FlatpakInfo,
        user_home: &dyn Fn() -> Option<PathBuf>,
        user_dir: &dyn Fn(UserFolder) -> Option<PathBuf>,
    ) -> Self {
        Self::detect(
            &|name| std::env::var_os(name),
            flatpak_info,
            user_home,
            user_dir,
        )
    }

    /// The sandbox `env` and `flatpak_info` describe. `user_home` reads the
    /// user's home from the system's user database; it is called only when
    /// a sandbox needs it and the environment does not name it (a Flatpak
    /// with no `HOME`, an App Sandbox whose `HOME` is not its container), so
    /// an unconfined process never asks the database. `user_dir` places a
    /// user folder as the user's `user-dirs.dirs` does, for a manifest's
    /// `xdg-*` entries, and answers none for a folder it does not configure.
    pub fn detect(
        env: &dyn Fn(&str) -> Option<OsString>,
        flatpak_info: &FlatpakInfo,
        user_home: &dyn Fn() -> Option<PathBuf>,
        user_dir: &dyn Fn(UserFolder) -> Option<PathBuf>,
    ) -> Self {
        if let FlatpakInfo::Present(text) = flatpak_info {
            return Self::Flatpak(flatpak(env, text, user_home, user_dir));
        }
        if let Some(id) = env("APP_SANDBOX_CONTAINER_ID").filter(is_one_name) {
            let container = Path::new("Library")
                .join("Containers")
                .join(&id)
                .join("Data");
            let home = folder(env, "HOME")
                .filter(|home| home.ends_with(&container))
                .or_else(|| user_home().map(|home| home.join(&container)));
            if let Some(home) = home {
                let temporary = usable_temporary(env, &home);
                return Self::AppSandbox(AppSandbox { home, temporary });
            }
        }
        Self::Unconfined
    }
}

/// The value of `key` in the `[group]` of a key file's `text`, its last
/// line winning.
fn key_file_value<'a>(text: &'a str, group: &str, key: &str) -> Option<&'a str> {
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
            value = Some(rest.trim_start());
        }
    }
    value
}

/// A key file's list value split at each `;` the writer did not escape,
/// each item unescaped as GLib's key file writes it (`\;`, `\\`, `\s`,
/// `\t`, `\n`, `\r`).
fn key_file_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(';') => item.push(';'),
                Some('s') => item.push(' '),
                Some('t') => item.push('\t'),
                Some('n') => item.push('\n'),
                Some('r') => item.push('\r'),
                Some('\\') | None => item.push('\\'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
            },
            ';' => items.push(std::mem::take(&mut item)),
            other => item.push(other),
        }
    }
    if !item.is_empty() {
        items.push(item);
    }
    items
}

fn flatpak(
    env: Environment<'_>,
    text: &str,
    user_home: &dyn Fn() -> Option<PathBuf>,
    user_dir: &dyn Fn(UserFolder) -> Option<PathBuf>,
) -> FlatpakSandbox {
    let id = key_file_value(text, "Application", "name")
        .map(OsString::from)
        .or_else(|| env("FLATPAK_ID"))
        .filter(is_one_name);
    let home = folder(env, "HOME").or_else(user_home);
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
    let filesystems = key_file_value(text, "Context", "filesystems")
        .map_or_else(Vec::new, |list| context.filesystems(list));
    FlatpakSandbox {
        own,
        data_home,
        documents,
        filesystems,
    }
}

/// What a manifest's `filesystems` entries resolve against.
struct Context<'a> {
    home: Option<&'a Path>,
    runtime: Option<&'a Path>,
    user_dir: &'a dyn Fn(UserFolder) -> Option<PathBuf>,
}

/// The folders under the user's home a manifest names as `xdg-<name>`.
const HOME_FOLDERS: [(&str, &str); 4] = [
    ("config", ".config"),
    ("cache", ".cache"),
    ("data", ".local/share"),
    ("state", ".local/state"),
];

impl Context<'_> {
    /// The folders `list` exposes, in order; an entry this library does not
    /// read, or one that climbs, is left out.
    fn filesystems(&self, list: &str) -> Vec<PathBuf> {
        key_file_list(list)
            .iter()
            .map(|entry| {
                // The access mode does not change what is reached.
                entry
                    .strip_suffix(":ro")
                    .or_else(|| entry.strip_suffix(":rw"))
                    .or_else(|| entry.strip_suffix(":create"))
                    .unwrap_or(entry)
            })
            .filter_map(|entry| self.resolve(entry))
            .filter(|folder| is_plain(folder))
            .collect()
    }

    /// The folder one entry names; `None` for one this library does not read,
    /// or a user folder the user's `user-dirs.dirs` does not configure, which
    /// Flatpak ignores too.
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
                (self.user_dir)(UserFolder::of_entry(name)?).map(under)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{
        Confinement, FlatpakInfo, UserFolder, has_description, key_file_list, read_description,
    };

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

    fn no_user_dirs(_: UserFolder) -> Option<PathBuf> {
        None
    }

    fn no_home() -> Option<PathBuf> {
        None
    }

    fn app_sandbox(confinement: Confinement) -> super::AppSandbox {
        let Confinement::AppSandbox(sandbox) = confinement else {
            panic!("the App Sandbox, not {confinement:?}");
        };
        sandbox
    }

    /// A case: its name, the environment, and how many times the user
    /// database is asked.
    type Case = (&'static str, Vec<(&'static str, OsString)>, usize);

    /// A row: its name, `HOME`, `TMPDIR`, and the temporary folder reached.
    type Row = (
        &'static str,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
    );

    #[test]
    fn a_flatpak_is_a_process_with_its_description_whatever_its_environment_says() {
        let inherited = environment(vec![("FLATPAK_ID", "org.example.Ide".into())]);
        assert_eq!(
            Confinement::detect(&inherited, &FlatpakInfo::Absent, &no_home, &no_user_dirs),
            Confinement::Unconfined,
            "a host process started from a Flatpak's terminal inherits FLATPAK_ID"
        );
        let home = absolute("home");
        let env = environment(vec![
            ("FLATPAK_ID", "org.example.Env".into()),
            ("HOME", home.clone().into()),
        ]);
        let info = FlatpakInfo::Present("[Application]\nname=org.example.App\n".to_owned());
        let Confinement::Flatpak(sandbox) =
            Confinement::detect(&env, &info, &no_home, &no_user_dirs)
        else {
            panic!("a process with the description is a Flatpak");
        };
        assert_eq!(
            sandbox.data_home(),
            Some(home.join(".var/app/org.example.App/data").as_path()),
            "the description names it"
        );
    }

    #[test]
    fn the_description_is_read_on_linux_alone_and_within_its_bound() {
        let info = std::env::temp_dir().join(format!("flui-flatpak-info-{}", std::process::id()));
        let _ = std::fs::remove_file(&info);
        assert_eq!(
            read_description(&info, true).ok(),
            Some(FlatpakInfo::Absent)
        );
        assert!(!has_description(&info, true));
        std::fs::write(&info, "[Application]\nname=a.b\n").expect("written");
        assert_eq!(
            read_description(&info, true).ok(),
            Some(FlatpakInfo::Present("[Application]\nname=a.b\n".to_owned()))
        );
        assert!(has_description(&info, true));
        assert_eq!(
            read_description(&info, false).ok(),
            Some(FlatpakInfo::Absent),
            "a system with no Flatpak reads no description, a file at its name included"
        );
        assert!(!has_description(&info, false));
        std::fs::write(&info, vec![b'x'; 64 * 1024 + 1]).expect("written");
        assert_eq!(
            read_description(&info, true).ok(),
            Some(FlatpakInfo::Present(String::new())),
            "one past the bound is refused by its size, a Flatpak with its own folders alone"
        );
        let _ = std::fs::remove_file(&info);
    }

    #[test]
    fn a_description_out_of_reach_is_no_answer_and_still_a_sign_of_flatpak() {
        // A name no file system opens: an error other than the file's absence.
        let unreachable = std::path::Path::new("flatpak\0info");
        assert!(
            read_description(unreachable, true).is_err(),
            "neither absent nor an empty description: the caller reads again later"
        );
        assert!(
            has_description(unreachable, true),
            "only the file's absence says the process is no Flatpak"
        );
    }

    /// A FIFO at the description's name would block an open until a writer
    /// came: it is judged by its metadata and never opened.
    #[cfg(unix)]
    #[test]
    fn a_description_that_is_no_file_is_never_opened() {
        let fifo = std::env::temp_dir().join(format!("flui-flatpak-fifo-{}", std::process::id()));
        let _ = std::fs::remove_file(&fifo);
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "a FIFO is made");
        let (sent, read) = std::sync::mpsc::channel();
        let reading = fifo.clone();
        std::thread::spawn(move || {
            let _ = sent.send(read_description(&reading, true).ok());
        });
        let answer = read.recv_timeout(std::time::Duration::from_secs(5));
        if answer.is_err() {
            // Unblock the reader so the test process can end.
            let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
        }
        let _ = std::fs::remove_file(&fifo);
        assert_eq!(
            answer.expect("the read does not block"),
            Some(FlatpakInfo::Present(String::new())),
            "no file: a Flatpak with its own folders alone"
        );
    }

    #[test]
    fn the_user_database_is_asked_only_when_a_sandbox_needs_it_and_never_home() {
        let user = absolute("user");
        let container = user.join("Library/Containers/com.example.app/Data");
        let asked = std::cell::Cell::new(0);
        let lookup = || {
            asked.set(asked.get() + 1);
            Some(user.clone())
        };
        let cases: [Case; 3] = [
            (
                "unconfined",
                vec![("HOME", absolute("me").into_os_string())],
                0,
            ),
            (
                "HOME the container",
                vec![
                    ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
                    ("HOME", container.clone().into_os_string()),
                ],
                0,
            ),
            (
                "HOME elsewhere",
                vec![
                    ("APP_SANDBOX_CONTAINER_ID", "com.example.app".into()),
                    ("HOME", absolute("elsewhere").into_os_string()),
                ],
                1,
            ),
        ];
        for (name, vars, asks) in cases {
            asked.set(0);
            let found = Confinement::detect(
                &environment(vars),
                &FlatpakInfo::Absent,
                &lookup,
                &no_user_dirs,
            );
            assert_eq!(
                asked.get(),
                asks,
                "{name}: times the user database is asked"
            );
            match found {
                Confinement::AppSandbox(sandbox) => assert_eq!(
                    sandbox.home(),
                    container.as_path(),
                    "{name}: the container under the database's home, never under HOME"
                ),
                Confinement::Unconfined => assert_eq!(name, "unconfined"),
                Confinement::Flatpak(_) => panic!("{name}: no description, no Flatpak"),
            }
        }
    }

    #[test]
    fn a_key_file_list_is_split_where_its_writer_did_not_escape() {
        assert_eq!(
            key_file_list(r"home;/media/a\;b;/srv/c\\d;~/with\sspace;"),
            ["home", "/media/a;b", r"/srv/c\d", "~/with space"]
        );
        assert_eq!(key_file_list(""), Vec::<String>::new());
    }

    /// A manifest's entries are Linux paths: the table runs where they are
    /// absolute.
    #[cfg(unix)]
    #[test]
    fn a_manifests_filesystems_name_what_the_flatpak_reaches() {
        let env = environment(vec![
            ("HOME", "/home/ann".into()),
            ("XDG_RUNTIME_DIR", "/run/user/1000".into()),
            ("TMPDIR", "/home/ann/tmp".into()),
        ]);
        let user_dir = |folder: UserFolder| {
            (folder == UserFolder::Documents).then(|| PathBuf::from("/home/ann/Dokumente"))
        };
        let text = "[Context]\nshared=network;\nfilesystems=home;xdg-documents/Notes:ro;\
                    xdg-download;xdg-config/app:create;xdg-run/pipewire-0;~/Projects:rw;\
                    /media/usb;/media/a\\;b;host-etc;../escape;/a/../b;unknown-thing;\n";
        let Confinement::Flatpak(sandbox) = Confinement::detect(
            &env,
            &FlatpakInfo::Present(text.to_owned()),
            &no_home,
            &user_dir,
        ) else {
            panic!("a Flatpak");
        };
        let exposed = [
            "/home/ann",
            "/home/ann/Dokumente/Notes",
            "/home/ann/.config/app",
            "/run/user/1000/pipewire-0",
            "/home/ann/Projects",
            "/media/usb",
            "/media/a;b",
            "/run/host/etc",
        ]
        .map(PathBuf::from);
        assert_eq!(
            sandbox.filesystems, exposed,
            "the mode dropped, an escaped `;` kept in its entry, an unconfigured user folder \
             (xdg-download) and an unknown or climbing entry left out"
        );
        assert!(
            !sandbox
                .reached()
                .any(|folder| folder == std::path::Path::new("/home/ann/tmp")),
            "Flatpak unsets TMPDIR: the variable is never read there"
        );
        let host = FlatpakInfo::Present("[Context]\nfilesystems=host;\n".to_owned());
        let Confinement::Flatpak(sandbox) =
            Confinement::detect(&env, &host, &no_home, &no_user_dirs)
        else {
            panic!("a Flatpak");
        };
        assert_eq!(
            sandbox.filesystems,
            [std::path::Path::new("/")],
            "host is the whole file system"
        );
    }

    /// Each row: the App Sandbox's `HOME` and `TMPDIR`, the container the
    /// sandbox keeps to and the temporary folder it reaches.
    #[test]
    fn an_app_sandbox_keeps_to_its_container_whatever_its_environment_names() {
        let user = absolute("user");
        let container = user.join("Library/Containers/com.example.app/Data");
        let root = std::env::temp_dir()
            .ancestors()
            .last()
            .expect("a path has a root")
            .to_path_buf();
        let rows: [Row; 6] = [
            (
                "HOME the container, TMPDIR inside it",
                Some(container.clone()),
                Some(container.join("tmp")),
                Some(container.join("tmp")),
            ),
            ("no HOME", None, None, None),
            ("HOME the disk's root", Some(root.clone()), None, None),
            (
                "HOME another application's container",
                Some(user.join("Library/Containers/com.other.app/Data")),
                None,
                None,
            ),
            ("TMPDIR a root", Some(container.clone()), Some(root), None),
            (
                "TMPDIR outside the container",
                Some(container.clone()),
                Some(absolute("elsewhere")),
                None,
            ),
        ];
        for (name, home, tmpdir, temporary) in rows {
            let mut vars = vec![(
                "APP_SANDBOX_CONTAINER_ID",
                OsString::from("com.example.app"),
            )];
            vars.extend(home.map(|home| ("HOME", home.into_os_string())));
            vars.extend(tmpdir.map(|tmp| ("TMPDIR", tmp.into_os_string())));
            let sandbox = app_sandbox(Confinement::detect(
                &environment(vars),
                &FlatpakInfo::Absent,
                &|| Some(user.clone()),
                &no_user_dirs,
            ));
            assert_eq!(sandbox.home(), container.as_path(), "{name}");
            assert_eq!(sandbox.temporary(), temporary.as_deref(), "{name}");
        }
        let climbing = environment(vec![("APP_SANDBOX_CONTAINER_ID", "../x".into())]);
        assert_eq!(
            Confinement::detect(
                &climbing,
                &FlatpakInfo::Absent,
                &|| Some(user.clone()),
                &no_user_dirs
            ),
            Confinement::Unconfined
        );
    }
}
