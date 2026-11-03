//! What one asynchronous WinHTTP request shares between the request's
//! thread, the status callback and whoever aborts it.
//!
//! The request's thread starts each operation and waits here for the
//! callback to complete it. An abort closes the request handle, which ends
//! a pending operation, but never while the thread is inside a WinHTTP call
//! on that handle: then it only marks the abort, and the thread closes the
//! handle as the call returns. So no call is ever made on a handle once it
//! is closed, when its value may already name another request's handle.

#![forbid(unsafe_code)]

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// How an operation the request's thread started ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The callback completed it: the bytes it moved (a read's length).
    Completed(u32),
    /// The call refused to start, or the callback reported the request's
    /// failure: WinHTTP's error code.
    Failed(u32),
    /// The request was aborted.
    Aborted,
}

/// One request's gate over its handle (an address, closed by `close`).
pub(super) struct Gate {
    state: Mutex<State>,
    /// Signalled when the callback completes an operation and on an abort.
    changed: Condvar,
    close: fn(usize),
}

struct State {
    handle: usize,
    aborted: bool,
    closed: bool,
    /// The request's thread is inside a WinHTTP call on the handle.
    in_call: bool,
    step: Step,
    /// What the last `SECURE_FAILURE` notification reported.
    secure_failure: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Idle,
    Pending,
    Done(Result<u32, u32>),
}

