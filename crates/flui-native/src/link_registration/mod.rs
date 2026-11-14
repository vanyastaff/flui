//! An unpackaged Windows application's registration of the URL schemes it
//! opens (ADR-0307 §4, the activation spec's AC11): one key per scheme under
//! `HKCU\Software\Classes`, whose `shell\open\command` launches the
//! application's executable with the link as its argument (`"%1"`), which
//! the host's launch arguments then open.
//!
//! Each key it writes carries the application's id under
//! [`MARKER`], and that marker is the only claim registration trusts:
//!
//! - a scheme no key names, in the user's classes or the machine's, is
//!   written, into a key the write itself creates: one another process
//!   created since it was read is left to that process ([`Claim::New`]),
//!   and one whose write fails is deleted again; the marker is the first
//!   value written, so a rewrite that stops part way leaves a key the next
//!   run still knows as this application's;
//! - a key marked with this application's id is rewritten only when its
//!   command names another executable (the application moved or was
//!   updated in place), and left as it is otherwise;
//! - a key without the marker, or marked by another application, belongs to
//!   someone else and is never touched: the scheme is refused, with a
//!   warning;
//! - a key marked with this application's id for a scheme it no longer
//!   declares is deleted when it registers the others; an application that
//!   declares no scheme any more registers nothing, so its keys stay until
//!   it is unregistered;
//! - [`unregister`] deletes only the user's keys marked with this
//!   application's id, so an uninstaller that runs the executable with
//!   `--flui-unregister-links` removes what registration wrote and nothing
//!   else.
//!
//! A packaged application (MSIX) registers through its package manifest and
//! never writes here ([`runs_packaged`]).
//!
//! The rules are here, over the [`Classes`] the registry gives on Windows
//! and a map gives in the tests, so they run on every host.

#[cfg(windows)]
mod registry;

/// The value each key registration writes holds: the application id that
/// owns the key.
pub(crate) const MARKER: &str = "FluiApplicationId";

/// The key's subkey whose default value is the command that opens a link.
const COMMAND: &str = "shell\\open\\command";

/// The schemes registration never writes, whatever the application declares,
/// the ones `flui`'s manifest check refuses: a web URL is the application's
/// by its associated domain, a file by its type, and the rest are the
/// system's.
const RESERVED: [&str; 7] = [
    "http",
    "https",
    "file",
    "content",
    "data",
    "javascript",
    "about",
];

/// What a key of the merged classes view holds, as far as registration
/// reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// No key has the name.
    Absent,
    /// The key carries [`MARKER`]: the application id it names, and the
    /// command it opens a link with, if it has one.
    Marked {
        app_id: String,
        command: Option<String>,
    },
    /// The key exists and carries no marker: another application's, or the
    /// system's.
    Unmarked,
}

/// Which key a write may set its values in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// Only a key the write itself creates: none had the name when it was
    /// read, and one another process creates in between is not touched.
    New,
    /// The key this application marked, created again if it went.
    Own,
}

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrote {
    /// Every value is set.
    Written,
    /// Under [`Claim::New`], the key existed by the time it was created:
    /// nothing is set.
    Taken,
}

/// One value a key is written with: under `subkey` of the key (`""` for the
/// key itself), the value `name` (`""` for the default value), a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) subkey: &'static str,
    pub(crate) name: &'static str,
    pub(crate) data: String,
}

/// The classes registration reads and writes: on Windows the registry, where
/// [`find`](Self::find) reads the merged view of the user's and the
/// machine's classes and the writes reach the user's alone.
pub(crate) trait Classes {
    /// What the key `name` holds.
    fn find(&self, name: &str) -> std::io::Result<Found>;
    /// Set `entries` in the user's key `name`, in their order. Under
    /// [`Claim::New`] the key is created, and a key that exists by then is
    /// left as it is ([`Wrote::Taken`]); a key the write created is deleted
    /// again when a later value fails, so a failure never leaves an empty or
    /// unmarked key behind.
    fn write(&mut self, name: &str, entries: &[Entry], claim: Claim) -> std::io::Result<Wrote>;
    /// Delete the user's key `name` with everything under it; a key that is
    /// gone already is not an error.
    fn delete(&mut self, name: &str) -> std::io::Result<()>;
    /// The user's keys whose [`MARKER`] is `app_id`.
    fn marked_by(&self, app_id: &str) -> std::io::Result<Vec<String>>;
}

