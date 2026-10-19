# flui-platform Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

The contracts this crate's backends implement — `PlatformWindow`,
`PlatformTextInput`, `PlatformHaptics`, `Clipboard`, the display snapshot
vocabulary (`DisplaySnapshot`, `DisplayId`, `RefreshPeriod`), the
`data_transfer` transport and the window and input vocabulary — live in
`flui-platform-api` (ADR-0082) and are re-exported here at their old paths
(`flui_platform::X`, `flui_platform::traits::X`, `flui_platform::data_transfer`),
so a backend or `flui-app` names them as before. What stays here is what names
an OS stack, AccessKit or tokio: `Platform`, the owner-thread capability,
`HostWindow` and every backend. `PlatformAccessibility`, the trait the backends'
AccessKit adapters implement, is `flui-semantics`' (ADR-0082 §2, amended) and is
re-exported at `flui_platform::traits`. Only `flui-app` may
depend on this crate (`allowed-dependents`); a crate that needs a capability
trait depends on `flui-platform-api` instead.

## Invariants

- **The winit fallback is a desktop backend.** Its module, public re-exports
  and `arboard` clipboard dependency are available only on Windows, macOS and
  Linux under `winit-backend`. The `desktop` compatibility selector enables no
  dependency; native desktop backends are selected by target OS. Enabling every feature on Android or iOS keeps the native backend;
  it does not enable desktop clipboard code. The required-target and
  all-features `cargo xtask cross-typecheck` configurations compile this
  boundary, including the selected packages' tests, examples and benches.
- **A window leaves a backend as an `Arc<dyn HostWindow>`.**
  `HostWindow: PlatformWindow` adds the one AccessKit-speaking accessor,
  `accessibility()`. `Platform::open_window`, `WindowOpen::Ready` and
  `PendingWindow` return it; every backend window implements it next to its
  `PlatformWindow` impl, and its accessibility bridge is fixed when the window
  is built, so a runner may read it once. `dyn HostWindow` has its own
  `HasWindowHandle`/`HasDisplayHandle` impls: without them an `open_window`
  result would not be a `flui_engine::WindowTarget` until upcast.
- **A window publishes a state snapshot and takes commands (ADR-0237).** There
  is one window trait. `PlatformWindow::state` returns an immutable
  `WindowState` with a revision, `apply` takes a `WindowCommand`, and
  `capabilities` says which commands the window honours: `apply` answers
  `Unsupported` exactly when the matching capability is `false`, and the
  capabilities of each backend are what its `apply` carries out today. The
  snapshot describes what the OS reported, not what a command asked for.
  Every backend keeps its last snapshot in a `shared::window_state::WindowStateCell`
  (built like `DisplaySource`): an OS event marks the cell stale, the next read
  samples the OS with no lock held, and a sample equal to the published one
  keeps the revision. `on_state_changed` is raised after the publish, outside
  the cell, only when the revision grew. Win32 and AppKit refresh from their
  window messages and delegate callbacks, winit from its window events, the
  Web from the layout, focus and visibility observers; the headless window
  keeps a model that stands in for the OS (`HeadlessWindowPolicy` makes it clamp a size
  or decline a maximize) and publishes after each command or `simulate_*`
  call. The iOS window rereads its metrics on each `state` call and does not
  yet raise `on_state_changed` from its view's metric updates.
- **The macOS backend enforces the window thread-affinity default
  mechanically.** `PlatformWindow`'s rule is that a method driving a window runs
  on the thread that owns the platform event loop. Every window-driving
  `PlatformWindow`/`MacOSWindowExtTrait` body on `MacOSWindow`
  re-enters the owner lane through `route_on_owner`, so a call from any thread
  is marshaled onto the window's owner lane before any AppKit message is sent.
  Two carve-outs: the class-E raw-handle accessors
  (`raw_window_handle`/`window_handle`, and `display_handle` which carries no
  pointer) take their NSView outside the routing — `!Send` outputs whose
  enforcement is upstream (raw-window-metal's main-thread hard panic plus
  `debug_assert_appkit_main_thread`-guarded platform entries, ADR-0039); and
  the `enable_tiling`/`disable_tiling`/`is_tiling_enabled` trio never routes
  (recorded for observability only until the native API is adopted).
- **Linux host events reach the owner through an inbox, and a machine that
  waits for the application waits for delivery, not for receipt.** Two sources
  run beside the owner and never run application code: an async logind task
  (`linux::host_notices::logind`) and a kernel-pressure thread
  (`linux::host_notices::pressure`). Each posts a `HostEvent` to one `Inbox`
  and wakes the owner, which drains it in its next turn
  (`platform/linux_notices.rs`). logind's sleep and shutdown carry a
  completion: the delay inhibitor is held until the owner has delivered the
  event (a panic in a subscriber drops the completion too) and is released
  after at most four seconds, inside logind's own `InhibitDelayMaxSec`.
  Shutdown on Linux is a notification: the answer to the session-end query is
  not read, because logind cannot be asked to wait for the user. The decisions
  (signal decoding, trigger requests, X11 atoms to mode) are pure models in
  `linux::lifecycle_model`, checked on every host.
- **The X11 window probe uses two connections on purpose.** One queries from
  whichever thread asks and sets the properties a client sets on an unmapped
  window; the other is read only by the watch thread, which records which
  windows' `WM_STATE` or `_NET_WM_STATE` changed. A reply wait on the
  watching connection would read events into its queue while the thread sleeps
  in `poll`, so no request there waits for a reply. A window that opens
  maximized or fullscreen is created unmapped, gets its mode written into
  `_NET_WM_STATE`, and is mapped afterwards, because a window manager reads
  the mode from the property when it maps a window.
- **An OS version is compared only inside `shared::os_version` (ADR-0244).**
  Windows, iOS and Android each have one version type (`WindowsBuild`,
  `AppleVersion`, `AndroidApi`), read once when the platform is built and kept
  in it, with its number private. An API newer than the floor in
  `platform-support.toml` is a marker type implementing `Gate`, whose doc says
  what the code does without it; `version.since::<F>()` returns a `Since<F>` proof
  or `None`, and the gated call takes the proof. A gate at or below the floor fails
  to compile, so raising a floor turns the gates it makes dead into errors, and an
  unreadable version unlocks nothing. Clippy refuses `objc2::available!`. The
  table `os_version_gate_table` checks every marker at the version before its
  `SINCE`, at `SINCE` and at an unknown version.

---

## Mapping decisions

### Android captures safe-area geometry on the Java view thread

System-bar and display-cutout insets are sampled from the Activity decor view.
Packaged applications require API 23, where `View.getRootWindowInsets` becomes
available; both SDK-tools packaging and generated Gradle configuration enforce it.
The owner accepts a reply only while its window remains live and the sampled
physical size matches the current native window. Stable insets exclude keyboard
occlusion. One query may be outstanding; accepted changes dispatch callbacks
after cache guards are released. `android_safe_area_native_metrics` pins density
conversion, stale orientation rejection and invalid conversion inputs; native
rotation and resume additionally require a device run.

Android printable keyboard keys come from the input device's `KeyCharacterMap`
using the event's native meta-state. Physical codes and named keys keep their
existing translation. A dead key remains `NamedKey::Dead`; unavailable maps
retain the legacy US keycode fallback, including Shift and Caps Lock. A successful
map reporting no character remains unidentified. `android_native_key_map_preserves_logical_keys`
pins the projection, while actual layout and Shift mapping require live input.

### Web subscriptions belong to one actual canvas registration

The canvas owns an exact DOM-object registration claim. Another live owner of
that same node is refused before modifying its layout or title; identifiers
and labels are not ownership. Keyboard listeners on the canvas require both
that source target and effective document/root focus before reading key data.
The existing dispatch result independently controls ancestor propagation and
cancelable defaults. A moved canvas retains ownership inside open or closed
shadow DOM; unrelated host inputs remain outside it.

Each private immutable Fn token owns its body and exact removal identity. JS
wrappers hold weak body references. Close fences delivery before detaching all
subscriptions and disconnecting the resize observer, and preserves admitted
callback/close order through the existing window FIFO. Final owner Drop uses
the same idempotent retirement. A refused detach transfers only its inert
weak wrapper to wasm-bindgen's managed JS lifetime; application captures still
retire. Normal removal drops the token without page-lifetime retention.
Installation rolls back its prefix before Created. ResizeObserver absence uses
the explicit window-resize fallback; failure of that mandatory fallback refuses
opening. The platform frame slot retires by callback-set identity, so stale
owners cannot erase replacements or publish redraws.

Web visibility follows the canvas document's `hidden` value, independently of
focus and CSS layout visibility. Construction samples it; an owned mandatory
Document `visibilitychange` listener updates it. Successful registration
reconciles both focus and visibility before Created, including source changes
during earlier listener installation.

Native focus and visibility observations carry their cache commit in the existing FIFO. A
queued callback therefore observes its own delivered value through getters,
even when input or focus callbacks synchronously trigger opposing transitions.
Accepted native focus and visibility observations also finish their commits and notifications
after a dispatch unwind. Recovery retains the same FIFO so a reentrant
observation follows the already accepted tail; ordinary aborted events are
discarded. `the_owner_thread_machinery_honours_its_contracts` pins single,
opposing and reentrant observations, a competing notification panic and the
next healthy observation through the private backend commit seam.

Native pointer capture custody is published on the exact canvas claim, apart
from its Rust buttons cache. A live body rechecks its admission after DOM
getters, translation and native capture calls. A host call that closes before
actually capturing cannot leave an outgoing capture behind. Outgoing release
checks whether a different current claim admitted that same pointer and does
not release the newer owner's capture. Retirement removes all of its native
admissions before the first release, which itself may reenter the host.
Terminal close, clear and retirement notices finish after an admitted callback unwinds;
ordinary nested events from the aborted transaction remain discarded. The
portable `panics_in_callbacks_and_tasks_leave_the_dispatcher_usable` family
pins capture release, the original failure, a competing retirement failure and
a subsequent healthy owner. Terminal close preserves the application's actual
cancellation hook before clearing captures; ordinary aborted events are not
replayed. An opaque FnOnce environment that double-panics during invocation
before reaching the containment boundary cannot be rescued. These checks do
not establish wasm panic-abort recovery.

The `browser_input_probe` canvas-ownership control and existing vocabulary,
pointer/wheel and getter-reentry controls exercise real registered DOM delivery
through retained public handles. Their README gives execution instructions;
browser compilation alone does not prove these manual contracts. Headless
source-focus controls are distinct from real system/page focus transitions.
The document-visibility control supplies a controlled `Document.hidden`
source rather than claiming a real background-tab transition. Constructor
events do not establish physical keyboard layouts or OS IME.

### Apple displays are resampled on events and keyed by display, not by connection

`display_snapshot` runs on every owner wake, so the macOS and iOS sources keep
the last sample and read the screens again only after something marked it
stale.

**macOS.** `NSApplicationDidChangeScreenParametersNotification` (attach,
removal, mode, refresh rate, arrangement, scale) is the invalidation: the
handler stores one atomic flag and wakes the owner, and the owner samples on its
own turn. A window raises `on_display_changed` from `windowDidChangeScreen:`
and from a backing-property change, and marks the snapshot stale first because
the snapshot carries each display's scale. A window that resolves its screen to
a display the published sample does not hold (a monitor plugged in since)
marks it stale too. The key is `CGDisplayCreateUUIDFromDisplayID` under
`DisplayKey::MacosUuid`, not the `CGDirectDisplayID`, which changes when the
same monitor is replugged. A screen with no `NSScreenNumber`, or no UUID, is
left out of the snapshot (a placeholder key would be shared by two such
screens), and a window on it reports no display. The function lives in
ColorSync, so the backend links that framework as winit does.

**iOS.** The source retains each `UIScreen` it numbers (`ScreenLedger`) and
keys the display `DisplayKey::IosScreen(number)`. A screen is recognised by
object identity against the retained ones, never by address: a released
screen's address can be reused, and its number is not. Screen and scene
notifications invalidate the snapshot; a window re-reads its scene's screen
when layout runs, since a scene can move to another screen without a new
attachment. The period in the snapshot is the cadence the window's
`CADisplayLink` measures (`targetTimestamp - timestamp`), published for the
screen the window is on only when it leaves a 5% band around the last
published value, so an adaptive ProMotion panel does not republish every tick.
Nothing is reported before the first tick (not the maximum), and a link that is
invalidated takes its period back.

**ProMotion.** An iPhone caps the link at 60 Hz unless the bundle sets
`CADisableMinimumFrameDurationOnPhone`, which `flui build` writes into the
`Info.plist`. On iOS 15 and later the link is asked for
`preferredFrameRateRange` 60 to `maximumFramesPerSecond` (a maximum under 60
lowers the minimum with it); before iOS 15 the property does not exist and the
link gets `preferredFramesPerSecond` instead. The platform reads
`NSProcessInfo.operatingSystemVersion` once into a value it owns (no `static`),
and the range call takes a `Since<FrameRateRangeProperty>` proof that only the
version check makes, so it cannot be written without the check. The 60 Hz
floor and the 5% band are choices to confirm on a device; no Apple code runs in CI.
macOS has no version-gated call here, so it reads no version.

The rules (UUID keying, screen ledger, version proof, frame-rate request, band)
are plain functions in `shared/` so Linux CI runs them; the UIKit and AppKit
glue around them is only type-checked: no Apple code runs in CI.

### Native text sizing captures conversion inputs on the owner lane

`HostWindow` and `OwnerPlatform` acquire an owner-local `CapturedTextSizing`;
native capsules remain in the backend and app composition root. Foundation
requests identify validated authored size and growth profile, and replies carry
only validated logical sizes. A retained capsule does not authorize publication
after presentation or capture replacement (ADR-0183).

Android copies `DisplayMetrics` before applying the native SP converter and
dividing by the captured density. UIKit retains the complete trait collection
and uses each profile's `UIFontMetrics` with a regular system reference font to
obtain numeric point sizes; portable shaping still selects the authored face.
Neither backend samples a sparse curve or substitutes a scalar for missing
native answers.

The private `android_capture_delivery_receipt` matrix exercises dropped and
stale receipts, bounded retry, settled inboxes, deadline expiry and late healing
through the acquisition protocol. It also runs on a non-Android test host, which
does not execute JNI conversion. Native Android and UIKit conversion require
their SDK and device verification; portable receipt tests do not establish it.

### Owner capability boundaries have external compiler witnesses

`OwnerPlatform` stays owner-local, and callers obtain its thread-safe residual
through `shared`. A text-store host is reached through that owner capability;
the hidden token cannot be imported or forged with `Default` outside this crate.
`trybuild_ui::ui_tests` pins the external caller diagnostics alongside valid
owner-local access and `SharedPlatform` thread bounds. It also checks each
retired tracker, embedder and timestamp name independently (ADR-0082 §5).

### Win32 mouse samples preserve their queued generation times

Each window rebases `GetMessageTime` onto the shared process epoch using
`GetTickCount64`. The 32-bit message tick is extended by a wrapping subtraction
from current uptime, so an uptime rollover cannot compress a queued gesture.
Messages older than an entire 32-bit wrap cannot be distinguished by Win32's
timestamp. Sent messages inheriting an older retrieved timestamp cannot move
the window's clock backwards. The sample timestamp is committed before hover
callbacks or native capture changes can dispatch another message.

`queued_mouse_samples_keep_native_message_time` drives the public native window
with delayed queued mouse samples. The private arithmetic row
`message_clock_preserves_wrapping_samples_and_window_epochs` covers rollover,
older sent timestamps, epoch rebasing and saturation: there is no public API
that advances the OS uptime by 49.7 days. These checks do not establish physical
mouse delivery, pen/touch translation, or other backend clocks.

### Win32 pointer readings keep counter resolution

`POINTER_INFO.dwTime` advances with the ~15.6 ms system tick, so a coalesced
120–240 Hz pen or touch burst from `GetPointer*InfoHistory` shares one value and
a velocity fit would see zero intervals. Each reading is stamped from its
`PerformanceCount` instead, rebased onto the shared epoch through a counter
reading taken beside the window clock's epoch (`Instant` is the same counter on
Windows). `dwTime` stamps a reading only when its source reports a zero count
or the counter could not be read. Conversion runs in 128-bit arithmetic and
saturates at the epoch range.

The current reading is the newest history entry and is decoded with it, so the
event and its last coalesced sample carry one time.

