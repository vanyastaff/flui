# ADR-0337: A clipboard write answers whether the clipboard took it, and Win32 serves its clipboard off the owner thread

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0152](ADR-0152-capability-seam-revised.md) §6 in part (the write's shape:
  `ClipboardHandle::write`, `ClipboardBackend::write` and `NativeClipboard::write` returned
  nothing, and `NativeClipboard` had a `write_text` shorthand);
  [ADR-0312](ADR-0312-drag-and-drop-owns-the-transfer-transport-and-the-clipboard-stays-plain-text.md)
  §6 in part (the same write shape, restated there)
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) §5 (a read answers through a
  resolver), [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (containment
  of code a thread does not own), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §10
  (the clipboard feature directory)
- **Sources:**
  `chromium/chromium@4568aba8b53d8573a57c99ee63fb0246f4e21941 ui/base/clipboard/clipboard_win.cc:102`
  ([permalink](https://github.com/chromium/chromium/blob/4568aba8b53d8573a57c99ee63fb0246f4e21941/ui/base/clipboard/clipboard_win.cc#L102-L127),
  five `OpenClipboard` attempts 5 ms apart; `:275`, the worker sequence reads run on),
  `chromium/chromium@4568aba8b53d8573a57c99ee63fb0246f4e21941 ui/base/ui_base_features.cc:459`
  ([permalink](https://github.com/chromium/chromium/blob/4568aba8b53d8573a57c99ee63fb0246f4e21941/ui/base/ui_base_features.cc#L459),
  `kNonBlockingOsClipboardReads` enabled by default),
  `mozilla-firefox/firefox@f898416c560977636760a1fdab1d3f468cb62486 widget/windows/nsClipboard.cpp:702`
  ([permalink](https://github.com/mozilla-firefox/firefox/blob/f898416c560977636760a1fdab1d3f468cb62486/widget/windows/nsClipboard.cpp#L702-L716),
  three OLE clipboard attempts 3 ms apart),
  `1Password/arboard@5f5c8637911ba219edee619105a19b4f74e66a9f` source directory, `platform/windows.rs:533`
  ([permalink](https://github.com/1Password/arboard/blob/5f5c8637911ba219edee619105a19b4f74e66a9f/src/platform/windows.rs#L533-L559),
  five attempts 5 ms apart)

## Context and problem statement

The Win32 clipboard opened the clipboard once and read it on the owner thread. Two runs on
Windows 11 measured what that costs (before this change):

| Case | Before |
|------|--------|
| Another thread holds the clipboard open for 100 ms, as a clipboard manager or `rdpclip.exe` does after every change; the host reads, then writes | the read answers `Unavailable` after 2 ms; the write returns after 2 ms and the clipboard still holds the old text, with nothing told to the caller |
| Another window owns text it renders only when asked (`SetClipboardData(format, NULL)`) and answers `WM_RENDERFORMAT` after 3 s | `GetClipboardData` returns after 3.0 s, on the thread that read |
| The same owner never answers | `GetClipboardData` fails with `ERROR_TIMEOUT` after 30.0 s |

On the owner thread the second case freezes every window of the application for as long as the
other application takes, up to thirty seconds. The first case drops accepted work: a cut deleted
the text it could not copy. Chromium tries `OpenClipboard` five times 5 ms apart and moves reads
to a worker sequence; Firefox tries its OLE clipboard calls three times 3 ms apart; `arboard` five
times 5 ms apart; all of them sleep on the calling thread, and none reports a refused write
through a typed answer (Sources).

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

## Pros and cons of the options

### 1. A resolver for the write; a clipboard thread on Win32

- Good, because the code that can act on a refused write (a cut, which deletes what it wrote)
  hears it, typed.
- Good, because no other process's wait (a held clipboard, a late renderer) reaches the owner
  thread.
- Good, because every host can answer it when it learns the outcome: X11 at the server's time, the
  web when the browser's promise settles.
- Bad, because a cut deletes its selection one owner turn after the chord, and every Win32
  clipboard answer crosses a thread.
- Bad, because the requests behind a read of a late renderer wait for it, up to Win32's thirty
  seconds.

### 2. Retry on the owner thread, write unanswered

- Good, because nothing in the public API changes.
- Bad, because a write still refused after the retries is dropped with nobody told.
- Bad, because the retries and a late renderer still hold the owner thread.

### 3. A synchronous write answer; a bounded wait for reads

- Good, because a caller has the answer at once.
- Bad, because X11 and the web learn a write's outcome only later, so they could answer only a
  guess.
- Bad, because the bounded wait still freezes the owner for the bound, and `GetClipboardData`
  cannot be cancelled: the worker stays stuck behind it.

## Decision

Chosen option: 1, because it is the only one that both reports a refused write to the code that
can act on it and keeps the owner thread free of another process's waits.

- **§1. A write answers.** `ClipboardWritten = Result<(), ClipboardError>` in `flui-platform`.
  `ClipboardBackend::write(&self, ClipboardWrite, Resolver<ClipboardWritten>)` and
  `NativeClipboard::write` take the same resolver; `ClipboardHandle::write` and `write_text`
  return `Completion<ClipboardWritten>`, which settles at an owner turn, never inside the call.
  `NativeClipboard` has no `write_text` shorthand. The answer means the same on every host:
  - `Ok(())`: the system clipboard took the text.
  - `ApplicationOnly`: the system clipboard cannot take the text now, and only this application
    can read it back: the host keeps it for the application's own reads. One situation, one
    outcome, on every host that can meet it (the table).
  - any other error: nothing was kept, and no later read in the process answers the text. The
    clipboard is as it was, except where a host must clear it before filling it (Win32's
    `EmptyClipboard`, AppKit's `clearContents`) and the system itself then refuses the text:
    the clipboard is then left without text.

  A Wayland compositor confirms no selection request, so a request it drops silently still
  answers `Ok`; a Wayland host names no input once the keyboard left the window it went to, so a
  write after that is kept for the application (`ApplicationOnly`) rather than sent under a stale
  serial. Worth a retry: `Unavailable` and `Timeout` (a clipboard held or slow now), `Denied` once
  the user acts; `Gone` is not: the host's clipboard will serve nothing more this run. A caller
  with nothing to undo (a copy) drops the completion, and treats `ApplicationOnly` as done;
  `EditableText`'s cut deletes the selection once the write answers `Ok` or `ApplicationOnly`
  (nothing is lost: a paste in the application returns the text), and only under the rule its
  start, its enablement and a paste follow at both ends (the field mounted, enabled, holding the
  focus, on the same controller), with its text and selection as the cut left them. Every path
  to a field's cut or paste focuses the field first, assistive technology's popup included. The
  write changes on every host in one change because the seam is shared: a write that answers on
  one host and not on the others would be two contracts.

  | Host | `Ok` | `ApplicationOnly` | Refused (nothing kept) |
  |------|------|-------------------|------------------------|
  | Win32 | the text and its markers are set | never | `Unavailable`: held past the patience (§2), memory not made (the clipboard untouched), `EmptyClipboard` or `SetClipboardData` failed (the clipboard left without text), more than `MAX_WAITING` requests wait; `Timeout`: queued past `LONGEST_QUEUED`; `Gone`: the clipboard thread could not start or has ended |
  | AppKit | the pasteboard took the string | never | `Unavailable`: a marker or the set failed after `clearContents` (the pasteboard left without text) |
  | UIKit | the pasteboard took the string | never | `Unavailable`: off its main thread (the set replaces the items in one call) |
  | Android | `setPrimaryClip` returned | never | `Unavailable`: a JNI failure, no clipboard service (the clip is built and marked before it is set) |
  | X11 | the clipboard thread owns `CLIPBOARD` at the server's time | never | `Unavailable`: another client took it first; a write a newer one replaced before it owned shares the newer one's answer |
  | Wayland | the seat's selection serves the write: a new source set under the input's serial, or the source set under it answering in place (a plain write through a hinted source is served marked secret) | no input serial to set it under (none yet, or the keyboard left the window): kept under no seat until another client's selection arrives, as the primary selection keeps a claim | `Unavailable`: no source could be made or set, or a concealed write would go out through a source without the hint |
  | Web | `writeText`'s promise resolved | no Clipboard API (an insecure context): the page keeps the text for its own reads | the browser's refusal (`Denied` for a permission), the page keeping its earlier text |
  | Headless, in memory | always | never | never; the scripted double answers through its write script as a test asks (`ScriptedClipboard::writes`, `fail_writes_with`, `hold_writes`) |

- **§2. Win32 retries a held clipboard on a stated schedule.** `OpenClipboard` fails while
  another window holds the clipboard; a request tries again first after `FIRST_DELAY` (4 ms), each
  delay twice the one before up to `LONGEST_DELAY` (64 ms), the last attempt when its `PATIENCE`
  (500 ms, counted from when its turn came) runs out, and is then refused `Unavailable`. A request
  queued behind a slow one keeps its full patience; the wait for its turn has a bound of its own,
  `LONGEST_QUEUED` (40 s), past which it is refused `Timeout` without an attempt. That bounds the
  wait for the turn, not the answer: a request whose turn comes just inside it still makes its
  attempts and its own call to Win32, which a late or hung owner keeps up to thirty seconds, so a
  request answers within `LONGEST_QUEUED`, `PATIENCE` and thirty seconds at worst. The schedule is
  a pure model (`clipboard::patience`) whose clock the caller passes, tested with instants of its
  own. Off the owner thread the patience can be a hundred times Chromium's: a copy can wait for a
  remote-desktop client reading over a slow link.
- **§3. Win32 serves its clipboard on a thread of its own.** One `flui-clipboard` thread per
  process owns the message-only clipboard window, opens the clipboard for every read and write
  and answers each through its resolver; the owner thread only queues, into a channel bounded at
  `MAX_WAITING` (64) requests, and a request past it is refused `Unavailable` at once. Requests are
  served in the order they were made, so a read after a write answers what the write left.
  Between attempts the thread waits in `MsgWaitForMultipleObjectsEx` on its wake event and its
  messages, pumping them: another process's `EmptyClipboard` sends the owner window
  `WM_DESTROYCLIPBOARD` and waits. A read of text rendered on demand waits on that thread, and
  Win32's own thirty-second bound ends it as `ClipboardError::Timeout`; the owner's frames keep
  turning meanwhile, and the requests behind it wait. A read whose readers all went away is still
  performed: a handle's shared read waits for the host's answer whatever its readers do.
  Everything the thread runs that it does not own is contained: a request's conversions and
  `tracing` events (a subscriber is user code) and the delivery of its answer. A request whose
  work panics is answered `Unavailable`, the first failure is reported and discarded, a panic in
  the report is discarded too, and the thread goes on to the next request. Nothing else in the
  thread's loop runs code it does not own (its window's messages cross `os_entry`), so the loop
  has no failure to restart from; were it to end all the same, its channel closes and every later
  request answers `Gone`, as every request does when the thread could not start. The thread uses
  `GetClipboardData`, which needs no COM apartment; `OleGetClipboard` would tie the data object to
  the thread's apartment and is not used. Since only this thread opens the clipboard, FLUI's
  sessions cannot overlap, and the process-wide clipboard lock that serialized sessions opened
  from any thread is gone.
- **§4. A late answer reaches only its request.** An answer settles the completion its resolver
  was made for: the resolver carries the completion's generational slot, so an answer after the
  caller dropped the completion settles nothing. While the thread holds the clipboard open no other
  window can change it, so a read's answer is the clipboard's at that moment;
  `GetClipboardSequenceNumber` adds nothing to the order the queue already gives.

### Consequences

- Good, because a copy that another application blocked is no longer lost silently, and a cut
  keeps the text the clipboard did not take.
- Good, because no other process can freeze the Win32 owner thread through the clipboard.
- Good, because a host that cannot reach a system clipboard now (an insecure web page, a Wayland
  window without an input serial) still cuts, copies and pastes inside the application, and says
  so.
- Bad, because a cut deletes its selection one owner turn after the chord rather than inside it.
- Bad, because every Win32 clipboard answer crosses a thread, and the requests behind a read of a
  late renderer wait up to Win32's thirty seconds.
- Neutral, because AppKit, UIKit and Android still read on the owner thread: a pasteboard item
  another application provides lazily, or an Android clip whose text is a content URI, still
  holds it while the other application answers ([gaps](../gaps.md#clipboard-is-broken-on-two-environments)).

### Confirmation

- `clipboard::patience` tests: `a_held_clipboard_is_tried_on_a_doubling_schedule_until_the_patience_ends`,
  `the_queue_has_a_bound_of_its_own`, `the_delay_saturates_without_overflow`.
- `clipboard::windows::tests` on Windows: `a_write_and_a_read_while_the_clipboard_is_held_for_a_moment_land_once_it_is_let_go`,
  `a_clipboard_held_past_the_patience_refuses_the_write`, `a_write_that_cannot_be_made_leaves_the_clipboard_as_it_was`,
  `a_panicking_subscriber_leaves_the_clipboard_thread_serving`, `a_read_of_text_rendered_late_never_holds_the_caller`,
  `a_read_of_text_never_rendered_in_time_answers_timeout`,
  `a_burst_of_clipboard_writes_is_answered_in_full`, `a_concealed_write_stays_out_of_the_windows_history`.
- `clipboard::linux::wayland::tests::a_write_answers_ok_only_where_the_clipboard_serves_it`,
  `owner::linux::wayland::user_input::tests::the_keyboard_leaving_ends_the_last_input_it_had`,
  `clipboard::linux::owners::tests::text_held_under_no_seat_answers_until_a_newer_one`,
  `clipboard::linux::x11::tests::a_clipboard_write_answers_once_it_owns_the_selection` (Xvfb),
  `clipboard::page::tests::a_refused_write_is_not_read_back`,
  `a_write_the_page_alone_keeps_is_not_ok_but_is_read_back`.
- `scenario::a_scripts_observer_hears_every_answer_however_it_is_given`,
  `capability_seam::a_refused_write_answers_its_refusal`,
  `a_scripted_write_step_refuses_that_write_alone`,
  `the_scripted_clipboard_keeps_one_order_across_reads_and_writes` (`flui-testing`),
  `editable_text_clipboard::a_cut_deletes_only_what_the_clipboard_took`,
  `a_cut_the_application_alone_keeps_deletes_and_pastes_back`,
  `a_cut_answered_after_its_field_left_deletes_nothing`,
  `selection_menu::a_toolbar_cut_at_a_field_without_the_focus_is_refused` and
  `assistive_technology_cuts_and_pastes_in_an_unfocused_field` (`flui-widgets`).
- The compiler: `ClipboardBackend::write` and `NativeClipboard::write` cannot be implemented
  without taking the resolver.
