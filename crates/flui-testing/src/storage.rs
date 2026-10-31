//! [`MemoryStorage`]: a [`Storage`] in memory that a test can hold, fail and
//! read back.
//!
//! Requests complete when the UI runtime polls them, on the owner thread, inside
//! a pumped frame, so a test decides with [`MemoryStorage::hold_writes`] and
//! [`MemoryStorage::hold_commits`] when a write lands instead of racing an
//! IO thread. The double follows [`Storage::publish`]'s acceptance contract
//! (a newer value replaces one not yet started) and a file's compare and
//! replace: a [`WriteMode::IfUnchanged`] write is refused with
//! [`StorageError::Conflict`] when it commits over another version.
//!
//! ```
//! use std::future::Future;
//! use std::pin::pin;
//! use std::task::{Context, Poll, Waker};
//!
//! use flui_platform::{Storage, StorageName, WriteMode};
//! use flui_testing::storage::MemoryStorage;
//!
//! const NOTES: StorageName = StorageName::from_static("notes");
//! let storage = MemoryStorage::new();
//! let mut cx = Context::from_waker(Waker::noop());
//!
//! let barrier = storage.hold_commits();
//! let mut write = pin!(storage.publish(&NOTES, b"saved".to_vec(), WriteMode::Replace));
//! assert!(write.as_mut().poll(&mut cx).is_pending(), "held at the barrier");
//! assert_eq!(storage.contents(&NOTES), None);
//!
//! barrier.commit();
//! assert!(matches!(write.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
//! assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"saved"[..]));
//! ```

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use flui_platform::{
    CopiedAside, Storage, StorageError, StorageFuture, StorageName, Stored, StoredVersion,
    WriteMode,
};
use parking_lot::Mutex;

/// A [`Storage`] that keeps its values in memory.
///
/// Clones share the same values, so a test keeps one clone to inspect and
/// hands another to the tree it mounts; mounting a new tree over the same
/// storage restarts the application without a disk.
#[derive(Clone, Default)]
pub struct MemoryStorage {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// The committed values: what a read returns.
    values: HashMap<StorageName, Vec<u8>>,
    /// Request futures not yet dropped.
    live: usize,
    next_write: u64,
    /// The answer slot of every write future not yet dropped.
    writes: HashMap<u64, WriteSlot>,
    /// Accepted while a [`WriteHold`] is alive; not started.
    queued: Vec<Write>,
    /// Written while a [`CommitBarrier`] is alive; not committed.
    written: Vec<Write>,
    write_holds: usize,
    commit_holds: usize,
    copy_holds: usize,
    /// Copies asked for while a [`CopyHold`] is alive, waiting to run.
    held_copies: Vec<Waker>,
    /// Errors the next publishes fail with, in order.
    fail_next: VecDeque<StorageError>,
    /// The error every read fails with, if set.
    fail_reads: Option<StorageError>,
    /// The error every copy aside fails with, if set.
    fail_copies: Option<StorageError>,
    /// The copies [`Storage::copy_aside`] kept of each name, in order.
    kept: HashMap<StorageName, Vec<Vec<u8>>>,
}

/// An accepted value on its way to [`State::values`].
struct Write {
    id: u64,
    name: StorageName,
    bytes: Vec<u8>,
    mode: WriteMode,
}

#[derive(Default)]
struct WriteSlot {
    answer: Option<Result<StoredVersion, StorageError>>,
    waker: Option<Waker>,
}

impl State {
    /// Answer write `id`, if its future is still alive; the waker to wake
    /// once the lock is released.
    fn answer(&mut self, id: u64, answer: Result<StoredVersion, StorageError>) -> Option<Waker> {
        let slot = self.writes.get_mut(&id)?;
        slot.answer = Some(answer);
        slot.waker.take()
    }

    /// Start `write`: commit it, or hold it at the commit barrier.
    fn start(&mut self, write: Write) -> Option<Waker> {
        if self.commit_holds > 0 {
            self.written.push(write);
            None
        } else {
            self.commit(write)
        }
    }

    /// Store `write`, unless it is based on a version the name no longer
    /// holds, which refuses it with [`StorageError::Conflict`].
    fn commit(&mut self, write: Write) -> Option<Waker> {
        if let WriteMode::IfUnchanged(base) = write.mode {
            let stored = self
                .values
                .get(&write.name)
                .map_or(StoredVersion::ABSENT, |bytes| {
                    StoredVersion::of_bytes(bytes)
                });
            if stored != base {
                return self.answer(write.id, Err(StorageError::Conflict));
            }
        }
        let version = StoredVersion::of_bytes(&write.bytes);
        self.values.insert(write.name, write.bytes);
        self.answer(write.id, Ok(version))
    }
}

