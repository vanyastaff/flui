//! What a Linux desktop registers about what the application opens
//! (ADR-0307 §4): a desktop entry whose `MimeType` names each URL scheme as
//! `x-scheme-handler/<scheme>` and each file type's MIME type, and a
//! shared-mime-info package that gives each file type its extension.
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

/// The desktop entry and MIME package for `bundle`, whose executable is
/// `executable`, written under `output_dir` when `declarations` gives the
/// application something a Linux desktop registers: a URL scheme or a file
/// type. The paths written, none when there is nothing to register.
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
    let Some(entry) = desktop_entry(bundle, executable, declarations) else {
        return Ok(Vec::new());
    };
    let mut written = Vec::new();
    let applications = output_dir.join("share").join("applications");
    std::fs::create_dir_all(&applications)?;
    let entry_path = applications.join(format!("{}.desktop", bundle.identifier));
    std::fs::write(&entry_path, entry)?;
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

/// The desktop entry, or `None` when the application opens no URL scheme
/// and no file type.
fn desktop_entry(
    bundle: &AppBundle,
    executable: &str,
    declarations: &Declarations,
) -> Option<String> {
    let mut mime_types: Vec<String> = declarations
        .url_schemes()
        .map(|(scheme, _)| format!("x-scheme-handler/{scheme}"))
        .collect();
    mime_types.extend(
        declarations
            .file_types()
            .map(|(file_type, _)| file_type.mime().to_owned()),
    );
    if mime_types.is_empty() {
        return None;
    }
    let mut mime_type = mime_types.join(";");
    mime_type.push(';');
    Some(format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nTerminal=false\n\
         MimeType={mime_type}\n",
        name = string_value(&bundle.name),
        exec = string_value(&format!("{} %U", exec_argument(executable))),
    ))
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
        let relative: Vec<String> = written
            .iter()
            .map(|path| {
                path.strip_prefix(output.path())
                    .expect("under the output")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            relative,
            [
                "share/applications/com.example.notes.desktop",
                "share/mime/packages/com.example.notes.xml",
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&written[0]).expect("the entry"),
            "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes %U\nTerminal=false\n\
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
        let nothing = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(
            stage(nothing.path(), &bundle, "notes", &web_only).expect("nothing fails"),
            Vec::<std::path::PathBuf>::new(),
            "an associated domain has no Linux registration"
        );
        assert!(!nothing.path().join("share").exists());
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
