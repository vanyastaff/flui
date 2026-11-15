//! An unpackaged Windows application's registration of the URL schemes and
//! the file types it opens (ADR-0307 §4, the activation spec's AC11), under
//! the user's `HKCU\Software\Classes`:
//!
//! - a scheme is one key, named by the scheme, whose `shell\open\command`
//!   launches the application's executable with the link as its argument
//!   (`"%1"`);
//! - a file type is one key, the application's ProgID for it
//!   (`<application id>.<extension>`), whose `shell\open\command` launches
//!   the executable with the file's path, and a value named by that ProgID
//!   under `.<extension>\OpenWithProgids`, which offers the application in
//!   the extension's "Open with" list. The extension's default stays the
//!   user's choice: registration never sets it.
//!
//! The host's launch arguments then open the link or the file.
//!
//! Each key it writes carries the application's id under [`MARKER`], and a
//! ProgID the extension it opens under [`FILE_TYPE`]; the marker is the only
//! claim registration trusts:
//!
//! - a key no one has, in the user's classes or the machine's, is written,
//!   into a key the write itself creates: one another process created since
//!   it was read is left to that process ([`Claim::New`]), and one whose
//!   write fails is deleted again; the marker is the first value written, so
//!   a rewrite that stops part way leaves a key the next run still knows as
//!   this application's;
//! - a key marked with this application's id is rewritten only when its
//!   command names another executable (the application moved or was
//!   updated in place), and left as it is otherwise;
//! - a key without the marker, or marked by another application, belongs to
//!   someone else and is never touched: the scheme or file type is refused,
//!   with a warning;
//! - a key marked with this application's id for a scheme or a file type it
//!   no longer declares is deleted when it registers the others, a ProgID
//!   after its `OpenWithProgids` value; an application that declares nothing
//!   any more registers nothing, so its keys stay until it is unregistered;
//! - [`unregister`] deletes only the user's keys marked with this
//!   application's id and the `OpenWithProgids` values of its ProgIDs, so an
//!   uninstaller that runs the executable with `--flui-unregister-associations`
//!   removes what registration wrote and nothing else.
//!
//! A change to a file type's registration is announced to the shell, which
//! caches associations.
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

/// The value a file type's ProgID holds besides [`MARKER`]: the extension it
/// opens, without its `.`.
pub(crate) const FILE_TYPE: &str = "FluiFileType";

/// The key's subkey whose default value is the command that opens a link or
/// a file.
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
    /// The key carries [`MARKER`]: the application id it names, the command
    /// it opens a link or a file with, if it has one, and the extension a
    /// ProgID names under [`FILE_TYPE`] (`None` for a scheme's key).
    Marked {
        app_id: String,
        command: Option<String>,
        file_type: Option<String>,
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
    fn marked_by(&self, app_id: &str) -> std::io::Result<Vec<Marked>>;
    /// Name `prog_id` under the user's `<extension_key>\OpenWithProgids`,
    /// creating the keys it needs, and say whether the value is new; a value
    /// that is there already is left as it is.
    fn offer(&mut self, extension_key: &str, prog_id: &str) -> std::io::Result<bool>;
    /// Delete the value `prog_id` of the user's
    /// `<extension_key>\OpenWithProgids`; a value or key that is gone already
    /// is not an error.
    fn withdraw(&mut self, extension_key: &str, prog_id: &str) -> std::io::Result<()>;
    /// Tell the shell that a file type's registration changed.
    fn announce(&mut self);
}

/// A user's key marked with an application's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    /// The key's name, as spelled when it was created.
    pub(crate) name: String,
    /// The extension a ProgID names under [`FILE_TYPE`]; `None` for a
    /// scheme's key.
    pub(crate) file_type: Option<String>,
}

/// A file type the application opens, as registration writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType<'a> {
    extension: &'a str,
    name: &'a str,
}

impl<'a> FileType<'a> {
    /// The file type with `extension`, without its `.`, that the system
    /// shows as `name`.
    #[must_use]
    pub const fn new(extension: &'a str, name: &'a str) -> Self {
        Self { extension, name }
    }
}

