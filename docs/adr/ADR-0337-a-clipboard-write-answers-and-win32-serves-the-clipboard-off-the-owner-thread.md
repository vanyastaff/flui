# ADR-0337: A clipboard write answers whether the clipboard took it, and Win32 serves its clipboard off the owner thread

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0152](ADR-0152-capability-seam-revised.md) §6 in part (the write's shape:
  `ClipboardHandle::write`, `ClipboardBackend::write` and `NativeClipboard::write` returned
  nothing, and `NativeClipboard` had a `write_text` shorthand)
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) §5 (a read answers through a
  resolver), [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (containment
  of code a thread does not own), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §10
  (the clipboard feature directory)

## Context and problem statement

The Win32 clipboard opened the clipboard once and read it on the owner thread. Two runs on
Windows 11 measured what that costs (`clipboard::windows::tests`, before this change):

| Case | Before |
|------|--------|
| Another thread holds the clipboard open for 100 ms, as a clipboard manager or `rdpclip.exe` does after every change; the host reads, then writes | the read answers `Unavailable` after 2 ms; the write returns after 2 ms and the clipboard still holds the old text, with nothing told to the caller |
| Another window owns text it renders only when asked (`SetClipboardData(format, NULL)`) and answers `WM_RENDERFORMAT` after 3 s | `GetClipboardData` returns after 3.0 s, on the thread that read |
| The same owner never answers | `GetClipboardData` fails with `ERROR_TIMEOUT` after 30.0 s |

On the owner thread the second case freezes every window of the application for as long as the
other application takes, up to thirty seconds. The first case drops accepted work: a cut deleted
the text it could not copy. Chromium (`ui/base/clipboard/clipboard_win.cc`) tries
`OpenClipboard` five times 5 ms apart and moves reads to a worker sequence
(`kNonBlockingOsClipboardReads`, enabled by default); Firefox (`widget/windows/nsClipboard.cpp`)
tries its OLE clipboard calls three times 3 ms apart; `arboard` five times 5 ms apart; all of them
sleep on the calling thread, and none reports a refused write through a typed answer.

How does a clipboard write tell its caller that it failed, and how does the Win32 host keep
another process's clipboard from holding the owner thread?

## Decision drivers

- Accepted work is delivered or refused with a typed error (AGENTS.md, "Before each push" item 3).
- The frame path is synchronous and never waits on another process.
- A read made after a write answers what the write left.
- Every host answers the same shape; a policy one host holds is at the seam (parity).
- Bounded: no unbounded retry, no unbounded queue.

## Considered options

1. A write answers through a `Resolver<ClipboardWritten>`, as a read does; the Win32 host serves
   every read and write on a clipboard thread of its own, which retries a held clipboard on a
   doubling schedule bounded by a patience.
2. Keep the write unanswered; retry on the owner thread a few times, as Chromium and Firefox do,
   and log a write that still fails.
3. A write answers synchronously (`write(&self, …) -> ClipboardWritten`); reads stay on the owner
   thread with a bounded wait for a worker that opens the clipboard.

## Decision

Chosen option: 1, because it is the only one that both reports a refused write to the code that
can act on it and keeps the owner thread free of another process's waits; option 2 drops the
failure and still blocks the owner for the retries and for a late renderer, and option 3 cannot
answer asynchronously where the host only learns later (X11 owns its selection at the server's
time, the browser's promise settles later) and its bounded wait still freezes the owner for the
bound, while `GetClipboardData` cannot be cancelled.

- **§1. A write answers.** `ClipboardWritten = Result<(), ClipboardError>` in `flui-platform`.
  `ClipboardBackend::write(&self, ClipboardWrite, Resolver<ClipboardWritten>)` and
  `NativeClipboard::write` take the same resolver; `ClipboardHandle::write` and `write_text`
  return `Completion<ClipboardWritten>`, which settles at an owner turn, never inside the call.
  `NativeClipboard` has no `write_text` shorthand. A refused write leaves the clipboard as it was,
  or empty where a concealed write could not be marked (ADR-0152 §6). A caller with nothing to undo
  (a copy) drops the completion; `EditableText`'s cut deletes the selection only once the write
  answers `Ok`, and only if the field and its text are as the cut left them.

  | Host | `Ok` | Refused |
  |------|------|---------|
  | Win32 | the text and its markers are set | `Unavailable`: held past the patience (§2), `EmptyClipboard` or a marker failed, more than `MAX_WAITING` requests wait |
  | AppKit, UIKit | the pasteboard took the string | `Unavailable`: a marker or the set failed, UIKit off its main thread |
  | Android | `setPrimaryClip` returned | `Unavailable`: a JNI failure, no clipboard service |
  | X11 | the clipboard thread owns `CLIPBOARD` at the server's time | `Unavailable`: another client took it first; a write a newer one replaced before it owned shares the newer one's answer |
  | Wayland | the seat's selection is set to the write's source | `Denied`: no user input to set it under; `Unavailable`: no source could be made or set |
  | Web | `writeText`'s promise resolved | its refusal (`Denied` for a permission), `Unavailable` with no Clipboard API; the page's own reads still answer the text |
  | Headless, in memory | always | never; the scripted double refuses as a test asks (`ScriptedClipboard::fail_writes_with`) |

- **§2. Win32 retries a held clipboard on a stated schedule.** `OpenClipboard` fails while
  another window holds the clipboard; a request tries again first after `FIRST_DELAY` (4 ms), each
  delay twice the one before up to `LONGEST_DELAY` (64 ms), the last attempt when its `PATIENCE`
  (500 ms, counted from the request's acceptance) runs out, and is then refused `Unavailable`. A
  request that reaches its turn after its patience ran out still gets one attempt. The schedule
  is a pure model (`clipboard::patience`) whose clock the caller passes, tested with instants of
  its own. Off the owner thread the patience can be a hundred times Chromium's: a copy can wait
  for a remote-desktop client reading over a slow link.
- **§3. Win32 serves its clipboard on a thread of its own.** One `flui-clipboard` thread per
  process owns the message-only clipboard window, opens the clipboard for every read and write
  and answers each through its resolver; the owner thread only queues, into a channel bounded at
  `MAX_WAITING` (64) requests, and a request past it is refused at once. Requests are served in
  the order they were made, so a read after a write answers what the write left. Between attempts
  the thread waits in `MsgWaitForMultipleObjectsEx` on its wake event and its messages, pumping
  them: another process's `EmptyClipboard` sends the owner window `WM_DESTROYCLIPBOARD` and waits.
  A read of text rendered on demand waits on that thread, and Win32's own thirty-second bound
  ends it as `ClipboardError::Timeout`; the owner's frames keep turning meanwhile, and the requests
  behind it wait. A job or a delivery that panics on the thread drops its resolver, which abandons
  that request, and the thread serves the rest. The thread uses `GetClipboardData`, which needs no
  COM apartment; `OleGetClipboard` would tie the data object to the thread's apartment and is not
  used. Since only this thread opens the clipboard, FLUI's sessions cannot overlap, and the
  process-wide clipboard lock that serialized sessions opened from any thread is gone.
- **§4. A late answer reaches only its request.** An answer settles the completion its resolver
  was made for: the resolver carries the completion's generational slot, so an answer after the
  caller dropped the completion settles nothing. While the thread holds the clipboard open no other
  window can change it, so a read's answer is the clipboard's at that moment;
  `GetClipboardSequenceNumber` adds nothing to the order the queue already gives.

### Consequences

- Good, because a copy that another application blocked is no longer lost silently, and a cut
  keeps the text the clipboard did not take.
- Good, because no other process can freeze the Win32 owner thread through the clipboard.
- Bad, because a cut deletes its selection one owner turn after the chord rather than inside it.
- Bad, because every Win32 clipboard answer crosses a thread, and the requests behind a read of a
  late renderer wait up to Win32's thirty seconds.
- Neutral, because AppKit, UIKit and Android still read on the owner thread: a pasteboard item
  another application provides lazily, or an Android clip whose text is a content URI, still
  holds it while the other application answers ([gaps](../gaps.md#clipboard-is-broken-on-two-environments)).

### Confirmation

- `clipboard::patience` tests: `a_held_clipboard_is_tried_on_a_doubling_schedule_until_the_patience_ends`,
  `a_request_past_its_patience_makes_one_attempt`, `the_delay_saturates_without_overflow`.
- `clipboard::windows::tests` on Windows: `a_write_and_a_read_while_the_clipboard_is_held_for_a_moment_land_once_it_is_let_go`,
  `a_clipboard_held_past_the_patience_refuses_the_write`, `a_read_of_text_rendered_late_never_holds_the_caller`,
  `a_burst_of_clipboard_writes_is_answered_in_full`, `a_concealed_write_stays_out_of_the_windows_history`.
- `clipboard::linux::x11::tests::a_clipboard_write_answers_once_it_owns_the_selection` (Xvfb).
- `capability_seam::a_refused_write_answers_its_refusal` (`flui-testing`) and
  `editable_text_clipboard::a_cut_deletes_only_what_the_clipboard_took` (`flui-widgets`).
- The compiler: `ClipboardBackend::write` and `NativeClipboard::write` cannot be implemented
  without taking the resolver.
