//! Executable macOS text-input coverage: does one `keyDown:` reach the
//! application exactly once, and does the `NSTextInputClient` conformance
//! answer AppKit's own queries?
//!
//! `FLUIContentView` conforms to `NSTextInputClient`, and `keyDown:` routes a
//! key press either into AppKit's input context (`interpretKeyEvents:`) or
//! through the keyboard conversion, never both — that mutual exclusion is
//! ADR-0069. This probe is its executable evidence: it runs the real backend
//! through the production launch path (`MacOSPlatform::new` → `Platform::run`
//! → a visible window → the real AppKit run loop), reaches the window's content
//! view through AppKit itself, and drives it the way an input method would.
//!
//! # What it asserts
//!
//! * **A — mutual exclusion, the assertion this probe exists for.** With
//!   `set_ime_allowed(true)`, a synthesized `keyDown:` for one letter must
//!   reach the application as exactly one `ImeEvent::Commit` of that letter and
//!   as **zero** `Key::Character`. With `set_ime_allowed(false)` the same press
//!   must produce the exact inverse: exactly one key event, zero `ImeEvent`.
//!   **Both halves are required** — an implementation that double-produced
//!   would pass the first half alone, which is the defect the pair catches. The
//!   second half also covers the disable's own contract: an in-progress
//!   composition is *dropped*, not committed, so the disable must not announce
//!   a `Commit`.
//! * **B — protocol conformance.** `setMarkedText:selectedRange:
//!   replacementRange:` must dispatch one `Preedit` whose `cursor` is a *byte*
//!   range — AppKit speaks UTF-16 and [`flui_types::ImeEvent`] speaks bytes, and
//!   the composition below is multi-byte, so a UTF-16 offset reaching the wire
//!   unchanged would be visible here. `hasMarkedText` and `markedRange` must
//!   answer while it composes, and `insertText:replacementRange:` must dispatch
//!   exactly one `Commit` and leave `hasMarkedText` answering NO.
//! * **C — cursor area.** `set_ime_cursor_area` must be answered by
//!   `firstRectForCharacterRange:actualRange:` with a non-zero rect — the
//!   candidate-window placement an input method asks for.
//! * **D — `unmarkText` announces the end of composition.** It must dispatch
//!   exactly one `ImeEvent::Preedit { text: "", cursor: None }` and leave
//!   `hasMarkedText` false: a client left holding composition state it was never
//!   told ended suppresses `Key::Character` for the rest of the focus session.
//! * **E — the release, and the gate that decides whether it is sent.**
//!   `keyUp:` is a separate AppKit entry point with its own arm (one every
//!   other assertion here leaves untouched, since all of them send `keyDown:`),
//!   and it is gated on the *composition* rather than on whether a text input is
//!   attached: with a text input attached and nothing composing, a synthesized
//!   `keyUp:` must reach the application as exactly one **release**
//!   (`KeyState::Up`) of that letter; with a composition open, the same call
//!   must produce nothing at all. The two halves are measured in that order on
//!   purpose — the reporting half proves the channel on this very window
//!   immediately before the suppressing half requires it to go quiet.
//! * **F — a *modified* key routes through the input method as well.** Every
//!   other assertion presses either a plain letter or nothing at all; this one
//!   presses Option-`E`, whose `characters` is a non-ASCII symbol, and then `e`.
//!   Both presses must arrive the same way — as input-method events, with the
//!   keyboard path silent — and no composition may be left open afterwards. The
//!   characters themselves are **reported, not required**, and F is deliberately
//!   *not* evidence that a composition ran: see below for the measurement that
//!   settled that, and why requiring the layout's resolved `é` here would encode
//!   a property of the host as a contract of this backend.
//!
//! # What it does NOT establish
//!
//! The key events here are synthesized with `+[NSEvent keyEventWithType:...]`
//! and delivered by messaging `keyDown:` directly. They are **not** human
//! keystrokes.
//!
//! **A real input method still does not drive a composition here, and F does not
//! claim one.** F was written to close that gap by letting the layout's own
//! dead-key table compose, and measurement refuted the premise: handed a
//! synthesized Option-`E`, the view receives an `insertText:` for each press and
//! never enters a composition. The layout is not what is missing —
//! `UCKeyTranslate` over the enabled layouts' tables shows
//! `com.apple.keylayout.US` defining Option-`E` as a dead key
//! (`deadKeyState = 1`) that resolves with `e` to `é` — and neither is a Latin
//! accent the operative cause, since the input *context* is what declines to
//! engage it. A CJK or other converting input method is not available to try
//! instead: this host has no input source of type `kTISTypeInputMethod` enabled,
//! only the raw keylayouts `com.apple.keylayout.US` and
//! `com.apple.keylayout.Russian`. The slice's plan carries the run record and
//! the standalone probes behind each measurement; enabling an input method is a
//! System Settings change, not something this probe may make for its user.
//!
//! macOS-only by construction: besides the main-thread floor (libtest runs
//! `#[test]` bodies on a worker thread, and AppKit requires main-thread window
//! construction), unbundled NSWindow construction throws
//! `_CFBundleGetValueForInfoKey`, a foreign NSException Rust cannot catch. Run
//! it on a real Mac via `just macos-ime`, which stages this example into a
//! minimal `.app` (the committed `Info.plist.ime_probe` clears the bundle
//! floor), launches it with `RUST_LOG=info`, and asserts exit 0 plus the PASS
//! marker. On every other target the binary is a compile-time no-op main.

