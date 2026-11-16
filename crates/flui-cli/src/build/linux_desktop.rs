//! What a Linux desktop knows the application by (ADR-0307 §4): a desktop
//! entry named by the application's identifier, which names it again as
//! `StartupWMClass` and whose `MimeType` names each URL scheme as
//! `x-scheme-handler/<scheme>` and each file type's MIME type, and a
//! shared-mime-info package that gives each file type its extension.
//!
//! The identifier is the one the application is compiled with
//! (`FLUI_APP_ID`), which its windows carry as their Wayland app id and their
//! X11 class: the shell matches a window to this entry by it, so the dock
//! shows the entry's name and groups the window under its launcher.
//!
//! `flui build linux` writes them under the output directory at the paths a
//! package installs them to below its `share` prefix
//! (`share/applications/<id>.desktop`, `share/mime/packages/<id>.xml`);
//! installing them, and running `update-desktop-database` and
//! `update-mime-database`, is the package's. The entry's `Exec` passes what
//! is opened as URIs (`%U`), so files arrive as `file:` URIs.

use std::path::{Path, PathBuf};

use crate::build::platform::AppBundle;
use crate::build::util::xml::xml_escape;
use crate::declarations::Declarations;

/// The desktop entry for `bundle`, whose executable is `executable`, and the
/// MIME package when `declarations` gives the application a file type,
/// written under `output_dir`. The paths written.
///
/// # Errors
///
/// The failure to create a directory or write a file.
pub(crate) fn stage(
    output_dir: &Path,
    bundle: &AppBundle,
    executable: &str,
    declarations: &Declarations,
) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    let applications = output_dir.join("share").join("applications");
    std::fs::create_dir_all(&applications)?;
    let entry_path = applications.join(format!("{}.desktop", bundle.identifier));
    std::fs::write(&entry_path, desktop_entry(bundle, executable, declarations))?;
    written.push(entry_path);
    if let Some(package) = mime_package(declarations) {
        let packages = output_dir.join("share").join("mime").join("packages");
        std::fs::create_dir_all(&packages)?;
        let package_path = packages.join(format!("{}.xml", bundle.identifier));
        std::fs::write(&package_path, package)?;
        written.push(package_path);
    }
    Ok(written)
}

/// The desktop entry: its `MimeType` line only when the application opens a
/// URL scheme or a file type.
fn desktop_entry(bundle: &AppBundle, executable: &str, declarations: &Declarations) -> String {
    let mut mime_types: Vec<String> = declarations
        .url_schemes()
        .map(|(scheme, _)| format!("x-scheme-handler/{scheme}"))
        .collect();
    mime_types.extend(
        declarations
            .file_types()
            .map(|(file_type, _)| file_type.mime().to_owned()),
    );
    let mut entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nTerminal=false\n\
         StartupWMClass={class}\n",
        name = string_value(&bundle.name),
        exec = string_value(&format!("{} %U", exec_argument(executable))),
        class = string_value(&bundle.identifier),
    );
    if !mime_types.is_empty() {
        entry.push_str("MimeType=");
        for mime_type in mime_types {
            entry.push_str(&mime_type);
            entry.push(';');
        }
        entry.push('\n');
    }
    entry
}

/// The shared-mime-info package giving each file type its extension, or
/// `None` when the application opens none.
fn mime_package(declarations: &Declarations) -> Option<String> {
    let types: Vec<String> = declarations
        .file_types()
        .map(|(file_type, _)| {
            format!(
                "  <mime-type type=\"{mime}\">\n    <comment>{name}</comment>\n    \
                 <glob pattern=\"*.{extension}\"/>\n  </mime-type>\n",
                mime = xml_escape(file_type.mime()),
                name = xml_escape(file_type.name()),
                extension = xml_escape(file_type.extension()),
            )
        })
        .collect();
    if types.is_empty() {
        return None;
    }
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n{}\
         </mime-info>\n",
        types.concat()
    ))
}