/// What one registration did with the schemes it was given and the keys it
/// wrote before.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Registered {
    /// Schemes whose key was written: absent before, or this application's
    /// with another executable.
    pub written: usize,
    /// Schemes whose key already opened links with this executable.
    pub current: usize,
    /// Schemes left alone: not a scheme, reserved, another application's
    /// key, or a write the registry refused. Each was warned about.
    pub refused: usize,
    /// Keys this application registered for a scheme it no longer declares,
    /// deleted.
    pub removed: usize,
}

/// Why a scheme was not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("{0:?} is not a URL scheme")]
    NotAScheme(String),
    #[error("`{0}` is opened by its domain or its file type, never registered as a scheme")]
    Reserved(String),
    #[error("the key of `{0}` belongs to another application and is left as it is")]
    Foreign(String),
    #[error("the key of `{scheme}` belongs to the application `{owner}` and is left as it is")]
    OtherApplication { scheme: String, owner: String },
    #[error(
        "`{0}` is one letter, which a launch's arguments read as a drive: its links would never \
         open the application"
    )]
    OneLetter(String),
    #[error(
        "another process created the key of `{0}` while it was registered; it is left as it is"
    )]
    Taken(String),
}

/// The application registration writes for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Application<'a> {
    /// Its reverse-DNS id, the [`MARKER`]'s value.
    pub(crate) app_id: &'a str,
    /// The name the system shows for its links.
    pub(crate) display_name: &'a str,
    /// The path of the executable a link launches, as the text a registry
    /// string holds.
    pub(crate) executable: &'a str,
}

impl Application<'_> {
    /// The command that launches the executable with a link:
    /// `"<path>" "%1"`.
    pub(crate) fn command(&self) -> String {
        format!("\"{}\" \"%1\"", self.executable)
    }
}

/// The key a declared `scheme` registers under: the scheme in lower case.
///
/// # Errors
///
/// [`Refusal::NotAScheme`] unless it is a letter followed by letters,
/// digits, `+`, `-` and `.` (RFC 3986 §3.1), which also keeps a key's name
/// from reaching another key; [`Refusal::OneLetter`] for a scheme a drive
/// letter would be mistaken for; [`Refusal::Reserved`] for the [`RESERVED`]
/// schemes.
pub(crate) fn scheme_key(scheme: &str) -> Result<String, Refusal> {
    let mut characters = scheme.chars();
    let well_formed = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !well_formed {
        return Err(Refusal::NotAScheme(scheme.to_owned()));
    }
    let key = scheme.to_ascii_lowercase();
    if key.len() == 1 {
        return Err(Refusal::OneLetter(key));
    }
    if RESERVED.contains(&key.as_str()) {
        return Err(Refusal::Reserved(key));
    }
    Ok(key)
}

/// The values a scheme's key is written with.
pub(crate) fn scheme_entries(application: &Application<'_>) -> [Entry; 5] {
    let entry = |subkey, name, data: String| Entry { subkey, name, data };
    // The marker goes first: a write that stops part way leaves a key the
    // next run knows as this application's and completes.
    [
        entry("", MARKER, application.app_id.to_owned()),
        entry("", "", format!("URL:{}", application.display_name)),
        entry("", "URL Protocol", String::new()),
        entry(
            "DefaultIcon",
            "",
            format!("\"{}\",0", application.executable),
        ),
        entry(COMMAND, "", application.command()),
    ]
}