// This probe is an AppKit FFI island: reaching the content view and playing
// input method *is* messaging live AppKit objects. Workspace `unsafe_code` is
// `warn` and CI lints with `-D warnings`, so the allow is scoped here the way
// each backend module scopes its own.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
mod appkit_ime_probe {
    // `cocoa` 0.27's whole surface is deprecated in favour of `objc2`, which
    // this workspace has not migrated to. The backend modules this probe talks
    // to carry the same expectation (`platforms/macos/mod.rs`), and the probe
    // is written against the same bindings rather than a second set.
    #![expect(deprecated)]

    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use flui_platform::traits::{Key, PlatformInput};
    use flui_platform::{
        DispatchEventResult, Platform, PlatformTextInput, PlatformWindow, WindowOptions,
    };
    use flui_types::ImeEvent;
    use flui_types::geometry::{Bounds, Point, Size, px};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{ClassType, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSNotFound, NSPoint, NSRange, NSRect, NSString};
    // Named from the dependency, not from a re-export: `KeyboardEvent` is
    // re-exported by `flui_platform::traits` but its `state` field's type is
    // not, so `KeyState` is unnameable through this crate's public surface even
    // though the field reading it is public. Assertion E judges an event on
    // that field, so it names the type at its source.
    use keyboard_types::KeyState;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    /// `id`/`nil`/`YES`/`NO` spelled as this probe's raw `msg_send!` shape
    /// expects. `NSWindow` is `MainThreadOnly` in objc2's typed API, and this
    /// probe messages windows from a main-queue block inside a bundled `.app`,
    /// so it stays on the raw macro (which objc2 accepts a raw pointer for).
    type ObjcId = *mut AnyObject;
    const NIL: ObjcId = std::ptr::null_mut();
    type BObjC = Bool;
    const YES: Bool = Bool::YES;
    const NO: Bool = Bool::NO;

    /// The window title the probe opens and then finds its window by.
    const TITLE: &str = "FLUI IME probe";

    /// `NSEventTypeKeyDown` (AppKit's `NSEvent.h`).
    const NS_EVENT_TYPE_KEY_DOWN: isize = 10;
    /// `NSEventTypeKeyUp`. The release assertion E drives.
    const NS_EVENT_TYPE_KEY_UP: isize = 11;

    /// The letter assertion A presses. A plain ASCII letter is the key whose
    /// route the active input method decides, so it is the key the mutual
    /// exclusion has to be judged on.
    const LETTER: &str = "a";

    /// The composition assertion B drives. Multi-byte on purpose: it is one
    /// UTF-16 unit and several bytes, so the UTF-16 → byte conversion the
    /// `ImeEvent` vocabulary requires is exercised on the wire rather than only
    /// in its unit tests.
    const COMPOSITION: &str = "に";

    /// `NSEventModifierFlagOption` (AppKit's `NSEvent.h`, `1 << 19`).
    const NS_MODIFIER_FLAG_OPTION: usize = 1 << 19;

    /// `kVK_ANSI_E` (`Events.h`) — the key both halves of assertion F press.
    ///
    /// The key *code*, not the character, is what a layout's tables are indexed
    /// by, so a synthesized Option-`E` that carried the right character but a
    /// different code would describe a key this input source has no row for.
    const KEY_CODE_E: u16 = 14;

    /// The acute accent — U+00B4, what a U.S.-layout Option-`E` press carries.
    ///
    /// Under `com.apple.keylayout.US` this is the *preview* of a dead key:
    /// reading that layout's own tables through `UCKeyTranslate` resolves
    /// Option-`E` to an empty string with `deadKeyState = 1`, and the `e` that
    /// follows it to `é`. The character is non-ASCII, which is the property
    /// assertion F routes on — a modifier-flagged press whose character is not a
    /// plain letter still belongs to the input method, not the keyboard path.
    const DEAD_ACUTE: &str = "´";

    /// How long the probe waits for the app to become active with a key window
    /// before it drives the assertions anyway (and reports that it did).
    const READY_DEADLINE: Duration = Duration::from_secs(10);
    /// How often the readiness gate re-checks.
    const READY_POLL: Duration = Duration::from_millis(100);
    /// How long a drive step waits for the event it produced.
    ///
    /// The backend dispatches input inline on the thread that produced it, so
    /// this is expected to hold on the first check. It exists so a delivery
    /// that is queued behind a reentrant platform callback is given a real
    /// window rather than assumed away — and a timeout is a reported failure
    /// with the events that did arrive, never a flake.
    const DELIVERY_DEADLINE: Duration = Duration::from_millis(500);
    /// How often a drive step re-checks for its event.
    const DELIVERY_POLL: Duration = Duration::from_millis(5);

    /// Report a fatal setup failure and exit non-zero.
    fn fatal(message: String) -> ! {
        tracing::error!("IME_PROBE_FAILURE={message}");
        tracing::error!("IME_PROBE_RESULT=FAIL");
        std::process::exit(1);
    }

    pub(crate) fn run() {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer())
            .init();

        // The production launch path rather than a hand-rolled loop: `run`
        // activates the app before it starts the event loop, and an app that is
        // never activated gets no input routing at all — which would make
        // assertion A fail for a reason that has nothing to do with the
        // routing it measures.
        let platform = flui_platform::MacOSPlatform::new()
            .expect("MacOSPlatform::new must succeed on the AppKit main thread");
        if platform.name() != "macOS (AppKit)" {
            fatal(format!(
                "expected platform macOS (AppKit), got {}",
                platform.name()
            ));
        }
        tracing::info!("platform: {}", platform.name());