fn wake_all(wakers: impl IntoIterator<Item = Waker>) {
    for waker in wakers {
        waker.wake();
    }
}

impl MemoryStorage {
    /// An empty storage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `bytes` under `name` directly, as a file an earlier run left.
    pub fn put(&self, name: &StorageName, bytes: impl Into<Vec<u8>>) {
        self.state.lock().values.insert(*name, bytes.into());
    }

    /// The committed value under `name`: what a read would return.
    #[must_use]
    pub fn contents(&self, name: &StorageName) -> Option<Vec<u8>> {
        self.state.lock().values.get(name).cloned()
    }

    /// Accept writes but start none while the returned hold is alive; a
    /// newer value of a name replaces the one waiting, whose future resolves
    /// to [`StorageError::Superseded`]. Dropping the last hold starts the
    /// waiting writes.
    #[must_use = "writes are held only while the hold is alive"]
    pub fn hold_writes(&self) -> WriteHold {
        self.state.lock().write_holds += 1;
        WriteHold {
            state: Arc::clone(&self.state),
        }
    }

    /// Write but commit nothing while the returned barrier is alive: reads
    /// and [`contents`](Self::contents) keep the old values and the write
    /// futures stay pending. Barriers are counted, and the last one released
    /// decides: [`CommitBarrier::commit`] commits the held writes in order,
    /// while dropping it uncommitted discards them, as a process killed
    /// before it replaced its files would, and their futures resolve to
    /// [`StorageError::Cancelled`]. Releasing a barrier while another is
    /// alive keeps the writes held.
    ///
    /// ```
    /// use std::future::Future;
    /// use std::pin::pin;
    /// use std::task::{Context, Poll, Waker};
    ///
    /// use flui_platform::{Storage, StorageError, StorageName, WriteMode};
    /// use flui_testing::storage::MemoryStorage;
    ///
    /// const NOTES: StorageName = StorageName::from_static("notes");
    /// let storage = MemoryStorage::new();
    /// let mut cx = Context::from_waker(Waker::noop());
    ///
    /// // A commit while another barrier is alive commits nothing.
    /// let (outer, inner) = (storage.hold_commits(), storage.hold_commits());
    /// let mut first = pin!(storage.publish(&NOTES, b"one".to_vec(), WriteMode::Replace));
    /// inner.commit();
    /// assert!(first.as_mut().poll(&mut cx).is_pending());
    /// assert_eq!(storage.contents(&NOTES), None);
    /// outer.commit();
    /// assert!(matches!(first.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
    /// assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"one"[..]));
    ///
    /// // Dropping a barrier while another is alive discards nothing; the
    /// // last one dropped uncommitted discards the held writes.
    /// let (outer, inner) = (storage.hold_commits(), storage.hold_commits());
    /// let mut second = pin!(storage.publish(&NOTES, b"two".to_vec(), WriteMode::Replace));
    /// drop(inner);
    /// assert!(second.as_mut().poll(&mut cx).is_pending());
    /// drop(outer);
    /// assert_eq!(
    ///     second.as_mut().poll(&mut cx),
    ///     Poll::Ready(Err(StorageError::Cancelled))
    /// );
    /// assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"one"[..]));
    /// ```
    #[must_use = "commits are held only while the barrier is alive"]
    pub fn hold_commits(&self) -> CommitBarrier {
        self.state.lock().commit_holds += 1;
        CommitBarrier {
            state: Arc::clone(&self.state),
            released: false,
        }
    }

    /// Fail the next publish with `error`, storing nothing. Each call queues
    /// one failure.
    pub fn fail_next(&self, error: StorageError) {
        self.state.lock().fail_next.push_back(error);
    }

    /// Fail every read with `error` from now on; `None` lets reads succeed
    /// again.
    pub fn fail_reads(&self, error: Option<StorageError>) {
        self.state.lock().fail_reads = error;
    }

    /// Fail every [`copy_aside`](Storage::copy_aside) with `error` from now
    /// on, keeping nothing; `None` lets copies succeed again.
    pub fn fail_copies(&self, error: Option<StorageError>) {
        self.state.lock().fail_copies = error;
    }