/// What registration does with a key that holds `found`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Write(Claim),
    Current,
    Refuse(Refusal),
}

fn step(scheme: &str, found: Found, application: &Application<'_>) -> Step {
    match found {
        Found::Absent => Step::Write(Claim::New),
        Found::Marked { app_id, command } if app_id == application.app_id => {
            if command.is_some_and(|command| command == application.command()) {
                Step::Current
            } else {
                Step::Write(Claim::Own)
            }
        }
        Found::Marked { app_id, .. } => Step::Refuse(Refusal::OtherApplication {
            scheme: scheme.to_owned(),
            owner: app_id,
        }),
        Found::Unmarked => Step::Refuse(Refusal::Foreign(scheme.to_owned())),
    }
}

/// Register `url_schemes` for `application` in `classes`, scheme by scheme:
/// a scheme that cannot be registered is warned about, and the next one is
/// still registered. A key this application marked for a scheme it no
/// longer declares is deleted.
pub(crate) fn register_in(
    classes: &mut impl Classes,
    application: &Application<'_>,
    url_schemes: &[&str],
) -> Registered {
    let mut registered = Registered::default();
    let mut declared = Vec::with_capacity(url_schemes.len());
    for scheme in url_schemes {
        let key = match scheme_key(scheme) {
            Ok(key) => key,
            Err(refusal) => {
                tracing::warn!(%refusal, "a declared URL scheme is not registered");
                registered.refused += 1;
                continue;
            }
        };
        declared.push(key.clone());
        let found = match classes.find(&key) {
            Ok(found) => found,
            Err(error) => {
                tracing::warn!(%error, scheme = %key, "the scheme's key cannot be read; it is not registered");
                registered.refused += 1;
                continue;
            }
        };
        match step(&key, found, application) {
            Step::Current => registered.current += 1,
            Step::Refuse(refusal) => {
                tracing::warn!(%refusal, "a declared URL scheme is not registered");
                registered.refused += 1;
            }
            Step::Write(claim) => match classes.write(&key, &scheme_entries(application), claim) {
                Ok(Wrote::Written) => registered.written += 1,
                Ok(Wrote::Taken) => {
                    let refusal = Refusal::Taken(key.clone());
                    tracing::warn!(%refusal, "a declared URL scheme is not registered");
                    registered.refused += 1;
                }
                Err(error) => {
                    tracing::warn!(%error, scheme = %key, "the scheme's key cannot be written");
                    registered.refused += 1;
                }
            },
        }
    }
    registered.removed = remove_undeclared(classes, application.app_id, &declared);
    registered
}

/// Delete the keys marked with `app_id` whose scheme is not in `declared`:
/// a scheme an earlier version declared stops opening this application.
fn remove_undeclared(classes: &mut impl Classes, app_id: &str, declared: &[String]) -> usize {
    let marked = match classes.marked_by(app_id) {
        Ok(marked) => marked,
        Err(error) => {
            tracing::warn!(%error, "the application's registered keys cannot be listed");
            return 0;
        }
    };
    let mut removed = 0;
    // The registry's names compare without case, as `find` does, and a key
    // keeps the spelling it was created with.
    let undeclared = marked.iter().filter(|key| {
        !declared
            .iter()
            .any(|declared| declared.eq_ignore_ascii_case(key))
    });
    for key in undeclared {
        match classes.delete(key) {
            Ok(()) => removed += 1,
            Err(error) => {
                tracing::warn!(%error, key = %key, "a key of a scheme no longer declared cannot be deleted");
            }
        }
    }
    removed
}

