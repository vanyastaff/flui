//! The user's folders as the XDG user directories name them
//! (`$XDG_CONFIG_HOME/user-dirs.dirs`, written by `xdg-user-dirs-update`).
//!
//! No portal names them: the file is read on the portal's thread, never on
//! the owner's. Inside Flatpak the application's `XDG_CONFIG_HOME` is its
//! own, which a sandbox without access to the host's file leaves to the
//! defaults; the parity source records that run as owed.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::Portal;
use crate::user_dirs::{UserDir, UserDirBackend, UserDirError, UserDirResult};

/// The largest `user-dirs.dirs` read, in bytes: the file names nine
/// folders; one larger is not that file.
const MAX_FILE_LEN: u64 = 64 * 1024;

/// [`UserDirBackend`] over the XDG user directories.
pub(crate) struct XdgUserDirs {
    portal: Rc<Portal>,
}

impl XdgUserDirs {
    pub(crate) fn new(portal: Rc<Portal>) -> Self {
        Self { portal }
    }
}

impl UserDirBackend for XdgUserDirs {
    fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
        if self.portal.start().is_err() {
            let _ = answer.resolve(Err(Unsupported::new(UnsupportedReason::Unavailable).into()));
            return;
        }
        self.portal.spawn(move |_| async move {
            let _ = answer.resolve(locate(dir, &Environment::of_process()));
        });
    }
}

/// What the lookup reads from the process: its home and its configuration
/// folder.
struct Environment {
    home: Option<PathBuf>,
    config_home: Option<PathBuf>,
}

impl Environment {
    fn of_process() -> Self {
        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        }
    }
}

/// The folder of `dir` in `environment`.
fn locate(dir: UserDir, environment: &Environment) -> UserDirResult {
    let home = environment
        .home
        .clone()
        .filter(|home| home.is_absolute())
        .ok_or(UserDirError::Unsupported(Unsupported::new(
            UnsupportedReason::Unavailable,
        )))?;
    let Some(key) = key(dir) else {
        return Ok(Some(home));
    };
    // A relative `XDG_CONFIG_HOME` is ignored, as the base directory
    // specification says.
    let config = environment
        .config_home
        .clone()
        .filter(|config| config.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let named = match read_bounded(&config.join("user-dirs.dirs")) {
        Ok(Some(text)) => named(&text, key, &home),
        Ok(None) => None,
        Err(failure) => return Err(failure.into()),
    };
    Ok(match named {
        // Pointed at the home folder: the user switched it off.
        Some(folder) if folder == home => None,
        Some(folder) => Some(folder),
        // Unnamed: the desktop is `~/Desktop`, as GLib and Qt answer; the
        // rest have no folder.
        None if dir == UserDir::Desktop => Some(home.join("Desktop")),
        None => None,
    })
}

/// The file's key for `dir`; `None` for the home folder, which no key names.
fn key(dir: UserDir) -> Option<&'static str> {
    Some(match dir {
        UserDir::Home => return None,
        UserDir::Desktop => "XDG_DESKTOP_DIR",
        UserDir::Documents => "XDG_DOCUMENTS_DIR",
        UserDir::Downloads => "XDG_DOWNLOAD_DIR",
        UserDir::Music => "XDG_MUSIC_DIR",
        UserDir::Pictures => "XDG_PICTURES_DIR",
        UserDir::Videos => "XDG_VIDEOS_DIR",
        UserDir::Templates => "XDG_TEMPLATES_DIR",
        UserDir::PublicShare => "XDG_PUBLICSHARE_DIR",
    })
}