/// `value` as a desktop-entry string value: a backslash, a newline, a tab
/// and a carriage return are escaped, so a value is one line.
fn string_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// `argument` as one argument of an `Exec` key: quoted when it holds a
/// character the key reserves, with `"`, `` ` ``, `$` and `\` escaped inside
/// the quotes, and `%` doubled, since a single one starts a field code.
fn exec_argument(argument: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let argument = argument.replace('%', "%%");
    if !argument.contains(RESERVED) {
        return argument;
    }
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    for ch in argument.chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{exec_argument, stage, string_value};
    use crate::build::platform::AppBundle;
    use crate::declarations::Declarations;

    /// A Linux desktop registers each scheme and file type in the desktop
    /// entry's `MimeType`, gives each file type its extension in a MIME
    /// package, and gets nothing to install when the application opens only
    /// web links (an associated domain has no Linux registration).
    #[test]
    fn a_linux_desktop_registers_what_the_application_opens() {
        let declarations = Declarations::of_table(
            "notes",
            "schema = 3\nurl-schemes = [\"notes\"]\nassociated-domains = [\"notes.example.com\"]\n\
             [[file-types]]\nextension = \"note\"\nmime = \"application/x-notes-note\"\n\
             name = \"Note & Sketch\"\n",
        );
        let bundle = AppBundle::new("Notes", "com.example.notes".to_owned());
        let output = tempfile::tempdir().expect("a temporary directory");
        let written =
            stage(output.path(), &bundle, "notes", &declarations).expect("the files are written");
        // Compared as paths, so the host's separator does not matter.
        let relative: Vec<&Path> = written
            .iter()
            .map(|path| path.strip_prefix(output.path()).expect("under the output"))
            .collect();
        assert_eq!(
            relative,
            [
                Path::new("share/applications/com.example.notes.desktop"),
                Path::new("share/mime/packages/com.example.notes.xml"),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&written[0]).expect("the entry"),
            "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes %U\nTerminal=false\n\
             StartupWMClass=com.example.notes\n\
             MimeType=x-scheme-handler/notes;application/x-notes-note;\n"
        );
        let package = std::fs::read_to_string(&written[1]).expect("the package");
        assert!(
            package.contains(
                "<mime-type type=\"application/x-notes-note\">\n    \
                 <comment>Note &amp; Sketch</comment>\n    <glob pattern=\"*.note\"/>"
            ),
            "{package}"
        );
        roxmltree::Document::parse(&package).expect("the package is XML");

        let web_only = Declarations::of_table(
            "notes",
            "schema = 3\nassociated-domains = [\"a.example\"]\n",
        );
        let entry_only = tempfile::tempdir().expect("a temporary directory");
        let written =
            stage(entry_only.path(), &bundle, "notes", &web_only).expect("the entry is written");
        assert_eq!(
            written,
            [entry_only
                .path()
                .join("share")
                .join("applications")
                .join("com.example.notes.desktop")],
            "an associated domain has no Linux registration: the entry alone, for the identity"
        );
        assert_eq!(
            std::fs::read_to_string(&written[0]).expect("the entry"),
            "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes %U\nTerminal=false\n\
             StartupWMClass=com.example.notes\n",
            "no MimeType for an application that opens nothing"
        );
        assert!(!entry_only.path().join("share").join("mime").exists());
    }

    /// One identifier names the desktop entry's file, is its `StartupWMClass`
    /// and is what the application is compiled with (`FLUI_APP_ID`), which
    /// its windows carry as their Wayland app id and X11 class: the shell
    /// matches the window to the entry, whether the identifier comes from
    /// `flui.toml` or, without one, from the application's directory.
    #[test]
    fn the_entry_and_the_window_are_named_by_one_identifier() {
        let configured = crate::app_id::AppId::try_new("com.example.notes").expect("an id");
        let unconfigured = crate::app_id::AppId::unconfigured("my notes");
        for app_id in [configured, unconfigured] {
            let bundle = AppBundle::new("Notes", app_id.desktop_entry_id());
            let compiled = crate::build::desktop::DesktopBuilder::compiled_identity(
                "x86_64-unknown-linux-gnu",
                Some(&bundle),
            );
            assert_eq!(
                compiled,
                Some((crate::config::APP_ID_ENV, app_id.as_str().to_owned())),
                "{app_id}: the Linux build compiles the identifier in"
            );
            let output = tempfile::tempdir().expect("a temporary directory");
            let written = stage(output.path(), &bundle, "notes", &Declarations::default())
                .expect("the entry is written");
            let entry_name = written[0]
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a UTF-8 file name");
            assert_eq!(entry_name, format!("{app_id}.desktop"), "{app_id}");
            let entry = std::fs::read_to_string(&written[0]).expect("the entry");
            let class = entry
                .lines()
                .find_map(|line| line.strip_prefix("StartupWMClass="))
                .expect("a StartupWMClass");
            assert_eq!(class, app_id.as_str(), "{app_id}");
        }
        assert_eq!(
            crate::build::desktop::DesktopBuilder::compiled_identity(
                "x86_64-pc-windows-msvc",
                Some(&AppBundle::new("Notes", "com.example.notes".to_owned()))
            ),
            None,
            "only a Linux build compiles the entry's identifier in"
        );
    }

    /// A name and an executable are each one value of their key, whatever
    /// they hold.
    #[test]
    fn desktop_entry_values_are_escaped() {
        assert_eq!(string_value("Notes\nExec=evil"), "Notes\\nExec=evil");
        assert_eq!(string_value("a\\b"), "a\\\\b");
        assert_eq!(exec_argument("notes"), "notes");
        assert_eq!(exec_argument("100%"), "100%%");
        assert_eq!(exec_argument("my notes"), "\"my notes\"");
        assert_eq!(exec_argument("a$b\"c"), "\"a\\$b\\\"c\"");
    }
}