/// Delete every key of the user's classes marked with `app_id`, and say how
/// many.
///
/// # Errors
///
/// The first failure, once every other key was tried: the keys could not be
/// listed, or one could not be deleted.
pub(crate) fn unregister_in(classes: &mut impl Classes, app_id: &str) -> std::io::Result<usize> {
    let keys = classes.marked_by(app_id)?;
    let mut deleted = 0;
    let mut first = None;
    for key in keys {
        match classes.delete(&key) {
            Ok(()) => deleted += 1,
            Err(error) => {
                tracing::warn!(%error, key = %key, "a registered key cannot be deleted");
                first.get_or_insert(error);
            }
        }
    }
    first.map_or(Ok(deleted), Err)
}

/// Register `url_schemes` in the user's classes for the running executable,
/// as the application `app_id` named `display_name`: each scheme no key
/// names is written, each this application registered for another
/// executable is rewritten, and each another application owns is left alone
/// and warned about. Blocks on the registry: the caller runs it off the
/// frame path.
#[cfg(windows)]
#[must_use = "what was written and refused is the caller's to report"]
pub fn register(app_id: &str, display_name: &str, url_schemes: &[&str]) -> Registered {
    let refused = Registered {
        refused: url_schemes.len(),
        ..Registered::default()
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            tracing::warn!(%error, "the running executable is unknown; no URL scheme is registered");
            return refused;
        }
    };
    let Some(executable) = executable.to_str() else {
        tracing::warn!(
            executable = %executable.display(),
            "the executable's path is not text; no URL scheme is registered"
        );
        return refused;
    };
    let application = Application {
        app_id,
        display_name,
        executable,
    };
    register_in(&mut registry::UserClasses, &application, url_schemes)
}

/// Delete the keys of the user's classes that registration wrote for the
/// application `app_id`, and say how many; keys without its marker are not
/// touched. Blocks on the registry.
///
/// # Errors
///
/// The registry's first failure, once every key was tried.
#[cfg(windows)]
pub fn unregister(app_id: &str) -> std::io::Result<usize> {
    unregister_in(&mut registry::UserClasses, app_id)
}