The private `native_pointer_decoding_contracts` rows
`coalesced_burst_is_ordered_by_performance_count` and
`current_reading_is_stamped_from_its_performance_count` drive the producer's
history decoding with one shared `dwTime`; the `reading_without_*` and
`count_*_epoch_*` rows pin the fallback and range. No public API synthesizes a
native pointer history. They do not establish physical pen or touch delivery.

### Browser capture ends each admitted contact once

The canvas requests native pointer capture before delivering Down. Its
owner-local admission set is updated before release or input callbacks:
Up and browser Cancel consume admission, and a capture loss cancels only a
still-admitted contact. The loss following a normal terminal event stays inert,
and a later contact can reuse the pointer identity. Native capture refusal is
reported through diagnostics without discarding the original Down. Synthetic
DOM events do not create native capture admission.

The DOM's floating-point `offsetX`/`offsetY` getters supply logical pointer and
wheel positions. Local wasm-bindgen getters preserve their CSSOM View double
precision where web-sys exposes the historical integer signatures. Some
browsers round wheel offsets themselves. For a canvas whose whole ancestry
has no CSS coordinate transform or zoom, viewport coordinates minus its border
box origin and border widths recover the padding-edge position, including
fractional origins. A transformed canvas retains the browser's local offsets;
its wheel precision remains browser-limited rather than applying an incorrect
bounding-rectangle inversion.

`examples/browser_input_probe` checks browser cancellation, duplicate capture
loss and recovery, fractional pointer/wheel positions, and a trusted drag whose
release occurs outside the canvas. Its README describes execution. These are
browser smoke contracts, not automatically executed CI tests; a wasm compile
does not prove that they pass. Browser touch-action policy remains page-owned.

### The Windows display snapshot is cached and invalidated, not sampled per turn

The host reads the snapshot on every owner turn, and one sample costs an
`EnumDisplayMonitors` pass, a `QueryDisplayConfig` and a few
`DisplayConfigGetDeviceInfo` calls. `WindowsDisplays` therefore answers from its
last sample until a flag set by `WM_DISPLAYCHANGE` (on the hidden settings
receiver, which handles nothing else of the display: it sets the flag and
wakes the owner, because the message can arrive inside a COM call), a
`SPI_SETWORKAREA` setting change, or a window's `WM_DPICHANGED` says the displays
may differ. The flag is cleared before the read, so an invalidation that lands
during a sample survives it. winit on Windows takes its snapshot from the same
source (its monitor list and GDI-name key are not read) and re-samples where
winit reports a move, a scale change or a resume.

The refresh period is the path's `targetInfo.refreshRate` rational, converted
through `RefreshPeriod::from_ratio` so 60000/1001 stays exact. A monitor whose
path has no rate (0/0) or has no path takes `EnumDisplaySettingsW`'s
`dmDisplayFrequency`, where 0 and 1 mean unknown. `QueryDisplayConfig` passes
`QDC_VIRTUAL_MODE_AWARE` only with the `Since<QdcVirtualModeAware>` proof `WindowsBuild`
hands out for build 18362 and later; the build comes from `RtlGetVersion` once
and lives in the source, not in a `static`.

A window's display is the sampled monitor whose handle equals
`MonitorFromWindow(MONITOR_DEFAULTTONEAREST)`, resolved through that monitor's
native key. The lookup reads the last sample and never takes one, because the
host calls it while deriving window displays from a snapshot it already
accepted. A handle the sample lacks (a monitor plugged in since) answers `None`,
never the primary, and invalidates the snapshot once per revision (a repeated
miss costs nothing), so the next owner turn samples it. The window raises `on_display_changed` from `WM_WINDOWPOSCHANGED` when that handle differs
from the last one it saw (the default procedure derives `WM_MOVE` and `WM_SIZE`
from it, so a drag on one monitor raises one hint at most) and from
`WM_DPICHANGED`. A move across monitors of different DPI raises two hints
(the monitor change, then the DPI change); both only wake the owner, which
derives once per turn, so they coalesce. Rows: `os_display_classification` and `os_capability_table`
(pure), `display_change_message_invalidates_the_snapshot_and_wakes_the_owner`
(in `src`, it counts samples, as do the DPI and work-area rows) and the
native contract rows `refresh_is_the_rational_active_mode`,
`window_display_is_a_display_of_the_snapshot`, `dpi_change_raises_the_display_hint` and
`moves_within_one_monitor_raise_no_display_hint`. A transition between two monitors needs a second monitor and
is not a CI row; it ran by hand with `examples/displays.rs` (see
`docs/platform-parity.md`). Unplugging a monitor on Windows was not run.

### Windows preferences have a window-independent owner lifetime

`Platform::preferences` reads text scale, animation preference, high contrast,
double-click interval, discrete wheel distances and ordered UI languages before
any user window exists.
Unknown observations remain absent. The owner context holds one WinRT STA entry,
native interfaces and subscriptions. Foreign reads are refused. Direct
`RoActivateInstance` avoids generated process-wide factory caching: recreating a
host after its apartment ended reproduced a stale-factory access violation with
`UISettings::new`. `windows_reads_preferences_before_a_user_window_exists` in
`preferences_contract` exercises successive host lifetimes, refusal, repeated
native reads and subsequent native window creation. Its independent `UISettings`
reader compares the public motion observation with the current Windows animation
setting; merely returning a present value cannot satisfy the assertion.

UI languages use `GetUserPreferredUILanguages`, sharing this source's receiver,
cache and retry lifetime (ADR-0173). The private
`preferred_language_buffer_contract` injects capacity races and malformed UTF-16
lists that cannot be produced deterministically through the public OS API. It
pins ordered complete tags, count/terminator validation and bounded retries;
failed query outputs never become a preference observation.

The same source samples mouse rectangle/drag metrics at an explicit 96-DPI
bootstrap context without user windows. Those raw observations only invalidate
consumers. Each presentation queries `GetDpiForWindow` and
`GetSystemMetricsForDpi` for its live HWND; scaling a representative window's
metrics cannot reproduce this query. Full double-click dimensions and drag
half-extents remain distinct, and legitimate zero axes are retained. The public
native row compares the projection with independent Win32 reads and checks
foreign-thread and closed-window refusal.

The source's hidden top-level HWND receives setting broadcasts outside user-window
membership and exit policy. WinRT text-scale and animation observers and the HWND
mark a refresh obligation and wake the owner; they never invoke runtime code.
Registering a new owner hook also wakes it if a refresh remains pending. Reads
clear the prior obligation before sampling so an invalidation during a read is
retained; error or unwind restores the obligation. A clean source reuses its last
observation. Reentrant reads are refused while native getters run, including when
an older cached observation exists; a failed refresh is returned as an error,
never as a successful stale observation. The private getter-injection table
`native_preference_read_recovery` covers cache reuse, in-read invalidation,
reentrant refusal and recovery after errors and panic. This private seam injects
failures that the public OS getters cannot deterministically produce.
An unfinished read also arms a 100ms source retry deadline. The source
admits failed-read attempts no more often than that interval even when
unrelated wakes arrive. Early and reentrant reads return `PreferencesDeferred`,
which publishes no observation. A retry wake does not postpone the read's
eligibility; healthy changes are not delayed. The deterministic row
`unrelated_wakes_do_not_repeat_failed_native_reads` covers this admission boundary.
The native message loop includes the retry in its timed wait independently of frame deadlines and wakes
the owner when due. It rearms before posting and disarms after a clean read;
shutdown excludes the deadline. `preference_failure_retries_without_a_user_window`
injects native-getter errors and panic, runs the actual Win32 loop without user
windows, and checks recovery followed by quiet idle.
Shutdown closes notification admission before removing subscriptions, whose
captures contain only ref-counted inert state.

`setting_messages_wake_the_owner_without_a_user_window` sends a message to the
platform's own native receiver and checks owner delivery and teardown. It does not
change OS preferences or establish actual OS-generated notifications, value-change
delivery or retry after a failing owner consumer. Those remain
pending under ADR-0172. The headless row checks that absent observations do not
claim OS defaults.

`native_setting_message_replaces_the_cached_observation` strengthens the native
message proof: a private getter seeds a distinguishable cached scale, then a
message to that exact receiver must cause a real OS read on the registered owner
callback. Removing only invalidation leaves the stale value and fails the test.
The OS configuration is never changed; this proves message-to-observation delivery,
not an actual OS preference change or a rendered native application's update.

### Native gesture sampling follows public API refresh limits

Browser motion observation and owner delivery follow ADR-0196. One host-owned
media query invalidates the existing signal; its callback runs on a later
microtask and remains independent of canvas creation. The public WASM case
`browser_motion_observations_wake_replace_and_retire_their_owner` covers the
adapter's initial observation, delivery, replacement and retirement through a
controlled JS boundary. It is not an actual OS-setting change test.

The same host observation set also reads motion. AppKit samples
`NSWorkspace::accessibilityDisplayShouldReduceMotion` on its existing owner-lane
timer. The same timer also serves the macOS winit fallback: `WinitApp` owns the
source and the platform stores only a weak lookup. Reads lease it outside platform state;
no AppKit query is added to `about_to_wait`. Shutdown and unwinding fence the
signal before retiring that source. `winit_preferences_probe` checks initial
native observations, foreign-thread refusal and reentrant retirement without a
user window. It requires a macOS main-thread process; local cross-compilation
does not establish that the probe passes.

Android reads `Settings.Global.ANIMATOR_DURATION_SCALE` through the
Activity's ContentResolver during preference sampling, validates it before
publication, and leaves geometry-only queries independent of that setting.
UIKit retains both bold-text and reduced-motion notification tokens on one
owner; notifications only wake that owner, which samples the getters and checks
admission before publishing. Successful registrations acquire ownership
immediately so partial registration unwinds retire earlier tokens. Closure makes
all callbacks inert before taking tokens out of the borrow and removing them.
These sources feed the existing ADR-0172 delivery path and ADR-0195 projection.
Local cross-target clippy compiles these native paths; it does not execute their
OS getters or external-setting notifications.

Linux winit owners use one Settings portal connection on the existing background
executor. Setting and service-owner subscriptions precede the initial read;
the accepted snapshot is committed before waking the owner. Initial bootstrap
waits at most 500ms before `on_ready`; unavailable or stalled observations stay
unknown and can recover later. No D-Bus call runs in the frame path. Successful
unsigned `org.freedesktop.appearance/reduced-motion` values map 1 to Reduce and
other values to NoPreference. A missing key permits the GNOME
`org.gnome.desktop.interface/enable-animations` fallback; transient errors
preserve the last accepted snapshot. Settings v1 uses its double-variant `Read`
reply. Healthy observations use notifications; failed reads retain a bounded
retry that unrelated notifications cannot postpone. Service-owner changes
refresh the same observation set. Shutdown fences owner admission before
cancelling the task, whose state holds no strong owner/source cycle.

The same connection reads `org.freedesktop.appearance/color-scheme`: 1 is dark,
everything else (no preference, prefers light, a missing key) light. It is kept
beside the snapshot, not in it, because brightness is presentation-specific
(ADR-0172): the owner's wake hands the accepted value to each window, and a
window with no winit theme of its own (X11 always, Wayland without an app
override) reports it and fires `on_appearance_changed` when it changed. One
read failing neither discards nor delays the other's accepted value.

`PlatformWindow::appearance` is the user's theme on every backend, never a
value the backend chose for its own chrome, and `on_appearance_changed` fires
only when it changed. Windows reads `AppsUseLightTheme` and sets the DWM dark
title-bar flag to match it on creation and on each `WM_SETTINGCHANGE` that
changed it, until the app picks the chrome through `WindowsWindowExt`
(`set_theme(System)` hands it back); reading the flag back would report the
backend's own choice. macOS
resolves the window's `effectiveAppearance` against Aqua and DarkAqua, with the
content view's `viewDidChangeEffectiveAppearance` as the change source. iOS
samples `userInterfaceStyle` on construction, on `traitCollectionDidChange:` and
on `didMoveToWindow`, apart from the window metrics: the theme needs neither a
laid-out size nor an attached window. Android reads the
configuration's night mode on `ConfigChanged`, and the Web listens for
`prefers-color-scheme`'s `change`.

The process-isolated `linux_preferences` table runs through public
`WinitPlatform` owner capabilities with a private session bus. It checks initial
sampling without a window, live changes, unknown values, unsupported observations,
GNOME and legacy portal reads, service replacement, failed-read recovery without
another notification, foreign-thread refusal and reentrant capture retirement.
`libtest-mimic` keeps execution on winit's required main thread and supplies the
Cargo/nextest protocol. `linux_preferences_probe` runs the same fixture scenarios
manually. These tests execute Linux D-Bus and winit, but do not change a desktop's
actual OS settings or establish live behavior of the other native platforms.

Thread-safe residual handles may outlive the event-loop owner. The background
executor's final shared owner requests nonblocking runtime shutdown, so releasing
a retired proxy inside another async runtime or inside the executor's own task
cannot invoke Tokio's blocking runtime destructor. This retains normal runtime
cancellation of queued async work; already executing blocking work finishes
independently. `background_executor_retirement` exercises both contexts through
the public executor, and the Linux table's `late-proxy` case covers the actual
preference source and stopped host.

AppKit observes `NSEvent::doubleClickInterval` on the application owner lane.
Android observes public `ViewConfiguration` timeouts and physical touch/fling
metrics using the Activity context and its resource density; its presentation
query samples that context again. Neither backend invents unsupported mouse or
touch geometry. iOS and web retain unknown numeric gesture observations.

Neither verified public API provides a complete external-setting notification
for these observations. The host therefore attempts a refresh every 500ms while
its owner loop can run, including without user windows. This is a sampling
cadence, not an immediate OS notification or a bound on a blocked owner's latency.
Android also invalidates on `ConfigChanged` and delivers accepted configuration
changes before subsequent input. AppKit timer captures hold only a weak source;
owner closure or source retirement makes queued work inert. Its next attempt is
armed before native work and contained diagnostics. Errors preserve the accepted
observation and the next refresh attempt.
AppKit installs this sampler before its first native getter. A cold source has
no accepted observation; an initial query error cannot abort platform bootstrap,
publish a fabricated default, or discard its retry. The shared production read
path's `cold_native_observation_recovers_without_forged_defaults` covers failed
initial observation, deferred retry, first successful publication and recovery
without a stale-success response. This portable seam does not execute AppKit's
native constructor or its GCD timer.
Successful recovery requests owner delivery even when the comparison value is
unchanged: a prior failed consumer read must not strand the accepted observation.
Healthy unchanged observations remain quiet.
Failed or unwinding native reads restore a 500ms admission deadline; unrelated
owner wakes cannot repeat the getter early. Reentrant reads return
`PreferencesDeferred` before native work, and foreign calls do not affect that
admission. `bounded_native_read_admission` exercises this portable production
guard through failure, competing admission and recovery after unwind.

