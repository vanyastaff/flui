//! Reading the documents the picker handed over.
//!
//! The picker opens a document in place: its URL names the user's own file
//! where it is (another application's container, iCloud Drive, a file
//! provider's storage), and the application may reach it only while it
//! holds the URL's security-scoped access. The access is started before the
//! document is judged and read and stopped after (`ScopedAccess`), and the
//! read is coordinated (`NSFileCoordinator`), which has a file provider
//! that keeps the file only in the cloud download it first; all of it runs
//! off the main thread (ADR-0360 §2).
//!
//! The access lasts for the read alone, so a pick is handed over as its
//! name and contents (`PickedForm::Contents`), never as a path the
//! application could not open once the access stopped. A request that keeps
//! access has each pick carry a bookmark, made while the access is held,
//! which `Grants::resolve` turns into the document again after a restart,
//! its access started for as long as the reacquired access lives.
//!
//! Nothing is read, and nothing downloaded, for an answer the request will
//! not get. Every pick is first judged by the request's filters
//! (`apple::content_types`), and one they do not mean refuses the whole
//! answer. Then the sizes the system declares for the picks, which it
//! knows of a document still in the cloud, are summed against the
//! request's `max_bytes` ([`admit_sizes`]), before any read is coordinated:
//! a coordinated read is what downloads. Each read is bounded again by
//! what is left of the bound ([`ReadBudget`]): the size the opened file
//! reports is checked before a byte of it is read, and no more than that
//! size is read.

use std::sync::Arc;

use flui_platform::SystemFailure;

use crate::apple::content_types::Judged;
use crate::apple::{failure, io_code};
use crate::dialog::{DialogError, PickedFile, admit_read};

/// Whether picks of these `sizes`, each as the system declares it (`None`
/// where it declares none), fit `limit` bytes together: what is checked
/// before any pick is read or downloaded. A pick of no declared size
/// counts for nothing here, and is held to the bound when it is read.
pub(super) fn admit_sizes(
    sizes: impl IntoIterator<Item = Option<u64>>,
    limit: u64,
) -> Result<(), DialogError> {
    let declared = sizes.into_iter().flatten().fold(0_u64, u64::saturating_add);
    admit_read(declared, limit)
}

/// How many bytes the picks of one request may still hold: the request's
/// `max_bytes`, less what was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReadBudget {
    limit: u64,
    read: u64,
}

impl ReadBudget {
    /// A budget of `limit` bytes, nothing read.
    pub(super) const fn new(limit: u64) -> Self {
        Self { limit, read: 0 }
    }

    /// Whether a document of `size` bytes, as its opened file reports it,
    /// fits what is left; nothing is charged.
    fn admit(&self, size: u64) -> Result<(), DialogError> {
        admit_read(self.read.saturating_add(size), self.limit)
    }

    /// Charge the `read` bytes of a document that was admitted: no more
    /// than the size it was admitted by, so the budget is never overdrawn.
    fn charge(&mut self, read: u64) {
        self.read = self.read.saturating_add(read);
    }
}

/// Read what `file` holds, a document that reports `declared` bytes, within
/// `budget`, charged to it.
///
/// The declared size is refused past the budget before a byte is read, and
/// at most that many bytes are read, into one buffer of that size: a
/// document that grew since is cut at the size it declared, and one that
/// shrank answers what it holds now.
pub(super) fn read_within(
    file: &mut impl std::io::Read,
    declared: u64,
    budget: &mut ReadBudget,
) -> Result<Arc<[u8]>, DialogError> {
    budget.admit(declared)?;
    let length = usize::try_from(declared).map_err(|_| DialogError::TooLarge {
        actual: declared,
        limit: budget.limit,
    })?;
    let mut contents: Arc<[u8]> = std::iter::repeat_n(0_u8, length).collect();
    let buffer =
        Arc::get_mut(&mut contents).expect("BUG: a buffer just made has no other owner yet");
    let mut filled = 0;
    while filled < length {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_failure(&error)),
        }
    }
    budget.charge(u64::try_from(filled).unwrap_or(u64::MAX));
    if filled < length {
        contents = Arc::from(&contents[..filled]);
    }
    Ok(contents)
}