/// Whether the process runs with a package identity (MSIX): its schemes and
/// file types are then the package manifest's, and registration writes
/// nothing.
#[cfg(windows)]
#[must_use]
pub fn runs_packaged() -> bool {
    registry::runs_packaged()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        Application, Claim, Classes, Entry, Found, MARKER, Refusal, Registered, Wrote, register_in,
        scheme_key, unregister_in,
    };

    /// The user's classes and the machine's, as maps of key to values, whose
    /// names compare without case as the registry's do.
    #[derive(Default)]
    struct Map {
        user: BTreeMap<String, Vec<Entry>>,
        machine: BTreeMap<String, Vec<Entry>>,
        refuse_writes: bool,
        refuse_delete: Option<&'static str>,
        /// A write sets this many values and then fails.
        stop_after: Option<usize>,
        /// A key another process creates between a scheme's read and its
        /// write.
        appears: Option<(&'static str, Vec<Entry>)>,
    }

    impl Map {
        fn value(entries: &[Entry], subkey: &str, name: &str) -> Option<String> {
            entries
                .iter()
                .find(|entry| entry.subkey == subkey && entry.name == name)
                .map(|entry| entry.data.clone())
        }

        fn spelled(keys: &BTreeMap<String, Vec<Entry>>, name: &str) -> Option<String> {
            keys.keys()
                .find(|key| key.eq_ignore_ascii_case(name))
                .cloned()
        }
    }

    impl Classes for Map {
        fn find(&self, name: &str) -> std::io::Result<Found> {
            let entries = Self::spelled(&self.user, name)
                .map(|key| &self.user[&key])
                .or_else(|| Self::spelled(&self.machine, name).map(|key| &self.machine[&key]));
            let Some(entries) = entries else {
                return Ok(Found::Absent);
            };
            Ok(match Self::value(entries, "", MARKER) {
                Some(app_id) => Found::Marked {
                    app_id,
                    command: Self::value(entries, "shell\\open\\command", ""),
                },
                None => Found::Unmarked,
            })
        }

        fn write(&mut self, name: &str, entries: &[Entry], claim: Claim) -> std::io::Result<Wrote> {
            if let Some((key, values)) = self.appears.take() {
                self.user.insert(key.to_owned(), values);
            }
            if self.refuse_writes {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            let existing = Self::spelled(&self.user, name);
            if claim == Claim::New && existing.is_some() {
                return Ok(Wrote::Taken);
            }
            let key = existing.unwrap_or_else(|| name.to_owned());
            for (set, entry) in entries.iter().enumerate() {
                if self.stop_after == Some(set) {
                    // The contract: a key the write created goes with it.
                    if claim == Claim::New {
                        self.user.remove(&key);
                    }
                    return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                }
                let values = self.user.entry(key.clone()).or_default();
                values.retain(|value| (value.subkey, value.name) != (entry.subkey, entry.name));
                values.push(entry.clone());
            }
            Ok(Wrote::Written)
        }

        fn delete(&mut self, name: &str) -> std::io::Result<()> {
            if self.refuse_delete == Some(name) {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            if let Some(key) = Self::spelled(&self.user, name) {
                self.user.remove(&key);
            }
            Ok(())
        }

        fn marked_by(&self, app_id: &str) -> std::io::Result<Vec<String>> {
            Ok(self
                .user
                .iter()
                .filter(|(_, entries)| Self::value(entries, "", MARKER).as_deref() == Some(app_id))
                .map(|(name, _)| name.clone())
                .collect())
        }
    }

    fn entry(subkey: &'static str, name: &'static str, data: &str) -> Entry {
        Entry {
            subkey,
            name,
            data: data.to_owned(),
        }
    }

    const NOTES: Application<'static> = Application {
        app_id: "com.example.notes",
        display_name: "Notes",
        executable: r"C:\Apps\Notes\notes.exe",
    };

    /// A scheme no key names is written with the link command, the
    /// application's marker, its name and its icon; written again, it is
    /// current and nothing changes.
    #[test]
    fn an_absent_scheme_is_written_once() {
        let mut classes = Map::default();
        let registered = register_in(&mut classes, &NOTES, &["Notes"]);
        assert_eq!(
            registered,
            Registered {
                written: 1,
                ..Registered::default()
            }
        );
        assert_eq!(
            classes.user["notes"],
            [
                entry("", MARKER, "com.example.notes"),
                entry("", "", "URL:Notes"),
                entry("", "URL Protocol", ""),
                entry("DefaultIcon", "", r#""C:\Apps\Notes\notes.exe",0"#),
                entry(
                    "shell\\open\\command",
                    "",
                    r#""C:\Apps\Notes\notes.exe" "%1""#
                ),
            ],
            "the key the scheme opens links with"
        );
        let again = register_in(&mut classes, &NOTES, &["notes"]);
        assert_eq!(
            again,
            Registered {
                current: 1,
                ..Registered::default()
            }
        );
    }

    /// This application's key for another executable is rewritten; a key
    /// without the marker, in the user's classes or only the machine's, and a
    /// key another application marked are left exactly as they were.
    #[test]
    fn only_this_applications_keys_are_written() {
        let mut classes = Map::default();
        let moved = vec![
            entry("", MARKER, "com.example.notes"),
            entry("shell\\open\\command", "", r#""D:\Old\notes.exe" "%1""#),
        ];
        let foreign = vec![entry(
            "shell\\open\\command",
            "",
            r#""C:\Other\other.exe" "%1""#,
        )];
        let other = vec![
            entry("", MARKER, "com.example.other"),
            entry("shell\\open\\command", "", r#""C:\Other\other.exe" "%1""#),
        ];
        classes.user.insert("notes".to_owned(), moved);
        classes.user.insert("foreign".to_owned(), foreign.clone());
        classes
            .machine
            .insert("machine".to_owned(), foreign.clone());
        classes.user.insert("other".to_owned(), other.clone());
        let registered = register_in(
            &mut classes,
            &NOTES,
            &["notes", "foreign", "machine", "other"],
        );
        assert_eq!(
            registered,
            Registered {
                written: 1,
                refused: 3,
                ..Registered::default()
            }
        );
        assert_eq!(
            classes.find("notes").expect("found"),
            Found::Marked {
                app_id: "com.example.notes".to_owned(),
                command: Some(NOTES.command()),
            },
            "the moved application's key names this executable"
        );
        assert_eq!(
            classes.user["foreign"], foreign,
            "an unmarked key is untouched"
        );
        assert!(
            !classes.user.contains_key("machine"),
            "a machine-wide key is not shadowed by a user key"
        );
        assert_eq!(
            classes.user["other"], other,
            "another application's key is untouched"
        );
    }

    /// A declared name that is not a scheme, or that would name another key,
    /// and the web and file schemes are refused before any key is read; a
    /// write the registry refuses counts as refused, and the next scheme is
    /// still registered.
    #[test]
    fn a_scheme_that_cannot_be_registered_does_not_stop_the_next() {
        for (scheme, refusal) in [
            ("", Refusal::NotAScheme(String::new())),
            ("1notes", Refusal::NotAScheme("1notes".to_owned())),
            (
                "notes\\shell",
                Refusal::NotAScheme("notes\\shell".to_owned()),
            ),
            ("no tes", Refusal::NotAScheme("no tes".to_owned())),
            ("HTTPS", Refusal::Reserved("https".to_owned())),
            ("http", Refusal::Reserved("http".to_owned())),
            ("file", Refusal::Reserved("file".to_owned())),
            ("Data", Refusal::Reserved("data".to_owned())),
            ("X", Refusal::OneLetter("x".to_owned())),
        ] {
            assert_eq!(scheme_key(scheme), Err(refusal), "{scheme:?}");
        }
        assert_eq!(scheme_key("x-notes+v1.2").as_deref(), Ok("x-notes+v1.2"));

        let mut classes = Map::default();
        let registered = register_in(&mut classes, &NOTES, &["https", "notes\\x", "notes"]);
        assert_eq!(
            registered,
            Registered {
                written: 1,
                refused: 2,
                ..Registered::default()
            }
        );
        assert_eq!(classes.user.keys().collect::<Vec<_>>(), ["notes"]);

        let mut refusing = Map {
            refuse_writes: true,
            ..Map::default()
        };
        let registered = register_in(&mut refusing, &NOTES, &["notes", "jot"]);
        assert_eq!(
            registered,
            Registered {
                refused: 2,
                ..Registered::default()
            }
        );
    }

    /// A write that stops part way never leaves a key a later run refuses:
    /// a key it created goes with it and is written whole next time, and a
    /// rewrite of this application's own key keeps the marker it starts
    /// with, so the next run completes it.
    #[test]
    fn a_write_that_stops_is_completed_by_the_next_run() {
        let mut classes = Map {
            stop_after: Some(1),
            ..Map::default()
        };
        let registered = register_in(&mut classes, &NOTES, &["notes"]);
        assert_eq!(
            registered,
            Registered {
                refused: 1,
                ..Registered::default()
            }
        );
        assert!(classes.user.is_empty(), "a created key goes with its write");
        classes.stop_after = None;
        let registered = register_in(&mut classes, &NOTES, &["notes"]);
        assert_eq!(registered.written, 1);

        let moved = Application {
            executable: r"D:\Moved\notes.exe",
            ..NOTES
        };
        classes.stop_after = Some(2);
        let _ = register_in(&mut classes, &moved, &["notes"]);
        assert!(
            matches!(
                classes.find("notes").expect("found"),
                Found::Marked { ref app_id, .. } if app_id == "com.example.notes"
            ),
            "the stopped rewrite leaves the application's marker"
        );
        classes.stop_after = None;
        let registered = register_in(&mut classes, &moved, &["notes"]);
        assert_eq!(registered.written, 1);
        assert_eq!(
            classes.find("notes").expect("found"),
            Found::Marked {
                app_id: "com.example.notes".to_owned(),
                command: Some(moved.command()),
            }
        );
    }

    /// A key another process creates between the read and the write, whether
    /// another application's registration or anyone's, is left exactly as it
    /// was created.
    #[test]
    fn a_key_created_while_registering_is_left_alone() {
        let other = vec![
            entry("", MARKER, "com.example.other"),
            entry("shell\\open\\command", "", r#""C:\Other\other.exe" "%1""#),
        ];
        let mut classes = Map {
            appears: Some(("notes", other.clone())),
            ..Map::default()
        };
        let registered = register_in(&mut classes, &NOTES, &["notes"]);
        assert_eq!(
            registered,
            Registered {
                refused: 1,
                ..Registered::default()
            }
        );
        assert_eq!(classes.user["notes"], other);
    }

    /// The application's key keeps the spelling it was created with, and the
    /// registry compares names without case: a key spelled `Jot` is the
    /// declared `jot`'s, current, and not removed.
    #[test]
    fn a_key_is_the_schemes_whatever_its_case() {
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &["jot"]);
        let values = classes.user.remove("jot").expect("written");
        classes.user.insert("Jot".to_owned(), values);
        let registered = register_in(&mut classes, &NOTES, &["jot"]);
        assert_eq!(
            registered,
            Registered {
                current: 1,
                ..Registered::default()
            }
        );
        assert!(classes.user.contains_key("Jot"));
    }

    /// A scheme an earlier version declared and this one does not stops
    /// opening the application: its marked key is deleted, while another
    /// application's key and an unmarked one stay.
    #[test]
    fn a_scheme_no_longer_declared_is_removed() {
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &["notes", "jot"]);
        classes.user.insert(
            "other".to_owned(),
            vec![entry("", MARKER, "com.example.other")],
        );
        classes.user.insert(
            "foreign".to_owned(),
            vec![entry("shell\\open\\command", "", "x")],
        );
        let registered = register_in(&mut classes, &NOTES, &["Notes"]);
        assert_eq!(
            registered,
            Registered {
                current: 1,
                removed: 1,
                ..Registered::default()
            }
        );
        assert_eq!(
            classes.user.keys().collect::<Vec<_>>(),
            ["foreign", "notes", "other"],
            "only the undeclared scheme of this application is gone"
        );
    }

    /// Unregistering deletes the user's keys marked with this application's
    /// id, including a scheme it no longer declares, and leaves every other
    /// key; a key the registry will not delete is reported once every other
    /// key was deleted.
    #[test]
    fn unregistering_deletes_only_this_applications_keys() {
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &["notes", "jot"]);
        classes.user.insert(
            "other".to_owned(),
            vec![entry("", MARKER, "com.example.other")],
        );
        classes.user.insert(
            "foreign".to_owned(),
            vec![entry("shell\\open\\command", "", "x")],
        );
        assert_eq!(
            unregister_in(&mut classes, "com.example.notes").ok(),
            Some(2)
        );
        assert_eq!(
            classes.user.keys().collect::<Vec<_>>(),
            ["foreign", "other"],
            "the other keys stay"
        );
        assert_eq!(
            unregister_in(&mut classes, "com.example.notes").ok(),
            Some(0)
        );

        let mut refusing = Map {
            refuse_delete: Some("jot"),
            ..Map::default()
        };
        let _ = register_in(&mut refusing, &NOTES, &["jot", "notes"]);
        let refused = unregister_in(&mut refusing, "com.example.notes")
            .expect_err("a key the registry would not delete is reported");
        assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            refusing.user.keys().collect::<Vec<_>>(),
            ["jot"],
            "the key after the refused one is still deleted"
        );
    }
}