Android API26 public horizontal/vertical scroll factors normalize fractional
axis values to logical pixel deltas exactly once. API21–25 resolve the public
`listPreferredItemHeight` theme attribute through the Activity's display metrics,
without hidden API reflection; this is the public
[AndroidX compatibility path](https://android.googlesource.com/platform/frameworks/support/+/34ede8799a022385ca94a1bc111f978ac2e65f45/core/core/src/main/java/androidx/core/view/ViewConfigurationCompat.java).
These pixel packets bypass discrete wheel preferences. A failed refresh keeps
the accepted logical factors; when no factor was ever observed, an explicit
authored compatibility policy treats each axis unit as one line. That fallback
does not claim an OS factor. `android_axis_factor_policy` runs the actual portable
cache/normalization seam and covers API selection, fractional values, failed
refresh, recovery and invalid projection. It does not prove execution on an
Android device. The AppKit/Android native sources require device validation in
addition to cross-target type checking.

### Android text weight shares the Activity preference source

The preference aggregate reads `Configuration.fontWeightAdjustment` through the
Activity's resources on API31 and later. Earlier SDKs never query the missing
field. Android's undefined sentinel is unavailable, zero is known no preference,
and signed adjustments remain exact until the text consumer applies its policy
(ADR-0174). JNI errors use the existing aggregate admission and retry path;
geometry-only reads skip this text observation entirely.

`android_text_weight_query_contract` exercises the private production query seam
with unsupported SDKs, undefined and zero values, signed extremes, query failure
and recovery. The public OS interface cannot deterministically inject SDK levels
or field-query failures, which is why this seam is tested privately. Removing
either SDK admission or sentinel handling makes its corresponding case fail.
This portable test and Android cross-target checking do not prove JNI execution
or configuration-change delivery on a device.

### Native Win32 owner delivery survives message-post refusal

Owner turns ordinarily use the internal HWND's posted message, including inside
native modal loops. If posting fails, the transport signals an unnamed auto-reset
kernel event. The FLUI message loop waits on that event alongside its message queue
and frame/source deadlines, including an infinite idle wait without user windows.
Both paths drive the same owner-turn slot and quit policy. A strong `OwnedHandle`
owner spans every wait; closing admission prevents a retired proxy from posting.
The event contains no callback or UI state and requires no polling thread.
`failed_native_posts_still_deliver_owner_turns_and_quit` injects physical post
refusal and verifies worker admission, reentrant continuation and quit through
the actual native loop. If both the post and kernel signal fail, the caller still
receives an error and OwnerSignal retains its debt. A third-party modal loop does
not wait on this private event; fallback delivery resumes when FLUI regains its loop.

### Native Win32 delivers admitted idle deadlines through live window paints

The owner loop keeps admitted deadline delivery separate from the next hook
query. When due, it invalidates each still-current window before a new query can
abandon a late deadline. A finite message wait rounds upwards, reserves the
infinite timeout sentinel, and drains input and quit messages without parking
again after readiness. Hook replacement is checked by allocation identity;
observing no deadline permits later admission of the same absolute instant.
The count of frame callbacks run at delivery is the delivery generation: the
same instant from the same hook re-arms after every frame callback that ran
since its delivery, and never when none did. A frame request no callback
answers (no window, or a window with no frame callback registered, visible or
hidden) is not counted, so a delivery that reaches no frame cannot spin.
Queries and outgoing capture retirement run outside handler borrows. Incoming
unwind retains the queried hook before hostile captures can replace the first
failure; healthy aggregate destruction retains its ordinary Rust limitations.

The bounded native rows `deadline_rearms_independent_windows_without_input`,
`deadline_hook_replacement_rearms_without_input`,
`deadline_query_can_close_an_independent_window`,
`deadline_none_allows_same_instant_readmission`,
`deadline_queued_three_times_at_one_instant_gets_three_frames`,
`deadline_no_frame_can_service_does_not_spin`,
`deadline_window_without_frame_callback_does_not_spin`,
`deadline_hidden_window_without_frame_callback_does_not_spin`, and
`deadline_query_unwind_retains_replaced_hostile_captures` belong to
`test_window_lifecycle_contract`. They exercise the native wait and paint
producer, cancellation, replacement, independent windows and recovery. They do
not establish GPU presentation, UIA timing or other platform backends.

### Windows identity admission precedes native window acquisition

Backend window identities admit the final nonzero value once, then latch zero
as permanent exhaustion. Both normal windows and owner-control windows reserve
identity before creating a native HWND, so a capacity failure cannot leak an
already acquired window. Catching that failure cannot reissue an old identity.

The Windows row `window_identity_exhaustion_permanently_refuses_retries` joins
`the_owner_thread_machinery_honours_its_contracts`. It drives
`WindowIdentity::admit`, the admission `WindowsWindow::new` runs around
`CreateWindowExW`, from a local counter with an acquisition that records
native creation, checks repeated refusal and verifies that refused admission
never acquires. This tests the admission boundary, not recycled HWND behavior.
The platform reserves its owner-control identity before it initializes COM or
creates its message-only window; the row `platform_identity_exhaustion_acquires_nothing`
retries an exhausted platform admission and checks that the thread's COM
apartment and the message-only windows are unchanged.

### The Win32 clipboard opens with a message-only owner window on a dedicated pump thread

`WindowsClipboard` has no single owning thread: any thread may read or
write, and the winit backend has no FLUI-pumped loop. Opening with a `NULL` owner, as `arboard` and FLUI used to,
does not exclude other threads of the process, since all `NULL` openers count
as one owner (see "Win32 clipboard: cross-thread sessions" in
`docs/safety-review.md`).

`ClipboardSession` therefore opens with one message-only window
(`HWND_MESSAGE` parent), created lazily on the first session and never
destroyed. The window lives on a dedicated `flui-clipboard-owner` thread whose
only job is `GetMessageW`/`DispatchMessageW`; every message goes to
`DefWindowProcW`. The owner must be pumped because `EmptyClipboard` in any
process *sends* `WM_DESTROYCLIPBOARD` to the current owner and waits for it.

Rejected: reusing the owner thread's loop. It exists only on the Win32
backend, not while a winit loop or a headless test drives the process, and
not before `run`. A session on a worker thread would also block on a
`SendMessage` to the UI thread, which deadlocks whenever the UI thread waits
on that worker. A thread per process costs one idle stack and removes all of
this.

The process-wide `clipboard_lock` stays: `arboard` sessions still open with a
`NULL` owner, and a `WindowsClipboard` session would be let in beside one.
`OpenClipboard(NULL)` also leaves `SetClipboardData` without an owner to
record, which Win32 documents as liable to fail; with an owner it is on the
documented path. If the window cannot be created, sessions fall back to a
`NULL` owner and log an error.

Test: `a_null_owner_open_on_another_thread_fails_while_a_session_is_open`
(fails with a `NULL` owner). The owner thread's message pump is not covered by
a test.

### Win32 callbacks live in the window's owner-thread context; off-owner registration is refused

Registration methods take
`&self` on `Send + Sync` traits, so any thread can call them. ADR-0082 §4 step
one for Win32 makes that safe without changing a signature.

- **Storage.** A window's callbacks (`WindowCallbacks`) live by value in its
  `WindowContext`; the platform-level handlers (`on_quit`, `on_window_event`,
  `on_keyboard_layout_change`) live as `Rc<RefCell<PlatformHandlers>>` in the
  `OwnerControlContext` of the owner message window, shared by every window
  context; the owner-turn callback lives in that context's `OwnerTurnSlot`.
  Both contexts sit behind `GWLP_USERDATA`, are reached only through the
  owner-thread gates (`with_window_context_checked`, `OwnerControl::shares`),
  and are freed by `WM_DESTROY` / `WM_NCDESTROY` on the owner thread. A
  platform dropped off its owner thread without running keeps the owner window,
  so its `OwnerControlContext` and the handlers in it are leaked, never freed
  on the wrong thread. Both are pinned `!Send + !Sync`.
- **Refusal.** A registration from any thread but the owner, on a window that
  is gone, or on a handle the OS recycled for another window, is refused. The
  callback is dropped right there, on the registering thread, so it never runs
  on the owner and is never freed by a thread that did not create it. A
  foreign-thread refusal logs an error ("registration refused"): the caller has
  just lost a callback. An off-owner `open_window` returns
  `OpenWindowError::Backend`.
- **Lease-style restore.** Dispatching the window-event or keyboard-layout
  handler takes it out of its slot, runs it with no borrow held, and puts it
  back only if the slot is still empty. A handler registered while it ran —
  including by itself — wins, the rule `OwnerSignal::drive` already follows.
  Before this, the old handler was put back unconditionally and the
  replacement was silently lost.
- **Quit order.** The run's teardown takes the quit callback while the owner
  context exists, destroys the owner window, then runs the callback.

The other backends still store `Send` callbacks behind locks until their own
steps.

Tests (CI only type-checks Win32): the owner-affinity refusal order is pinned
by `refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot`
(run by `the_owner_thread_machinery_honours_its_contracts` in `shared/handlers.rs`), and the callback panic and re-entrancy rules by
`tests/window_callback_unwind.rs`.

### Each Win32 window owns its text services, activated at creation and deactivated by `WM_DESTROY`

A window's TSF connection (`text_services::TextServices`, ADR-0135 §3) lives in its
`WindowContext`, activated on the owner thread right after the context is installed (TSF's
calls read the context's scale), so the host exists before any field can focus; a lazy
activation would put the first `ITfThreadMgr::Activate` inside a focus change, under owner
code. A failed activation leaves `None` and the `WM_CHAR` path. `WM_DESTROY` deactivates it
after the close callbacks (the presentation's close unfocuses its field through the still
active host) and before the context retires; the shutdown is a host operation, so one that
arrives inside a TSF call into a store waits for that call. Its application-code failures are
logged, contained: the window procedure aborts on a panic. Test:
`a_window_offers_its_text_services_as_its_host`, and the `window destroyed inside a TSF call`
row of `the_text_services_answer_a_completion_for_its_store`.

### AppKit reopen signals use a loop-owned serialized callback pump

The owned application delegate implements
[`applicationShouldHandleReopen:hasVisibleWindows:`](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldhandlereopen(_:hasvisiblewindows:)).
It always returns false: FLUI owns window policy and does not ask AppKit to create
an untitled document. Both visible-window flag values deliver the same existing
`on_reopen` signal. Apple's flag counts miniaturized windows as visible; it is
not a substitute for presentation visibility or frame eligibility.

The Objective-C method only records a signal and schedules weak loop control.
A pending count, queued flag and active guard serialize owner-lane delivery,
including callbacks that pump nested native events. Starting-phase signals wait
until the loop runs. The registration current at delivery receives each signal;
a replacement installed before that turn receives it. During delivery, callbacks
are leased outside locks and restored only if no replacement exists and quit has
not been requested. Active remains set through callback destruction, so a nested
signal is retained until its predecessor's lease and cleanup finish.

Explicit quit fences registration, delivery, restoration and window admission
immediately, including before deferred termination evaluation. Old delegates
reference only their old loop; stopped loops cannot act on a later run. Handler
replacement/destruction happens outside locks, preserving the existing
handlers-then-state lock order. Callback, destructor and native/dispatch boundary
panics use hostile-payload-safe containment; a panicking registration does not
prevent another pending signal from reaching its replacement or restored slot.

`reopen_probe` and `tools/device-checks/check-macos-reopen.py` separate direct delegate
routing from actual LaunchServices reopen AppleEvents. The latter invokes
`open -a` on the exact running temporary bundle and requires callback and normal
return in the original PID. These checks cover the platform signal, not a
rendered resident application or a framework root-window creation API.
Run the native matrix on macOS with an active GUI session:

```sh
cargo build -p flui-platform --locked --example reopen_probe
python3 tools/device-checks/check-macos-reopen.py target/debug/examples/reopen_probe
```

### Headless explicit quit consumes its callback outside platform state

`HeadlessPlatform::quit` uses the same take-then-invoke discipline as its window
close and exit-policy re-evaluation paths: it marks the loop stopped and takes
the current quit callback under the state lock, then invokes and drops it after
the lock is released. Recursive or repeated quit does not re-invoke that
registration. This is consumption of the existing callback slot, not a new
process-wide latch; registering a later callback still replaces the slot.
Callback panics propagate to the Rust caller after releasing state, and the
consumed callback is not restored. Tests exercise the actual `Platform::quit`
seam with a reentrant getter/quit, a captured-data drop sentinel, and an original
panic payload. Their first nonblocking lock assertion makes a regression fail
immediately instead of hanging while trying to re-enter the same mutex.


### Standalone AppKit stops its loop and returns through Rust cleanup

The existing `Platform` exit-policy hook remains the boundary: `flui-app`
owns UI runtime/service policy; `flui-platform` owns window bookkeeping and native
loop actuation. AppKit now implements both policy installation and coalesced,
any-thread re-evaluation. Close callbacks finish before the deferred owner turn
consults the hook. Hooks run outside locks, and both loop phase and window count
are checked again afterward because a hook may open a replacement window.
Reentrant requests get another owner turn; a replaced hook is never overwritten
by restoring the old leased callback. No public policy variant is added.

`LoopOwnership` accepts only a standalone NSApplication: a running application
or existing delegate is rejected before bootstrap or activation mutation. The
constructor merely acquires the singleton. The run scope retains its delegate,
sets Regular activation, and restores the previous policy only while its own
delegate and installed policy are still current. External replacements survive.
A terminate request nobody announced goes into the same deferred quit path and
is answered TerminateCancel, so AppKit does not exit the process itself; a
session end is answered differently (see the section after this one).

Explicit quit bypasses the last-window veto. `stop(None)` plus an application
NSEvent wakes the native wait; quit during bootstrap skips `run()` entirely.
This follows [Apple's stop contract](https://developer.apple.com/documentation/appkit/nsapplication/stop(_:))
and the installed winit 0.30.13 `stop_app_immediately` implementation. This is the
standalone main loop contract; nested modal loops and foreign-loop embedding are
not supported by this owner. Physical keyboard/menu wiring is a separate input
concern, not proved by sending `terminate:` in a probe.

Shutdown first closes admission, clears callback cycles and closes remaining
windows on main, then invokes quit once outside locks. Cleanup also runs on
bootstrap error/unwind. User callback/destructor panics are contained per cleanup
step and logged; exceptional panic payloads are deliberately leaked rather than
risk a second panic during unwinding. Resource cleanup after such a panic is
best effort, while the guard still detaches its own delegate.

Native window release now executes inline on its owner, including after the
loop returns, through the same panic boundary used by lifecycle cleanup: a
panic payload whose destructor also panics is retained without destruction
while ordinary local resource destructors still execute. `windowWillClose:` pins its receiver with an extra retain transferred
to the surrounding autorelease pool before callbacks can drop the final wrapper.
That prevents deallocation under the native close stack. Worker drops remain
nonblocking queued tails; if the owner no longer services them, that existing
fallback can leak native resources and never releases them off-main.

**Verification:** `cargo build -p flui-platform --locked --example exit_policy_probe`
then `python3 tools/device-checks/check-macos-exit.py target/debug/examples/exit_policy_probe`
stages a fresh bundled subprocess per case with an eight-second kill/reap limit.
Cases require post-return, quit, callback-drop and stack-drop markers, and weak
NSWindow references must be nil after an explicit autorelease pool drains.
Replacement windows must survive a later owner turn before the probe closes them;
replacement hooks must actually execute. Foreign ownership, pre-run/bootstrap
quit, bootstrap failure/unwind and quit panic each have their own case. Unsupported
hosts and unknown case names fail explicitly. The older close-path and frame-pump
probes retain their narrower lifecycle/frame assertions.

### An AppKit window's mode, the host's notices and a session end (ADR-0237)

AppKit has no mode property. `shared::appkit_window_mode` reads a window's mode
from its flags (full screen outranks miniaturized, which outranks zoomed, and a
zoom counts only for a resizable window) and plans the toggles that reach
another mode from those flags. `ModeSequence` orders the commands: a full-screen
or Dock step animates, AppKit ignores a toggle sent meanwhile, and what the
window looks like afterwards is known only afterwards, so a command that
arrives during one, and the rest of a mode whose first step is one, are held
and planned again from the real flags at the step's `windowDid…` notification.
The newest command wins. A step whose notification never comes stops holding
after three seconds. AppKit keeps no rectangle to return to from a zoomed,
minimized or full-screen window, so the backend records the outer rectangle
just before the window leaves normal mode.

A position is the outer top-left in global logical points: AppKit measures from
the bottom-left corner of the primary screen, so `shared::appkit_window_geometry`
flips through the primary screen's height. The display snapshot is measured the
same way (device pixels of that screen from the primary screen's top-left
corner), so a position and a display origin share one space. `Platform::open_window`
turns a position measured on a display into a global one before the window is
built.

The notice sources (`NSWorkspace` sleep and wake, the distributed screen lock
notifications, the memory-pressure dispatch source) start in `run`, after the
application has installed its hooks, and only forward to `SystemNotices`; a wake
marks the displays and every window's state stale first. The distributed centre
is asked to deliver while the application is inactive, and is put back when the
sources drop. The memory-pressure source's handler holds the source and
`Drop` cancels it, which ends that cycle.

A session end is two events. `NSWorkspaceWillPowerOffNotification` asks the
session-end hook and `TerminateGate` remembers the answer; the terminate request
that follows is answered `NSTerminateNow` (after the hook hears `Ending`), or
`NSTerminateLater` while the application holds the session end, with the reply
sent when it releases its blocks or after four seconds. The release reaches the
gate as a task of its own on the main queue, never from the stack that dropped
the block. AppKit says nothing when the user calls a log out off, so an
announcement no terminate request follows within ten seconds is reported
`Cancelled`; a Cmd-Q inside those ten seconds is taken for the session end,
which the live checks record. A terminate request with no announcement is an
ordinary quit. The quit callback is not run for a session end: the process ends
with the session, and the application was told what to save.

### Win32 decides the exit policy on an owner-window turn

A `WM_DESTROY` that leaves the platform tracking no top-level window does not
consult the exit-policy hook itself: it posts one coalesced message to the
owner message-only window, and `request_exit_policy_reevaluation` posts the
same message from any thread (refused once the owner window is closed). The
owner procedure then checks that no window is tracked and no quit is under way,
leases the hook out of the handler set, runs it with no borrow held, restores it
unless a newer hook replaced it, and checks the window count again, since the
hook may have opened a replacement window. An allowed exit ends the loop the way
`quit` does. No hook allows the exit, as on winit. Deferring the decision keeps
the hook, which re-enters the embedder, out of a `WM_DESTROY` nested inside
embedder code that closed the window. The native rows in `tests/contract.rs`
(`closing_the_last_window_ends_the_loop`,
`exit_policy_veto_holds_until_a_reevaluation_allows_exit`,
`window_opened_by_the_exit_policy_keeps_the_loop`) run the real message loop.

### The winit backend delegates the whole keyboard event to `ui-events-winit`; Win32/AppKit keep hand-written tables

**Rule:** every native keyboard event this crate receives must be normalized
into FLUI's owned keyboard vocabulary (`Code`, `Key`,
`Location`) at the platform boundary — see the module doc of `crates/flui-platform-api/src/input.rs`.
`Code::Unidentified` must mean the backend genuinely could not identify the
physical key, never that a conversion table was incomplete (issue #1092).

**Choice:** the winit backend's `platforms/winit/events.rs::keyboard_event`
converts the whole `winit::event::KeyEvent` (code, logical key, location,
down/up state, `repeat`, and modifiers) through
`ui_events_winit::keyboard::from_winit_keyboard_event` (crates.io
`ui-events-winit`, the same bridge Masonry and Xilem use) instead of
hand-assembling any `KeyboardEvent` field itself; `platform.rs`'s
`ModifiersChanged` handler stores winit's raw `ModifiersState` rather than a
pre-converted `keyboard_types::Modifiers`, and each read site (pointer paths
and the keyboard path alike) converts through the crate's
`from_winit_modifier_state` at the point of use. The Win32 (`shared/keys.rs`)
and AppKit (`shared/keys_macos.rs`) backends keep their own hand-written
`Code`/`Key` tables, because no equivalent bridge crate exists for
`WM_KEYDOWN` scancodes or `NSEvent.keyCode` — those two native id spaces are
FLUI-specific translation work with no upstream crate to delegate to.

Location is also sourced differently than before this issue: winit's own
`KeyEvent.location: KeyLocation` field feeds `from_winit_keyboard_event`
directly, rather than being re-derived from the physical `Code` (the
pre-#1092 code matched a hand-picked list of `KeyCode` variants against
`Location::Numpad`/`Left`/`Right` — exactly the incomplete-table failure
mode this issue fixed for `code` too). There is no "does `Code::X` imply
`Location::Numpad`" property left to hold; winit already computes location
per-platform and exposes it, so nothing here re-derives it.

This asymmetry between backends is deliberate, not inconsistent: the rule is
"delegate to an audited bridge when one exists for this native surface," not
"every backend must look the same." `platforms/winit/events/keyboard_tests.rs`'s
`cross_backend_physical_key_agreement` test cross-checks a curated set of
physical keys against both hand-written tables so the two authored
translations and the delegated one cannot silently drift apart.

The Winit and Android ecosystem mappings remain boundary dependencies:
`shared::keyboard_adapter` converts their results to owned `KeyEvent` values.
Win32 and AppKit tables return owned enums directly, and Web parses the DOM's
W3C spellings against the generated owned tables. No pointer event uses the
keyboard adapter. `keyboard_adapter_contract` preserves all named/physical
key spellings, legacy Meta aliases and every event field. This conversion
test lives beside the private adapter because Winit's complete `KeyEvent`
contains private platform state and cannot be constructed by an integration
test; native backend probes cover actual input delivery separately.

**Alternatives considered:**

- Complete the crate's own hand-written `winit::KeyCode → Code` table (kept
  as a dev-dependency oracle test against `ui-events-winit`, never shipped).
  Rejected: it is a second, unaudited copy of exactly the table the bridge
  crate already maintains and tests upstream — an ownership cost with no
  behavioral upside, and the class of bug this issue exists to fix (a
  hand-rolled table quietly falling behind the enum it mirrors).
- Delegate only the three field-level conversions (`from_winit_code`,
  `from_winit_key`, `from_winit_location`) and keep hand-assembling
  `KeyboardEvent`'s remaining fields (down/up state, `repeat`,
  `is_composing`, and a separately hand-written modifiers conversion) in
  `keyboard_event`. Rejected: `from_winit_keyboard_event` and
  `from_winit_modifier_state` already perform exactly that assembly, so a
  hand-written half is byte-identical duplicate logic — the same
  re-derived-seam risk the issue exists to close, just smaller.
- Vendor `ui-events-winit`'s tables directly into this crate (a `// PORT
  NOTE`-style copy). Rejected for the same reason as the first bullet, plus
  it would need re-syncing by hand on every winit/`ui-events` bump instead
  of `cargo update` doing it.

**Trade-off:** this crate's winit backend now tracks `ui-events-winit`'s
release cadence for keyboard fidelity — a regression or gap introduced
upstream reaches FLUI without a code review here. The `.github/dependabot.yml`
`ui-events-cohort` group (with `ui-events` itself) and the
`every_winit_keycode_maps_to_a_canonical_code` regression test are the
mitigations: a bump that drops coverage for a `KeyCode` variant fails CI
immediately rather than shipping silently — though that test can only pin
the dependency's own completeness, not a regression inside `keyboard_event`
itself, since `winit::event::KeyEvent` cannot be constructed outside winit
(`platform_specific` is `pub(crate)`) and so `keyboard_event` can never be
called directly from a test; see that test's doc for the full reasoning. If
the workspace's `winit` pin ever needs to move ahead of what
`ui-events-winit` supports (it pins `winit ^0.30`), the escape hatch is a
`[patch.crates-io]` pin at the upstream `ui-events-winit` git repo (plus
adding that repo to `deny.toml`'s `sources.allow-git`), not reviving a hand
table — see the dependency comment in `Cargo.toml`.

**One observable behavior change for already-working input:** a winit
`Key::Dead(_)` (a dead-key composition in progress) now reports
`Key::Named(NamedKey::Dead)` instead of `Key::Named(NamedKey::Unidentified)`
— `from_winit_key` distinguishes the two where the deleted hand table
collapsed both to `Unidentified`. Verified inert today:
`crates/flui-widgets/src/text/editable_text.rs`'s key handler matches
specific `NamedKey` variants and falls through everything else, `Dead`
included, to `Key::Named(_) => KeyEventResult::Ignored` — no consumer in the
workspace currently branches on `NamedKey::Dead` specifically (`rg
'NamedKey::Dead'` across `crates/` has no hits outside this record). Named
here so a future consumer that starts caring about dead-key state — an IME
composition indicator, for instance — knows this signal already exists on
the winit backend and does not need a new one.

**Further reading:** [`.rust-studio/specs/1092-winit-physical-key-map/survey.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/.rust-studio/specs/1092-winit-physical-key-map/survey.md)
is the market/reference survey that motivated this decision (`ui-events-winit`
coverage measurement, a generated physical-key catalog as
the completeness precedent, and the three options evaluated before choosing
production delegation) — read it for context, not as the source of a fact;
figures cited in this entry are measured directly against this repository
and `ui-events-winit`'s own source, not against the survey.

