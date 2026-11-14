//! What an intent the Activity receives opens (the activation spec, design
//! §4): the intent that launched it (`onCreate`) and each one that reaches it
//! running (`onNewIntent`, the template's `launchMode="singleTop"`), as
//! `FluiHost.record` lays them out on the Java side.
//!
//! A `VIEW` intent's data is a link (a custom scheme, a verified `https` app
//! link) or a document; a share (`SEND`, `SEND_MULTIPLE`) carries documents
//! as its streams. A document comes as a `file:` URI, opened by its path, or
//! as a `content:` URI a provider serves, which names no path: the owner
//! opens it for reading through the content resolver and the application
//! hears the open descriptor, held by its `OpenedFile` (`/proc/self/fd/N`)
//! with the name the provider shows.

use std::path::PathBuf;

use flui_platform::{Launch, MAX_OPENED_ITEMS, OpenTarget, Opened, OpenedFile};

use crate::owner::file_uri::{PathConvention, file_uri_path};

/// `Intent.ACTION_VIEW`.
const ACTION_VIEW: &str = "android.intent.action.VIEW";
/// `Intent.ACTION_SEND`.
const ACTION_SEND: &str = "android.intent.action.SEND";
/// `Intent.ACTION_SEND_MULTIPLE`.
const ACTION_SEND_MULTIPLE: &str = "android.intent.action.SEND_MULTIPLE";
/// `FluiHost.COLD`: the intent started the Activity.
pub(crate) const COLD: &str = "cold";
/// `FluiHost.RUNNING`: the intent reached the running Activity.
pub(crate) const RUNNING: &str = "running";

/// An intent as `FluiHost` queued it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceivedIntent {
    /// Whether it started the Activity.
    pub(crate) launch: Launch,
    /// Its action, empty for none.
    pub(crate) action: String,
    /// Its data, then each stream of a share, in the sender's order.
    pub(crate) uris: Vec<String>,
}

impl ReceivedIntent {
    /// The intent `fields` records: its launch, its action, then its URIs.
    /// A record without a known launch and an action is a defect of one side
    /// (the handshake rules out another layout), logged and skipped.
    pub(crate) fn from_record(fields: Vec<String>) -> Option<Self> {
        let mut fields = fields.into_iter();
        let launch = match fields.next().as_deref() {
            Some(COLD) => Launch::Cold,
            Some(RUNNING) => Launch::Running,
            launch => {
                tracing::warn!(
                    target: "flui_native::android",
                    ?launch,
                    "FluiHost queued an intent with a launch this library does not know"
                );
                return None;
            }
        };
        let action = fields.next()?;
        Some(Self {
            launch,
            action,
            uris: fields.collect(),
        })
    }
}

/// A document an intent opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Document {
    /// A `file:` URI's path.
    Path(PathBuf),
    /// A `content:` URI, which a provider serves and names no path.
    Content(String),
}

/// What an intent opens, before its provider's documents are opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opening {
    /// Its links, in order.
    pub(crate) links: Vec<String>,
    /// Its documents, in order.
    pub(crate) documents: Vec<Document>,
    /// Whether it started the Activity.
    pub(crate) launch: Launch,
}

impl Opening {
    /// What `intent` opens. A `VIEW` intent's data is a link unless it is a
    /// document; a share's streams are documents; any other action (the
    /// launcher's `MAIN`) opens nothing. A `file:` URI naming no local path
    /// and a share's stream that is not a document are logged.
    pub(crate) fn of(intent: ReceivedIntent) -> Self {
        let mut opening = Self {
            links: Vec::new(),
            documents: Vec::new(),
            launch: intent.launch,
        };
        let documents_only = match intent.action.as_str() {
            ACTION_VIEW => false,
            ACTION_SEND | ACTION_SEND_MULTIPLE => true,
            _ => return opening,
        };
        let mut unopened = 0_usize;
        for uri in intent.uris {
            match scheme(&uri) {
                Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
                    match file_uri_path(&uri, PathConvention::HOST) {
                        Some(path) => opening.documents.push(Document::Path(path)),
                        None => unopened += 1,
                    }
                }
                Some(scheme) if scheme.eq_ignore_ascii_case("content") => {
                    opening.documents.push(Document::Content(uri));
                }
                Some(_) if !documents_only => opening.links.push(uri),
                _ => unopened += 1,
            }
        }
        if unopened > 0 {
            tracing::debug!(
                target: "flui_native::android",
                unopened,
                "an intent's documents that name no local path and no provider are not opened"
            );
        }
        opening
    }

    /// What the application hears, for the application as a whole: Android
    /// keeps one task of the Activity, and its window's id changes with each
    /// surface, so the runner's last focused window is the one the intent
    /// reached. Each provider's document is opened by `open` (the content
    /// resolver, on the owner thread), at most as many as an [`Opened`]
    /// carries; one it cannot open is left out, and `open` logs why.
    pub(crate) fn opened(self, mut open: impl FnMut(&str) -> Option<OpenedFile>) -> Opened {
        let room = MAX_OPENED_ITEMS.saturating_sub(self.links.len());
        let files: Vec<OpenedFile> = self
            .documents
            .into_iter()
            .take(room)
            .filter_map(|document| match document {
                Document::Path(path) => Some(OpenedFile::new(path)),
                Document::Content(uri) => open(&uri),
            })
            .collect();
        Opened::new(self.links, files, OpenTarget::Application, self.launch)
    }
}