/// What one registration did with the schemes and file types it was given
/// and the keys it wrote before.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Registered {
    /// Schemes and file types whose key was written: absent before, or this
    /// application's with another executable.
    pub written: usize,
    /// Schemes and file types whose key already opened them with this
    /// executable.
    pub current: usize,
    /// Schemes and file types left alone: not a scheme or an extension,
    /// reserved, another application's key, or a write the registry
    /// refused. Each was warned about.
    pub refused: usize,
    /// Keys this application registered for a scheme or a file type it no
    /// longer declares, deleted.
    pub removed: usize,
}

/// Why a scheme or a file type was not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("{0:?} is not a URL scheme")]
    NotAScheme(String),
    #[error("`{0}` is opened by its domain or its file type, never registered as a scheme")]
    Reserved(String),
    #[error("the key `{0}` belongs to another application and is left as it is")]
    Foreign(String),
    #[error("the key `{key}` belongs to the application `{owner}` and is left as it is")]
    OtherApplication { key: String, owner: String },
    #[error(
        "`{0}` is one letter, which a launch's arguments read as a drive: its links would never \
         open the application"
    )]
    OneLetter(String),
    #[error("another process created the key `{0}` while it was registered; it is left as it is")]
    Taken(String),
    #[error("{0:?} is not a file extension: 1 to 32 ASCII letters and digits, without its `.`")]
    NotAnExtension(String),
    #[error("the file type's ProgID `{0}` is also a declared URL scheme, whose key it would share")]
    SchemesKey(String),
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

/// The extension `text` names, in lower case: 1 to 32 ASCII letters and
/// digits, the extensions `flui`'s manifest check admits, which also keeps a
/// key's name from reaching another key.
fn extension(text: &str) -> Option<String> {
    ((1..=32).contains(&text.len()) && text.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| text.to_ascii_lowercase())
}

/// The ProgID a declared file type with `extension` registers under for the
/// application `app_id`, `<app_id>.<extension>`, and the extension in lower
/// case.
///
/// # Errors
///
/// [`Refusal::NotAnExtension`] unless the extension is 1 to 32 ASCII letters
/// and digits.
pub(crate) fn file_type_key(app_id: &str, extension: &str) -> Result<(String, String), Refusal> {
    let lower =
        self::extension(extension).ok_or_else(|| Refusal::NotAnExtension(extension.to_owned()))?;
    Ok((format!("{app_id}.{lower}"), lower))
}

/// The values a file type's ProgID is written with.
pub(crate) fn file_type_entries(
    application: &Application<'_>,
    extension: &str,
    name: &str,
) -> [Entry; 5] {
    let entry = |subkey, name, data: String| Entry { subkey, name, data };
    // The marker goes first, as a scheme's does.
    [
        entry("", MARKER, application.app_id.to_owned()),
        entry("", FILE_TYPE, extension.to_owned()),
        entry("", "", name.to_owned()),
        entry(
            "DefaultIcon",
            "",
            format!("\"{}\",0", application.executable),
        ),
        entry(COMMAND, "", application.command()),
    ]
}

/// The key of the extension `extension`, without its `.`.
fn extension_key(extension: &str) -> String {
    format!(".{extension}")
}

/// What registration does with a key that holds `found`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Write(Claim),
    /// The key is this application's but of the other kind (a scheme's key
    /// where a ProgID is declared, or the reverse, or a ProgID of another
    /// extension): it is removed, its offer withdrawn, and written anew.
    Replace(Marked),
    Current,
    Refuse(Refusal),
}

/// What registration does with the key `key` that holds `found`, for a
/// scheme (`file_type` is `None`) or the file type of the extension
/// `file_type`.
fn step(key: &str, found: Found, application: &Application<'_>, file_type: Option<&str>) -> Step {
    match found {
        Found::Absent => Step::Write(Claim::New),
        Found::Marked {
            app_id,
            command,
            file_type: found_type,
        } if app_id == application.app_id => {
            if found_type.as_deref() != file_type {
                Step::Replace(Marked {
                    name: key.to_owned(),
                    file_type: found_type,
                })
            } else if command.is_some_and(|command| command == application.command()) {
                Step::Current
            } else {
                Step::Write(Claim::Own)
            }
        }
        Found::Marked { app_id, .. } => Step::Refuse(Refusal::OtherApplication {
            key: key.to_owned(),
            owner: app_id,
        }),
        Found::Unmarked => Step::Refuse(Refusal::Foreign(key.to_owned())),
    }
}

/// What registering one scheme or file type came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Written,
    Current,
    Refused,
}