impl Gate {
    pub(super) fn new(handle: usize, close: fn(usize)) -> Self {
        Self {
            state: Mutex::new(State {
                handle,
                aborted: false,
                closed: false,
                in_call: false,
                step: Step::Idle,
                secure_failure: 0,
            }),
            changed: Condvar::new(),
            close,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing panics while the lock is held; a poisoned one is still
        // whole.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `call` on the open handle, or `None` once the request was
    /// aborted, without calling it. Only the request's thread calls.
    pub(super) fn call<T>(&self, call: impl FnOnce(usize) -> T) -> Option<T> {
        self.enter(Step::Idle).map(|handle| {
            let answer = call(handle);
            self.leave(false);
            answer
        })
    }

    /// Starts an operation with `start` and waits for the callback to
    /// complete it. A start that fails reports its error code; the callback
    /// then reports nothing for it.
    pub(super) fn run(&self, start: impl FnOnce(usize) -> Result<(), u32>) -> Outcome {
        let Some(handle) = self.enter(Step::Pending) else {
            return Outcome::Aborted;
        };
        // The callback may complete the operation before `start` returns,
        // on this thread: no lock is held across the call.
        let started = start(handle);
        self.leave(started.is_err());
        if let Err(code) = started {
            return Outcome::Failed(code);
        }
        let mut state = self.lock();
        loop {
            if let Step::Done(result) = state.step {
                state.step = Step::Idle;
                return match result {
                    Ok(moved) => Outcome::Completed(moved),
                    Err(code) => Outcome::Failed(code),
                };
            }
            if state.aborted {
                return Outcome::Aborted;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The handle, marked in a call that sets `step`; `None` once aborted.
    fn enter(&self, step: Step) -> Option<usize> {
        let mut state = self.lock();
        if state.aborted {
            return None;
        }
        state.in_call = true;
        if step == Step::Pending {
            state.step = Step::Pending;
        }
        Some(state.handle)
    }

    /// The call returned; `refused` when it started nothing. An abort it
    /// held off closes the handle now.
    fn leave(&self, refused: bool) {
        let close = {
            let mut state = self.lock();
            state.in_call = false;
            if refused {
                state.step = Step::Idle;
            }
            state.aborted && !state.closed
        };
        if close {
            self.close_once();
        }
    }

    /// The callback completed the pending operation with `result`.
    pub(super) fn complete(&self, result: Result<u32, u32>) {
        {
            let mut state = self.lock();
            if state.step != Step::Pending {
                return;
            }
            state.step = Step::Done(result);
        }
        self.changed.notify_all();
    }

    /// The callback reported a failed certificate or secure channel check.
    pub(super) fn note_secure_failure(&self, flags: u32) {
        self.lock().secure_failure = flags;
    }

    /// The flags of the last failed secure check, 0 for none.
    pub(super) fn secure_failure(&self) -> u32 {
        self.lock().secure_failure
    }

    /// Aborts the request from any thread, once: the handle closes now, or
    /// as the request's thread leaves the call it is in, and the thread's
    /// wait ends.
    pub(super) fn abort(&self) {
        let close = {
            let mut state = self.lock();
            if state.aborted {
                return;
            }
            state.aborted = true;
            !state.in_call && !state.closed
        };
        self.changed.notify_all();
        if close {
            self.close_once();
        }
    }

    fn close_once(&self) {
        let handle = {
            let mut state = self.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            state.handle
        };
        // Outside the lock: the close runs the callback's last notification,
        // which may run on this thread.
        (self.close)(handle);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    use super::*;

    // Each test closes a handle of its own value, so the tests share this
    // counter without reading each other's closes.
    static CLOSED: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

    fn close(handle: usize) {
        CLOSED[handle].fetch_add(1, Ordering::SeqCst);
    }

    fn closes(handle: usize) -> usize {
        CLOSED[handle].load(Ordering::SeqCst)
    }

    /// An abort while the request's thread is inside a call closes the
    /// handle only as the call returns, and the call after it never runs.
    ///
    /// Red-check: close at once whatever the thread is doing (the handle is
    /// closed inside the call).
    #[test]
    fn an_abort_inside_a_call_closes_the_handle_as_it_returns() {
        let gate = Gate::new(1, close);
        let answer = gate.call(|handle| {
            gate.abort();
            assert_eq!(closes(1), 0, "the handle stays open inside the call");
            handle
        });
        assert_eq!(answer, Some(1));
        assert_eq!(closes(1), 1, "the returning call closes it");
        assert_eq!(gate.call(|_| unreachable!("no call after the abort")), None);
        assert_eq!(
            gate.run(|_| unreachable!("no start after it")),
            Outcome::Aborted
        );
        gate.abort();
        assert_eq!(closes(1), 1, "a handle closes once");
    }

    /// An abort while the thread waits for the callback closes the handle at
    /// once and ends the wait, whatever the callback reports later.
    ///
    /// Red-check: leave the wait to the callback (the thread waits on).
    #[test]
    fn an_abort_while_waiting_closes_the_handle_and_ends_the_wait() {
        let gate = Arc::new(Gate::new(2, close));
        let (started, waiting) = mpsc::channel();
        let thread = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                gate.run(|_| {
                    let _ = started.send(());
                    Ok(())
                })
            })
        };
        waiting.recv().expect("the operation started");
        // Whether the thread is still leaving the call or already waits,
        // the abort closes the handle once and ends the wait.
        gate.abort();
        assert_eq!(thread.join().expect("the thread returns"), Outcome::Aborted);
        assert_eq!(closes(2), 1);
        gate.complete(Err(12017));
        assert_eq!(closes(2), 1);
    }

    /// The callback's completion, even one delivered inside the starting
    /// call, ends the wait with what it moved or the request's error; a
    /// start that fails reports its own error and expects no completion.
    ///
    /// Red-check: set the operation pending after the start returns (the
    /// completion delivered inside it is lost and the wait never ends).
    #[test]
    fn a_completion_ends_the_wait_even_from_inside_the_start() {
        let gate = Gate::new(3, close);
        assert_eq!(
            gate.run(|_| {
                gate.complete(Ok(512));
                Ok(())
            }),
            Outcome::Completed(512)
        );
        assert_eq!(gate.run(|_| Err(12029)), Outcome::Failed(12029));
        gate.complete(Ok(7));
        assert_eq!(
            gate.run(|_| {
                gate.complete(Err(12002));
                Ok(())
            }),
            Outcome::Failed(12002),
            "a completion with no operation pending is not kept"
        );
        gate.note_secure_failure(8);
        assert_eq!(gate.secure_failure(), 8);
        assert_eq!(closes(3), 0, "nothing aborted, nothing closed");
    }
}