/// The scheme of `uri` (RFC 3986 §3.1), `None` when it has none.
fn scheme(uri: &str) -> Option<&str> {
    let (scheme, _) = uri.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then_some(scheme)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_platform::{Launch, MAX_OPENED_ITEMS, OpenTarget, OpenedFile};

    use super::{Document, Opening, ReceivedIntent};

    fn record(fields: &[&str]) -> Vec<String> {
        fields.iter().map(|field| (*field).to_owned()).collect()
    }

    fn opening(fields: &[&str]) -> Opening {
        Opening::of(
            ReceivedIntent::from_record(record(fields)).expect("a record FluiHost lays out"),
        )
    }

    /// A `VIEW` intent's data opens as a link, a custom scheme's or a
    /// verified web link, cold from `onCreate` and running from
    /// `onNewIntent`; a `file:` URI is a document by its path and a
    /// `content:` one a document its provider serves; a share's streams are
    /// its documents in the sender's order; a stream that is not a document,
    /// the launcher's `MAIN` and no action open nothing.
    #[test]
    fn an_intent_opens_its_link_or_documents() {
        assert_eq!(
            opening(&["cold", "android.intent.action.VIEW", "notes:///note/3"]),
            Opening {
                links: vec!["notes:///note/3".to_owned()],
                documents: Vec::new(),
                launch: Launch::Cold,
            }
        );
        let running = opening(&[
            "running",
            "android.intent.action.VIEW",
            "https://notes.example.com/note/4?ref=mail",
        ]);
        assert_eq!(running.links, ["https://notes.example.com/note/4?ref=mail"]);
        assert_eq!(running.launch, Launch::Running);
        assert_eq!(
            opening(&[
                "running",
                "android.intent.action.VIEW",
                "file:///sdcard/Notes/a%20b.note"
            ])
            .documents,
            [Document::Path(PathBuf::from("/sdcard/Notes/a b.note"))]
        );
        assert_eq!(
            opening(&[
                "cold",
                "android.intent.action.VIEW",
                "content://com.example.notes/doc/3"
            ])
            .documents,
            [Document::Content(
                "content://com.example.notes/doc/3".to_owned()
            )]
        );
        assert_eq!(
            opening(&[
                "running",
                "android.intent.action.SEND_MULTIPLE",
                "file:///sdcard/a.note",
                "content://com.example.notes/doc/3",
                "https://not-a-document.example.com",
                "file://other-host/b.note",
            ]),
            Opening {
                links: Vec::new(),
                documents: vec![
                    Document::Path(PathBuf::from("/sdcard/a.note")),
                    Document::Content("content://com.example.notes/doc/3".to_owned()),
                ],
                launch: Launch::Running,
            }
        );
        for fields in [
            &["cold", "android.intent.action.MAIN"][..],
            &["cold", "", "notes:///note/3"],
        ] {
            let nothing = opening(fields);
            assert!(
                nothing.links.is_empty() && nothing.documents.is_empty(),
                "{fields:?}"
            );
        }
    }

    /// The application hears an intent's links and documents for the
    /// application as a whole: a provider's document as the content resolver
    /// opened it, one it could not open left out, and no more opened than an
    /// `Opened` carries beside the links.
    #[test]
    fn a_provider_document_opens_through_the_resolver_or_is_left_out() {
        let mut asked = Vec::new();
        let opened = Opening {
            links: vec!["notes:///note/3".to_owned()],
            documents: vec![
                Document::Content("content://notes/doc/1".to_owned()),
                Document::Path(PathBuf::from("/sdcard/a.note")),
                Document::Content("content://notes/doc/refused".to_owned()),
            ],
            launch: Launch::Running,
        }
        .opened(|uri| {
            asked.push(uri.to_owned());
            (!uri.ends_with("refused")).then(|| OpenedFile::new("/proc/self/fd/40"))
        });
        assert_eq!(opened.urls, ["notes:///note/3"]);
        assert_eq!(
            opened.files,
            [
                OpenedFile::new("/proc/self/fd/40"),
                OpenedFile::new("/sdcard/a.note")
            ]
        );
        assert_eq!(opened.target, OpenTarget::Application);
        assert_eq!(opened.launch, Launch::Running);
        assert_eq!(
            asked,
            ["content://notes/doc/1", "content://notes/doc/refused"]
        );

        let mut opens = 0_usize;
        let full = Opening {
            links: (0..MAX_OPENED_ITEMS - 1)
                .map(|n| format!("notes:///note/{n}"))
                .collect(),
            documents: vec![Document::Content("content://notes/doc/1".to_owned()); 3],
            launch: Launch::Cold,
        }
        .opened(|_| {
            opens += 1;
            Some(OpenedFile::new("/proc/self/fd/41"))
        });
        assert_eq!(opens, 1, "a document past the bound is not opened");
        assert_eq!(full.files.len(), 1);
    }

    /// `FluiHost.record` lays out the launch, the action, then the URIs; a
    /// record with another launch or without an action is skipped.
    #[test]
    fn an_intent_record_is_read_as_the_java_classes_lay_it_out() {
        assert_eq!(
            ReceivedIntent::from_record(record(&["running", "", "notes:///a"])),
            Some(ReceivedIntent {
                launch: Launch::Running,
                action: String::new(),
                uris: vec!["notes:///a".to_owned()],
            })
        );
        assert_eq!(ReceivedIntent::from_record(record(&["warm", "x"])), None);
        assert_eq!(ReceivedIntent::from_record(record(&["cold"])), None);
        assert_eq!(ReceivedIntent::from_record(Vec::new()), None);
    }
}