**Replacement coverage:** `platforms/winit/events/keyboard_tests.rs`'s
`keyboard_conversion_tests` module (`every_winit_keycode_maps_to_a_canonical_code`
over all 194 winit 0.30.13 `KeyCode` variants — length- and
duplicate-checked against the winit source) and
`cross_backend_physical_key_agreement` (winit vs. Win32 vs. AppKit on a
shared physical-key set). Before this change, `convert_physical_key`'s hand
table mapped 71 of winit 0.30.13's 194 `KeyCode` variants to `Code` (the
count measured at issue #1092's baseline) and `convert_winit_key` mapped 37 of 306
`NamedKey` variants to `Key`, falling through to `Unidentified` for the rest
in both cases.

### The surface-availability signal is a per-window `bool` callback, not a polled query

**Rule:** a backend that learns the GPU surface's native backing is about to go
away must say so through `PlatformWindow::on_surface_status_change` **before** the
handle behind any surface built from it dies, so the presentation can drop that
surface at the right moment. A backend that never emits the signal is harmless:
the surface is never released, which is the pre-#1146 behavior and the same shape
the visibility callback already documents. A backend that emits `false` and never
`true` is not harmless, because the presentation stays released, every later frame
is a skipped one, and the window paints nothing forever.

**Choice:** `on_surface_status_change(Box<dyn FnMut(bool) + Send>)`, a defaulted
no-op, following the `on_active_status_change`/`on_visibility_status_change`
family's shape: stored in `shared::handlers::WindowCallbacks`, dispatched through
the same FIFO and `CallbackLease`. `false` means "release the surface before this
callback returns"; `true` means "a surface valid for the handle available now must
exist", which the runner acts on unconditionally rather than by comparing it
against what it already holds — that statelessness is the point, since a `false`
can be lost and an "already present, do nothing" rule would then keep a surface
built from a dead window for the rest of the process's life. The parameter is a
`bool`, not an enum: a third state of the *surface* belongs in its own callback,
per ADR-0035's split of this family by signal rather than by arity. Android and the Web
emit it. Android's `Pause` and `TerminateWindow` send `false`, `Resume` and `InitWindow`
send `true` — `platforms/android/mod.rs`'s module doc carries why that pair, and not
`Pause` alone, is the correct mapping. The Web sends `false` on `webglcontextlost` and
`true` on `webglcontextrestored`: the canvas stays, the context behind it is gone, and
whatever was built on it must be rebuilt once the browser gives one back. The loss is
cancelled with `preventDefault` — without that the browser never restores the context —
and a browser that never restores it leaves the page without a surface, which is all the
page can know. A lost WebGPU device is not a surface event, so it does not use this
signal; the renderer reports it to its owner directly.

**Alternatives considered:**

- A polled query on the window (a `surface_is_available()` the runner reads each
  iteration). Rejected: a poll cannot carry this deadline. The drop has to happen
  while the native handle is still valid, which on Android means inside the
  `TerminateWindow` callback; a query consulted at the next loop iteration reports
  a window that is already gone.
- A `#[non_exhaustive] enum WindowSurfaceStatus` parameter instead of `bool`.
  Rejected for the parameter, not for the concept: it is the shape this crate uses
  for input and owner status, but every consumer of this signal branches on two
  states, and a callback's parameter type is a one-way door that changing later
  breaks for every registrant.
- Delegating the surface to the platform, the shape Leptos reaches on Android by
  hosting a Tauri v2 WebView. Rejected as unavailable rather than inferior: it
  requires giving up the adapter, the swapchain, and any ability to composite
  native-rendered content into the framework's own UI. It would not escape the
  problem either, since a wgpu surface inside a WebView is canvas-backed and
  wgpu's wasm backend does not recover from `webglcontextlost` (gfx-rs/wgpu#3679).

