# ADR-0362: A concealed clipboard entry clears itself at its time and on session lock, if still there

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) §6 (a concealed write is kept out
  of history and sync; this record adds its lifetime),
  [ADR-0337](ADR-0337-a-clipboard-write-answers-and-win32-serves-the-clipboard-off-the-owner-thread.md)
  (a write answers, requests keep one order, Win32 serves the clipboard off the owner thread),
  [ADR-0343](ADR-0343-the-owner-promises-turn-deadlines-and-the-window-states-user-placement.md)
  §1 (the turn deadline the clear is served at),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the entry mint is a counter grant)

## Context and problem statement

A password manager copies a secret concealed (`ClipboardWrite::concealed`), and the secret then
stays on the clipboard until something replaces it: past the moment the user pasted it, past
the screen locking, past the application's exit. Password managers clear it after a time
themselves (KeePassXC, Bitwarden, 1Password, Rune's `copy_secret`), and every one of them has to
answer one question: is the entry on the clipboard still the one it wrote? Clearing a copy the
user made since, in this application or another one, loses the user's data; a clear that waits
for the check and then clears leaves a window where a new copy falls between the two.

What does a write ask for, which rule decides when the clear runs, and how does each host tell
its own entry from a later one?

## Decision drivers

- The clear takes only the entry the application wrote: identity is what the operating system
  gives (a sequence number and owner, a change count, a selection's ownership time, the source
  object, marks the clip carries), compared at the clear, with no window between the check and
  the clear where the system offers an atomic form, and the remaining race stated where it does
  not.
- No polling: the time is a deadline of the shared turn-deadline rule (ADR-0343 §1); a served
  deadline is not armed again; an older wake never clears a newer obligation.
- The rule lives at the seam, once, for every host, headless included; a host knows only how to
  clear an entry it recorded.
- A host without a conditional clear, or without a session-lock signal, says so in a type and a
  parity row.
- The expiry is checked: no zero, no time past what a clock can add.
- Neither the journal nor a log holds the concealed text.

## Considered options

1. An expiry on the write (`ClipboardWrite::expires_after(ClipboardExpiry)`) that names a new
   entry; the composition root owes each expiring write a clear at its time, served through the
   turn deadline and at a session lock; each host records the entry with the operating system's
   token when the write lands and clears only while the token still matches.
2. Each host arms its own timer and listens for the lock itself.
3. The application clears with a method on the handle at a time it chooses (`clear_if_ours`).
4. Compare the clipboard's text with a fingerprint of the secret at the time (Rune's way), polling
   the change count a few times a second.

Option 2 puts the rule in seven places, each with its own timer and lock listener, and leaves the
headless host to imitate them. Option 3 leaves every application to rebuild the deadline, the lock
and the exit, which is the defect. Option 4 polls, takes a later copy of the same text for the
entry, and reads the clipboard, which Android refuses in the background and a browser may prompt
for.

## Decision

Chosen option: 1.

### §1 The write

`ClipboardWrite::expires_after(self, after: ClipboardExpiry) -> Self` makes a concealed write
that names a new `ClipboardEntryId`, minted by the process's `AtomicIdMint` (never reissued; a
clone of the write names the same entry). `ClipboardExpiry::try_new` admits
`ClipboardExpiry::MIN` (a second: nobody can paste sooner) to `ClipboardExpiry::MAX` (a day: past
it the entry is no momentary secret, and every clock adds it without overflow) and refuses the
rest with `ClipboardExpiryError`. `ClipboardWrite::expiry` and `ClipboardWrite::entry` read them
back. The write's `Debug` shows its length and how it is kept, never its text.

What a host does with it is `ExpiryClearing` (`AtTimeAndLock`, `AtTime`, `Unavailable`), which
`ClipboardHandle::expiry_clearing` hands an application, as `concealment` does for concealment.

### §2 The rule, at the seam

