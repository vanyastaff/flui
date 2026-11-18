# ADR-0378: Preferences arrive by notice, are accepted once delivered, and a failed read backs off

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) decision 3 (Android
  reads preferences every 500 ms while resumed) and the Consequences' sentence that a resumed,
  idle app wakes every 500 ms for that read; decisions 1, 2 and 4 stand
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md) (the host owns the system
  preferences), [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) decisions 1 and 2
  (the loop blocks until woken), [ADR-0366](ADR-0366-an-android-activity-runs-while-it-is-on-screen-and-a-process-runs-one.md)
  (the Activity's on-screen gate), [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)
  (a payload nobody resumes is discarded)

## Context and problem statement

The AppKit and Android hosts read the system preferences through one source each, and both
cost the owner turns nobody needed:

- the Android loop read the preferences every 500 ms for as long as `FluiView` was not in the
  window (forever, if its installation stopped short), a JNI read per tick in the foreground;
- a read inside the owner's turn that found a change woke the owner again, so one AppKit
  notification or one `FluiView` report cost two turns;
- a failed or panicking read was retried every 500 ms with no limit, on macOS also while the
  application was inactive (a panicking getter gave 120 turns a minute), and on Android a panic
  in a read the loop made itself ended the Activity;
- a change a turn read was counted as accepted at once, so a turn that dropped it (its host
  replaced during the getters, `update_preferences` refused) left it undelivered until the next
  change.

`FluiView` already reports the settings once as it enters the window, after registering its
observers, so a change made before observation began reaches the loop without a cadence. The
question: when is a preference read, when does a read wake the owner, and how is a failed one
retried, on the hosts whose source is told of changes (AppKit, Android)?

## Decision drivers

- R11.2 of the platform layer: an idle owner wakes for no timer; a timer poll is a defect, not a
  fallback.
- One change costs one owner turn; a delivery that does not happen is not lost.
- A failure that never heals costs a bounded number of reads, and none while the application
  cannot use them.
- The rule is held by a test on the Linux host, which runs neither AppKit nor JNI.

## Considered options

1. Keep the Android cadence until `FluiView` is in the window; fix only the double wake.
2. Notices only, with one model for both hosts, acceptance on confirmed delivery, and a bounded
   retry.
3. Read on every owner turn and compare in the application, with no notion of a change in the
   host.

## Decision

Chosen option: 2.

1. **The system is asked on a notice, never on a cadence.** The AppKit and Android sources are
   `preferences::read::NoticedPreferences<S: SystemReads>`. Every application turn reads the
   source, which asks the system only when a notice came since its latest answer, or at a due
   retry, and otherwise returns that answer as it is. A notice is an AppKit notification (each
   observer runs `notice_handler`, which notices and wakes the owner), a setting `FluiView`
   reports (`JavaView::settings_changed` tells the source before it wakes the owner), and
   `ConfigChanged` and `Start`, which the Android loop hears through
   `owner::android::poll::LoopReads::hear`. `ConfigChanged` and a view report read at once,
   `Start` at the loop's next turn. The Android loop reads on its own only on its first turn,
   after `Start` and at a due retry (`LoopReads`); an installation of `FluiView` that fails or
   stops short reaches the owner once as `ViewEvent::Installed(Err(ViewMissing))`, logged there,
   and nothing samples in its place.
2. **A value is accepted when its delivery is confirmed.** The application's turn reads the
   source and, once `update_preferences` accepts the value, calls
   `Platform::preferences_delivered` (`OwnerPlatform::preferences_delivered`). The method is
   required on `Platform`, on the host position `host::Preferences::delivered` and on
   `SystemPreferenceSource::delivered`: a host or a source that has nothing to settle says so in
   an explicit no-op, so one that forgets it does not compile. Only a delivery of the latest
   read's answer is accepted; one of an older read (a delivery that re-entered the owner, which
   read and delivered a newer value, confirms its own read last) is ignored. A change read
   inside the application's turn (`OwnerSignal::in_application_turn`, not the owner-work part of
   a turn) is owed to that turn (`OwnerSignal::owe_delivery`): confirmed there it costs nothing
   more; unconfirmed, the turn's end wakes the owner once more, and a second unconfirmed read of
   the same value waits for a notice. A change read anywhere else wakes the owner. A read off the
   owner thread is refused with `PlatformError::Preferences` before the admission and changes
   nothing.