**Market lineage:** [bevy#6830](https://github.com/bevyengine/bevy/pull/6830), where
an `android-activity` maintainer states that only the top-level render target needs
recreating, not the wgpu stack; [bevy#9937](https://github.com/bevyengine/bevy/pull/9937),
which first despawned the window on suspend and was revised to "keep Bevy window,
only recreate Winit window and wgpu surface"; and
[winit#3786](https://github.com/rust-windowing/winit/pull/3786), which forwards
Android's `suspended()`/`resumed()` and is a named second emitter for this same
`false`/`true` pair, since winit is already a workspace dependency with a backend
here. Flutter asks for the same contract under another name:
[flutter#160933](https://github.com/flutter/flutter/issues/160933) requested an
`onSurfaceDestroying` that precedes the engine's `cleanup()`, having found the
after-the-fact callback fires once the native surface is gone. The spelling that
landed upstream is not that one: the API that exists is
`SurfaceProducer.onSurfaceCleanup`, the before-signal that replaced
`onSurfaceDestroyed` because another thread could still touch the surface after
the fact, so a grep for the requested name finds the request rather than the
implementation (ADR-0063 names both spellings). One divergence from
winit and bevy is deliberate: winit 0.30.13 keys on the window events alone —
`src/platform_impl/android/mod.rs` maps `MainEvent::InitWindow` to `Resumed`
and `MainEvent::TerminateWindow` to `Suspended`, while its `Start` and `Stop`
arms are `warn!("TODO: forward …")` stubs that forward nothing — and bevy
consumes those same two events. This backend keys on `Pause`/`Resume` *as
well as* `TerminateWindow`/`InitWindow`: the window pair is where the handle
actually dies and returns, and the lifecycle pair is where the framework is
told the app is going away, so emitting on both is the more conservative
choice, at the cost the trade-off below books.

**Trade-off:** releasing on `Pause` unconditionally makes the cost per-edge rather
than per-defect. Android maps `false` to both `Pause` and `TerminateWindow` and
`true` to both `Resume` and `InitWindow`, and the seam attempts a recreation on
every `true`, so every `Pause`/`Resume` pair pays a release, a create/configure,
a lane generation mint and a full repaint — including the pauses where the native
window was never destroyed, such as a dialog over the activity or a multi-window
deactivation. A second `true` over a surface still held on the same window is not
paid twice but refused: Vulkan allows one `VkSurfaceKHR` per `ANativeWindow`, so
the second create fails, the held surface stays, and no second mint or repaint
follows (ADR-0063 decision 6 books what that refusal costs under the vendored
`wgpu-hal`, and marks the ordering that could produce it as unverified). Dropping
a configured `wgpu::Surface` is
not cheap either: it reaches `vkDeviceWaitIdle` before destroying anything, which is
an unbounded wait on whichever thread runs the callback. That is why the release is
a synchronous, surface-only operation that performs no lane handoff, no probe and no
submit, and why the `TerminateWindow` release that follows `Pause` in the ordinary
cycle is an idempotent no-op — while its caller holds the raster lane's guard across
it by design, so the wait does happen with the lane held.

**Unasserted:** no test pins this. A covering test has to be a wire test —
registration through the `PlatformWindow` trait method, the `simulate_surface_status`
affordance driving the platform's own dispatch, and both edges asserted in the
closure — because a backend left out of `impl_window_callback_setters!` still
compiles the registration and silently drops it, which a direct
`dispatch_surface_status_change` test cannot see. The Android arms themselves are
**type-checked by `cargo xtask cross-typecheck` and executed by nothing**: no gate
on this host runs the Android backend, so their mapping is an inference from `android-activity`'s
documented contract, recorded rather than measured.

### AppKit's frame source stays `drawRect:`, and its re-arm crosses one owner-lane turn

**Rule:** a display-driven backend's produce signal must be able to re-arm itself
from inside the frame it just produced. AppKit discards a `setNeedsDisplay:`
issued while it is displaying the view, so a backend whose only frame source is
`drawRect:` and whose re-arm happens there is circular — one frame, no wake, no
frame. A re-arm AppKit silently drops is worse than a missing feature: the pump
stops with no error, no log line, and a perfectly healthy-looking request behind
it.

**Choice:** `drawRect:` stays the frame source. It is the signal AppKit actually
delivers — the view is live and the frame carries its own dirty rect — and the
produce contract the engine consumes is dispatched from it (`view.rs::draw_rect`
→ `WindowCallbacks::dispatch_request_frame`). What changed is the re-arm.
`platforms/macos/display_pass.rs` marks the thread for the duration of the frame
AppKit asked for (an RAII guard, nesting-safe, thread-local), and
`window.rs::dispatch_redraw_request` — the single decision point `request_redraw`
calls — sends the `setNeedsDisplay:` inline at any other moment, but hands it to
the next owner-lane turn when the caller is inside that pass. This is ADR-0039
§4(c) realised on AppKit: the frame transaction is a region the drain gate is
closed for, and a wake arriving while it is closed defers rather than draining.
The deferred turn re-checks the marker before messaging AppKit and re-defers,
bounded at `MAX_DEFER_HOPS`, because a display pass that opens a nested run loop
services the same lane from inside itself — which the deferral would otherwise
reproduce silently.

Measured on macOS 15.7 (Darwin 24.6), a 100 Hz panel, isolated probe arms of 4 s
each, every arm with the window fully visible (`occlusionState` carries the
visible bit):

| re-arm issued from inside `drawRect:` | draws over 4 s |
|---|---|
| `[view setNeedsDisplay:YES]` | 1 |
| nothing at all (control) | 1 |
| `[view.layer setNeedsDisplay]` | 1 |
| `setNeedsDisplay:` deferred to the next main-queue turn | 396 |

**Alternatives considered:**

- Re-arm through the backing layer (`[view.layer setNeedsDisplay]`). Rejected by
  measurement, not by taste: the layer's `needsDisplay` reads `YES` right after
  the call while the view is still never redisplayed — the flag that survives is
  not the flag AppKit acts on, so it is not a usable signal to test against
  either.
- Move the frame off the display pass entirely (`CADisplayLink`, i.e.
  `NSView.displayLink(target:selector:)` on macOS 14+, or `CVDisplayLink`).
  Deferred, not rejected: ADR-0044 names `CADisplayLink` as macOS's intended
  produce signal, and ADR-0045 (Proposed) keeps macOS on `RasterMode::Inline`
  until the locked `wgpu-hal`'s Metal path stops messaging `NSView`/`NSWindow`
  from the calling thread. Until that lands, the frame has to be able to re-arm
  from where it runs.
- A diagnostic redraw tick in the backend (an `NSTimer`/thread poke behind a
  `FLUI_MACOS_REDRAW_POKE_MS`-style knob, which was implemented while diagnosing
  this). Deleted, not deferred: it contradicts ADR-0058 — the platform paces
  production, a sleep never does — and it pre-empts the `PlatformProxy` redraw
  verb ADR-0045 decision 5 scopes to #559 by shipping a second, private frame
  source. The primer that starts a measurement belongs to the probe, which is
  where it now lives (`examples/frame_pump_probe.rs`).

**Market lineage:** nobody shipping a Rust macOS UI builds its frame loop this
way. gpui-ce (`crates/gpui_macos/src/display_link.rs`, `main` @ `8e36ac0`,
2026-09-15) and upstream Zed (the same crate, `main` @ `c24e309`, 2026-09-17)
both pace on `CVDisplayLink` — one immortal link per `CGDirectDisplayID` in a
`static Mutex<Registry>`, windows subscribing rather than owning, driven by
`NSWindowOcclusionState::Visible` as the master start/stop switch and re-armed on
screen change — and render **directly** from the link's callback on the main
queue, with `displayLayer:` as the second, AppKit-initiated entry; both paths
stop the link, render, and restart it. `CADisplayLink` /
`NSView.displayLink(target:selector:)` appears nowhere in either repository, and
**neither calls `setNeedsDisplay:` at all** (the sole hit is
`setNeedsDisplayOnBoundsChange(true)` on the `CAMetalLayer`), nor has any
`drawRect` path (`0` hits). That last point is the one worth keeping straight:
it is evidence that the path this backend was on is a path the market left, not
evidence about AppKit's behaviour — the discarded in-pass `setNeedsDisplay:` is
this entry's own four-arm measurement, and the market never reaches the
question. Their link is also leaked by design, because `CVDisplayLinkStop` is
asynchronous and releasing raced its io thread (upstream segfaults #32116 /
ZED-7XR) — the wart `CADisplayLink` does not have, which reads as support for
ADR-0044's choice with the caveat that the market sits on the API macOS 15
deprecates. Their deferral is a different one and not to be confused with the
one above: upstream Zed PR #3592 (`f12510b8`, 2023-12-11) defers *drawing* until
CoreAnimation calls `displayLayer:`, i.e. defers to a platform signal rather than
around a discarded request; core gpui's `deferring re-entrant window draw
request` is its own comment's description of a Windows-only re-entrancy case.
One behaviour of theirs this backend still lacks is the fix for the stall that
keeps a cold start from beginning: a single synchronous frame on re-activation,
gated on `activated_least_once`.

Basis for the above: source reading at the two commits named (clone grep plus
`gh search code`, which agree), not by running either project — the clones are
shallow, so an older revision containing a `setNeedsDisplay:` call is not ruled
out, and no Apple documentation was consulted for the API-deprecation claim.

**Trade-off:** this deferral is AppKit-local, and the same defect has a second
instance on another backend with the opposite sign. Web's `request_redraw`
dispatches the frame immediately and `WindowCallbacks::drain_events` drains
synchronously until the queue is empty, so a frame callback that re-arms recurses
*within* the originating call rather than reaching a browser frame turn — measured
at 4 frames inside one JS-to-Rust call, with no RAF turn, GPU setup or renderer
involved (issue #1047, open). AppKit drops the re-arm that never crosses out of
the display pass; web honours one that never crosses into a future turn. Both are
the same contract failing: **a redraw request issued from inside a frame request
must be deferred to the platform's next frame turn, not serviced in place.** The
macOS half is correct now and the web half is not, which is the cost this entry
books — the concept lives in `platforms/macos/display_pass.rs` rather than in a
shared place both backends could hold, so a third backend writing a frame loop
has to rediscover it. Lifting it to the `WindowCallbacks`/`request_redraw` layer
is the obvious shape and is not done here: it would edit the web backend, which
no gate on this host executes, and §Definition-of-Done forbids claiming a fix on
a path nothing runs.

**Replacement coverage:** three tiers, because no single one reaches the claim.
`display_pass.rs`'s four tests pin the marker (set inside a pass, restored on
drop, nesting, thread grain). Two always-run window tests pin the decision
without an NSWindow
(`a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline` and its
complement — deleting the deferral branch fails the first, checked by mutation:
with `dispatch_redraw_request`'s body replaced by an unconditional inline send,
that test fails and the other five in scope still pass), because a bare test
process cannot construct an NSWindow at all. The behavioural end-to-end pin is
`examples/frame_pump_probe.rs` (`cargo xtask device macos-frame-pump`): a real visible window
on the real AppKit run loop, whose frame callback re-arms the way the engine's
frame does, behind a primer that stops at the first frame so the measurement
cannot be explained by it. Measured 2026-09-17: 301 frames in 3.010 s (100.0 fps
on the 100 Hz panel) with the deferral, against 1 frame total and 0 in the
measurement window with the deferral branch removed. Both runs' marker lines
(§4 of its `plan.md`) and the raw run logs are kept with the
[pacing evidence](https://github.com/vanyastaff/flui/tree/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/.rust-studio/specs/macos-native-vsync-pacing-evidence).

### macOS reports its display period as the current mode's rate, in the display snapshot

**Rule:** a backend that can tell an embedder the display's refresh period should
report the rate of the mode that display is *in* — not a capability of the panel
and not a generic default. The value is a floor for pacing ("never produce faster
than this", the display snapshot's `refresh_period`), so a rate that is too high paces
against a cadence the display is not running, and a rate that cannot be
determined must be reported as unknown rather than guessed at.

**Choice:** `Platform::display_snapshot` carries, for every `NSScreen`, a period
from `CGDisplayModeGetRefreshRate` over the display's *current* mode
(`platforms/macos/display.rs::refresh_period_for_screen`), reached from the
screen via `NSScreenNumber` — the same display id `display.rs` reads for its
bounds. A rate of 0 is `None`: CoreGraphics reports 0 for modes it cannot
describe and some displays do, and a 0 turned into a period is infinite. The
window names its display through `PlatformWindow::display_id`, **cached** in
`MacOSWindowState` beside `scale_factor`: re-read in the handler that already
updates the scale (`handle_backing_properties_changed`, reached from
`handle_screen_changed`), which then raises `on_display_changed`, and seeded when
the platform binds its display registry to the new window, which is safe there
because the window is created at the origin — a point on a screen — so
`-[NSWindow screen]` answers rather than returning nil. Caching is the one place
this diverges from the winit implementation it otherwise mirrors: winit queries
the monitor per call, but this backend's AppKit traffic belongs to the owner lane
while `display_id` is a plain `&self` accessor any thread may call, so the query
happens on the lane and the accessor reads the lock. The screen key is the
`CGDirectDisplayID` (a label, stable while the display stays attached), a stopgap
the display-UUID source replaces.

**Alternatives considered:**
- `NSScreen.maximumFramesPerSecond` — rejected twice over. It is macOS 12+, above
  this crate's 11.0 deployment floor, so an unguarded call would crash on 11;
  and it reports the panel's hardware *maximum*, so it over-reports on a
  ProMotion display running a 60 Hz mode — exactly the too-high rate the rule
  above forbids. winit does not use it.
- Querying per call, the way winit does — rejected here for lane safety, above.
- A `CVDisplayLink` fallback for displays whose mode reports 0 (winit has one) —
  **deferred, not dropped**: it needs a `core-video` dependency, and CVDisplayLink
  is this backend's intended produce signal anyway (ADR-0044), so it belongs with
  that tick source rather than ahead of it. Until then `None` is honest and
  observable: `flui-app`'s runner logs `reported=false` and paces against its
  documented default.

**Market lineage:** winit 0.30.13's macOS `MonitorHandle::refresh_rate_millihertz`
(`src/platform_impl/macos/monitor.rs:260`) reads `CGDisplayCopyDisplayMode` →
`CGDisplayModeGetRefreshRate`, returns it when it is `> 0.0`, and only then falls
back to `CVDisplayLinkGetNominalOutputVideoRefreshPeriod`. That is the source
read here and the reason for the `> 0` guard. Read at the version pinned in this
workspace's `Cargo.lock`, not recalled.

**Trade-off:** the cached display follows the window across screens and backing
changes. A mode change that leaves the window where it is (a user switching
refresh rate in System Settings) is **not** observed yet: this backend observes no
screen-parameters notification, so the new period is picked up the next time the
host samples the snapshot (a move, a resize or another owner wake) rather than
immediately. Named rather than assumed away — it is unobservable on the single
fixed-mode display this was measured on.

**Replacement coverage:** the arithmetic is free-standing and AppKit-free
(`RefreshPeriod::from_hz`, reached through `shared::display_period`), pinned by
`refresh_value_classification_table` (non-cadence inputs — zero, negative, NaN,
±infinity — are unknown rather than an infinite or panicking period) and by
`macos_shim_keeps_the_period_it_reports_today`, which holds the shim
to the value this backend reported before the snapshot. The live path is
pinned behaviourally by the same bundled probe that pins the frame pump
(`cargo xtask device macos-frame-pump`), which reports `FRAME_PUMP_PROBE_REFRESH_PERIOD` from
the real backend on the real display: measured 2026-09-17 as `period_us=10000
hz=100.0`, matching the independent AppKit probe's `CGDisplayModeGetRefreshRate`
of 100.000 Hz on the same panel. Like every macOS-gated test here, these run
locally only — CI has no macOS test job and `cross-typecheck` is lint-only.

### AppKit's input method is a route for a `keyDown:`, not a second producer of one

**Decision.** `FLUIContentView` conforms to `NSTextInputClient` (all 11 methods plus
`add_protocol`), and `keyDown:` becomes a gate rather than a direct conversion:
while `TextInputState.ime_allowed` is **false** it calls `handle_input_event`
exactly as before; while it is **true** it hands the event to
`interpretKeyEvents:` and emits nothing itself, with `doCommandBySelector:`
re-dispatching the events the input method declines. `PlatformTextInput` is
implemented by `MacOSTextInput`, whose two setters route through the owner lane.

**Why.** AppKit's composition pipeline is entered only by `interpretKeyEvents:`,
which calls *back* into the same view (`setMarkedText:` while composing,
`insertText:` on commit). Calling it alongside the existing conversion would make
one physical press reach the application twice — a `Key::Character` **and** an
`ImeEvent::Commit`. That is ADR-0044 §3's double-producer defect class, and
ADR-0069 states the contract it violates. The gate makes attachment the *only*
variable: the pre-existing path is byte-identical at the default, so the
conformance adds no regression surface to a window that never attaches a text
input.

**Market lineage.** winit 0.30.13 documents the same rule in the same terms
(`Window::set_ime_allowed`): with IME allowed the window receives `Ime` events
and, during the preedit phase, no `KeyboardInput`; with it disallowed the window
receives no `Ime` events and a `KeyboardInput` for every keypress; and IME is
**not** allowed by default. The winit backend inherits this by delegation
(`WinitTextInput`); the native backend owns the pipeline instead of delegating,
so it implements the protocol rather than wrapping one. The documentation covers
the *press*; reading winit's macOS implementation shows the release is gated too
and on a different variable — `key_up` queues a `KeyboardInput` only from its
`Ground` and `Disabled` states, so an open preedit suppresses the release while a
committed character's release still arrives (`ImeState::Committed` is reset to
`Ground` inside `keyDown:`). `key_up` here gates on the same condition expressed
in this backend's own state (`TextInputState::reports_key_release`), so a release
inside an open composition is suppressed for the same reason the press was: the
application never saw that key go down.

**Teardown drops the composition.** `set_ime_allowed(false)` **drops**
the composition (ADR-0069) and `unmarkText` emits
`ImeEvent::Preedit { text: String::new(), cursor: None }` rather than nothing.
It is grounded in the client-side bug class
`flui-platform-api/src/ime.rs` records — a client left holding composition state it was
never told ended suppresses `Key::Character` for the rest of the focus session
and keeps the cancelled slice in its buffer — and in winit's own macOS
implementation, which drops the marked text on `set_ime_allowed(false)`, so
ADR-0069 cites that implementation for the drop-vs-commit half. AppKit's header does not say whether an empty
`setMarkedText:` always precedes `unmarkText`, so both paths are covered; the
event is inert when nothing is composing, which is why the callback carries no
`hasMarkedText` check.

**Rejected.** Unconditional `interpretKeyEvents:` (the two-producer defect).
Gating on `-[NSTextInputContext handleEvent:]`'s `BOOL` — the routing decision is
already correct before the call, and consulting AppKit first would take the
keyboard path in exactly the states where both producers are live. A pre-call
predicate on the key, which is undecidable before the input method runs, and
wrong per layout: on a U.S. layout a plain letter *does* reach `insertText:`.

**Marked-range convention, a choice a document-less view cannot do better than.**
`markedRange` answers `(0, utf16_len)` while composing and `{NSNotFound, 0}`
otherwise; `selectedRange` echoes what `setMarkedText:` delivered. `NSRange` is
UTF-16 and `ImeEvent::Preedit.cursor` is a **byte** range, so the conversion is
`utf16_range_to_byte_range` — AppKit-free and tested.

**Trade-off, named rather than assumed.** The class-registration block cannot run
under a bare `cargo test` (an unbundled `NSWindow` throws
`_CFBundleGetValueForInfoKey`), so every encoding and selector here was
hand-checked against `objc-0.2.7`'s `add_method` preconditions until the bundled
probe executed them — which it now has, on a real Mac, with all five assertions
passing. What the probe measures about the rect is that it is non-zero and
screen-relative; the **direction** of the `firstRectForCharacterRange:` Y-flip is
still designed rather than measured, because a flip of the wrong sign produces a
non-zero screen rect too. Measuring which side of the caret the candidate window
would land on needs a real input method to ask, which is the same gap the probe's
own module doc states.

**Replacement coverage.** The conversion carries seven always-run tests
(ASCII identity, multi-byte divergence from UTF-16 offsets, CJK unit-vs-byte, a
range splitting a surrogate pair, past-the-end, empty text, and AppKit's real
`NSNotFound` location), and an eighth pins the `keyUp:` gate across all four
states it distinguishes — unattached, attached-but-idle, composing, and a
composition that has just ended. It is mutation-checked against the gate a
reader would first reach for (`!ime_allowed` alone fails it on the
attached-but-idle case). `cargo xtask device macos-ime` drives the live path from a bundled
`.app` and reports `IME_PROBE_RESULT=PASS` on a real Mac: the inverse pair
(attached → one `Commit` and no key event; detached → one key event and no
`ImeEvent`), the `Preedit` → `Commit` sequence with the byte cursor asserted, the
empty-`Preedit` that ends a composition, a non-zero cursor rect whose
`actualRange:` is answered rather than left not-found, and the `keyUp:` gate
end-to-end — a release reported while a text input is attached with nothing
composing, and the same call suppressed while a composition is open. The gate's
arm is mutation-checked against the ungated `key_up`: removing the check leaks a
`KeyboardEvent { state: Up, key: Character("a") }` into that arm and fails the
probe. Its own module doc states that a synthesized `NSEvent` proves the
*routing* and is not a genuine input method, so real composition (press-and-hold
or a CJK source) remains **not driven**.

### iOS binds UIKit through `objc2`, and its loop-exit signal is `applicationWillTerminate:`

**Decision.** The iOS backend (`platforms/ios/`) binds UIKit through `objc2`
0.6 / `objc2-ui-kit` 0.3 / `objc2-quartz-core` / `block2` /
`dispatch2`, and takes its framework loop-exit signal from
`applicationWillTerminate:`. ADR-0071 carries the full record.

**Why `objc2` and not the macOS backend's `cocoa`/`objc`.** The choice is not
consistency-versus-modernity: `objc` has not released since 2019 and `cocoa`
has **no UIKit surface at all**, so the macOS stack cannot express this platform
even in principle. `icrate`, which this module's stub doc once named, is a
deprecated alias split into the `objc2-*` crates. Every shipping Rust Apple
stack is on `objc2` (winit's iOS backend since 0.30, wgpu-hal, egui, slint,
gpui), and the macOS module's own header already commits to migrating there.

**Why `applicationWillTerminate:`.** `UIApplicationMain` never returns, so
there is no "after `Platform::run`" for the runner to use — the desktop and
Android runners call `teardown_platform_ui_runtime()` there. Without a deliberate
choice the framework never receives its loop-exit signal on iOS and leaks every
UI runtime, service pool and the clipboard for the process's life. That delegate
method is the only pre-exit notification iOS sends, so the platform fires its
quit handler from it and the runner runs the teardown.

**Alternatives considered.**
- *A process-global for the delegate's session state* — rejected: it would add
  a new process-global to the runtime's ambient reach, and the state is
  main-thread-only anyway. A thread-local is the owner-affine
  scope ADR-0027 prefers and matches winit's own `ACTIVE_EVENT_LOOP`.
- *`UIScene` adoption* — deferred, not dropped. It is the multi-window iPadOS
  feature and would make `UIScreen.mainScreen` (deprecated in the scene era)
  scene-relative; this backend presents one full-screen window, so the app-wide
  accessor is the honest spelling and the module carries an
  `expect(deprecated)` with that reason.
- *`objc2-ui-kit` with default features* — rejected: it enables all ~456
  header features, compiling the whole framework's bindings for a handful of
  classes. The backend names the classes it messages, as `winit-uikit` does.

**Trade-off.** Two `objc2` generations are in the tree: this backend uses
0.6.4/0.3.2 (already there via `wgpu-hal`), while `winit` 0.30 and
`accesskit_macos` 0.27 still pull 0.5.2/0.2.2. The duplicate resolves when
those move (H10 / winit 0.31), not by anything here. A real device is not
covered — the simulator slice is, via `cargo xtask device ios-sim` — and no CI job boots a
simulator; `cross-typecheck` gained an `aarch64-apple-ios` clippy line so a
broken iOS build is at least loud.

**Replacement coverage.** `cargo xtask device ios-sim` (executing; asserts a Metal device
and a rendered frame from the app's own log, plus a screenshot) for the
end-to-end path, and three host-run unit tests on `display.rs`'s bounds
arithmetic. The engine-side portability fix this uncovered —
`required_limits` clamped to the adapter's own, because the simulator's Metal
adapter caps `max_inter_stage_shader_variables` at 15 where
`wgpu::Limits::default()` asks for 16 — carries a comment at the clamp.

### macOS and iOS share the `objc2` binding stack; the `cocoa`/`objc` pair is gone

**Decision.** Both Apple backends bind their platform frameworks through the
`objc2` family (`objc2`, `objc2-app-kit` for macOS/AppKit, `objc2-ui-kit` for
iOS/UIKit, `objc2-foundation`, `objc2-quartz-core`) at the
versions `wgpu-hal` already pins. The `cocoa` 0.27 / `objc` 0.2 dependency pair
and the `build.rs` that existed only for its `cfg` macros are removed from the
crate; neither appears in `Cargo.lock` any more. ADR-0071 carries the full
record (ADR-0071 chose the stack for iOS first).

**Why.** `objc` has not released since 2019 and points at `objc2` as its
successor; `cocoa` deprecated its whole surface in the same direction and has no
UIKit bindings at all. Every shipping Rust Apple stack is on `objc2` (winit since
0.30, `wgpu-hal`, egui, slint, gpui). Carrying both stacks was a two-generation
wart with a `build.rs` to feed.

**The raw `msg_send!` shape is kept on purpose, not left un-migrated.** objc2's
macro accepts a raw `*mut AnyObject` receiver, `Bool` arguments and a manual
`release`, so `window.rs`'s already-reviewed safety shape survives. The concrete
reason: `NSWindow`/`NSView` are `MainThreadOnly` in objc2's typed API, while the
backend constructs test windows on a caller-supplied off-main serial lane
(`MacOSWindow::for_test`), which a `MainThreadMarker`-gated method would refuse.
`ClassBuilder` replaces `objc` 0.2's `ClassDecl` for the two runtime classes
(`FLUIContentView`, `FLUIWindowDelegate`).

**Two defects the stricter macro caught.** objc2's `msg_send!` verifies return
types against the selector encoding at run time and found `makeFirstResponder:`
declared `void` where AppKit returns `BOOL` — silently accepted by objc 0.2. It
also made the cursor-icon match's explicit `arrowCursor` arm list
`clippy::match_same_arms`-visible (kept as documentation under a scoped allow).
The newer macro is a stricter oracle; that is part of the migration's value.

**Trade-off.** A second `objc2` generation is still in the lock via
`accesskit_macos` 0.27 and winit 0.30 (both on 0.5); resolved when those move
(H10), not by anything here.

**Replacement coverage.** The four bundled macOS probes are unchanged in what
they assert and all PASS on `objc2`, driving the migrated `msg_send!` sites
through the production launch path; the five real-`NSPasteboard` tests and the
`display.rs` arithmetic run in the normal suite.

### Window-independent owner turns

`shared::owner_signal` owns coalescing, callback leases and quit admission for
native desktop and headless. Physical posting is serialized with queued-state
acknowledgement: a concurrent sender cannot receive success for another sender's
failed post. Callbacks and capture drops execute outside locks; active delivery
covers both, so nested owner dispatch cannot recursively invoke a replacement.
Quit fences admission before posting and remains retryable after posting failure.
Per-run weak proxy stamps retain the original owner thread even after expiry.

The signal holds admission and scheduling state; the callback waits between
turns in a `TurnSlot` the caller passes to `register_in` and `drive_in`. Win32
keeps an `OwnerTurnSlot` (`!Send`) in its owner control context, so closing or
dropping the last `Arc<OwnerSignal>` on another thread never drops the
callback there; the owner clears the slot on quit, or `WM_NCDESTROY` frees it
with the context, and a platform dropped off its owner leaks it with the
context. **Unasserted:** no test pins this. macOS, UIKit, winit and headless
still use `register` and `drive`, backed by a shared slot inside the signal,
until their own steps of ADR-0082 §4.

macOS posts through GCD, winit through its existing EventLoopProxy, and Win32
through a dedicated message-only class with its own typed userdata. Headless
provides `HeadlessOwnerTurns`, an owner-local driver and one-shot posting failure
injection for deterministic app recovery tests. Successful headless run return
leaves the retained logical owner alive; failed bootstrap, quit and destruction
close it. The app uses this transport for pending-window completion without a
window or UI runtime frame. It is not a generic closure executor.

Run the live macOS oracle with a GUI session:

```sh
cargo build -p flui-platform --locked --example owner_wake_probe
python3 tools/device-checks/check-owner-wake.py target/debug/examples/owner_wake_probe
```

External counters assert owner affinity, nonrecursive delivery, shutdown capture
release and rejected work after quit. On Windows, a turn through the real
message loop runs on the owner and is released there. **Unasserted:** no test
pins this. CI only type-checks Win32. Mobile/web registration remains explicitly
unsupported; proxy window creation support is separate from wake/quit support.

### Reveal without changing window mode

`PlatformWindow::show` is distinct from `restore`: restoring also removes
maximization, while revealing a resident application's existing main window must
preserve maximized/fullscreen mode and geometry. The operation reveals and
requests focus; operating-system focus policy still decides whether focus moves.
AppKit deminiaturizes then orders the window forward. Win32 queries
`WINDOWPLACEMENT` fallibly and interprets `WPF_RESTORETOMAXIMIZED` only for minimized
placement, using `SW_SHOW` for non-minimized windows. Winit 0.30.13 Wayland reveal
is rejected as `WindowShowError::Unsupported` before mutation because unminimize
and focus are unsupported there. Closed retained windows return `Closed`.

Sources: [winit Window](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html),
[Win32 WINDOWPLACEMENT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement).
Headless tests model minimized independently from maximized/fullscreen. Native
macOS `window_show_probe` covers visible, hidden, minimized, maximized,
minimized-after-maximized and observed fullscreen transitions; Windows evidence
is strict cross-compilation, not a native runtime claim.

Win32 show verifies retained callback-Arc identity under the owner context guard
before reveal and again after callback-capable show/foreground calls. Recycled
HWND or context addresses cannot redirect a retained wrapper's show operation.
The portable operation-sequence test asserts that stale identity causes neither
reveal nor focus. This does not change the separate legacy teardown route or
claim native Windows runtime verification.

### The first reveal waits for the first presented frame, behind alpha 0

AppKit shows a window the moment it is ordered front, and the first frame
reaches the compositor only once the GPU stack behind it exists and the first
present lands — 2.81 s on a measured cold launch — so ordering an opaque window
at open shows its bare background for that whole gap (the launch blank-window
observation in `docs/BETA.md`). The deferral is the caller's to ask for,
through `WindowOptions::reveal`: with `WindowReveal::AfterFirstFrame`, macOS
orders a `visible: true` window front at `alphaValue` 0 and restores alpha 1
when the embedder reports the first presented frame through
`PlatformWindow::reveal_after_first_frame`, or when the window is shown
explicitly (`show`, `set_visible(true)`, `activate`). The flag is settled under
the window's state lock, never inside an owner route, so exactly one caller
performs the reveal.

Opt-in rather than the backend's default, because the reveal report is a
promise only a frame-loop owner can keep, and the promise is selected at the
one place that keeps it: `flui-app`'s desktop runner asks for
`AfterFirstFrame` at its two open sites (the main window and a content-bearing
secondary window, through `runner::desktop::rendered_window_options`), whose
frame closure performs the reveal. Its shared `From<&AppConfig> for
WindowOptions` conversion stays at `AtOpen` on purpose — `run_direct` and the
bare `open_secondary_window` use it and have no frame loop that would reveal.
A direct consumer of this crate — `examples/platform_window.rs`, `wgpu_window.rs`,
the platform probes — likewise has no first frame to report, and a window
whose reveal waits for a call that never comes is a window nobody sees. The
default `WindowReveal::AtOpen` therefore leaves alpha untouched and those
windows appear at open, as they always did. A backend that cannot defer treats
`AfterFirstFrame` as `AtOpen`.

Transparent rather than hidden, because a hidden alternative was tried and
measured on 2026-09-21: an un-ordered window gets no Metal drawable — wgpu
reports the surface occluded on every acquire (30+ withheld frames in the log)
— so a reveal that waited for a present into a hidden window waited for
something that cannot happen and fell through to the embedder's 1 s fallback.
Ordered at alpha 0, the first acquire still comes back occluded once (a
post-configure quirk of the Metal surface) and the second presents; the reveal
followed the first frame by 85 ms, and the window's first on-screen sighting
through the CoreGraphics window list was already painted.

The embedder's half is `flui-app`'s `FirstReveal` policy: the first presented
frame earns the reveal, and a frame that ran and presented nothing arms a
bounded fallback (one second from that outcome, not from install, so a slow
cold frame still inside its render arms nothing) after which the window is
revealed regardless — a surface that never presents yields a window the user
can see and close rather than a process with no window. The fallback deadline
joins the desktop wake-deadline hook and the frame closure's dirty predicate
like every other wake-deadline source there.

The winit backend reveals at open, unchanged: whether a hidden X11 or Win32
window hands out swapchain images could not be verified here, and a wrong guess
would cost every Linux launch the fallback's second. Wayland maps a surface at
its first commit, which is the first present, so the deferral there is the
protocol's own. Win32's native backend is likewise unchanged.

### Per-window execution eligibility

`WindowExecutionState` separates native execution permission from focus, visibility
and GPU readiness (ADR-0072). The callback slot is private. iOS serializes native
observations in a separate queue and delivers effects through immediate callback
leases, independently of the general input/frame FIFO. Invocation and capture
destruction are contained separately, outside storage locks. Only resource intents
advance the generation: focus reentry cannot cancel a resource transition, while
superseded resource continuations and queued restorations are rejected. A queued
background pauses native ticks immediately, including during nested UIKit loops. Temporary inactivity preserves the surface;
true background suspension pauses the display link and releases it. Foreground
restores execution while unfocused; duplicate foreground preserves established
focus. Headless simulation exercises the same observation contract.

Android publishes `Resume`/`Pause` as a shared execution snapshot before invoking
embedder callbacks. Its window reports `Running` only while resumed and otherwise
reports `Suspended`; focus and native-window availability remain separate facts.
This makes redraw-backed owner continuations honest: the loop consumes redraw
flags only after polling confirms execution is still running. A queued `Pause`
therefore preserves the flag rather than acknowledging a continuation whose frame
will be refused; `Resume` makes the callback deliverable again. The app also retains
a logical continuation when the initial actuator refuses it and retries from the
next fresh native root.

The iOS protocol probe is reproducible with a dedicated booted simulator:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -p flui-platform --example ios_execution_probe --target aarch64-apple-ios-sim --no-default-features
python3.12 tools/device-checks/check-ios-execution.py <UDID> target/aarch64-apple-ios-sim/debug/examples/ios_execution_probe
```

Use `--case foreground-reentered-background`, `background-reentered-foreground`,
`duplicate-foreground`, or `foreground-callback-panic` for the original cases.
Additional cases cover `background-nested-active`, `background-nested-inactive`,
`foreground-nested-active`, `foreground-nested-inactive`,
`nested-foreground-then-panic`, `superseded-foreground`, and
`background-nested-run-loop`. The latter verifies that a queued background stops
frames before the observer pumps a nested native run loop.
The runner terminates only its owned probe after marker completion; this is not a
claim of normal UIKit loop return or a real OS background transition. Xcode 26.2
is the verified SDK. UIScene ownership and migration remain separate work.

### UIKit scene attachments

UIKit process bootstrap and scene connection are separate ownership boundaries
(ADR-0073). `IOSPlatform::on_scene_event` registers the fallible, owner-thread
consumer before UIKit connections. A stable logical view backs a session; native
windows/controllers and display links belong to individual attachments. A
reversible disconnect retains the logical owner but denies new raw handles.
Terminal close fences callbacks and retires the attachment before disposal.

`MainThreadBound` protects native storage. Final worker drops enqueue retirement
on main rather than synchronously waiting for it. Plain cached metrics remain
readable without touching UIKit. The native weak-view probe verifies both
retention through logical close and eventual release after a worker's final drop.
OwnerSignal/GCD carries typed process wake/quit independently of display links;
it grants no operating-system background execution entitlement.

### A ready task imposes no executor bounds on its result

`Task<T>` moves its result out and never exposes a pinned borrow to it.
The handle is therefore `Unpin` independently of `T`, as is the native
`JoinHandle<T>` variant. Its `Future` implementation accepts local, borrowed
and non-`Unpin` results; `Send + 'static` belongs to executor admission,
where work crosses threads, rather than to polling a completed value.
Dropping a spawned handle detaches the Tokio task; it does not cancel it.
Pinned by `ready_tasks_return_results_without_executor_bounds` through the
public API.

### Browser display units, held buttons and callback reentry

Browser screen dimensions are CSS pixels; display bounds expose rounded device
pixels after applying the device pixel ratio. Pointer coordinates retain the
platform input contract of logical pixels, including the upstream
`PhysicalPosition` storage name. DOM button masks preserve X1/X2 on down, up,
move and wheel events. The `browser_input_probe` example records translated
public input and display results for real DOM dispatch at a non-unit DPR.

Platform window-event handlers are leased outside the state mutex and restored
unless the callback installed a replacement. Setter retirement also occurs
outside the mutex. Quit commits stopped state and consumes its callback before
invoking it, so reentry cannot invoke the same quit callback again. Owning
callback envelopes remain outside the caught invocation; a failed displaced
callback is retained rather than destroyed while propagating its first failure.
`created_callback_probe` and `quit_callback_probe` in that example exercise
reentrant clipboard access. Each probe requires a fresh page and an external
deadline: the previous implementation blocks or aborts the browser thread under the
state mutex. Source inspection and type checking do not prove browser execution
or arbitrary aggregate destructor containment.

### The Web display period is measured from animation frames and is an estimate

**Rule:** a browser page cannot ask for its display's refresh rate, so the Web
backend reports one only as a measurement of what the page can see, says so, and
reports unknown until it has measured. Capabilities are detected by asking for the
function, never by reading a browser version.

**Choice:** `platforms/web/display.rs` owns the snapshot. The period comes from the
timestamps `requestAnimationFrame` hands the platform's existing frame loop
(`platform.rs`), fed to `shared::refresh_estimate::RefreshEstimator`: the trimmed
mean (without the fastest and slowest tenth) of the last 30 intervals, with
intervals longer than three times the window's lower quartile left out (a background
tab, a debugger pause, throttled frames: a window that is mostly throttled has a slow
median, so the baseline is the fast end, not the median). A mean and not a median
because a clock clamped to whole milliseconds reports 120 Hz as 8 and 9 ms
intervals, whose median is 8.0; the trim keeps a few dropped frames out. The first
value waits for a full window. A replacement is published only when it has left a 3%
band around the published value and then stayed within 1% of where that run began for
five consecutive frames, while the mean of the latest ten intervals agrees with it
(within 6%): while the window holds two rates the estimate glides from one to the
other at a value the display never ran at (a live run published 95 Hz on the way from
144 to 100 Hz before the stability rule), and a gliding estimate is never still. The band keeps 59.94 and 60 Hz one value and 60, 75, 90,
120 and 144 Hz apart. The estimate restarts when the tab becomes visible and when
the pixel ratio changes (the intervals no longer describe the cadence; the published
period stays until a fresh estimate replaces it), and is forgotten when the
`currentScreen` object changes or, without screen access, when `window.screen` raises
`change` (the only sign of a move to another monitor, indistinguishable from a
metrics-only change), whose rate the old value does not describe. A
published change invalidates the cached snapshot and wakes the owner through the
`OwnerSignal`; the owner's next turn reads the new snapshot.

`Platform::display_snapshot` runs on every owner wake, so the snapshot is cached and
read from the browser again only after an event that can change it: a pixel-ratio
change, a published period, `change` on `window.screen`, a change of the screen list,
of a listed screen or of the window's screen.
The pixel ratio is watched with `matchMedia("(resolution: Xdppx)")`
(`shared::dppx_query`): the query matches while the ratio is exactly `X`, so it
fires on any change and is spent by it, and the next one is armed for the new ratio.
Arming checks `matches()` and reads the ratio again, a bounded number of times, when
it moved between the read and the listening (a query that does not match would never
fire for a change that already happened); a refused query is retried for as long as it is refused, after a growing wait
(one frame, two, four, up to 120), and also at once on a window `resize` or a
`window.screen` change, which zoom already fires: a watch that stays unarmed would
miss every zoom and scale change (accepted work must stay deliverable). Every query is listened to with one retained JS function, so the callback
that is running never frees itself. `window.screen` raises `change` for a size, ratio
or position change in Chromium without any permission, detected by `onchange` being
present (`WebFeature<ScreenChange>`); the browser calls that need a capability are
methods on its proof (`platforms/web/features.rs`).

Several screens are listed only when `getScreenDetails` exists and the
`window-management` permission already reads `granted` (the older
`window-placement` name is tried when the current one is rejected). The platform
never requests the permission: a prompt is the application's explicit act, and a
permission granted later is noticed through the `PermissionStatus` change event.
Each `ScreenDetailed` object is the screen's identity, held in a retained list
(`shared::screen_identity`): a screen found in the list keeps its `DisplayId`, an
object that matches nothing gets a fresh one, and an id leaves the list with its
screen and is never issued again. Labels are not keys; two identical monitors share
one. A `change` on a listed screen (it is followed with the list) refreshes the
snapshot. The estimate belongs to the screen the window is on (`currentScreen`); the
others report unknown. The current screen reports `window.devicePixelRatio` as its
scale, which includes page zoom, exactly as in the single-screen case. Bounds do not
move with zoom (a live run showed them inflating): with access they use the screen's
own `devicePixelRatio`, origin included, so a layout of mixed-ratio screens is the
browser's coordinate space expressed per screen, not one global device grid; without
it they use the page's ratio when `window.screen` was first seen (Chrome's
`screen.width` ignores zoom and `devicePixelRatio` is OS scale times zoom, so the
zoom cannot be separated afterwards), taken again on a `change` of `window.screen`.
A page that starts zoomed overstates its bounds until then. The first time a listing
appears, its `currentScreen` adopts the id of the single-screen display, so the
window sees the display gain a name, not change.

**Limits:** in a browser whose `screen` has no `onchange`, moving the window between
two screens of equal pixel ratio but different size raises no event, so the bounds are
stale until the next one. The estimate is the presented cadence: a power saver, a
throttled frame or a variable-refresh panel lowers it. Both are documented on the
module. `WebWindow::display_id` names the current screen; the window raises no
`on_display_changed` because every event above already wakes the owner, whose turn
re-derives each window's display from the snapshot.

**Alternatives considered:**
- Reading `screen.refreshRate` or any version-keyed table: no such property is
  standard, and a version says nothing about what the user enabled. Rejected.
- Requesting Window Management access on startup: the prompt would appear in every
  application. Rejected; the application asks, the platform follows.
- Keying a screen by its label or position: both repeat or move. Rejected for the
  object, the only identity the API offers.

**Coverage:** the rules are free-standing and run on every host
(`web_refresh_estimate_table`, `dpr_change_arms_the_query_for_the_new_ratio`,
`retained_screen_keeps_its_id`, `web_feature_absent_gives_no_handle`); the browser
glue runs on Node in the wasm lane against a controlled boundary
(`tests/wasm32/display.rs`) and by hand through the `browser_display_probe` example.

### Web logical keys validate their source grammar

Web keyboard and MouseEvent-based pointer/wheel input preserve positive
canonical getModifierState observations, including AltGraph and toggle states,
alongside the four convenience booleans. Supported legacy Hyper/Super modifiers
normalize to Meta. No source inference from Ctrl+Alt, AltRight or a named-key
sequence is made; raw facts remain distinct from widget command interpretation
(ADR-0201). Native observation completeness is not implied by the owned DTO.

The browser probe's `observed-modifiers`, `modifier-getter-reentry` and
`modifier-getter-retirement` controls test registered DOM delivery through public
input, source metadata, same-family reentry and retired-owner silence followed
by healthy replacement delivery. FnLock/SymbolLock and legacy source overrides
are explicitly controlled-getter cases. Constructor delivery is not a physical
keyboard/layout or native-IME witness.

The private DOM getter retains the original UTF-16 string until admission.
`JsString::is_valid_utf16` refuses an unpaired surrogate before conversion can
replace it with U+FFFD. The literal replacement character and valid surrogate
pairs remain valid inputs. The getter runs once, including during reentry.

Both DOM keyboard states resolve the owned named-key vocabulary and accepted
Hyper/Super aliases before admitting a character value. A Web character is a
nonempty NFC string with zero or one non-control base followed by combining
characters, as defined by
[W3C key strings](https://www.w3.org/TR/2025/REC-uievents-key-20250422/#key-string).
Control means Unicode category Cc; combining means Mc or a nonzero canonical
combining class. A scalar or grapheme count does not establish that grammar.
Refused values become Unidentified without discarding physical code, location,
timestamp, modifiers, composition or the Down/Up observation. No normalization
rewrites source meaning. Common KeyText and native producers keep their
nonempty-text contract (ADR-0143).

The private Web conversion uses ICU4X `icu_properties` and `icu_normalizer`
2.3 compiled data. General categories, canonical combining classes and NFC
therefore come from one library cohort; combining classes reuse normalization
data. Dependencies are wasm-target-only, and admission adds no owning service,
mutable cache or public parser. The `browser_input_probe` example's
`owned-keyboard-check` exercises registered DOM events through public on_input,
including multi-scalar text, refused-value recovery, complete event metadata
and JSON-sensitive values. Browser execution establishes constructor-driven
translation; it does not establish physical layouts or IME text commits.

### Off-owner accessibility adapter retention precedes diagnostics

The Win32 and AppKit accessibility adapters cannot retire their native subclass
state off its owner thread. Their existing exceptional retention fallback moves
the adapter into permanent retention before attempting diagnostics. A subscriber
failure is contained independently; it cannot restore a destructor obligation on
the wrong thread or replace an unwind already in progress.

The private shared `retain_off_owner_resource` seam is used by both adapter
Drop paths. `the_owner_thread_machinery_honours_its_contracts` exercises the same
production boundary with an opaque aggregate resource surrogate: successful
reporting, an opaque reporting failure, an earlier teardown failure alone and
competing with reporting, and a subsequent callback. These rows do not execute real UIA or
NSAccessibility adapters; they prove the shared boundary. Native wiring remains source-reviewed and subject to
platform type-checking.

### Input processing has no unused platform-side duplicate

ADR-0082 §5's helper retirement removes the zero-consumer `PlatformEmbedder`
abstraction and platform velocity/timestamp helper types. Backends retain their
actual event clocks and translation paths; interaction owns velocity tracking.
The compile-fail example on `flui_platform::traits` checks that retired imports
are unavailable through the consumer surface. This does not adopt the ADR's
remaining backend consolidation or capability removals.

### Android has no separate page-aligned container

Android exposes no framework allocator or page-aligned vector. The removed
container had no production consumers, while its generic element retirement
could leave already-dropped elements marked initialized after a destructor
panic. Framework buffers use standard owning containers; wgpu owns GPU
allocation and its backend alignment requirements. A device's native page size
does not require every application-side vector to use a custom allocator.
This removes an unused unsafe surface rather than asserting native allocation
behavior: Android execution remains unavailable on this host.

### AppKit tab joins borrow live windows

`MacOSWindowExt::add_tab_to_window` takes `&dyn HostWindow` and returns whether
AppKit received the join. A numeric ID cannot establish ownership of an
Objective-C object. The backend rejects a different backend, different owner
lanes, non-main lanes, closed windows and joining a window to itself before
messaging AppKit. Both borrowed wrappers own their NSWindows, which are created
with `releasedWhenClosed:NO`; explicit native retains span the callback-capable
join on their shared main owner lane. No state or registry mutex spans that call.

The method's public compile-fail doctest rejects numeric IDs, and its compiling
example pins the borrowed-window signature and boolean result. These compiler
checks do not prove native tab grouping or close/reentry behavior. AppKit runtime
execution remains unverified on the Windows audit host.

### Win32 observations and closed-window tracking

A window context holds weak references to the wrapper's observation cache and
platform registry. Native size, position, visibility, focus and DPI messages
publish cache changes before invoking callbacks; no cache mutex spans a user
callback or native call. Initial dimensions are the native client dimensions,
and setters do not overwrite a synchronous native observation with their
requested outer dimensions. Minimization keeps the last non-minimized size.

`WM_DESTROY` clears userdata and retires its context ledger before removing the
matching window identity from tracking. The removed owning Arc is dropped outside
the registry mutex. Consequently its destructor cannot recursively destroy the
in-flight native window, and an old wrapper cannot remove a reused HWND's newer
registry entry. Weak context references do not alter the wrapper's final-state
owner count or introduce another cycle. WNDPROC callback failures retain the
existing fail-stop policy; this does not claim recoverable native unwinding.

`test_window_lifecycle_contract` adds bounded Windows subprocess rows
`native_close_retires_final_registry_owner`, `closed_window_retires_tracking`,
`close_callback_releases_external_owner`,
`resize_callback_observes_current_client_bounds` and
`hidden_popup_is_natively_hidden` and
`move_callback_observes_full_native_coordinates`. These create actual hidden native windows,
check owner retirement (including the registry's final owner during native
`WM_CLOSE`) and subsequent opening, compare getters inside and after
real native resize delivery, query native popup visibility and compare full
client-origin observations across a native move beyond signed16 coordinates
and a subsequent ordinary move. Cache and Moved events use `ClientToScreen`;
packed message coordinates remain a fallback only if that query fails. These rows do not
exercise monitor DPI migration or interactive focus changes.


`resize_callback_preserves_large_native_dimensions` pins unsigned `WM_SIZE`
decoding in the same bounded native family. A hidden popup is resized separately
to a width and height above 32767, comparing `GetClientRect` with callback sizes
and public getters inside and after delivery, followed by an ordinary resize.
The row requires the actual native dimension to reach the unsigned range; an OS
limit reports an unavailable witness instead of allowing a false positive.
Zero-size clamping and minimized-window handling retain their existing behavior.

### Win32 keyboard delivery preserves permitted native defaults

Keyboard down/up messages run the input callback first and reach `DefWindowProcW`
unless its result prevents native default handling. This includes the conservative
`DispatchEventResult::DEFERRED` returned during callback reentry. System-key defaults
provide native actions such as Alt+F4, whose close command still passes through the
existing close-veto hook. A callback may close its window; the outgoing context's
guard pins its allocation while a userdata comparison refuses native defaults on
a retired context or a recycled HWND.

The bounded Windows subprocess rows `unhandled_system_key_closes_window`,
`allowed_system_key_closes_window`, `prevented_system_key_preserves_window`,
`system_key_close_honours_veto`, `system_key_callback_can_close_window`, and
`deferred_system_key_preserves_window` send the system-key message through an actual
owned HWND and dispatch the resulting native system command. These are native
message-protocol checks, not physical keyboard input or rendered application checks.

### Linux displays come from the compositor on a connection of their own, and a winit window names its display by native number

`platforms/linux/display/` reads the outputs of a Wayland compositor or an X
server without a window system: `wayland_model` and `x11_model` are pure (they
take events or a RandR state and answer `OutputView`s, so every host checks
them), `wayland.rs` and `x11.rs` are thin adapters behind the `wayland` and
`x11` features, and `source.rs` resolves the views into the snapshot and keeps
them current from the thread `flui-display-linux`. The winit backend takes its
snapshot from it on Linux; Windows and macOS take theirs from their native
sources, and winit's own monitor list is read nowhere.

**Why a connection of its own.** winit enumerates monitors only through a live
event loop and reports no change outside it, and its Wayland connection
belongs to winit. A second connection costs a socket and a thread and gives a
snapshot before the loop runs, with the same global names winit sees. The
thread only waits (`poll` on the connection and a stop `eventfd`), swaps the
new list in, marks the snapshot stale and wakes the owner through a `Weak`
signal; application code never runs on it. Shutdown closes the owner signal's
admission first, then writes the stop descriptor and joins; the thread drops
the connection before it ends. A source that cannot connect, or loses its
connection, answers every snapshot with that error until a watcher starts
again, instead of reconnecting on every owner wake.

**Which protocol.** The rule is winit 0.30.13's, read from its source:
`WAYLAND_DISPLAY` or `WAYLAND_SOCKET` set and non-empty means Wayland, then a
non-empty `DISPLAY` means X11. winit 0.30 no longer reads `WINIT_UNIX_BACKEND`.
The winit backend then checks the choice against the display winit actually
opened (the event loop's `RawDisplayHandle`, which is the one its windows use)
and replaces the watcher on a mismatch. `WAYLAND_SOCKET` hands over a single
connected descriptor that winit consumes, so the source connects by
`WAYLAND_DISPLAY` only.

**Wayland.** An output is listed after its first `wl_output.done`, and each
later burst replaces what is listed only at `done`
(`wayland_output_applies_only_after_done`). `wl_output` is bound at version 4
where offered; version 1 has no `done` and is not bound. `zxdg_output_v1`
supplies the logical position and size, and its own `done` closes a burst only
below version 3. Bounds are in the compositor's logical space (Wayland has no
global device-pixel space), the scale is the mode's width over the logical
width (1.5 where `wl_output.scale` says 2), and `refresh` is millihertz, zero or
less meaning unknown. There is no primary output and no work area. The key is
the version 4 `name`; an output without one gets a fresh id that it keeps while
its global exists (`wayland_below_v4_mints_fresh`), and two live outputs that
report one name are two displays, the later keyed fresh
(`two_outputs_reporting_one_name_are_two_displays`).

**X11.** RandR 1.5 monitors are the displays; an older server is an error. A
monitor is keyed by the XID of its **first** output
(`x11_monitor_is_keyed_by_its_first_output`); a monitor with no output (made by
`xrandr --setmonitor`) is not listed. The refresh rate is `xrandr`'s:
`dot_clock / (htotal * vtotal)` with `vtotal` doubled for a double-scanned mode
and the interlace factor folded into the numerator, in 128 bits
(`x11_big_dot_clock_does_not_overflow`), and zero totals are unknown
(`x11_zero_htotal_or_vtotal_is_unknown`). The scale is `Xft.dpi / 96`, the usable
area the monitor cut to `_NET_WORKAREA` of the current desktop. A whole
reconfiguration (screen, CRTC and output notifications) is one read of the
state, and a state whose displays are unchanged wakes nobody.

**Native number.** winit 0.30.13's `MonitorHandleExtX11::native_id` is the
**CRTC** (not the RandR output), and
`MonitorHandleExtWayland::native_id` is the `wl_output` global name. A display
therefore answers to the CRTCs behind all of its monitor's outputs and to its
global
(`winit_window_matches_its_output_by_native_id`: mirrored outputs of one
rectangle stay two displays, a tiled monitor is one). A monitor handle with no
match, or a window with no monitor, has no display.

**What reaches the window.** `Moved`, `ScaleFactorChanged` and `Resized` ask
the window whether its display changed. A change of the monitor list needs no
event of its own: the thread wakes the owner and the host derives every window's
display from the new snapshot. Two Wayland gaps are named. A window is on no
output until its first frame enters one, and winit reports no event for that, so
a window without a display is looked at again each time the loop goes idle
(`about_to_wait`), which follows the compositor's `enter`. And winit reports
nothing when a window crosses to another output of equal scale, so a refresh
rate that differs between two same-scale outputs is followed only at the next
event of the kinds above; the `wl_surface.enter` events that would tell belong
to winit's connection.

**What was not run.** The pure rows run on every host. The thread is tested
against a scripted session on Linux (`display_thread_contract`: change, wake,
stop-and-join, failure, panic, closed owner, dropped signal). The Wayland and X11
adapters were run live against WSLg (Wayland and XWayland), a headless weston and
an Xorg dummy server whose modes `xrandr` changed; the macOS winit source is
compiled and type-checked (`aarch64-apple-darwin`) but not run, and a window
moved between two real monitors was not observed.

### The file store replaces a value with `std::fs::rename`, and probes an access refusal

`storage::FileStore::write` stages the bytes in a file `tempfile` names uniquely
in the target's own directory, but opens it with `std` and moves it with
`std::fs::rename`, never `NamedTempFile::persist`. On Windows `tempfile` passes
`MoveFileExW` and `SetFileAttributesW` a path without the `\\?\` prefix, which
fails past 260 characters, and marks the file it creates
`FILE_ATTRIBUTE_TEMPORARY`, which the replaced value would keep; `std` prefixes
the path and falls back to POSIX-semantics replacement. A write based on a
version holds an exclusive `File::try_lock` on `<root>/.flui-storage.lock` from
comparing the stored version to the rename, so two processes on one directory
cannot both succeed from the same base. The comparison checks the stored
length against the base's (`StoredVersion::byte_len`) before reading, so a
value of another length is never read for it. A file system without locks
refuses such a write with `LockUnsupported` rather than writing unguarded:
`std` maps only some of its codes to `ErrorKind::Unsupported`, so
`ERROR_INVALID_FUNCTION` (1), `ERROR_NOT_SUPPORTED` (50) and `ENOLCK` are
recognised by code.

Each write stages its bytes in a `.flui-staged-*` file. A writer killed before
its rename leaves that file behind; a based write, holding the root's lock,
removes the store's own staged files older than 60 seconds. A live write
finishes its file within one call, so it is never that old; another program's
files never carry the prefix.

Windows reports a sharing violation (32) and a lock violation (33) only by code;
both are `Busy`. `ERROR_ACCESS_DENIED` (5) is both lasting (a read-only file,
an ACL, a directory in the file's place) and passing (a holder that did not
grant delete sharing, a file pending delete), so a refused rename probes the
target. A directory or a read-only file is `Inaccessible`. A target that
refuses even an open requesting no access is pending delete and `Busy`: an ACL
does not refuse that open, and `fs::metadata` cannot tell, because on that
refusal `std` reads the attributes from the directory listing. A target that
then refuses to be opened for writing with code 5 is `Inaccessible` and never
retried; one that opens, or is held without write sharing, is `Busy`.
Machine-local values always live in `<local root>/.machine-local/`, so they
stay apart from roaming values when both roots are one directory (Linux),
however the two paths are spelled: the store never compares paths. No
`StorageName` starts with `.`, so neither that directory, the lock file nor a
staged file collides with a value.

`storage::file_store::tests::file_store_interruption_matrix` stops a write at
each step and reads the old whole value until the rename and the new one after
it; `two_threads_interleave_and_exactly_one_conflicts` holds one writer between
its comparison and its rename until the other has tried, and fails without the
lock; `a_based_write_sweeps_stale_staged_files` ages one orphan and keeps a
young one and a foreign file. `storage::os_error::tests::os_error_classification_table`
pins the mapping of codes to `StorageError`, including the lock codes. The
`file_store_contract` table in `tests/file_store.rs` covers the directory
created on the first write, a directory in the file's place, the lock, the
read limit, the two scopes under separate roots, one root and one root spelled
two ways, a long non-ASCII path; on Unix only, a stale base refused against a
sparse terabyte without reading it; on Windows only, a read-only target
(`Inaccessible`), one held without delete sharing and one pending delete
(both `Busy`, reached through code 5 and the probe). `sync_all` matters only on
power loss, which no test causes; a killed process (`taskkill /F`) and two
processes on one directory are left to the dated native run.

### The platform names the data roots; the Web keeps values in IndexedDB

`Platform::data_dirs` answers where an application keeps its data, so the host
asks the running backend instead of guessing from the target. The default is
`storage::data_dirs` (the `dirs` crate). Android overrides it, because `dirs`
has no root there: data under `internal_data_path()`, which Auto Backup copies,
and machine-local values under its sibling `no_backup`, which Android never
backs up. On wasm32 `FileStore` keeps the native methods, returning futures:
one record per name in the IndexedDB database `flui-storage`, keyed
`<root>/<name>`, the value a `Uint8Array`. Each call is one transaction on a
connection it opens and closes, so an idle connection never blocks another
tab's upgrade. A based write reads, compares and puts inside one readwrite
transaction; the browser runs readwrite transactions over one store one at a
time across every connection of the origin, so of two tabs writing from one
base exactly one succeeds. `Replace` skips the comparison, as on native. A
quota abort is `Full`, a page without IndexedDB `Unavailable`, a record that is
not a `Uint8Array` `Inaccessible` on read and never a base. Each value's
length is a record in a second object store, `lengths`, put in the same
transaction as the value, so it can never pair with another tab's value. A
read over the limit and a based write whose base has another length
(`StoredVersion::byte_len`) are decided from that record alone, without a
cursor over the value; a value with no record (another script wrote it) is
loaded and measured. The JavaScript values stay
in a task of the page (`spawn_local`) and the answer crosses a oneshot, so the
returned futures are `Send` and the host's IO lane awaits them.

A Web Lock around a compare-and-replace in `localStorage` was rejected:
`localStorage` has no lock across tabs, but neither is it consistent across
them. Each tab reads a cache the browser updates asynchronously, so a tab
granted the lock next can still compare against a stale value. IndexedDB's
transaction scheduling covers ordering and visibility at once.
`tests/wasm32.rs` (`page_store`) covers a based write from its base and from a
stale one, two writes from one base started together (exactly one lands), the
read limit and a base of another length decided without a value cursor (the
stand-in counts them), and a page without IndexedDB. Node has no IndexedDB, so CI runs
them against a stand-in that runs one transaction at a time; with
`WASM_BINDGEN_USE_BROWSER=1` they run against a browser's own. Storage needs no Cargo feature:
`AppConfig::with_storage_dir` is the one switch.

### Android samples its displays through JNI and refreshes them on announced change only

**Rule:** `Platform::display_snapshot` runs on every owner wake, so a backend whose
source costs a native round trip keeps the last snapshot and reads again only after
something that can change it has announced itself. Nothing polls.

**Choice:** `platforms/android/display.rs` reads every `Display`
(`DisplayManager.getDisplays`): the real size and density (`getRealMetrics`,
scale `densityDpi / 160`), the rate of the current `Display.Mode`
(`getRefreshRate` when the mode is absent), the highest rate among
`getSupportedModes`, and `getDisplayId`, which keys the identity
(`DisplayKey::AndroidId`; `getUniqueId` is hidden). The Activity's display is
`Activity.getDisplay` from API 30 and `WindowManager.getDefaultDisplay` below; it
is what `AndroidWindow::display_id` answers. The snapshot goes stale on
`ConfigChanged`, on `Resume` and on the choreographer's refresh-rate callback
(API 30+: an adaptive 60/120 Hz switch raises no configuration change); each of
those wakes the owner, whose next wake does the read. Below API 30 the rate is read
at the first two events only. The kept snapshot is the shared `DisplaySource`
cache (`invalidate`, `snapshot_with`); an Activity whose display is absent from
a sample is `note_miss`ed, which allows one more sample per snapshot revision.
`getDisplays()` also lists virtual displays, and a display id is never reused
within a boot, so a re-plugged display gets a new identity. The callback runs in
the app thread's own `poll_events`, invalidates the source and wakes the owner; it is taken down
(`AChoreographer_unregisterRefreshRateCallback`) on that thread when the loop
exits, before the data it points at is freed, and a registration dropped on any
other thread leaks that data instead of freeing what a late call could reach.
The idle `poll_events` timeout is the display's period, cut short by the time
left to the next wake deadline (`idle_wait_until`), and 16 ms while the period is
unknown. The loop does not block until the deadline even though `OwnerSignal`
wakes the looper: the same iteration samples preferences every 500 ms, settles
the safe area and consumes redraw requests that set a flag without waking the
looper, so each of those would wait for the deadline.

`shared/android_ndk.rs` reads the API level once, when the platform is
built, and keeps it in the platform (no `static`): `android_get_device_api_level`
looked up by name, then `__system_property_get("ro.build.version.sdk")`. The
function is exported by `libc.so` only from API 29 and is an inline function in the
NDK header below it, so linking it directly would break `minSdk` 23; the property
reader is exported from API 23. NDK functions newer than `minSdk`
(`ANativeWindow_setFrameRate`, `AChoreographer_registerRefreshRateCallback`: API
30) come back as `NdkFn<N>`, a typed handle that exists only when the level and the
symbol both do, and that keeps the `dlopen` handle of its library alive. `N` is a
marker of the sealed `NdkFunction` trait, which pairs the symbol name, the first
level (as its `Gate`) and the pointer type taken from the NDK header, so resolving is
safe code. The Java-side gates (`Context.getDisplay`, `fontWeightAdjustment`, the
scaled scroll factors) take the same level, and an unreadable level is
`AndroidApi::UNKNOWN`, which unlocks nothing.

`PlatformWindow::set_frame_rate_hint` is `ANativeWindow_setFrameRate(window, max, DEFAULT)`
for `High`, with `max` the highest supported-mode rate of the Activity's display, and
`0.0` for `Default`. The window keeps its last hint and sends it again on
`InitWindow`, because the request rides on one `ANativeWindow`, and again when a
sample finds another highest rate for the Activity's display (a `High` hint
skipped while that rate was unknown). The host also sends its demand on
`PrimarySurfaceRestored`; the two overlap on a resume and are idempotent.

**Alternatives considered:**
- `DisplayManager.DisplayListener` — it needs a Java object implementing the
  interface, which a `NativeActivity` app without Java code does not have.
- Polling `Display.getRefreshRate` — the 500 ms preference poll is a bound on
  configuration getters with no notification, and a display read would cost a
  second round trip each time for a value the choreographer announces.
- Linking `android_get_device_api_level` or the API 30 functions directly — the
  binary does not load on an older system.

**Trade-off:** below API 30 an adaptive-rate switch that raises no configuration
change goes unnoticed until the next one or a resume. The JNI path, the
choreographer registration and the frame-rate request run only on a device; the
host tests cover the level lookup (`os_capability_table`), the value
classification (`android_display_table`) and the idle wait
(`idle_wait_follows_the_real_period`), and CI type-checks the rest for
`aarch64-linux-android`.

### A Win32 window opens hidden, in its mode, and is shown by its first frame

**Rule:** `CreateWindowExW` never gets `WS_VISIBLE`. The window is placed first (`SetWindowPlacement`
with `SW_HIDE` stores the rectangle it returns to), put in its opening mode (fullscreen is entered
while hidden), and only then shown, with the command of that mode (`SW_SHOWMAXIMIZED`,
`SW_SHOWMINIMIZED`, else `SW_SHOW`). Its first sight is therefore the final one: no normal window that
jumps to maximized. `WindowReveal::AtOpen` shows it before `open_window` returns;
`AfterFirstFrame` waits for `reveal_after_first_frame`; `visible: false` waits for `show`.

**Why a deferred window answers `visible`:** the runtime gates frames on visibility, and the frame that
earns the reveal must run. A deferred window therefore reports `visible` (and seeds its visibility edge
filter `true`) while it is natively hidden. A hidden window is never sent `WM_PAINT`, so
`request_redraw` on one posts a frame request to the window instead.

**The first show is one slot** (`WindowContext::first_show`), taken by whichever of the reveal, `show`,
`activate` or a mode that shows the window comes first. A mode set on a window still waiting for its
first frame is staged in the slot, not shown early.

**Frame and limits:** `resizable` is `WS_THICKFRAME | WS_MAXIMIZEBOX`, `decorated` is the caption; a
popup has neither, so a window without a frame reports `resizable: false` and gets its wish back with
the frame. `WM_GETMINMAXINFO` turns the app's logical inner-size limits into outer limits in the
window's current DPI with the frame measured for the style the window has
(`shared::windows_window_limits`); a fullscreen window keeps the system's rectangle.

**Fullscreen from another mode:** a visible maximized or minimized window is restored (`SC_RESTORE`)
before the style is replaced, and the style saved to return to has no `WS_MAXIMIZE` or
`WS_MINIMIZE`. A resize does not end fullscreen, and a minimized fullscreen window comes back
fullscreen. `WINDOWPLACEMENT` rectangles are workspace coordinates; a task bar on the top or left edge
moves the work area, so they are converted with the monitor's work-area offset.

**System events** arrive on the hidden top-level receiver (`preferences/receiver.rs`), which a
message-only window could not be: `WM_QUERYENDSESSION` and `WM_ENDSESSION`, `WM_POWERBROADCAST`,
`WM_WTSSESSION_CHANGE`, and a posted message from a pool thread waiting on the low-memory event
(`platforms::windows::host_events`; rules in `shared::windows_host_messages` and
`shared::windows_memory_watch`). A session-end query the host answers with a block is refused and
registered as a shutdown reason on the first visible window of the thread (the receiver otherwise);
`Platform::release_session_end` withdraws it. The host's answer carries no reason text, so the system is given
a fixed neutral one. A critical session end is never blocked, and a panicking hook answers
`Proceed`. The low-memory event is a level, not an edge: the watch reports once, stays deaf for
`HOLD_INTERVAL`, and only then waits again. `WTSRegisterSessionNotification` asks for this session only.

**Verified:** the pure rules by host tests; the window and the receiver against the real system by
`windows_window_commands` and `windows_host_events` (messages sent to the real windows). A real
log-off, sleep, lock or low-memory state, and the first frame seen by eye, have not run.

**Modes on a window that is not shown:** `Normal` keeps a hidden window hidden and decides how it is
first shown; `Minimized` and `Maximized` show it, as `ShowWindow` does; only a window waiting for its
first frame stages a mode without showing it. Size limits also hold a window without a sizing border
(`WM_WINDOWPOSCHANGING`), since the system asks for them only while sizing one that has it. A change
that takes several steps (entering fullscreen, changing the frame) publishes one snapshot.
`WTSRegisterSessionNotification` is asked once at startup; if it fails (an app started before
Remote Desktop Services at logon), lock and unlock stay unreported for the process.

### Android reads the Activity's lifecycle as three facts and a cycle

**Rule:** an Activity callback is mapped by one pure rule
(`shared/android_lifecycle.rs`) and delivered through `ActivityTarget`, which
`AndroidPlatform` implements; nothing in the event loop decides what a callback
means. The window's published state is refreshed before its observer is told.

**Choice:** `onStart`/`onStop` give visibility, `onResume`/`onPause` the execution
state, `onWindowFocusChanged` focus; none implies another. NativeActivity has no
`onTrimMemory`, so `lastTrimLevel` is read through JNI (`android/memory.rs`) on
`LowMemory`, `Pause`, `Stop` and `Resume`, never on a timer, and a level is
remembered when it is read: notices are not replayed, so a level seen before the
host listens is not announced later. The session ends once per start-to-stop
cycle, by whichever of `Stop` and `SaveState` comes first (the order differs
below and above API 28; a Back that finishes the Activity never saves state),
so the notice repeats on every trip to the background and asks for saving. The
session-end hook is out of its slot while it runs and is put back even when it
panics (`PlatformHandlers::run_session_end`).

**Verified by:** `android_lifecycle_mapping*` and `session_end_hook_*` on the
host; the `MainEvent` match and the JNI read run only on a device.
