//! What an Apple platform asks an application to open (the activation spec,
//! design §4): the `NSURL`s AppKit hands `application:openURLs:`, and on iOS
//! the URL contexts and web-browsing activities a scene connects with or
//! receives (`scene:openURLContexts:`, `scene:continueUserActivity:`). A
//! document comes as a `file:` URL and a link as any other.
//!
//! A document is not checked against the disk: the system opened it, and a
//! document package (`.rtfd`, a bundle) is a directory. The split into links
//! and documents is [`opened_by_urls`](crate::owner::file_uri::opened_by_urls),
//! which a Linux instance reads what a second one forwards with too.

/// The activity type of a web link the system hands an application, a
/// universal link: the value of Foundation's `NSUserActivityTypeBrowsingWeb`.
const BROWSING_WEB: &str = "NSUserActivityTypeBrowsingWeb";

/// The link a user activity of `activity_type` opens: its `webpage_url` when
/// it is a web browsing activity (a universal link), and nothing for any
/// other activity (Handoff of the application's own state, a Spotlight
/// item), which addresses no route.
#[cfg_attr(
    not(any(target_os = "ios", test)),
    expect(dead_code, reason = "only a UIKit scene continues a user activity")
)]
pub(crate) fn browsing_web_url(activity_type: &str, webpage_url: Option<String>) -> Option<String> {
    (activity_type == BROWSING_WEB)
        .then_some(webpage_url)
        .flatten()
}

/// The absolute strings of `urls`, in the platform's order: no more than an
/// [`Opened`] carries, and none longer than the longest it keeps, measured
/// before any is copied. What is dropped is logged.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn absolute_strings(
    urls: impl IntoIterator<Item = objc2::rc::Retained<objc2_foundation::NSURL>>,
) -> Vec<String> {
    let mut kept = Vec::new();
    let mut over = 0_usize;
    let mut too_long = 0_usize;
    for url in urls {
        if kept.len() == flui_platform::MAX_OPENED_ITEMS {
            over += 1;
            continue;
        }
        let Some(text) = url.absoluteString() else {
            continue;
        };
        if text.len() > flui_platform::MAX_OPENED_URL_LEN {
            too_long += 1;
        } else {
            kept.push(text.to_string());
        }
    }
    if over > 0 || too_long > 0 {
        tracing::warn!(
            over,
            too_long,
            "the platform opened more URLs, or longer ones, than the application hears; they \
             were dropped"
        );
    }
    kept
}

#[cfg(test)]
mod tests {
    use flui_platform::{Launch, OpenTarget, OpenedFile, WindowId};

    use super::browsing_web_url;
    use crate::owner::file_uri::{PathConvention, opened_by_urls};

    /// The platform's URLs split into links and documents in their order: a
    /// link of the application's scheme or the web is a link, a `file:` URL
    /// is a document by its decoded path whether it exists or not (a package
    /// is a directory), and one naming another host opens nothing; the
    /// target and launch are what the delegate saw.
    #[test]
    fn apple_urls_split_into_links_and_documents() {
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
            OpenTarget::Application,
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

        // A scene's URLs address its window.
        let window = WindowId::try_from_raw(7).expect("a nonzero id");
        let scene = opened_by_urls(
            ["notes:///note/5".to_owned()],
            PathConvention::Unix,
            OpenTarget::Window(window),
            Launch::Running,
        );
        assert_eq!(scene.target, OpenTarget::Window(window));
        assert_eq!(scene.launch, Launch::Running);
        assert!(
            opened_by_urls(
                Vec::new(),
                PathConvention::Unix,
                OpenTarget::Application,
                Launch::Running
            )
            .is_empty()
        );
    }

    /// A web browsing activity opens its page's URL; any other activity, and
    /// a browsing one without a page, opens nothing.
    #[test]
    fn a_user_activity_opens_only_a_web_page() {
        let page = || Some("https://notes.example.com/note/3".to_owned());
        assert_eq!(
            browsing_web_url("NSUserActivityTypeBrowsingWeb", page()).as_deref(),
            Some("https://notes.example.com/note/3")
        );
        assert_eq!(
            browsing_web_url("NSUserActivityTypeBrowsingWeb", None),
            None
        );
        assert_eq!(browsing_web_url("com.example.notes.editing", page()), None);
    }
}
