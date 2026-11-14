//! What AppKit asks an application to open: the `NSURL`s
//! `application:openURLs:` hands it (the activation spec, design §4), a
//! document as a `file:` URL and a link as any other. AppKit calls it before
//! `applicationDidFinishLaunching:` with what launched the application, and
//! while it runs with what reaches it later.
//!
//! A document is not checked against the disk: the system opened it, and a
//! document package (`.rtfd`, a bundle) is a directory.

use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};

use crate::owner::file_uri::{PathConvention, file_uri_path};

/// What `urls`, the absolute strings of the `NSURL`s AppKit handed over, in
/// its order, open for the application as a whole: each `file:` URL of this
/// host (`convention`, Unix on every Apple platform) is a document, any
/// other URL a link. A `file:` URL that names no local path (another host,
/// a malformed escape) opens nothing.
pub(crate) fn opened_by_urls(
    urls: impl IntoIterator<Item = String>,
    convention: PathConvention,
    launch: Launch,
) -> Opened {
    let mut links = Vec::new();
    let mut files = Vec::new();
    for url in urls {
        let is_file = url
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"));
        if !is_file {
            links.push(url);
        } else if let Some(path) = file_uri_path(&url, convention) {
            files.push(OpenedFile::new(path));
        } else {
            tracing::debug!("a file URL AppKit opened names no path on this host");
        }
    }
    Opened::new(links, files, OpenTarget::Application, launch)
}

#[cfg(test)]
mod tests {
    use flui_platform::{Launch, OpenTarget, OpenedFile};

    use super::opened_by_urls;
    use crate::owner::file_uri::PathConvention;

    /// AppKit's URLs split into links and documents in their order: a link
    /// of the application's scheme or the web is a link, a `file:` URL is a
    /// document by its decoded path whether it exists or not (a package is a
    /// directory), and one naming another host opens nothing; the launch is
    /// what the delegate saw.
    #[test]
    fn appkit_urls_split_into_links_and_documents() {
        let opened = opened_by_urls(
            [
                "notes:///note/3",
                "file:///Users/ana/Notes/a%20b.note",
                "https://notes.example.com/note/4?ref=mail",
                "file:///Users/ana/Notes/Plan.rtfd/",
                "file://other-host/Users/ana/x.note",
                "FILE:///Users/ana/upper.note",
            ]
            .map(String::from),
            PathConvention::Unix,
            Launch::Cold,
        );
        assert_eq!(
            opened.urls,
            [
                "notes:///note/3",
                "https://notes.example.com/note/4?ref=mail"
            ]
        );
        assert_eq!(
            opened.files,
            [
                OpenedFile::new("/Users/ana/Notes/a b.note"),
                OpenedFile::new("/Users/ana/Notes/Plan.rtfd/"),
                OpenedFile::new("/Users/ana/upper.note"),
            ]
        );
        assert_eq!(opened.target, OpenTarget::Application);
        assert_eq!(opened.launch, Launch::Cold);

        let running = opened_by_urls(
            ["notes:///note/5".to_owned()],
            PathConvention::Unix,
            Launch::Running,
        );
        assert_eq!(running.launch, Launch::Running);
        assert!(opened_by_urls(Vec::new(), PathConvention::Unix, Launch::Running).is_empty());
    }
}