/// The failure a file system call of a dialog reports, by its code.
pub(super) fn io_failure(error: &std::io::Error) -> DialogError {
    DialogError::Failed(failure(io_code(error)))
}

/// The pick named `name` holding `contents`; a name that is no file name is
/// an answer outside the system's contract.
pub(super) fn picked(name: &str, contents: Arc<[u8]>) -> Result<PickedFile, DialogError> {
    PickedFile::try_from_contents(name, contents)
        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
}

/// Whether the picks, each as the contract's rule judged it, are what the
/// request asked for: one the rule refused refuses them all with
/// [`DialogError::NotInFilter`]. A pick whose type could not be read is
/// not refused for that.
pub(super) fn admit_judged(judged: impl IntoIterator<Item = Judged>) -> Result<(), DialogError> {
    if judged.into_iter().any(|judged| judged == Judged::Refused) {
        Err(DialogError::NotInFilter)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "ios")]
pub(super) use foundation::read_picks;

#[cfg(target_os = "ios")]
#[expect(
    unsafe_code,
    reason = "security-scoped access, Foundation's resource keys and the coordinator's reader"
)]
mod foundation {
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::Arc;

    use block2::RcBlock;
    use flui_platform::SystemFailure;
    use objc2::rc::Retained;
    use objc2_foundation::{
        NSArray, NSError, NSFileCoordinator, NSFileCoordinatorReadingOptions, NSNumber, NSURL,
    };

    use super::{ReadBudget, admit_judged, admit_sizes, io_failure, picked, read_within};
    use crate::apple::bookmark::bookmark;
    use crate::apple::content_types::{ContentFilter, HasContentTypes, Judged, NameRule};
    use crate::apple::{error_code, failure, url_path};
    use crate::dialog::{DialogError, OpenResult, PickedFile, Picks};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// A picked document's security-scoped access, held while this lives:
    /// started when it is made, stopped when it drops. A URL in the
    /// application's own container needs no access, and stops none.
    struct ScopedAccess {
        url: Retained<NSURL>,
        started: bool,
    }

    impl ScopedAccess {
        /// Start the access `url` grants.
        fn start(url: Retained<NSURL>) -> Self {
            // SAFETY: a method of a URL that takes nothing; the access it
            // starts is stopped once, when this value drops.
            let started = unsafe { url.startAccessingSecurityScopedResource() };
            Self { url, started }
        }

        /// What `rule` makes of the document, read while the access is
        /// held.
        fn judge(&self, rule: &NameRule) -> Judged {
            rule.judge(&self.url, || rule.type_conforms(&self.url))
        }

        /// The size the system declares for the document, without reading
        /// or downloading it: its logical size, which a file provider knows
        /// of a document it keeps in the cloud, or else what it occupies on
        /// the volume. `None` when the system says neither.
        fn declared_size(&self) -> Option<u64> {
            // SAFETY: Foundation's resource key constants live for the
            // process; both are in every iOS FLUI supports.
            let (size_key, allocated_key) = unsafe {
                (
                    objc2_foundation::NSURLFileSizeKey,
                    objc2_foundation::NSURLTotalFileAllocatedSizeKey,
                )
            };
            let values = self
                .url
                .resourceValuesForKeys_error(&NSArray::from_slice(&[size_key, allocated_key]))
                .ok()?;
            [size_key, allocated_key].into_iter().find_map(|key| {
                values
                    .objectForKey(key)
                    .and_then(|size| size.downcast::<NSNumber>().ok())
                    .map(|size| size.unsignedLongLongValue())
            })
        }

