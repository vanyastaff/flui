//! What the operating system opens an application with is bounded once, for
//! every host: the items past `MAX_OPENED_ITEMS` and a URL past
//! `MAX_OPENED_URL_LEN` bytes are dropped before anything parses them.

use flui_platform::{Launch, MAX_OPENED_ITEMS, MAX_OPENED_URL_LEN, OpenTarget, Opened, OpenedFile};

fn url(n: usize) -> String {
    format!("myapp:///note/{n}")
}

#[test]
fn opened_items_and_urls_are_bounded_before_they_are_read() {
    // The 65th item is dropped, files counting after URLs.
    let opened = Opened::new(
        (0..MAX_OPENED_ITEMS - 1).map(url),
        ["/a.note", "/b.note"].map(OpenedFile::new),
        OpenTarget::Application,
        Launch::Cold,
    );
    assert_eq!(opened.urls.len(), MAX_OPENED_ITEMS - 1);
    assert_eq!(opened.files, [OpenedFile::new("/a.note")]);

    let urls = (0..=MAX_OPENED_ITEMS).map(url).collect::<Vec<_>>();
    let opened = Opened::new(urls.clone(), [], OpenTarget::Application, Launch::Running);
    assert_eq!(
        opened.urls,
        urls[..MAX_OPENED_ITEMS],
        "the first 64, in order"
    );

    // A URL of the bound is kept; one byte more drops it whole, and it
    // takes no item's place.
    let prefix = "myapp:///";
    let fits = format!("{prefix}{}", "a".repeat(MAX_OPENED_URL_LEN - prefix.len()));
    let over = format!("{fits}a");
    let opened = Opened::new(
        [over, fits.clone(), url(1)],
        [],
        OpenTarget::Application,
        Launch::Running,
    );
    assert_eq!(opened.urls, [fits, url(1)]);

    assert!(Opened::new([], [], OpenTarget::Application, Launch::Running).is_empty());
}

/// A file is called by its file name; a held descriptor's file by the name
/// it came with, readable at its path while a clone lives and closed with the
/// last one, and duplicated so that a started process does not inherit it.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn a_held_descriptor_stays_open_until_its_last_file_drops() {
    use std::io::Read as _;
    use std::os::fd::{AsRawFd as _, OwnedFd};
    use std::os::unix::fs::MetadataExt as _;

    assert_eq!(OpenedFile::new("/notes/a b.note").name(), Some("a b.note"));
    assert_eq!(OpenedFile::new("/").name(), None);

    let source = std::env::temp_dir().join(format!("flui-opened-{}.note", std::process::id()));
    std::fs::write(&source, b"note 3").expect("a scratch file");
    let identity = |path: &std::path::Path| {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()))
    };
    let expected = identity(&source);

    let descriptor = OwnedFd::from(std::fs::File::open(&source).expect("open the scratch file"));
    let passed = descriptor.as_raw_fd();
    let file = OpenedFile::try_from_descriptor(descriptor, Some("Note 3.note".to_owned()))
        .expect("a duplicate descriptor");
    assert_eq!(file.name(), Some("Note 3.note"));
    assert_ne!(
        file.path,
        std::path::PathBuf::from(format!("/proc/self/fd/{passed}")),
        "the descriptor passed in is replaced by its close-on-exec duplicate"
    );
    let clone = file.clone();
    assert_eq!(clone, file);
    drop(file);
    let mut text = String::new();
    std::fs::File::open(&clone.path)
        .and_then(|mut opened| opened.read_to_string(&mut text))
        .expect("the clone holds the descriptor open");
    assert_eq!(text, "note 3");

    let path = clone.path.clone();
    assert_eq!(identity(&path), expected);
    drop(clone);
    // Closed: the number names nothing, or a file another thread opened since.
    assert_ne!(
        identity(&path),
        expected,
        "the last clone closes the descriptor"
    );
    std::fs::remove_file(&source).expect("remove the scratch file");
}