/// The text of `path`, `None` when it does not exist; at most
/// [`MAX_FILE_LEN`] bytes, a larger file failing as malformed.
fn read_bounded(path: &Path) -> Result<Option<String>, SystemFailure> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SystemFailure::Os(error.raw_os_error().unwrap_or(libc::EIO))),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_LEN + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SystemFailure::Os(error.raw_os_error().unwrap_or(libc::EIO)))?;
    if u64::try_from(bytes.len()).is_ok_and(|len| len > MAX_FILE_LEN) {
        return Err(SystemFailure::MalformedAnswer);
    }
    // A folder is a path, and the file's paths are UTF-8 as its writer
    // writes them; one that is not names no folder.
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// The folder `text` names under `key`: the last line `KEY="value"`, its
/// value `$HOME`, `$HOME/…` or an absolute path, with `\` escaping the next
/// character, as `xdg-user-dirs` writes and GLib reads it. A value of any
/// other form names nothing.
fn named(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    text.lines().rev().find_map(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        let quoted = rest.strip_prefix('"')?;
        let mut value = String::new();
        let mut characters = quoted.chars();
        loop {
            match characters.next()? {
                '"' => break,
                '\\' => value.push(characters.next()?),
                character => value.push(character),
            }
        }
        folder(&value, home)
    })
}

/// The folder a value names: relative to `home` after `$HOME`, or absolute.
fn folder(value: &str, home: &Path) -> Option<PathBuf> {
    if let Some(relative) = value.strip_prefix("$HOME") {
        return match relative.strip_prefix('/') {
            Some(relative) => Some(home.join(relative)),
            None if relative.is_empty() => Some(home.to_path_buf()),
            None => None,
        };
    }
    value.starts_with('/').then(|| PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Environment, locate, named};
    use crate::user_dirs::UserDir;

    #[test]
    fn a_line_names_its_folder_under_home_or_absolute() {
        let home = Path::new("/home/ann");
        let text = "# written by xdg-user-dirs-update\n\
                    XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n\
                    XDG_MUSIC_DIR=\"/mnt/music\"\n\
                    XDG_VIDEOS_DIR=\"$HOME/My \\\"Videos\\\"\"\n\
                    XDG_DOWNLOAD_DIR=\"$HOME/Incoming\"\n\
                    XDG_TEMPLATES_DIR=\"relative\"\n\
                    XDG_PICTURES_DIR=\"$HOMEPICS\"\n";
        let cases: [(&str, Option<&str>); 5] = [
            // The last line of a key wins.
            ("XDG_DOWNLOAD_DIR", Some("/home/ann/Incoming")),
            ("XDG_MUSIC_DIR", Some("/mnt/music")),
            ("XDG_VIDEOS_DIR", Some("/home/ann/My \"Videos\"")),
            ("XDG_TEMPLATES_DIR", None),
            ("XDG_PICTURES_DIR", None),
        ];
        for (key, folder) in cases {
            assert_eq!(named(text, key, home), folder.map(PathBuf::from), "{key}");
        }
    }

    #[test]
    fn a_folder_pointed_at_home_is_switched_off_and_an_unnamed_desktop_is_the_default() {
        let directory = std::env::temp_dir().join(format!("flui-user-dirs-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch folder");
        std::fs::write(
            directory.join("user-dirs.dirs"),
            "XDG_DOCUMENTS_DIR=\"$HOME/\"\nXDG_MUSIC_DIR=\"$HOME/Music\"\n",
        )
        .expect("the file is written");
        let environment = Environment {
            home: Some(PathBuf::from("/home/ann")),
            config_home: Some(directory.clone()),
        };
        assert_eq!(locate(UserDir::Documents, &environment), Ok(None));
        assert_eq!(
            locate(UserDir::Music, &environment),
            Ok(Some(PathBuf::from("/home/ann/Music")))
        );
        assert_eq!(
            locate(UserDir::Desktop, &environment),
            Ok(Some(PathBuf::from("/home/ann/Desktop")))
        );
        assert_eq!(locate(UserDir::Videos, &environment), Ok(None));
        assert_eq!(
            locate(UserDir::Home, &environment),
            Ok(Some(PathBuf::from("/home/ann")))
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
