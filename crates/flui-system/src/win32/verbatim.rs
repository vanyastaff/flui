//! A verbatim path (`\\?\…`) in the form the shell reads.
//!
//! `std::fs::canonicalize` and `GetFinalPathNameByHandleW` answer verbatim
//! paths. The file API takes them; the shell's parser
//! (`SHCreateItemFromParsingName`, `ShellExecuteExW`) does not, and answers
//! as if the file were not there.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf, Prefix};

/// `path` without its verbatim prefix, when the plain form names the same
/// file: `\\?\C:\docs` as `C:\docs`, `\\?\UNC\server\share\docs` as
/// `\\server\share\docs`. Any other path is returned as it is.
///
/// A verbatim path is taken as written, so it can name what no plain path
/// does: a name that ends in a dot or a space, a device's name (`NUL`,
/// `COM1`), `.` or `..` as a name, a `/` inside one. Such a path keeps its
/// prefix, as does one whose prefix has no plain form (`\\?\Volume{…}`,
/// `\\?\GLOBALROOT`): what it names is then its caller's to refuse, never
/// another file's to stand in for.
pub(super) fn without_verbatim(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_path_buf();
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", char::from(letter))),
        Prefix::VerbatimUNC(server, share) => {
            let mut root = OsString::from(r"\\");
            root.push(server);
            root.push(r"\");
            root.push(share);
            PathBuf::from(root)
        }
        Prefix::Verbatim(_) | Prefix::DeviceNS(_) | Prefix::UNC(_, _) | Prefix::Disk(_) => {
            return path.to_path_buf();
        }
    };
    for component in components {
        match component {
            Component::RootDir => plain.push(r"\"),
            Component::Normal(name) if reads_the_same_plain(name) => plain.push(name),
            Component::Normal(_)
            | Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_) => return path.to_path_buf(),
        }
    }
    plain
}

/// The device names Windows reads in any folder, whatever extension follows.
const DEVICE_NAMES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];

/// Whether a plain path reads `name` as the verbatim path wrote it: no
/// character a plain path gives a meaning or refuses, no dot or space at its
/// end (which Windows drops), and not a device's name.
fn reads_the_same_plain(name: &OsStr) -> bool {
    let units = name.encode_wide().collect::<Vec<u16>>();
    let special = |unit: &u16| {
        *unit < 0x20
            || br#"<>:"/\|?*"#
                .iter()
                .any(|character| u16::from(*character) == *unit)
    };
    let ends_plainly = units
        .last()
        .is_some_and(|last| *last != u16::from(b'.') && *last != u16::from(b' '));
    ends_plainly && !units.iter().any(special) && !is_device_name(&units)
}

/// Whether `name` is a device's: one of [`DEVICE_NAMES`], or `COM` or `LPT`
/// and a digit, ignoring ASCII case, alone or before an extension.
fn is_device_name(name: &[u16]) -> bool {
    let stem = name
        .split(|unit| *unit == u16::from(b'.'))
        .next()
        .unwrap_or_default();
    let stem = String::from_utf16_lossy(stem);
    let stem = stem.trim_end_matches(' ');
    let numbered = |port: &str| {
        stem.len() == port.len() + 1
            && stem
                .get(..port.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(port))
            && stem.ends_with(|digit: char| digit.is_ascii_digit())
    };
    DEVICE_NAMES
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
        || numbered("COM")
        || numbered("LPT")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::without_verbatim;

    #[test]
    fn a_verbatim_path_loses_its_prefix_only_where_the_plain_form_names_the_same_file() {
        let cases = [
            (r"\\?\C:\docs\a.pdf", r"C:\docs\a.pdf"),
            (r"\\?\c:\", r"c:\"),
            (
                r"\\?\UNC\server\share\docs\a.pdf",
                r"\\server\share\docs\a.pdf",
            ),
            (r"\\?\UNC\server\share", r"\\server\share"),
            // Already plain.
            (r"C:\docs\a.pdf", r"C:\docs\a.pdf"),
            (r"\\server\share\a.pdf", r"\\server\share\a.pdf"),
            // A prefix with no plain form.
            (
                r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\docs",
                r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\docs",
            ),
            (
                r"\\?\GLOBALROOT\Device\Mup\server\share",
                r"\\?\GLOBALROOT\Device\Mup\server\share",
            ),
            (r"\\.\COM1", r"\\.\COM1"),
            // What only a verbatim path names: the plain form would name
            // another file, or a device.
            (r"\\?\C:\docs\notes.", r"\\?\C:\docs\notes."),
            (r"\\?\C:\docs\notes ", r"\\?\C:\docs\notes "),
            (r"\\?\C:\docs\NUL", r"\\?\C:\docs\NUL"),
            (r"\\?\C:\docs\com1.txt", r"\\?\C:\docs\com1.txt"),
            (r"\\?\C:\docs\..\secret", r"\\?\C:\docs\..\secret"),
            (r"\\?\C:\docs\a/b", r"\\?\C:\docs\a/b"),
            (r"\\?\C:\docs\what?", r"\\?\C:\docs\what?"),
            // Names that only look like a device's.
            (r"\\?\C:\docs\console", r"C:\docs\console"),
            (r"\\?\C:\docs\com10", r"C:\docs\com10"),
        ];
        for (verbatim, plain) in cases {
            assert_eq!(
                without_verbatim(Path::new(verbatim)),
                PathBuf::from(plain),
                "{verbatim}"
            );
        }
    }
}