    /// Answer no [`copy_aside`](Storage::copy_aside) while the returned hold
    /// is alive: a copy asked for meanwhile is made when the last hold
    /// drops, from what is stored then, as a slow disk would make it.
    ///
    /// ```
    /// use std::future::Future;
    /// use std::pin::pin;
    /// use std::task::{Context, Poll, Waker};
    ///
    /// use flui_platform::{Storage, StorageName};
    /// use flui_testing::storage::MemoryStorage;
    ///
    /// const NOTES: StorageName = StorageName::from_static("notes");
    /// let storage = MemoryStorage::new();
    /// storage.put(&NOTES, "old");
    /// let mut cx = Context::from_waker(Waker::noop());
    ///
    /// let held = storage.hold_copies();
    /// let mut copy = pin!(storage.copy_aside(&NOTES));
    /// assert!(copy.as_mut().poll(&mut cx).is_pending());
    /// storage.put(&NOTES, "new");
    /// drop(held);
    /// assert!(matches!(copy.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
    /// assert_eq!(storage.kept_aside(&NOTES), [b"new".to_vec()]);
    /// ```
    #[must_use = "copies are held only while the hold is alive"]
    pub fn hold_copies(&self) -> CopyHold {
        self.state.lock().copy_holds += 1;
        CopyHold {
            state: Arc::clone(&self.state),
        }
    }

    /// The copies [`copy_aside`](Storage::copy_aside) kept of `name`, oldest
    /// first: the copy named `<name>.corrupt-<n>` is at index `n - 1`.
    #[must_use]
    pub fn kept_aside(&self, name: &StorageName) -> Vec<Vec<u8>> {
        self.state
            .lock()
            .kept
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// How many read and write futures are alive: issued and not yet
    /// dropped, whether or not they completed.
    #[must_use]
    pub fn live_requests(&self) -> usize {
        self.state.lock().live
    }
}

impl fmt::Debug for MemoryStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock();
        let mut names: Vec<&str> = state.values.keys().map(StorageName::as_str).collect();
        names.sort_unstable();
        f.debug_struct("MemoryStorage")
            .field("names", &names)
            .field("live_requests", &state.live)
            .field("queued", &state.queued.len())
            .field("uncommitted", &state.written.len())
            .finish_non_exhaustive()
    }
}

impl Storage for MemoryStorage {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored> {
        let mut state = self.state.lock();
        state.live += 1;
        let answer = match (&state.fail_reads, state.values.get(name)) {
            (Some(error), _) => Err(error.clone()),
            (None, None) => Ok(Stored {
                bytes: None,
                version: StoredVersion::ABSENT,
            }),
            (None, Some(bytes)) => {
                let len = u64::try_from(bytes.len()).expect("BUG: a byte length fits in u64");
                if len > limit {
                    Err(StorageError::TooLarge { len, limit })
                } else {
                    Ok(Stored {
                        bytes: Some(bytes.clone()),
                        version: StoredVersion::of_bytes(bytes),
                    })
                }
            }
        };
        drop(state);
        Box::pin(Answered {
            state: Arc::clone(&self.state),
            answer: Some(answer),
        })
    }

    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: WriteMode,
    ) -> StorageFuture<StoredVersion> {
        let mut state = self.state.lock();
        state.live += 1;
        let id = state.next_write;
        state.next_write += 1;
        state.writes.insert(id, WriteSlot::default());
        let write = Write {
            id,
            name: *name,
            bytes,
            mode,
        };
        let mut wakers = Vec::new();
        if let Some(error) = state.fail_next.pop_front() {
            wakers.extend(state.answer(id, Err(error)));
        } else if state.write_holds > 0 {
            let (superseded, waiting): (Vec<Write>, Vec<Write>) = std::mem::take(&mut state.queued)
                .into_iter()
                .partition(|queued| queued.name == *name);
            state.queued = waiting;
            for queued in superseded {
                wakers.extend(state.answer(queued.id, Err(StorageError::Superseded)));
            }
            state.queued.push(write);
        } else {
            wakers.extend(state.start(write));
        }
        drop(state);
        wake_all(wakers);
        Box::pin(WriteRequest {
            state: Arc::clone(&self.state),
            id,
        })
    }

    fn copy_aside(&self, name: &StorageName) -> StorageFuture<CopiedAside> {
        self.state.lock().live += 1;
        Box::pin(CopyRequest {
            state: Arc::clone(&self.state),
            name: *name,
            answered: false,
        })
    }
}

/// While alive, [`MemoryStorage`] starts no write; see
/// [`MemoryStorage::hold_writes`].
pub struct WriteHold {
    state: Arc<Mutex<State>>,
}