        /// The pick this access hands out: the document's name and what it
        /// holds, read within `budget` and charged to it. Blocks while a
        /// file provider downloads the document.
        fn pick(&self, budget: &mut ReadBudget) -> Result<PickedFile, DialogError> {
            let name = self
                .url
                .lastPathComponent()
                .map(|name| name.to_string())
                .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
            let coordinator = NSFileCoordinator::new();
            let read = RefCell::new(None::<Result<Arc<[u8]>, DialogError>>);
            let budget = RefCell::new(budget);
            let reader = RcBlock::new(|url: NonNull<NSURL>| {
                os_entry(
                    "coordinateReadingItemAtURL reader",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || {
                        let contents = read_at(url, &mut budget.borrow_mut());
                        *read.borrow_mut() = Some(contents);
                    },
                );
            });
            let mut error: Option<Retained<NSError>> = None;
            coordinator.coordinateReadingItemAtURL_options_error_byAccessor(
                &self.url,
                NSFileCoordinatorReadingOptions::empty(),
                Some(&mut error),
                &reader,
            );
            drop(reader);
            match (read.into_inner(), error) {
                (Some(contents), _) => picked(&name, contents?),
                (None, Some(error)) => Err(DialogError::Failed(failure(error_code(&error)))),
                // The reader never ran, or panicked, and the coordinator
                // named no error.
                (None, None) => Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            }
        }
    }

    impl Drop for ScopedAccess {
        fn drop(&mut self) {
            if self.started {
                // SAFETY: balances the start this value made.
                unsafe { self.url.stopAccessingSecurityScopedResource() };
            }
        }
    }

    /// What the coordinator's reader reads at the URL it was handed, which
    /// names the document for the length of the reader's call.
    fn read_at(url: NonNull<NSURL>, budget: &mut ReadBudget) -> Result<Arc<[u8]>, DialogError> {
        // SAFETY: the coordinator hands its reader a URL that is valid for
        // the reader's call, which this runs inside.
        let url = unsafe { url.as_ref() };
        let path = url_path(url).ok_or(DialogError::NotAFile)?;
        let mut file = std::fs::File::open(&path).map_err(|error| io_failure(&error))?;
        let metadata = file.metadata().map_err(|error| io_failure(&error))?;
        if !metadata.is_file() {
            // A package another application keeps as one document is a
            // folder, with no contents of its own to hand over.
            return Err(DialogError::NotAFile);
        }
        read_within(&mut file, metadata.len(), budget)
    }

