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
