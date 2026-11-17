# ADR-0366: An Android Activity runs while it is on screen, and a process runs one

- **Status:** Implemented (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0345](ADR-0345-the-lifecycle-starts-where-its-window-is-and-every-change-walks-the-ladder.md)
  §1, its Android sentence only (`onResume`/`onPause` give the execution state);
  [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) decisions 2 and 3, the gate only
  (what they say of a resumed and a paused Activity); and
  [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decision 6, the reason of its `launchMode` bullet
  only (`singleTop` keeps a second Activity and a second Rust runtime from starting)
- **Related:** [ADR-0237](ADR-0237-window-state-and-host-notices.md) (Android's three signals),
  [ADR-0350](ADR-0350-a-quit-the-user-asks-for-asks-every-window-and-a-phone-holds-its-background-for-the-writes-in-flight.md)
  (what a phone does on its way to the background), [ADR-0255](ADR-0255-mobile-and-web-hosts.md)
  decision 2 (`FluiHost`'s queue)
- **Sources:** Flutter 3.50.0, `flutter/flutter@ab2912035e`
  [`engine/src/flutter/shell/platform/android/io/flutter/embedding/engine/systemchannels/LifecycleChannel.java`](https://github.com/flutter/flutter/blob/ab2912035e/engine/src/flutter/shell/platform/android/io/flutter/embedding/engine/systemchannels/LifecycleChannel.java)
  (a paused Activity is `inactive`, a stopped one `hidden` then `paused`; `resumed` needs the
  focus too); the `android-activity` 0.6.1 crate, whose native-activity glue's
  `ANativeActivity_onCreate` spawns one `android_main` thread per Activity instance

## Context and problem statement

The Android loop mapped `onResume`/`onPause` to the window's execution state, and gated every
frame, the input queue, the preference reads and the surface on it. Android keeps a paused
Activity on screen wherever another one holds the foreground without covering it: the other half
of a split screen below API 29, a translucent or dialog Activity on top, picture-in-picture. Such a
window released its surface and froze; owner jobs that run in its frames (posted jobs,
completions, a dropped ticket's cancel) waited for the user to tap it. Android's multi-window
guidance is to keep drawing until `onStop`.

`FluiHost` holds the queues `FluiActivity` fills (trim levels, intents, activity results) and one
wake handle, in Java statics. `singleTop` only reuses an instance at the top of its own task, so a
`VIEW` or `SEND` from another application starts a second `FluiActivity` in the caller's task, and
`android-activity` starts a second `android_main` for it. The second platform's `bind` replaced the
wake handle, so the first one's intents and trim levels woke nobody; its `unbind` then cleared the
handle for both. Between a new instance's `onCreate` and its `bind`, a wake went to the old
platform, which drained the new instance's intent.

Which lifecycle fact gates an Android window, and how many FLUI platforms does a process run?

## Decision drivers

- A window the user can see keeps drawing and taking input, as on every other host; only a window
  off screen stops (ADR-0035 §3: frames off still serve background work where the host has a
  pump).
- The lifecycle a listener sees walks the same ladder as on every host (ADR-0345 §3): a paused,
  visible Activity is `Inactive`, a stopped one `Hidden` and then `Paused`.
- Nothing an Activity receives (an intent, an answer, a trim level) reaches a platform that is not
  the one the user sees, and nothing is lost to a replaced wake handle.
- The rest of the stack (flui-app's thread-local runtime, the application's own statics) is
  written for one platform per process.

## Considered options

For the gate:

1. Running while visible (`onStart` to `onStop`); the foreground refines the focus.
2. Running while resumed (`onResume` to `onPause`), as before.

For two instances:

3. One platform per process: the queues belong to one Activity, and a second instance hands its
   intent to it and finishes.
4. One queue and wake handle per Activity, and as many platforms as instances.
5. `launchMode="singleTask"`, so Android never creates a second instance.

## Decision

Chosen options: 1 and 3, because a visible window that freezes is the defect, and a process that
runs one platform is what the rest of the stack assumes, while `singleTask` would cost the
application the activities it started for results.

### §1 The window runs while the Activity is on screen

`owner::android::activity_callbacks::ActivityStanding` holds the three facts (`visible`, `foreground`,
`focused`) that `onStart`/`onStop`, `onResume`/`onPause` and `onWindowFocusChanged` report
(`ActivitySignal::{Visibility, Foreground, Focus}`), and answers what the window reports:

- `runs()` and `execution()`: running while visible, `Suspended` from `onStop` to `onStart`;
- `focus()`: focused while focused and in the foreground, so a paused Activity is `Inactive` from
  `onPause` on even before its focus report, as Flutter's lifecycle channel reads it.

The loop and the window share the standing as one atomic byte (`ActivityFlags`). The loop's frame
gate, its input drain, its preference cadence and `poll_timeout` read `runs()`; `Stop` releases
the surface (after recording the suspension, before telling the window) and `Start` asks for it
again; `Pause` ends the input contacts and changes nothing else. While the Activity is off screen
the loop still answers every input event, as unhandled, so the system hears back about the cancel
it sends for a window it took away.

A stopped Activity draws no frame, so the application's owner jobs, which run in the window's
frames on Android (`AppRuntime::owner_turn_window_poke`), wait for `Start`, as on iOS, whose
backgrounded pump also runs only between a frame and the scheduler disabling frames (ADR-0268).
The loop's own turn still serves the host's queue and the result bridge while stopped: a request
whose ticket was dropped before the Activity stopped is cancelled at once.

### §2 A process runs one FLUI platform

`FluiHost.created(activity)`, called from `FluiActivity.onCreate` before `NativeActivity` starts the
library, makes `activity` the owner of the queues unless another owner is live and not finishing.
Only the owner binds a wake handle (`bind(Activity, long)` answers `false` otherwise) and reads the
queues (`peekEntries`, `peekOpened`, `peekAnswers` take the Activity and answer nothing to another);
a new owner forgets the old wake handle, and a destroyed owner gives the queues up. A second
instance hands its intent to the owner as an intent to the running Activity
(`FluiHost.opened(intent, false)`), brings the owner's task to the front
(`ActivityManager.AppTask.moveToFront`) and finishes; its platform refuses to start with
`PlatformError::SecondActivity`, logged, before any native method is bound
(`java_host::check`). An Activity created while the owner is finishing (a relaunch right after
Back) becomes the owner, and the finishing platform hears nothing more. The manifest keeps
`singleTop`. `FluiHost.PROTOCOL` is 7.

### Consequences

- Good, because split screen, picture-in-picture and a dialog Activity on top show a live window,
  and its owner jobs, animations and input continue.
- Good, because a share or a document from another application reaches the running application,
  in its own task, and no platform drains another's intent or answer.
- Good, because a touch held while the Activity stops is answered, not reported as not responding.
- Bad, because a stopped Activity runs none of the application's owner jobs: code that would drop a
  ticket while a full-screen started activity covers FLUI runs when FLUI is shown again. The host's
  own sweep still ends a request whose ticket was dropped before.
- Bad, because a launcher or a large-screen shell that opens a second window of the application
  (`FLAG_ACTIVITY_MULTIPLE_TASK`) gets the running one brought to the front instead.
- Neutral, because a visible, paused Activity now pays its frames, which the user sees.

### Confirmation

- `a_paused_activity_that_stays_visible_keeps_running` (flui-native, `lifecycle::android`) replays
  each callback run through the rule and the standing the loop and window read: a paused,
  visible Activity runs and is `Inactive`; only `Stop` suspends it.
- `a_stopped_loop_blocks_until_it_is_woken` and
  `a_stop_during_poll_preserves_redraw_until_a_running_iteration` pin the poll and the redraw gate
  on the Activity running.
- `the_platform_starts_only_over_its_own_java_classes` refuses a second instance with
  `SecondActivity`, and `only_the_activity_that_owns_the_queues_binds_and_reads_them` holds the
  Java side to §2; `cargo xtask android-dex` holds the dex to the sources.
- A run on a device is owed (`crates/flui-native/platform-parity.toml`, the "Suspend, resume,
  GPU-surface loss" row).

## Pros and cons of the options

### 1. Running while visible

- Good, because it is what Android's multi-window guidance asks and what Flutter's embedder does.
- Bad, because a paused Activity under a translucent one renders frames the user may barely see.

### 2. Running while resumed

- Good, because it renders nothing behind another Activity.
- Bad, because every visible, paused window freezes, which on API 24 to 28 is half of every split
  screen.

### 3. One platform per process

- Good, because flui-app, the application's statics and the queues all assume one platform.
- Good, because the user's running application takes what another application opens.
- Bad, because a second window of the application cannot exist.

### 4. One queue per Activity, one platform per instance

- Good, because each instance is independent, as Flutter's engines are.
- Bad, because two platforms in a process share every process-wide state the stack and the
  application keep, which nothing is written for, and the leftovers of a destroyed instance need a
  rule for which instance inherits them.

### 5. `singleTask`

- Good, because Android itself routes every launch to the one instance.
- Bad, because a launch from the launcher clears the activities above the root, so a file picker or
  any activity started for a result is destroyed when the user switches back through the launcher.
- Bad, because a share from another application moves the user out of the sender's task.

## More information

Revisit §2 when a second window of the application is wanted on Android (large screens, desktop
mode): that is option 4, with the process-wide state it needs moved per platform first.