impl fmt::Debug for WriteHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteHold").finish_non_exhaustive()
    }
}

impl Drop for WriteHold {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.write_holds -= 1;
        let mut wakers = Vec::new();
        if state.write_holds == 0 {
            for write in std::mem::take(&mut state.queued) {
                wakers.extend(state.start(write));
            }
        }
        drop(state);
        wake_all(wakers);
    }
}

/// While alive, [`MemoryStorage`] commits no write; see
/// [`MemoryStorage::hold_commits`].
pub struct CommitBarrier {
    state: Arc<Mutex<State>>,
    released: bool,
}

impl fmt::Debug for CommitBarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommitBarrier").finish_non_exhaustive()
    }
}

impl CommitBarrier {
    /// Release this barrier; if it is the last one, commit every held write,
    /// in the order they were written.
    pub fn commit(mut self) {
        self.released = true;
        let mut state = self.state.lock();
        state.commit_holds -= 1;
        let mut wakers = Vec::new();
        let last = state.commit_holds == 0;
        if last {
            for write in std::mem::take(&mut state.written) {
                wakers.extend(state.commit(write));
            }
        }
        drop(state);
        wake_all(wakers);
    }
}

impl Drop for CommitBarrier {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = self.state.lock();
        state.commit_holds -= 1;
        let mut wakers = Vec::new();
        if state.commit_holds == 0 {
            for write in std::mem::take(&mut state.written) {
                wakers.extend(state.answer(write.id, Err(StorageError::Cancelled)));
            }
        }
        drop(state);
        wake_all(wakers);
    }
}

/// A read or a copy: answered when issued, counted until dropped.
struct Answered<T: Unpin> {
    state: Arc<Mutex<State>>,
    answer: Option<Result<T, StorageError>>,
}

impl<T: Unpin> Future for Answered<T> {
    type Output = Result<T, StorageError>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.answer.take().map_or(Poll::Pending, Poll::Ready)
    }
}

impl<T: Unpin> Drop for Answered<T> {
    fn drop(&mut self) {
        self.state.lock().live -= 1;
    }
}

/// A write: answered when it commits, fails or is replaced.
struct WriteRequest {
    state: Arc<Mutex<State>>,
    id: u64,
}

impl Future for WriteRequest {
    type Output = Result<StoredVersion, StorageError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock();
        let slot = state
            .writes
            .get_mut(&self.id)
            .expect("BUG: a live write future keeps its slot");
        if let Some(answer) = slot.answer.take() {
            return Poll::Ready(answer);
        }
        slot.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for WriteRequest {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.writes.remove(&self.id);
        state.live -= 1;
    }
}

impl State {
    /// Keep a copy of `name`'s stored bytes, or fail as configured.
    fn copy_aside(&mut self, name: &StorageName) -> Result<CopiedAside, StorageError> {
        if let Some(error) = &self.fail_copies {
            return Err(error.clone());
        }
        let bytes = self
            .values
            .get(name)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        let version = StoredVersion::of_bytes(&bytes);
        let kept = self.kept.entry(*name).or_default();
        kept.push(bytes);
        Ok(CopiedAside {
            kept_as: format!("{}.corrupt-{}", name.as_str(), kept.len()),
            version,
        })
    }
}

/// A copy aside: made when it is first polled with no [`CopyHold`] alive.
struct CopyRequest {
    state: Arc<Mutex<State>>,
    name: StorageName,
    answered: bool,
}

impl Future for CopyRequest {
    type Output = Result<CopiedAside, StorageError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.answered {
            return Poll::Pending;
        }
        let mut state = self.state.lock();
        if state.copy_holds > 0 {
            state.held_copies.push(cx.waker().clone());
            return Poll::Pending;
        }
        let answer = state.copy_aside(&self.name);
        drop(state);
        self.answered = true;
        Poll::Ready(answer)
    }
}

impl Drop for CopyRequest {
    fn drop(&mut self) {
        self.state.lock().live -= 1;
    }
}

/// While alive, [`MemoryStorage`] answers no copy aside; see
/// [`MemoryStorage::hold_copies`].
pub struct CopyHold {
    state: Arc<Mutex<State>>,
}

impl fmt::Debug for CopyHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CopyHold").finish_non_exhaustive()
    }
}

impl Drop for CopyHold {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.copy_holds -= 1;
        let wakers = if state.copy_holds == 0 {
            std::mem::take(&mut state.held_copies)
        } else {
            Vec::new()
        };
        drop(state);
        wake_all(wakers);
    }
}