impl Registered {
    fn count(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Written => self.written += 1,
            Outcome::Current => self.current += 1,
            Outcome::Refused => self.refused += 1,
        }
    }
}

/// Write `entries` into the key `key` of a declared `what` (a URL scheme, or
/// the file type of the extension `file_type`), unless what it holds is
/// current or someone else's. An offer withdrawn on the way, from a key of
/// the other kind this application wrote, is counted in `withdrawn`.
fn register_key(
    classes: &mut impl Classes,
    application: &Application<'_>,
    key: &str,
    entries: &[Entry],
    what: &str,
    file_type: Option<&str>,
    withdrawn: &mut usize,
) -> Outcome {
    let found = match classes.find(key) {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, key, "the key of a declared {what} cannot be read; it is not registered");
            return Outcome::Refused;
        }
    };
    match step(key, found, application, file_type) {
        Step::Current => Outcome::Current,
        Step::Refuse(refusal) => {
            tracing::warn!(%refusal, "a declared {what} is not registered");
            Outcome::Refused
        }
        Step::Write(claim) => write_key(classes, key, entries, claim, what),
        Step::Replace(marked) => {
            // Removed whole, so no value of the other kind stays behind
            // (`URL Protocol`, `FluiFileType`, an offer), then created anew.
            let mut replaced = Removed::default();
            let removal = remove(classes, &marked, &mut replaced);
            *withdrawn += replaced.withdrawn;
            if let Err(error) = removal {
                tracing::warn!(%error, key, "a key of the other kind cannot be replaced by a declared {what}");
                return Outcome::Refused;
            }
            write_key(classes, key, entries, Claim::New, what)
        }
    }
}

/// Write `entries` into the key `key` of a declared `what` under `claim`.
fn write_key(
    classes: &mut impl Classes,
    key: &str,
    entries: &[Entry],
    claim: Claim,
    what: &str,
) -> Outcome {
    match classes.write(key, entries, claim) {
        Ok(Wrote::Written) => Outcome::Written,
        Ok(Wrote::Taken) => {
            let refusal = Refusal::Taken(key.to_owned());
            tracing::warn!(%refusal, "a declared {what} is not registered");
            Outcome::Refused
        }
        Err(error) => {
            tracing::warn!(%error, key, "the key of a declared {what} cannot be written");
            Outcome::Refused
        }
    }
}