3. **A failed read backs off, then waits for a notice.** A read that fails or panics (the panic
   is contained in the source and its payload discarded, ADR-0281) is retried on
   `preferences::retries::Retries::new(500 ms, 6)`: 0.5, 1, 2, 4, 8 and 16 s after each failure.
   After the sixth retry the source reads again only after the next notice, and each notice
   then allows one read. How a retry wakes the owner is the system's `Retrying`: on AppKit a
   timer on the owner queue, armed only while the application is active (an inactive one is read
   on its activation notice); on Android the loop's wait, bounded by the source's retry
   deadline and gated on the Activity being on screen. A notice during a pending retry does not
   shorten its pause.

### Consequences

- Good, because an idle AppKit or Android owner makes no turn and no read, and one change costs
  one turn.
- Good, because a dropped delivery is read again, once, without waiting for the next change.
- Good, because a failing getter costs at most seven reads per notice, none in the background on
  macOS, and a panicking one no longer ends the Android Activity.
- Good, because the application's turn, which reads the preferences on every wake, asks the
  system nothing between notices.
- Bad, because `Platform` gains `preferences_delivered`, which an application that reads
  preferences itself must call to settle a change; one that never calls it gets at most one
  extra turn per change on these two hosts.
- Neutral, because the other hosts keep their own sources; their `preferences_delivered` does
  nothing.

### Confirmation

Tests on the Linux host drive `NoticedPreferences` through `SystemReads` with a counting
`OwnerSignal` and its turn, in virtual time: `an_idle_source_never_wakes_the_owner`,
`application_turns_ask_the_system_only_after_a_notice`,
`one_change_notice_is_one_owner_turn`, `a_change_a_turn_drops_is_read_again_once`,
`a_confirmation_of_an_older_read_is_ignored`,
`a_change_read_in_owner_work_wakes_the_owner`, `failed_reads_back_off_and_stop_until_the_next_notice`,
`a_panicking_read_is_a_failed_read`, `an_inactive_application_retries_only_on_its_next_notice`
and `a_read_off_the_owner_thread_is_refused_and_changes_nothing`
(`crates/flui-native/src/preferences/read/tests.rs`); the Android loop's own reads are held by
`the_loop_reads_on_its_own_only_at_start_and_at_a_due_retry` and
`each_preference_notice_tells_the_source_and_reads` (`owner/android/poll.rs`). The compiler
holds the required `delivered` on every host position and source. The wakes per
idle minute on a Mac and on an Android device are rows of `docs/platform-parity.md` ("Zero CPU
when idle").

## Pros and cons of the options

### 1. Keep the Android cadence until `FluiView` is in the window

- Good, because it changes the least.
- Bad, because a view that never gets into the window polls for the Activity's life, and the
  cadence stands in for a notice the view already sends.

### 2. Notices only, acceptance on confirmed delivery, bounded retry

- Good, because the idle cost is zero and the delivery cost is one turn.
- Good, because the model compiles on every host, so its tests run where CI runs.
- Bad, because the application confirms each delivery through a new `Platform` method.

### 3. Read on every owner turn and compare in the application

- Good, because the host needs no notion of a change.
- Bad, because a JNI or AppKit read on every owner turn costs the turns the read was meant to
  avoid, and an idle owner still needs a timer to notice anything at all.

## More information

`FluiView.onAttachedToWindow` registers its content observers and the contrast listener, then
calls `settingsChanged()` once (`crates/flui-native/android/java/dev/flui/platform/FluiView.java`).
Revisit when a host gains a preference with no notice at all: it then needs its own decision,
not a cadence.