        Box::new(platform)
            .run(Box::new(|owner| {
                setup_and_run(owner);
                Ok(())
            }))
            .expect("Platform::run must not return an error");
    }

    /// The setup half, run from `on_finish_launching` — after the app exists
    /// and immediately before `NSApplication::run`.
    fn setup_and_run(owner: flui_platform::OwnerPlatform) {
        // A VISIBLE window, ordered front: AppKit routes a key event into the
        // input context of a key window, and a window that is never ordered
        // front never becomes one.
        let window = match owner.open_window(WindowOptions {
            title: TITLE.to_string(),
            size: Size::new(px(480.0), px(320.0)),
            resizable: false,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
            ..Default::default()
        }) {
            Ok(pending) => match pending.try_ready() {
                Ok(window) => window,
                Err(error) => fatal(format!("the window was not ready: {error:?}")),
            },
            Err(error) => fatal(format!("open_window was refused: {error:?}")),
        };
        window.activate();

        // `None` here is a failure worth reporting on its own, not a setup
        // detail: without the capability composition can never be enabled, so
        // every key press would take the keyboard route and assertion A could
        // not be driven at all.
        let Some(text_input) = window.text_input() else {
            fatal(
                "PlatformWindow::text_input() returned None on the macOS backend. Its \
                 `MacOSTextInput` capability is what enables composition, so with no capability \
                 every key press takes the keyboard route and the input-method route ADR-0069 \
                 decides is unreachable."
                    .to_string(),
            );
        };
        tracing::info!("obligation: text_input() returned Some");

        // The assertions cannot run from here: `NSApplication::run` starts when
        // this callback returns, and until it does the app is not active — so
        // the AppKit input machinery this probe needs is not live yet. Hand the
        // drive to the main queue and let it wait for the state a real key
        // press arrives in.
        let deadline = Instant::now() + READY_DEADLINE;
        dispatch::Queue::main().exec_after(READY_POLL, move || {
            poll_until_ready(TITLE.to_string(), window, text_input, deadline);
        });
    }

    /// Re-check the readiness gate on the main queue and drive once it holds
    /// (or once the deadline passes, which the drive reports on).
    fn poll_until_ready(
        title: String,
        window: Arc<dyn PlatformWindow>,
        text_input: Arc<dyn PlatformTextInput>,
        deadline: Instant,
    ) {
        if window_is_ready(&title) || Instant::now() >= deadline {
            // Diverges: the process exits from here, so there is nothing left
            // to re-arm.
            drive(&title, &window, &text_input);
        }
        dispatch::Queue::main().exec_after(READY_POLL, move || {
            poll_until_ready(title, window, text_input, deadline);
        });
    }

    /// Whether the app is active and the probe's window is key — the state a
    /// real key press reaches a text input in.
    fn window_is_ready(title: &str) -> bool {
        // SAFETY: this runs on the main queue; `sharedApplication` is the live
        // NSApplication singleton and both flags are AppKit's own getters on
        // live objects.
        unsafe {
            let Some(window) = window_with_title(title) else {
                return false;
            };
            let app: ObjcId = msg_send![NSApplication::class(), sharedApplication];
            let active: BObjC = msg_send![app, isActive];
            let key: BObjC = msg_send![window, isKeyWindow];
            active == YES && key == YES
        }
    }

    /// The probe's `NSWindow*`, found in `[NSApp windows]` by title.
    ///
    /// By title rather than by index: nothing promises this is the only window
    /// the platform has open, and "the first one" is an assumption a probe
    /// should not bake in.
    fn window_with_title(title: &str) -> Option<ObjcId> {
        // SAFETY: main thread; `sharedApplication`/`windows` are AppKit's own
        // accessors, every object messaged below comes from that array, and the
        // comparison string is copied into a fresh NSString that outlives the
        // loop.
        unsafe {
            let app: ObjcId = msg_send![NSApplication::class(), sharedApplication];
            if app == NIL {
                return None;
            }
            let windows: ObjcId = msg_send![app, windows];
            if windows == NIL {
                return None;
            }
            let expected = ns_string(title);
            let count: usize = msg_send![windows, count];
            for index in 0..count {
                let window: ObjcId = msg_send![windows, objectAtIndex: index];
                if window == NIL {
                    continue;
                }
                let window_title: ObjcId = msg_send![window, title];
                let matches: BObjC = msg_send![window_title, isEqualToString: expected];
                if matches == YES {
                    return Some(window);
                }
            }
            None
        }
    }

    /// An `NSString` holding `text`.
    ///
    /// `alloc`/`init_str` returns a `+1` object the probe never releases: it
    /// exits through `std::process::exit` and never drains an autorelease pool,
    /// so a release here would buy nothing.
    ///
    /// # Safety
    ///
    /// Main thread only, like every other AppKit call in this file.
    fn ns_string(text: &str) -> ObjcId {
        // `NSString::from_str` allocates and inits a live string object. The
        // returned `Retained` is deliberately leaked (`ManuallyDrop`) because
        // the probe never drains an autorelease pool, so a release would buy
        // nothing; the raw pointer is what the raw `msg_send!` shape wants.
        let string = NSString::from_str(text);
        let ptr = objc2::rc::Retained::as_ptr(&string) as ObjcId;
        std::mem::forget(string);
        ptr
    }

    /// The pair AppKit uses to say "no range": `{NSNotFound, 0}`.
    fn not_found_range() -> NSRange {
        NSRange::new(NSNotFound as usize, 0)
    }

    /// Send the content view one synthesized `keyDown:` for [`LETTER`].
    ///
    /// `+[NSEvent keyEventWithType:...]` and a direct `keyDown:` message: this
    /// is the probe standing in for the platform's event delivery, not a human
    /// keystroke — see this file's module doc for what that does and does not
    /// establish.
    fn send_letter_key_down(window_number: isize, content_view: ObjcId) {
        send_letter_key(window_number, content_view, true);
    }

    /// Send the content view one synthesized `keyUp:` for [`LETTER`].
    ///
    /// The release half of the same synthesized press. It takes the keyboard
    /// path even while a text input is attached — a commit returns the input
    /// context to ground before the release arrives — except while a
    /// composition is still open, which is the gate assertion E measures.
    fn send_letter_key_up(window_number: isize, content_view: ObjcId) {
        send_letter_key(window_number, content_view, false);
    }

    /// `+[NSEvent keyEventWithType:...]` for [`LETTER`], sent to `content_view`.
    ///
    /// SAFETY: main thread; `keyEventWithType:...` is a class constructor on a
    /// class AppKit always provides (its result is nil-checked below), and the
    /// `keyDown:`/`keyUp:` message goes to the live content view AppKit handed
    /// us. The event is autoreleased and retained for the duration of the send.
    fn send_letter_key(window_number: isize, content_view: ObjcId, is_down: bool) {
        send_key(
            window_number,
            content_view,
            KeyStroke {
                characters: LETTER,
                characters_ignoring_modifiers: LETTER,
                modifier_flags: 0,
                key_code: 0,
            },
            is_down,
        );
    }

    /// One synthesized keystroke's event fields.
    ///
    /// Split out for assertion F, which is the only place in this probe that
    /// needs a *real* event shape rather than a minimal one: a press carrying a
    /// modifier is defined by modifiers, a character and a key code agreeing,
    /// and a layout's tables are indexed by the code rather than the character.
    struct KeyStroke {
        /// What `characters` carries — for a dead key, the accent the layout
        /// produces for that modifier combination.
        characters: &'static str,
        /// What `charactersIgnoringModifiers` carries — the unmodified key.
        characters_ignoring_modifiers: &'static str,
        /// `NSEventModifierFlag*` bits. Option is what makes it a dead key.
        modifier_flags: usize,
        /// The `kVK_*` code the input source's own tables are indexed by.
        key_code: u16,
    }

    /// `+[NSEvent keyEventWithType:...]` for `stroke`, sent to `content_view`.
    ///
    /// SAFETY: main thread; `keyEventWithType:...` is a class constructor on a
    /// class AppKit always provides (its result is nil-checked below), and the
    /// `keyDown:`/`keyUp:` message goes to the live content view AppKit handed
    /// us. The event is autoreleased and retained for the duration of the send.
    fn send_key(window_number: isize, content_view: ObjcId, stroke: KeyStroke, is_down: bool) {
        // SAFETY: as this function's own note above.
        unsafe {
            let characters = ns_string(stroke.characters);
            let event_type = if is_down {
                NS_EVENT_TYPE_KEY_DOWN
            } else {
                NS_EVENT_TYPE_KEY_UP
            };
            let event: ObjcId = msg_send![AnyClass::get(c"NSEvent").expect("BUG: NSEvent is always registered"),
                keyEventWithType: event_type
                location: NSPoint::new(10.0, 10.0)
                modifierFlags: stroke.modifier_flags
                timestamp: 0.0f64
                windowNumber: window_number
                context: NIL
                characters: characters
                charactersIgnoringModifiers: ns_string(stroke.characters_ignoring_modifiers)
                isARepeat: NO
                keyCode: stroke.key_code
            ];
            if event == NIL {
                fatal(format!(
                    "+[NSEvent keyEventWithType:...] returned NIL for the synthesized key \
                     {direction}, so the press could not be delivered and the assertion that \
                     depends on it cannot be driven",
                    direction = if is_down { "press" } else { "release" },
                ));
            }
            if is_down {
                let _: () = msg_send![content_view, keyDown: event];
            } else {
                let _: () = msg_send![content_view, keyUp: event];
            }
        }
    }

    /// Send the content view an Option-`E` press carrying [`DEAD_ACUTE`], then a
    /// plain `e` — the two-press sequence a U.S. layout defines as a dead key
    /// resolving to `é`.
    ///
    /// Nothing here synthesizes a composition: both presses are ordinary key
    /// events, and whether AppKit's input context composes them is what assertion
    /// F reports rather than requires. A `setMarkedText:` would be this probe
    /// standing in for the input method again, which is the one thing this
    /// sequence exists to stop doing.
    fn send_dead_key_sequence(window_number: isize, content_view: ObjcId) {
        send_key(
            window_number,
            content_view,
            KeyStroke {
                characters: DEAD_ACUTE,
                characters_ignoring_modifiers: "e",
                modifier_flags: NS_MODIFIER_FLAG_OPTION,
                key_code: KEY_CODE_E,
            },
            true,
        );
        send_key(
            window_number,
            content_view,
            KeyStroke {
                characters: "e",
                characters_ignoring_modifiers: "e",
                modifier_flags: 0,
                key_code: KEY_CODE_E,
            },
            true,
        );
    }

    /// `setMarkedText:selectedRange:replacementRange:` — the composition
    /// update an input method sends while composing.
    fn set_marked_text(content_view: ObjcId, text: &str, selected: NSRange) {
        // SAFETY: main thread; `content_view` is the live content view and
        // `text` is copied into a fresh NSString before the message is sent.
        unsafe {
            let _: () = msg_send![content_view,
                setMarkedText: ns_string(text)
                selectedRange: selected
                replacementRange: not_found_range()
            ];
        }
    }

    /// `insertText:replacementRange:` — the commit an input method sends.
    fn insert_text(content_view: ObjcId, text: &str) {
        // SAFETY: as `set_marked_text`.
        unsafe {
            let _: () = msg_send![content_view,
                insertText: ns_string(text)
                replacementRange: not_found_range()
            ];
        }
    }

    /// `unmarkText` — the input method abandoning a composition.
    fn unmark_text(content_view: ObjcId) {
        // SAFETY: main thread; `unmarkText` is an `NSTextInputClient` message
        // sent to the live content view.
        unsafe {
            let _: () = msg_send![content_view, unmarkText];
        }
    }

    /// The probe's view of what the window dispatched.
    ///
    /// `on_input` registration replaces any previous callback, so installing a
    /// fresh collector gives each drive step an observation window containing
    /// only its own events.
    struct Observed {
        events: Arc<Mutex<Vec<PlatformInput>>>,
    }

    impl Observed {
        /// Register a fresh collector on `window` and return it.
        fn install(window: &Arc<dyn PlatformWindow>) -> Self {
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&events);
            window.on_input(Box::new(move |input| {
                sink.lock()
                    .expect("BUG: the collector mutex is only ever held for a push")
                    .push(input);
                DispatchEventResult::DEFERRED
            }));
            Self { events }
        }

        /// Whether `predicate` holds over everything collected so far.
        fn holds(&self, predicate: impl Fn(&[PlatformInput]) -> bool) -> bool {
            predicate(
                &self
                    .events
                    .lock()
                    .expect("BUG: the collector mutex is only ever held for a push"),
            )
        }

        /// Wait for `predicate` to hold.
        ///
        /// The backend dispatches input inline on the thread that produced it,
        /// so the first check is expected to succeed. The bounded wait is here
        /// because a delivery queued behind a reentrant platform callback would
        /// land a moment later, and that deserves a real window rather than
        /// being assumed away.
        fn wait_for(&self, predicate: impl Fn(&[PlatformInput]) -> bool) -> bool {
            let deadline = Instant::now() + DELIVERY_DEADLINE;
            while !self.holds(&predicate) {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(DELIVERY_POLL);
            }
            true
        }

        /// Everything collected so far, and nothing afterwards.
        fn take(&self) -> Vec<PlatformInput> {
            std::mem::take(
                &mut *self
                    .events
                    .lock()
                    .expect("BUG: the collector mutex is only ever held for a push"),
            )
        }

        /// Wait until nothing further arrives for one poll interval.
        ///
        /// For a drive whose event *count* is a property of the host rather than
        /// of the backend: assertion F's two presses commit separately on one
        /// host and compose into a single commit on another, so waiting for a
        /// fixed count would either time out or stop before the sequence ended.
        /// Waiting for the stream to go quiet reports whatever the host actually
        /// produced. Bounded by [`DELIVERY_DEADLINE`] and infallible — the events
        /// collected are the report either way.
        fn settle(&self) {
            let deadline = Instant::now() + DELIVERY_DEADLINE;
            loop {
                let before = self.collected();
                if Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(DELIVERY_POLL);
                if self.collected() == before {
                    return;
                }
            }
        }

        /// How many events have been collected so far.
        fn collected(&self) -> usize {
            self.events
                .lock()
                .expect("BUG: the collector mutex is only ever held for a push")
                .len()
        }
    }

    /// Every `ImeEvent` in `events`, in delivery order.
    fn ime_events(events: &[PlatformInput]) -> Vec<ImeEvent> {
        events
            .iter()
            .filter_map(|input| input.as_ime().cloned())
            .collect()
    }

    /// The committed strings in `events`, in delivery order.
    fn commits(events: &[PlatformInput]) -> Vec<String> {
        ime_events(events)
            .into_iter()
            .filter_map(|event| match event {
                ImeEvent::Commit(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    /// The compositions in `events`, in delivery order.
    fn preedits(events: &[PlatformInput]) -> Vec<(String, Option<(usize, usize)>)> {
        ime_events(events)
            .into_iter()
            .filter_map(|event| match event {
                ImeEvent::Preedit { text, cursor } => Some((text, cursor)),
                _ => None,
            })
            .collect()
    }

    /// The `Key::Character` payloads in `events`, in delivery order — what the
    /// keyboard route produces for a printable key.
    fn typed_characters(events: &[PlatformInput]) -> Vec<String> {
        events
            .iter()
            .filter_map(|input| input.as_keyboard())
            .filter_map(|event| match &event.key {
                Key::Character(characters) => Some(characters.clone()),
                Key::Named(_) => None,
            })
            .collect()
    }

    /// The `Key::Character` payloads of the **releases** in `events`, in
    /// delivery order.
    ///
    /// The release half of [`typed_characters`], and separate from it on
    /// purpose: the claim assertion E measures is that a *release* is routed,
    /// and a payload-only reading cannot tell a release from a press — an
    /// implementation that answered `keyUp:` with the `keyDown:` path would
    /// look identical to a correct one under [`typed_characters`].
    fn released_characters(events: &[PlatformInput]) -> Vec<String> {
        events
            .iter()
            .filter_map(|input| input.as_keyboard())
            .filter_map(|event| match (event.state, &event.key) {
                (KeyState::Up, Key::Character(characters)) => Some(characters.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `Preedit` a `setMarkedText:` of `text` with the whole string
    /// selected must dispatch: the text, and the caret's byte range.
    fn expected_marked_preedit(text: &str) -> (String, Option<(usize, usize)>) {
        (text.to_string(), Some((0, text.len())))
    }

    /// Drive the four assertions on the main thread and report the result.
    fn drive(
        title: &str,
        window: &Arc<dyn PlatformWindow>,
        text_input: &Arc<dyn PlatformTextInput>,
    ) -> ! {
        let mut failures: Vec<String> = Vec::new();

        if !window_is_ready(title) {
            failures.push(format!(
                "assertion A precondition: the app is not active with a key window after \
                 {READY_DEADLINE:?}. Assertion A was driven anyway — its result below is the \
                 routing of an unfocused window. B/C/D do not depend on it."
            ));
        }

        let Some(ns_window) = window_with_title(title) else {
            fatal(format!("no window titled {title:?} in [NSApp windows]"));
        };
        // SAFETY: main thread; `contentView` and `windowNumber` are AppKit's
        // own getters on the live window.
        let (content_view, window_number): (ObjcId, isize) = unsafe {
            (
                msg_send![ns_window, contentView],
                msg_send![ns_window, windowNumber],
            )
        };
        if content_view == NIL {
            fatal(format!("the window titled {title:?} has no content view"));
        }

        // Focus the view, the way an application that attached a text input and
        // focused it does: the probe is playing that application.
        // SAFETY: main thread; `makeFirstResponder:` on the live window with
        // AppKit's own content view.
        let became_responder: BObjC =
            unsafe { msg_send![ns_window, makeFirstResponder: content_view] };
        if became_responder != YES {
            failures.push(
                "the content view refused to become first responder, so in a real session no \
                 key event would reach it"
                    .to_string(),
            );
        }

        // AppKit answers `-[NSView inputContext]` with nil unless the receiver
        // conforms to `NSTextInputClient` — so this one query says whether the
        // conformance is registered at all, and it is worth reporting on its
        // own rather than letting assertion A fail without a reason.
        // SAFETY: main thread; `inputContext` is AppKit's own getter on the
        // live content view.
        let input_context: ObjcId = unsafe { msg_send![content_view, inputContext] };
        if input_context == NIL {
            failures.push(
                "the content view has no NSTextInputContext: `-[NSView inputContext]` answers \
                 NIL unless the receiver conforms to `NSTextInputClient`, so no key event can \
                 reach the input method at all and assertion A cannot be driven"
                    .to_string(),
            );
        } else {
            tracing::info!("obligation: the content view has a live NSTextInputContext");
        }

        // ---- A: mutual exclusion -------------------------------------------
        let observed = Observed::install(window);

        text_input.set_ime_allowed(true);
        tracing::info!(
            events = observed.take().len(),
            "set_ime_allowed(true) announced"
        );

        send_letter_key_down(window_number, content_view);
        let commit_arrived = observed.wait_for(|events| !commits(events).is_empty());
        let attached = observed.take();
        let attached_commits = commits(&attached);
        let attached_typed = typed_characters(&attached);
        let attached_ime = ime_events(&attached);

        if !commit_arrived {
            failures.push(format!(
                "assertion A (text input attached): no ImeEvent::Commit arrived within \
                 {DELIVERY_DEADLINE:?} of a synthesized `{LETTER}` keyDown — the press reached \
                 neither the input context nor the keyboard path, or the input context declined \
                 it. Collected: {attached:?}"
            ));
        }
        if attached_commits != vec![LETTER.to_string()] {
            failures.push(format!(
                "assertion A (text input attached): expected exactly one Commit(\"{LETTER}\"), \
                 got {attached_commits:?} — one key press must reach the application as one \
                 semantic event (ADR-0069)"
            ));
        }
        if !attached_typed.is_empty() {
            failures.push(format!(
                "assertion A (text input attached): the same press ALSO produced \
                 {attached_typed:?} on the keyboard path. One physical key reached the \
                 application twice — the defect ADR-0069 exists to forbid."
            ));
        }
        if attached.len() != 1 {
            // Not a failure on its own: an *input source that composes* marks
            // first and commits second, which is still one press reaching the
            // application once. It is reported because it changes what this run
            // measured, and a duplicate Commit or a stray key event is already
            // caught by the two checks above.
            tracing::warn!(
                events = ?attached,
                "assertion A (text input attached): more than one event for the press — a \
                 composing input source explains a Preedit before the Commit; anything else is \
                 worth reading in full"
            );
        }
        tracing::info!(
            commits = ?attached_commits,
            typed = ?attached_typed,
            ime = ?attached_ime,
            "assertion A (text input attached) measured"
        );

        // The disable's own contract first: the trait documents that
        // `set_ime_allowed(false)` DROPS an in-progress composition rather than
        // committing it, so a Commit announced by the disable would insert text
        // the user never confirmed. Give it something to drop.
        set_marked_text(content_view, LETTER, NSRange::new(0, LETTER.len()));
        let composed_before_disable = observed.wait_for(|events| !preedits(events).is_empty());
        if !composed_before_disable {
            failures.push(format!(
                "assertion A (disable) precondition: no Preedit arrived within \
                 {DELIVERY_DEADLINE:?} for the composition the disable was supposed to drop, so \
                 the drop-vs-commit contract went unexercised"
            ));
        }
        let composing_before_disable = observed.take();

        text_input.set_ime_allowed(false);
        let on_disable = observed.take();
        if !commits(&on_disable).is_empty() {
            failures.push(format!(
                "assertion A (disable): set_ime_allowed(false) committed the in-progress \
                 composition it was holding: {on_disable:?}. The documented contract is to drop \
                 it, not to insert text the user never confirmed."
            ));
        }
        if !ime_events(&on_disable).contains(&ImeEvent::Disabled) {
            failures.push(format!(
                "assertion A (disable): set_ime_allowed(false) did not announce \
                 ImeEvent::Disabled; collected {on_disable:?}"
            ));
        }
        tracing::info!(
            dropped = ?composing_before_disable,
            on_disable = ?on_disable,
            "assertion A (disable) measured"
        );

        send_letter_key_down(window_number, content_view);
        let key_arrived = observed.wait_for(|events| !events.is_empty());
        let detached = observed.take();
        let detached_typed = typed_characters(&detached);
        let detached_ime = ime_events(&detached);

        if !key_arrived {
            failures.push(format!(
                "assertion A (no text input attached): no input event arrived within \
                 {DELIVERY_DEADLINE:?} of a synthesized `{LETTER}` keyDown"
            ));
        }
        if detached_typed != vec![LETTER.to_string()] {
            failures.push(format!(
                "assertion A (no text input attached): expected exactly one key event carrying \
                 `{LETTER}`, got {detached_typed:?} — with no text input attached the press must \
                 take the keyboard route, byte for byte as it did before the conformance existed"
            ));
        }
        if !detached_ime.is_empty() {
            failures.push(format!(
                "assertion A (no text input attached): the press ALSO produced \
                 {detached_ime:?} through the input context. The two routes are mutually \
                 exclusive, and which one a press takes depends only on whether a text input is \
                 attached (ADR-0069)."
            ));
        }
        if detached.len() != 1 {
            failures.push(format!(
                "assertion A (no text input attached): expected exactly one input event for the \
                 press, got {}: {detached:?}",
                detached.len()
            ));
        }
        tracing::info!(
            typed = ?detached_typed,
            ime = ?detached_ime,
            "assertion A (no text input attached) measured"
        );

        // ---- B: protocol conformance ---------------------------------------
        text_input.set_ime_allowed(true);
        let _ = observed.take();

        let utf16_len = COMPOSITION.encode_utf16().count();
        let marked = NSRange::new(0, utf16_len);
        set_marked_text(content_view, COMPOSITION, marked);
        let preedit_arrived = observed.wait_for(|events| !preedits(events).is_empty());
        // SAFETY: main thread; both are `NSTextInputClient` queries AppKit
        // answers on the live content view.
        let (is_marked, reported_range): (BObjC, NSRange) = unsafe {
            (
                msg_send![content_view, hasMarkedText],
                msg_send![content_view, markedRange],
            )
        };
        let composing = observed.take();
        let expected_preedit = expected_marked_preedit(COMPOSITION);

        if !preedit_arrived {
            failures.push(format!(
                "assertion B: setMarkedText: dispatched no Preedit within {DELIVERY_DEADLINE:?}; \
                 collected {composing:?}"
            ));
        }
        if preedits(&composing) != vec![expected_preedit.clone()] {
            failures.push(format!(
                "assertion B: setMarkedText: of {COMPOSITION:?} with its whole UTF-16 range \
                 selected must dispatch exactly one Preedit {expected_preedit:?} — the cursor is \
                 a BYTE range, and {COMPOSITION:?} is {utf16_len} UTF-16 unit(s) in {} byte(s), \
                 so a UTF-16 offset reaching the wire unchanged would show up right here; got \
                 {:?}",
                COMPOSITION.len(),
                preedits(&composing)
            ));
        }
        if is_marked != YES {
            failures.push(
                "assertion B: hasMarkedText answered NO while a composition was in progress; \
                 AppKit queries this to decide whether the composition is live"
                    .to_string(),
            );
        }
        if (reported_range.location, reported_range.length) != (0, utf16_len) {
            failures.push(format!(
                "assertion B: markedRange must report the whole composition in UTF-16 units — \
                 expected (0, {utf16_len}), got ({}, {})",
                reported_range.location, reported_range.length
            ));
        }
        tracing::info!(
            preedits = ?preedits(&composing),
            has_marked_text = bool::from(is_marked),
            marked_range = ?(reported_range.location, reported_range.length),
            "assertion B (composition) measured"
        );

        insert_text(content_view, COMPOSITION);
        let commit_of_composition = observed.wait_for(|events| !commits(events).is_empty());
        let committed = observed.take();
        // SAFETY: main thread; `NSTextInputClient` query on the live view.
        let still_marked: BObjC = unsafe { msg_send![content_view, hasMarkedText] };

        if !commit_of_composition {
            failures.push(format!(
                "assertion B: insertText:replacementRange: dispatched no Commit within \
                 {DELIVERY_DEADLINE:?}; collected {committed:?}"
            ));
        }
        if commits(&committed) != vec![COMPOSITION.to_string()] {
            failures.push(format!(
                "assertion B: expected exactly one Commit(\"{COMPOSITION}\"), got {:?}",
                commits(&committed)
            ));
        }
        if still_marked != NO {
            failures.push(
                "assertion B: hasMarkedText still answered YES after a commit; the committed \
                 text supersedes the composition, and AppKit requires NO once insertText: \
                 returns"
                    .to_string(),
            );
        }
        tracing::info!(
            commits = ?commits(&committed),
            has_marked_text = bool::from(still_marked),
            "assertion B (commit) measured"
        );

        // ---- C: the cursor area reaches the candidate-window query ----------
        let area = Bounds::new(Point::new(px(10.0), px(20.0)), Size::new(px(2.0), px(18.0)));
        text_input.set_ime_cursor_area(area);
        // `actualRange:` is answered, not left null: the SDK requires it to hold
        // "the character range corresponding to the returned area", so a query
        // that comes back with a real rect and a not-found range is telling the
        // input method the opposite of what it returned.
        let mut actual = NSRange::new(0, 0);
        // SAFETY: main thread; `firstRectForCharacterRange:actualRange:` is an
        // `NSTextInputClient` query on the live view, and `actual` is a live,
        // writable `NSRange` for the duration of the call.
        let rect: NSRect = unsafe {
            msg_send![content_view,
                firstRectForCharacterRange: NSRange::new(0, utf16_len)
                actualRange: &raw mut actual
            ]
        };
        tracing::info!(
            origin_x = rect.origin.x,
            origin_y = rect.origin.y,
            width = rect.size.width,
            height = rect.size.height,
            actual_location = actual.location,
            actual_length = actual.length,
            "firstRectForCharacterRange: answered"
        );
        if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
            failures.push(format!(
                "assertion C: firstRectForCharacterRange: returned a zero-sized rect \
                 (origin {:?}, size {:?}) after set_ime_cursor_area({area:?}); an input method \
                 asking where to put its candidate window would have nowhere to go",
                (rect.origin.x, rect.origin.y),
                (rect.size.width, rect.size.height),
            ));
        }
        if actual.location == NSNotFound as usize {
            failures.push(
                "assertion C: firstRectForCharacterRange: answered a real rect but reported \
                 actualRange as {NSNotFound, 0}; the SDK contract is that actualRange holds the \
                 character range corresponding to the returned area, so this tells the input \
                 method there is no range behind a rect it was just given"
                    .to_string(),
            );
        } else if actual != NSRange::new(0, 0) {
            // No composition is open at this point (assertion B committed, and
            // the empty preedit of a commit clears the marked range), so the
            // area is the caret's and the range that goes with it is zero-length
            // at the view's own origin — the same origin `markedRange` reports
            // from, this view having no document of its own.
            failures.push(format!(
                "assertion C: firstRectForCharacterRange: answered actualRange {:?} with no \
                 composition open; the caret's own range is a zero-length one at the view's \
                 origin, {{0, 0}}",
                (actual.location, actual.length),
            ));
        }

        // ---- D: unmarkText announces the end of composition -----------------
        set_marked_text(content_view, COMPOSITION, marked);
        let marked_again = observed.wait_for(|events| !preedits(events).is_empty());
        let re_marked = observed.take();
        if !marked_again || preedits(&re_marked) != vec![expected_preedit.clone()] {
            failures.push(format!(
                "assertion D precondition: a second setMarkedText: of {COMPOSITION:?} did not \
                 dispatch the expected Preedit {expected_preedit:?}; collected {re_marked:?}"
            ));
        }

        unmark_text(content_view);
        let unmark_arrived = observed.wait_for(|events| !events.is_empty());
        let unmarked = observed.take();
        // SAFETY: main thread; `NSTextInputClient` query on the live view.
        let marked_after_unmark: BObjC = unsafe { msg_send![content_view, hasMarkedText] };

        if !unmark_arrived {
            failures.push(format!(
                "assertion D: unmarkText dispatched nothing within {DELIVERY_DEADLINE:?}; a \
                 client left holding composition state it was never told ended suppresses \
                 Key::Character for the rest of the focus session"
            ));
        }
        if preedits(&unmarked) != vec![(String::new(), None)] {
            failures.push(format!(
                "assertion D: unmarkText must dispatch exactly one ImeEvent::Preedit with an \
                 empty text and no cursor — the vocabulary's spelling of \"the composition \
                 ended\"; got {:?}",
                ime_events(&unmarked)
            ));
        }
        if marked_after_unmark != NO {
            failures.push(
                "assertion D: hasMarkedText still answered YES after unmarkText; the composition \
                 it announced as ended is still reported as live"
                    .to_string(),
            );
        }
        tracing::info!(
            after_unmark = ?ime_events(&unmarked),
            has_marked_text = bool::from(marked_after_unmark),
            "assertion D measured"
        );

        // ---- E: the release, and the gate that decides whether it is sent ---
        // Every synthesized event above is a `keyDown:`, so `keyUp:` — a
        // separate AppKit entry point with its own arm — has not been touched.
        // It is gated on the *composition*, not on whether a text input is
        // attached, so a commit returning the input context to ground must
        // still let the release through: that is the reporting half, and it
        // goes first, because the suppressing half is an absence and an absence
        // is only worth anything on a channel just proven live.
        text_input.set_ime_allowed(true);
        let reattached =
            observed.wait_for(|events| ime_events(events).contains(&ImeEvent::Enabled));
        let on_reattach = observed.take();
        if !reattached {
            failures.push(format!(
                "assertion E precondition: set_ime_allowed(true) dispatched no \
                 ImeEvent::Enabled within {DELIVERY_DEADLINE:?}, so the attached state the \
                 release is measured in was never entered; collected {on_reattach:?}"
            ));
        }

        send_letter_key_up(window_number, content_view);
        let release_arrived = observed.wait_for(|events| !released_characters(events).is_empty());
        let released = observed.take();

        if !release_arrived {
            failures.push(format!(
                "assertion E (attached, nothing composing): no key release arrived within \
                 {DELIVERY_DEADLINE:?} of a synthesized `{LETTER}` keyUp. A commit returns the \
                 input context to ground before the release arrives, so this press's release is \
                 the keyboard path's to report — suppressing it here would leave a client holding \
                 a key down forever. Collected: {released:?}"
            ));
        }
        if released_characters(&released) != vec![LETTER.to_string()] {
            failures.push(format!(
                "assertion E (attached, nothing composing): expected exactly one RELEASE of \
                 `{LETTER}`, got {:?}. A release reported as a press means the `keyUp:` arm ran \
                 the keyboard path without its state; a client keying on `state` would then see \
                 two presses and no release for the key it is holding.",
                released_characters(&released)
            ));
        }
        tracing::info!(
            released = ?released_characters(&released),
            "assertion E (release reported) measured"
        );

        set_marked_text(content_view, COMPOSITION, marked);
        let composing_again = observed.wait_for(|events| !preedits(events).is_empty());
        let _ = observed.take();
        if !composing_again {
            failures.push(format!(
                "assertion E precondition: no Preedit arrived within {DELIVERY_DEADLINE:?} for the \
                 composition the release was supposed to be suppressed by, so the gate went \
                 unexercised"
            ));
        }

        send_letter_key_up(window_number, content_view);
        // A wait that is *expected* to expire: the same 5 ms poll and 500 ms
        // deadline every arrival above uses, on a window where the identical
        // call was delivered inline a moment ago.
        let release_during_composition = observed.wait_for(|events| !events.is_empty());
        let while_composing = observed.take();

        if release_during_composition {
            failures.push(format!(
                "assertion E (composing): a `{LETTER}` keyUp landed while a composition was \
                 open: {while_composing:?}. A release is not input to the composition — the input \
                 method is mid-word and there is no committed text to release — and a client that \
                 keys its own held-key state on the keyboard path sees a release whose press it \
                 was never shown (the press became a Commit)."
            ));
        }
        tracing::info!(
            while_composing = ?while_composing,
            "assertion E (release suppressed by an open composition) measured"
        );

        // ---- F: a MODIFIED key routes through the input method ----------------
        // Assertion A presses one unmodified letter, so nothing above covers a
        // press that carries modifiers and a non-ASCII character. This one does:
        // Option-E, then `e`.
        //
        // What is required is the *route* — both presses arrive as input-method
        // events and the keyboard path stays silent — because that is the
        // contract this backend owns. The characters are reported instead: which
        // character a layout gives Option-`E`, and whether the input context
        // composes the two presses at all, are properties of the host, not of
        // this backend. On this host it does not compose them (see the module
        // doc for the measurement and the layout tables it was checked against),
        // so requiring the layout's resolved `é` would pin a host property as a
        // contract and fail for a reason that is not a defect.
        //
        // Assertion E left a composition open; close it first with the same
        // call an input method makes, so this sequence starts from ground.
        unmark_text(content_view);
        let _ = observed.wait_for(|events| !preedits(events).is_empty());
        let _ = observed.take();

        send_dead_key_sequence(window_number, content_view);
        // The first press's text is the signal that the sequence started; the
        // settle then collects whatever followed it. Waiting for a *count* here
        // would encode the assumption this assertion exists to avoid making.
        let _ = observed.wait_for(|events| !commits(events).is_empty());
        observed.settle();
        let composed = observed.take();
        // SAFETY: main thread; `NSTextInputClient` query on the live view.
        let marked_after_composition: BObjC = unsafe { msg_send![content_view, hasMarkedText] };

        tracing::info!(
            preedits = ?preedits(&composed),
            commits = ?commits(&composed),
            typed = ?typed_characters(&composed),
            has_marked_text = bool::from(marked_after_composition),
            "assertion F (modified key: Option-E then `e`) measured"
        );

        // Text has to have gone *somewhere*: a press that produced neither a
        // preedit nor a commit was routed past the input method entirely.
        if commits(&composed).is_empty() && preedits(&composed).is_empty() {
            failures.push(
                "assertion F (modified key): pressing Option-E then `e` with a text input attached \
                 produced no input-method event at all, so the presses were routed past the input \
                 method instead of through it"
                    .to_string(),
            );
        }
        // And it must all have gone the same way. A single character on the
        // keyboard path means the route was keyed on the modifier or on the
        // character being non-ASCII — the one shape assertion A cannot see,
        // since A's letter is plain on both counts.
        if !typed_characters(&composed).is_empty() {
            failures.push(format!(
                "assertion F (modified key): presses carrying a modifier and a non-ASCII character \
                 must not double-produce — the keyboard path delivered {:?} as well as the input \
                 method's {:?} (preedits {:?})",
                typed_characters(&composed),
                commits(&composed),
                preedits(&composed),
            ));
        }
        // The composition must also be *over*: a client that saw the text land
        // but still held marked text would suppress `Key::Character` for the
        // rest of the focus session (the bug class assertion D covers for
        // `unmarkText`).
        if marked_after_composition != NO {
            failures.push(
                "assertion F (modified key): hasMarkedText still answered YES after the presses \
                 landed, so a composition the commits ended is still reported as live"
                    .to_string(),
            );
        }

        // ---- report ---------------------------------------------------------
        if failures.is_empty() {
            tracing::info!("IME_PROBE_RESULT=PASS");
            std::process::exit(0);
        }
        for failure in &failures {
            tracing::error!("FAILURE: {failure}");
        }
        tracing::error!("IME_PROBE_RESULT=FAIL");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn main() {
    appkit_ime_probe::run();
}

/// Non-macOS build placeholder: this probe needs the AppKit main thread, a
/// bundle, and a live window to route input through; on other targets it
/// exists only so the workspace compiles. Run it with `just macos-ime` on a
/// real Mac.
#[cfg(not(target_os = "macos"))]
fn main() {}