The built-in clipboard provider (`flui-app`'s `ClipboardExpiries`, one per loop) owes every
expiring write one clear at its time, counted from the write's request, answered through the
writer's UI runtime:

- The earliest owed time is part of `AppRuntime::turn_deadline` on every host, and the owner's
  turn serves every clear due by its own time; a served clear is no longer owed.
- Each clear names its own write's entry, so a later write that lands (an expiring one of this
  application, an ordinary one, another application's) takes the clipboard from the entry and the
  host answers `ExpiredClear::NotHeld`: a second expiring write replaces the first's time, and
  the first's clear does nothing; where the second failed, the first entry is still there and
  its own clear takes it.
- `SystemNotice::SessionLocked` and `SystemNotice::Sleep` serve every owed clear at once, before
  the application's subscribers hear the notice.
- The loop's end asks the host to clear whatever entry is still there
  (`NativeClipboard::clear_expired_before_exit`), within the services' part of the exit deadline
  and at most `EXIT_CLIPBOARD_BOUND`.
- A clear the host could not do for now (`ExpiredClear::is_worth_a_retry`: the clipboard held by
  another process, out of sight in the background) is owed again on `CLEAR_RETRY`, at most
  `MAX_CLEAR_RETRIES` times. At most `MAX_OWED` clears are owed at once; past it the oldest is
  given up.
- No clear is owed where the host answers `ExpiryClearing::Unavailable`.

### §3 The host seam

`NativeClipboard` gains `expiry_clearing`, `clear_expired(ExpiredEntry, Resolver<ExpiredClear>)`
and `clear_expired_before_exit(Duration) -> ExpiredClear`. A clear is served in order with the
writes (ADR-0337 §3): one asked after a write is checked against what that write left. When an
expiring write lands, the host records its entry with the operating system's token; any other
write that lands forgets it; a clear clears only while the recorded entry is the one named and the
token still matches (`ExpiringRecord`, the pure part every host with a token shares).

| Host | Token | The check and the clear | Lock | The loop's end |
|------|-------|-------------------------|------|----------------|
| Win32 | `GetClipboardSequenceNumber` and `GetClipboardOwner`, read after the write's session closed (closing it moves the number), recorded only while the thread's window owns it | one open session: no other process can change the clipboard between the stamp and `EmptyClipboard` | `WM_WTSSESSION_CHANGE` | cleared if still there, waited for within the bound: the entry outlives the process otherwise |
| macOS | `NSPasteboard.changeCount` | two calls on the owner lane: a copy another process makes between them is cleared too | `com.apple.screenIsLocked` | cleared if still there |
| iOS | `UIPasteboard.changeCount`, and `UIPasteboardOptionExpirationDate` on the item | UIKit removes the item at its time, after the process too, and only that item; the loop's own clear is two calls, as macOS | none: `AtTime` | the expiration date stands |
| Android | the writer mark and the entry number in the clip description's extras, beside `EXTRA_IS_SENSITIVE` (from API 24; below it `Unavailable`) | two binder calls (`getPrimaryClipDescription`, then `clearPrimaryClip` from API 28, an empty clip before); without input focus the system hands no description, and the clear answers `Unseen` | none: `AtTime` | cleared if still there and in sight |
| X11 | the selection's ownership time and the owned content | `SetSelectionOwner(None, CLIPBOARD, time)`, which the server ignores once another client took the selection later: one step of the server | logind | the selection ends with the clipboard's connection |
| Wayland | the source the seat's selection was set with, and the write it answers | the source is destroyed; the compositor serves requests in order, so a selection set first has already cancelled it, and one set after takes the cleared selection | logind | the selection ends with the connection |
| Web | none | no conditional form: `Unavailable`, `ExpiredClear::Unsupported` | none | the entry stays |
| Headless | the in-memory clipboard's revision | one step under its lock | what the test reports | cleared if still there |

A clipboard manager that copied the entry while it was offered keeps its copy everywhere: the
concealment marker is what keeps one out (ADR-0152 §6), and the compositor's own clipboard
(Weston's module) or a manager without it re-offers the text after the source goes.

### Consequences

- Good, because a secret a password manager copies leaves the clipboard at its time and when the
  user walks away, on every host that can tell its own entry, and never takes a later copy.
- Good, because the rule is one module with one test table, and a host adds only its token.
- Bad, because macOS, iOS and Android check and clear in two calls: a copy made in the
  microseconds between them is cleared with the entry. Their systems offer no atomic form.
- Bad, because an Android application in the background cannot see the clipboard: its clear
  waits for the retry schedule and is given up after the last retry.
- Bad, because the web clears nothing, and iOS and Android hear no lock.
- Neutral, because `NativeClipboard` implementors outside the hosts (test doubles) answer three
  more methods.

### Confirmation

- `an_expiry_is_at_least_a_second_and_at_most_a_day` and
  `an_expiring_write_is_concealed_and_names_its_own_entry` in `flui-platform`'s `clipboard`.
- The rule: `a_clear_is_owed_at_its_time_and_served_once`,
  `another_applications_copy_survives_the_time`, `a_second_expiring_write_replaces_the_firsts_time`,
  `nothing_is_owed_without_an_expiry_or_a_conditional_clear`, `a_session_lock_clears_now`,
  `a_clear_the_host_could_not_do_is_tried_again` and `the_loop_end_clears_an_owed_entry` in
  `flui-app`'s `clipboard_expiry`, over a host whose operating system the test speaks for.
- The record: `a_record_forgets_an_entry_its_token_no_longer_matches` and
  `an_expiring_entry_is_cleared_only_while_it_is_still_there` (the in-memory clipboard).
- Win32: `an_expiring_entry_is_cleared_only_under_its_stamp` over scripted stamps, and
  `an_expiring_entry_clears_only_while_the_clipboard_still_holds_it` on the real clipboard,
  saved and put back, with another window's copy.
- X11: `an_expiring_entry_is_given_up_only_while_the_clipboard_still_holds_it` between two
  connections of Xvfb. Wayland: the expiring step of `the_selection_crosses_between_clients`
  under Weston.
- macOS, iOS and Android are type-checked; their device runs are notes of the parity row
  "Concealed clipboard entries clear themselves" in
  [`docs/platform-parity.md`](../platform-parity.md).

## Pros and cons of the options

### 1. An expiry on the write, the rule at the seam, a token per host

- Good, because identity is the operating system's own and the rule is written once.
- Bad, because every host carries a record and a token.

### 2. Each host times and listens itself

- Good, because a host could clear without the composition root.
- Bad, because seven timers and lock listeners drift apart, and none serves the turn deadline.

### 3. The application clears

- Good, because the framework adds only a conditional clear.
- Bad, because the deadline, the lock and the exit are left to every application.

### 4. A fingerprint and polling

- Good, because it needs no token from the system.
- Bad, because it polls, reads the clipboard, and takes a later copy of the same text for the
  entry.