    /// What an open answers for the documents at `urls`, the picker's
    /// picks: each held to `filter`, then their declared sizes to `limit`
    /// bytes together, before any is read or downloaded; then read, within
    /// `limit` again. A request that `keep_access` has each pick carry a
    /// bookmark, made while its access is held. Off the main thread only: it
    /// reads the file system and may wait on a download.
    pub(in crate::ios) fn read_picks(
        urls: Vec<Retained<NSURL>>,
        filter: Option<&ContentFilter>,
        content_types: Option<HasContentTypes>,
        limit: u64,
        keep_access: bool,
    ) -> OpenResult {
        let accesses = urls
            .into_iter()
            .map(ScopedAccess::start)
            .collect::<Vec<ScopedAccess>>();
        if let Some(filter) = filter {
            let rule = NameRule::new(content_types, filter);
            admit_judged(accesses.iter().map(|access| access.judge(&rule)))?;
        }
        admit_sizes(accesses.iter().map(ScopedAccess::declared_size), limit)?;
        let mut budget = ReadBudget::new(limit);
        let files = accesses
            .iter()
            .map(|access| {
                let pick = access.pick(&mut budget)?;
                Ok(match keep_access.then(|| bookmark(&access.url)).flatten() {
                    Some(grant) => pick.granted(grant),
                    None => pick,
                })
            })
            .collect::<Result<Vec<PickedFile>, DialogError>>()?;
        // The picker answers a pick with at least one document.
        Picks::try_new(files)
            .map(Some)
            .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::{ReadBudget, admit_judged, admit_sizes, picked, read_within};
    use crate::apple::content_types::Judged;
    use crate::dialog::{DialogError, PickedForm};

    #[test]
    fn the_picks_declared_sizes_are_refused_past_the_bound_before_any_is_read() {
        assert_eq!(admit_sizes([Some(3), Some(5)], 8), Ok(()));
        assert_eq!(
            admit_sizes([Some(3), Some(6)], 8),
            Err(DialogError::TooLarge {
                actual: 9,
                limit: 8
            }),
            "the picks together, not each alone"
        );
        // A document still in the cloud declares its size, and is refused
        // by it before anything is downloaded.
        let gigabytes: u64 = 10 << 30;
        assert_eq!(
            admit_sizes([Some(1), Some(gigabytes)], 8 << 20),
            Err(DialogError::TooLarge {
                actual: gigabytes + 1,
                limit: 8 << 20
            })
        );
        assert_eq!(
            admit_sizes([Some(u64::MAX), Some(u64::MAX)], u64::MAX - 1),
            Err(DialogError::TooLarge {
                actual: u64::MAX,
                limit: u64::MAX - 1
            }),
            "the sum saturates instead of wrapping"
        );
        assert_eq!(
            admit_sizes([None, Some(8), None], 8),
            Ok(()),
            "a pick of no declared size is held to the bound when it is read"
        );
        assert_eq!(admit_sizes([], 0), Ok(()));
    }

    #[test]
    fn the_picks_are_read_within_the_requests_bound_and_refused_past_it() {
        let mut budget = ReadBudget::new(10);
        assert_eq!(
            read_within(&mut &b"abcd"[..], 4, &mut budget).as_deref(),
            Ok(&b"abcd"[..])
        );
        assert_eq!(
            read_within(&mut &b"efghij"[..], 6, &mut budget).as_deref(),
            Ok(&b"efghij"[..])
        );
        let mut untouched = &b"k"[..];
        assert_eq!(
            read_within(&mut untouched, 1, &mut budget),
            Err(DialogError::TooLarge {
                actual: 11,
                limit: 10
            }),
            "the picks together are past the bound"
        );
        assert_eq!(
            untouched, b"k",
            "refused by its size, before a byte is read"
        );
        assert_eq!(
            read_within(&mut &b""[..], 0, &mut budget).as_deref(),
            Ok(&b""[..]),
            "an empty document still fits a spent budget"
        );
    }

    #[test]
    fn a_document_is_read_to_the_size_it_declared_and_no_further() {
        // One that grew since its size was read is cut at that size.
        let mut budget = ReadBudget::new(3);
        let mut grown = &b"abcdefgh"[..];
        assert_eq!(
            read_within(&mut grown, 2, &mut budget).as_deref(),
            Ok(&b"ab"[..])
        );
        assert_eq!(grown, b"cdefgh", "nothing past the declared size was read");
        // One that shrank answers what it holds, and is charged for that.
        assert_eq!(
            read_within(&mut &b"c"[..], 1, &mut budget).as_deref(),
            Ok(&b"c"[..])
        );
        let mut budget = ReadBudget::new(8);
        assert_eq!(
            read_within(&mut &b"xy"[..], 6, &mut budget).as_deref(),
            Ok(&b"xy"[..])
        );
        assert_eq!(
            read_within(&mut &b"123456"[..], 6, &mut budget).as_deref(),
            Ok(&b"123456"[..]),
            "the bytes read are what is charged, not the size declared"
        );
        assert_eq!(
            read_within(&mut &b"7"[..], 1, &mut budget),
            Err(DialogError::TooLarge {
                actual: 9,
                limit: 8
            })
        );
        assert_eq!(
            read_within(&mut &b""[..], u64::MAX, &mut ReadBudget::new(u64::MAX - 1)),
            Err(DialogError::TooLarge {
                actual: u64::MAX,
                limit: u64::MAX - 1
            }),
            "a size past the budget is refused before a buffer is made for it"
        );
    }

    #[test]
    fn a_pick_is_its_name_and_contents() {
        let pick = picked("Report.pdf", b"%PDF"[..].into()).expect("a file name");
        assert_eq!(
            pick.form(),
            PickedForm::Contents {
                name: "Report.pdf",
                contents: b"%PDF"
            }
        );
        assert_eq!(pick.path(), None, "no path outlasts the access");
        assert_eq!(
            picked("..", b""[..].into()),
            Err(DialogError::Failed(SystemFailure::MalformedAnswer))
        );
    }

    #[test]
    fn one_pick_the_filters_do_not_mean_refuses_them_all() {
        assert_eq!(admit_judged([Judged::Admitted, Judged::Admitted]), Ok(()));
        assert_eq!(
            admit_judged([Judged::Admitted, Judged::Refused]),
            Err(DialogError::NotInFilter)
        );
        assert_eq!(
            admit_judged([Judged::Unread, Judged::Admitted]),
            Ok(()),
            "a type that could not be read refuses nothing"
        );
        assert_eq!(admit_judged([]), Ok(()));
    }
}