/// Register `url_schemes` and `file_types` for `application` in `classes`,
/// one by one: one that cannot be registered is warned about, and the next
/// is still registered. A key this application marked for a scheme or a
/// file type it no longer declares is deleted.
pub(crate) fn register_in(
    classes: &mut impl Classes,
    application: &Application<'_>,
    url_schemes: &[&str],
    file_types: &[FileType<'_>],
) -> Registered {
    let mut registered = Registered::default();
    let mut declared = Vec::with_capacity(url_schemes.len() + file_types.len());
    // Offers withdrawn from keys of the other kind, which the shell hears about.
    let mut withdrawn = 0;
    for scheme in url_schemes {
        let key = match scheme_key(scheme) {
            Ok(key) => key,
            Err(refusal) => {
                tracing::warn!(%refusal, "a declared URL scheme is not registered");
                registered.refused += 1;
                continue;
            }
        };
        let outcome = register_key(
            classes,
            application,
            &key,
            &scheme_entries(application),
            "URL scheme",
            None,
            &mut withdrawn,
        );
        registered.count(outcome);
        declared.push(key);
    }
    let schemes = declared.len();
    let mut changed = false;
    for file_type in file_types {
        let (prog_id, extension) = match file_type_key(application.app_id, file_type.extension) {
            Ok(keys) => keys,
            Err(refusal) => {
                tracing::warn!(%refusal, "a declared file type is not registered");
                registered.refused += 1;
                continue;
            }
        };
        if declared[..schemes]
            .iter()
            .any(|scheme| scheme.eq_ignore_ascii_case(&prog_id))
        {
            let refusal = Refusal::SchemesKey(prog_id);
            tracing::warn!(%refusal, "a declared file type is not registered");
            registered.refused += 1;
            continue;
        }
        let entries = file_type_entries(application, &extension, file_type.name);
        let outcome = register_key(
            classes,
            application,
            &prog_id,
            &entries,
            "file type",
            Some(&extension),
            &mut withdrawn,
        );
        changed |= outcome == Outcome::Written;
        let outcome = match outcome {
            Outcome::Refused => Outcome::Refused,
            // A current ProgID is offered again: a value someone deleted
            // comes back.
            done => match classes.offer(&extension_key(&extension), &prog_id) {
                Ok(added) => {
                    changed |= added;
                    done
                }
                Err(error) => {
                    tracing::warn!(%error, prog_id = %prog_id, "a file type cannot be offered in its extension's Open with list");
                    Outcome::Refused
                }
            },
        };
        registered.count(outcome);
        declared.push(prog_id);
    }
    let removed = remove_undeclared(classes, application.app_id, &declared);
    registered.removed = removed.keys;
    if changed || withdrawn > 0 || removed.withdrawn > 0 {
        classes.announce();
    }
    registered
}

/// What deleting marked keys deleted.
#[derive(Debug, Default, Clone, Copy)]
struct Removed {
    keys: usize,
    /// The `OpenWithProgids` values withdrawn, counted as each goes: the
    /// shell hears of a withdrawal whose key then fails to go too.
    withdrawn: usize,
}

/// Delete `marked`: a ProgID's `OpenWithProgids` value first, so a key that
/// goes is never left offered and a value that stays keeps its key for the
/// next attempt.
fn remove(
    classes: &mut impl Classes,
    marked: &Marked,
    removed: &mut Removed,
) -> std::io::Result<()> {
    // An extension the marker does not hold as written names no key this
    // application offered itself under.
    if let Some(extension) = marked.file_type.as_deref().and_then(extension) {
        classes.withdraw(&extension_key(&extension), &marked.name)?;
        removed.withdrawn += 1;
    }
    classes.delete(&marked.name)?;
    removed.keys += 1;
    Ok(())
}

/// Delete the keys marked with `app_id` that `declared` does not name: a
/// scheme or a file type an earlier version declared stops opening this
/// application.
fn remove_undeclared(classes: &mut impl Classes, app_id: &str, declared: &[String]) -> Removed {
    let mut removed = Removed::default();
    let marked = match classes.marked_by(app_id) {
        Ok(marked) => marked,
        Err(error) => {
            tracing::warn!(%error, "the application's registered keys cannot be listed");
            return removed;
        }
    };
    // The registry's names compare without case, as `find` does, and a key
    // keeps the spelling it was created with.
    let undeclared = marked.iter().filter(|marked| {
        !declared
            .iter()
            .any(|declared| declared.eq_ignore_ascii_case(&marked.name))
    });
    for marked in undeclared {
        if let Err(error) = remove(classes, marked, &mut removed) {
            tracing::warn!(%error, key = %marked.name, "a key no longer declared cannot be deleted");
        }
    }
    removed
}

/// Delete every key of the user's classes marked with `app_id` and the
/// `OpenWithProgids` values of its ProgIDs, and say how many keys.
///
/// # Errors
///
/// The first failure, once every other key was tried: the keys could not be
/// listed, or one could not be deleted.
pub(crate) fn unregister_in(classes: &mut impl Classes, app_id: &str) -> std::io::Result<usize> {
    let keys = classes.marked_by(app_id)?;
    let mut removed = Removed::default();
    let mut first = None;
    for key in &keys {
        if let Err(error) = remove(classes, key, &mut removed) {
            tracing::warn!(%error, key = %key.name, "a registered key cannot be deleted");
            first.get_or_insert(error);
        }
    }
    if removed.withdrawn > 0 {
        classes.announce();
    }
    first.map_or(Ok(removed.keys), Err)
}

/// Register `url_schemes` and `file_types` in the user's classes for the
/// running executable, as the application `app_id` named `display_name`:
/// each one no key names is written, each this application registered for
/// another executable is rewritten, and each another application owns is
/// left alone and warned about. Blocks on the registry: the caller runs it
/// off the frame path.
#[cfg(windows)]
#[must_use = "what was written and refused is the caller's to report"]
pub fn register(
    app_id: &str,
    display_name: &str,
    url_schemes: &[&str],
    file_types: &[FileType<'_>],
) -> Registered {
    let refused = Registered {
        refused: url_schemes.len() + file_types.len(),
        ..Registered::default()
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            tracing::warn!(%error, "the running executable is unknown; nothing is registered");
            return refused;
        }
    };
    let Some(executable) = executable.to_str() else {
        tracing::warn!(
            executable = %executable.display(),
            "the executable's path is not text; nothing is registered"
        );
        return refused;
    };
    let application = Application {
        app_id,
        display_name,
        executable,
    };
    register_in(
        &mut registry::UserClasses,
        &application,
        url_schemes,
        file_types,
    )
}

/// Delete the keys of the user's classes that registration wrote for the
/// application `app_id` and the `OpenWithProgids` values of its ProgIDs, and
/// say how many keys; keys without its marker are not touched. Blocks on the
/// registry.
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
        Application, Claim, Classes, Entry, FILE_TYPE, FileType, Found, MARKER, Marked, Refusal,
        Registered, Wrote, file_type_entries, file_type_key, register_in, scheme_entries,
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
        /// `OpenWithProgids` values, by the extension's key in lower case.
        offered: BTreeMap<String, Vec<String>>,
        refuse_offers: bool,
        refuse_withdraw: bool,
        /// How often the shell was told that a file type changed.
        announced: usize,
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
                    file_type: Self::value(entries, "", FILE_TYPE),
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

        fn marked_by(&self, app_id: &str) -> std::io::Result<Vec<Marked>> {
            Ok(self
                .user
                .iter()
                .filter(|(_, entries)| Self::value(entries, "", MARKER).as_deref() == Some(app_id))
                .map(|(name, entries)| Marked {
                    name: name.clone(),
                    file_type: Self::value(entries, "", FILE_TYPE),
                })
                .collect())
        }

        fn offer(&mut self, extension_key: &str, prog_id: &str) -> std::io::Result<bool> {
            if self.refuse_offers {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            let offered = self
                .offered
                .entry(extension_key.to_ascii_lowercase())
                .or_default();
            if offered.iter().any(|id| id.eq_ignore_ascii_case(prog_id)) {
                return Ok(false);
            }
            offered.push(prog_id.to_owned());
            Ok(true)
        }

        fn withdraw(&mut self, extension_key: &str, prog_id: &str) -> std::io::Result<()> {
            if self.refuse_withdraw {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            if let Some(offered) = self.offered.get_mut(&extension_key.to_ascii_lowercase()) {
                offered.retain(|id| !id.eq_ignore_ascii_case(prog_id));
            }
            Ok(())
        }

        fn announce(&mut self) {
            self.announced += 1;
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
        let registered = register_in(&mut classes, &NOTES, &["Notes"], &[]);
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
        let again = register_in(&mut classes, &NOTES, &["notes"], &[]);
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
            &[],
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
                file_type: None,
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
        let registered = register_in(&mut classes, &NOTES, &["https", "notes\\x", "notes"], &[]);
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
        let registered = register_in(&mut refusing, &NOTES, &["notes", "jot"], &[]);
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
        let registered = register_in(&mut classes, &NOTES, &["notes"], &[]);
        assert_eq!(
            registered,
            Registered {
                refused: 1,
                ..Registered::default()
            }
        );
        assert!(classes.user.is_empty(), "a created key goes with its write");
        classes.stop_after = None;
        let registered = register_in(&mut classes, &NOTES, &["notes"], &[]);
        assert_eq!(registered.written, 1);

        let moved = Application {
            executable: r"D:\Moved\notes.exe",
            ..NOTES
        };
        classes.stop_after = Some(2);
        let _ = register_in(&mut classes, &moved, &["notes"], &[]);
        assert!(
            matches!(
                classes.find("notes").expect("found"),
                Found::Marked { ref app_id, .. } if app_id == "com.example.notes"
            ),
            "the stopped rewrite leaves the application's marker"
        );
        classes.stop_after = None;
        let registered = register_in(&mut classes, &moved, &["notes"], &[]);
        assert_eq!(registered.written, 1);
        assert_eq!(
            classes.find("notes").expect("found"),
            Found::Marked {
                app_id: "com.example.notes".to_owned(),
                command: Some(moved.command()),
                file_type: None,
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
        let registered = register_in(&mut classes, &NOTES, &["notes"], &[]);
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
        let _ = register_in(&mut classes, &NOTES, &["jot"], &[]);
        let values = classes.user.remove("jot").expect("written");
        classes.user.insert("Jot".to_owned(), values);
        let registered = register_in(&mut classes, &NOTES, &["jot"], &[]);
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
        let _ = register_in(&mut classes, &NOTES, &["notes", "jot"], &[]);
        classes.user.insert(
            "other".to_owned(),
            vec![entry("", MARKER, "com.example.other")],
        );
        classes.user.insert(
            "foreign".to_owned(),
            vec![entry("shell\\open\\command", "", "x")],
        );
        let registered = register_in(&mut classes, &NOTES, &["Notes"], &[]);
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
        let _ = register_in(&mut classes, &NOTES, &["notes", "jot"], &[]);
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
        let _ = register_in(&mut refusing, &NOTES, &["jot", "notes"], &[]);
        let refused = unregister_in(&mut refusing, "com.example.notes")
            .expect_err("a key the registry would not delete is reported");
        assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            refusing.user.keys().collect::<Vec<_>>(),
            ["jot"],
            "the key after the refused one is still deleted"
        );
    }

    const NOTE: FileType<'static> = FileType::new("Note", "Notes note");
    const JOT: FileType<'static> = FileType::new("jot", "Notes jot");

    /// A file type no key names gets the application's ProgID, written with
    /// the open command, the marker, its extension and its name, and is
    /// offered in its extension's "Open with" list, which the shell hears
    /// about; registered again it is current and the shell hears nothing,
    /// and an offer someone deleted comes back.
    #[test]
    fn a_file_type_is_written_and_offered_once() {
        let mut classes = Map::default();
        let registered = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(
            registered,
            Registered {
                written: 1,
                ..Registered::default()
            }
        );
        assert_eq!(
            classes.user["com.example.notes.note"],
            [
                entry("", MARKER, "com.example.notes"),
                entry("", FILE_TYPE, "note"),
                entry("", "", "Notes note"),
                entry("DefaultIcon", "", r#""C:\Apps\Notes\notes.exe",0"#),
                entry(
                    "shell\\open\\command",
                    "",
                    r#""C:\Apps\Notes\notes.exe" "%1""#
                ),
            ],
            "the ProgID the file type opens with"
        );
        assert_eq!(classes.offered[".note"], ["com.example.notes.note"]);
        assert_eq!(classes.announced, 1);

        let again = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(
            again,
            Registered {
                current: 1,
                ..Registered::default()
            }
        );
        assert_eq!(classes.announced, 1, "nothing changed");

        classes.offered.clear();
        let restored = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(restored.current, 1);
        assert_eq!(classes.offered[".note"], ["com.example.notes.note"]);
        assert_eq!(classes.announced, 2, "the restored offer is announced");
    }

    /// A declared extension that is not one, or that would name another key,
    /// is refused before any key is read; a ProgID another application or
    /// no application marked, one a declared scheme already names, and an
    /// offer the registry refuses each count as refused without stopping the
    /// next file type, and a refused offer is made by the next run.
    #[test]
    fn a_file_type_that_cannot_be_registered_does_not_stop_the_next() {
        for extension in ["", ".note", "no te", "note\\shell", &"x".repeat(33)] {
            assert_eq!(
                file_type_key("com.example.notes", extension),
                Err(Refusal::NotAnExtension(extension.to_owned())),
                "{extension:?}"
            );
        }
        assert_eq!(
            file_type_key("com.example.notes", "MD5"),
            Ok(("com.example.notes.md5".to_owned(), "md5".to_owned()))
        );

        let foreign = vec![entry("shell\\open\\command", "", "x")];
        let mut classes = Map::default();
        classes
            .user
            .insert("com.example.notes.jot".to_owned(), foreign.clone());
        let registered = register_in(
            &mut classes,
            &NOTES,
            &["com.example.notes.txt"],
            &[
                FileType::new(".note", "Note"),
                JOT,
                FileType::new("TXT", "Text"),
                NOTE,
            ],
        );
        assert_eq!(
            registered,
            Registered {
                written: 2,
                refused: 3,
                ..Registered::default()
            },
            "the scheme and `note` are written"
        );
        assert_eq!(classes.user["com.example.notes.jot"], foreign);
        assert_eq!(
            classes.offered.keys().collect::<Vec<_>>(),
            [".note"],
            "only the written file type is offered"
        );

        let mut refusing = Map {
            refuse_offers: true,
            ..Map::default()
        };
        let registered = register_in(&mut refusing, &NOTES, &[], &[NOTE, JOT]);
        assert_eq!(registered.refused, 2);
        refusing.refuse_offers = false;
        let registered = register_in(&mut refusing, &NOTES, &[], &[NOTE, JOT]);
        assert_eq!(
            registered,
            Registered {
                current: 2,
                ..Registered::default()
            }
        );
        assert_eq!(
            refusing.offered.keys().collect::<Vec<_>>(),
            [".jot", ".note"],
            "the next run makes the refused offers"
        );
    }

    /// A file type an earlier version declared and this one does not is
    /// withdrawn from its extension's list and its ProgID deleted, and the
    /// shell hears about it; a withdrawal the registry refuses keeps the
    /// ProgID, so a later run still finds and removes it.
    #[test]
    fn a_file_type_no_longer_declared_is_withdrawn_and_removed() {
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &["notes"], &[NOTE, JOT]);
        let announced = classes.announced;
        let registered = register_in(&mut classes, &NOTES, &["notes"], &[NOTE]);
        assert_eq!(
            registered,
            Registered {
                current: 2,
                removed: 1,
                ..Registered::default()
            }
        );
        assert!(!classes.user.contains_key("com.example.notes.jot"));
        assert_eq!(
            classes.offered[".jot"],
            Vec::<String>::new(),
            "the offer is withdrawn"
        );
        assert_eq!(classes.offered[".note"], ["com.example.notes.note"]);
        assert_eq!(classes.announced, announced + 1);

        let _ = register_in(&mut classes, &NOTES, &[], &[NOTE, JOT]);
        classes.refuse_withdraw = true;
        let registered = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(registered.removed, 0);
        assert!(
            classes.user.contains_key("com.example.notes.jot"),
            "a ProgID still offered is kept"
        );
        classes.refuse_withdraw = false;
        let registered = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(registered.removed, 1);
        assert_eq!(classes.offered[".jot"], Vec::<String>::new());

        // The offer goes and the key stays: the shell still hears that the
        // extension's list changed.
        let _ = register_in(&mut classes, &NOTES, &[], &[NOTE, JOT]);
        classes.refuse_delete = Some("com.example.notes.jot");
        let announced = classes.announced;
        let registered = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(registered.removed, 0);
        assert_eq!(classes.offered[".jot"], Vec::<String>::new());
        assert_eq!(
            classes.announced,
            announced + 1,
            "a withdrawal whose key stays is announced"
        );
    }

    /// A key this application wrote for a scheme that it now declares as a
    /// file type's ProgID, or the reverse, is replaced whole: nothing of the
    /// other kind stays, and the shell hears of the offer that comes or goes.
    #[test]
    fn a_key_of_the_other_kind_is_replaced() {
        let prog_id = "com.example.notes.note";
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &[prog_id], &[]);
        assert_eq!(classes.user[prog_id], scheme_entries(&NOTES));

        let registered = register_in(&mut classes, &NOTES, &[], &[NOTE]);
        assert_eq!(
            registered,
            Registered {
                written: 1,
                ..Registered::default()
            },
            "a scheme's key is no current ProgID"
        );
        assert_eq!(
            classes.user[prog_id],
            file_type_entries(&NOTES, "note", "Notes note"),
            "no `URL Protocol` stays"
        );
        assert_eq!(classes.offered[".note"], [prog_id]);

        let announced = classes.announced;
        let registered = register_in(&mut classes, &NOTES, &[prog_id], &[]);
        assert_eq!(
            registered,
            Registered {
                written: 1,
                ..Registered::default()
            },
            "a ProgID is no current scheme's key"
        );
        assert_eq!(classes.user[prog_id], scheme_entries(&NOTES));
        assert_eq!(
            classes.offered[".note"],
            Vec::<String>::new(),
            "the offer is withdrawn"
        );
        assert_eq!(classes.announced, announced + 1);
    }

    /// Unregistering withdraws the application's offers and deletes its
    /// ProgIDs with its schemes' keys, and leaves another application's offer
    /// of the same extension.
    #[test]
    fn unregistering_withdraws_the_file_types_offers() {
        let mut classes = Map::default();
        let _ = register_in(&mut classes, &NOTES, &["notes"], &[NOTE]);
        classes
            .offered
            .get_mut(".note")
            .expect("offered")
            .push("com.example.other.note".to_owned());
        let announced = classes.announced;
        assert_eq!(
            unregister_in(&mut classes, "com.example.notes").ok(),
            Some(2)
        );
        assert!(classes.user.is_empty());
        assert_eq!(classes.offered[".note"], ["com.example.other.note"]);
        assert_eq!(classes.announced, announced + 1);
    }
}
