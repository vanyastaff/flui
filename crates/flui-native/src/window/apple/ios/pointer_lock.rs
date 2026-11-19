//! The iOS host's one owner of the pointer lock (ADR-0354 §2).
//!
//! The owner keeps, for every window of the process, whether its root
//! controller prefers the lock and whether the system holds its scene's
//! pointer locked ([`LockLedger`]), and the one set of `GCMouse` handlers the
//! locked mouse's motion comes through: installed on every mouse, those that
//! connect during a lock included, while some window holds the lock. Once
//! none does, each mouse that still carries FLUI's handler gets back the one
//! FLUI found on it (the application's, or none); a mouse whose handler was
//! replaced since keeps the new one ([`HandlerSwaps`]). The motion goes to
//! the window that holds the lock, on the main thread only. The window asks
//! and lets go through the owner; the content view tells it what the system
//! said; the controller answers `prefersPointerLocked` from it.
//!
//! UIKit's and GameController's notification names are read from the loaded
//! frameworks at run time ([`exported_string`]): their symbols exist from
//! iPadOS 14, and a linked reference would keep the app from loading on the
//! iOS 13 floor. Where UIKit has no lock notification, no window locks.

use std::cell::RefCell;
use std::ffi::CStr;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue, NSString,
};

use flui_platform::WindowId;

use super::FluiView;
use super::status_bar::StatusBar;
use crate::owner::apple::ios::NativeOwner;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::window_delivery::WindowDelivery;
use crate::window::apple::pointer_lock::{HandlerSwaps, LockLedger, LockStep, routable_delta};

// GameController's classes and notification names are found by name only
// once it is loaded.
#[link(name = "GameController", kind = "framework")]
unsafe extern "C" {}

/// The handler type `mouseMovedHandler` takes: the input that moved and its
/// deltas.
type Moved = dyn Fn(*mut AnyObject, f32, f32);

/// A window as the owner reaches it, weakly: its content view (where its
/// motion goes), its root controller (which reads the preference again) and
/// its delivery (which publishes its snapshot).
struct LockWindow {
    id: WindowId,
    view: objc2::rc::Weak<FluiView>,
    status_bar: std::sync::Weak<StatusBar>,
    delivery: std::sync::Weak<WindowDelivery>,
}

/// A mouse, compared by identity.
struct Mouse(Retained<AnyObject>);

impl PartialEq for Mouse {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(Retained::as_ptr(&self.0), Retained::as_ptr(&other.0))
    }
}

/// The handlers FLUI installed while a window holds the lock.
struct MouseWatch {
    /// The one handler every mouse gets.
    moved: RcBlock<Moved>,
    /// The mice FLUI set `moved` on, each with the handler it found there.
    installed: HandlerSwaps<Mouse, RcBlock<Moved>>,
    /// The connect and disconnect observers.
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

/// The process's pointer lock (see the module doc). Main thread only: every
/// window, view and controller that reaches it lives there.
pub(crate) struct PointerLocks {
    ledger: RefCell<LockLedger<WindowId>>,
    windows: RefCell<Vec<LockWindow>>,
    mice: RefCell<Option<MouseWatch>>,
    /// `UIPointerLockStateDidChangeNotification`, from the loaded UIKit;
    /// `None` where UIKit has none.
    lock_changed: Option<Retained<NSString>>,
    this: Weak<PointerLocks>,
}

impl std::fmt::Debug for PointerLocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerLocks")
            .field("holder", &self.ledger.borrow().holder())
            .field("notified", &self.lock_changed.is_some())
            .finish_non_exhaustive()
    }
}

impl PointerLocks {
    /// The owner, with UIKit's lock notification read once.
    pub(crate) fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            ledger: RefCell::default(),
            windows: RefCell::default(),
            mice: RefCell::new(None),
            lock_changed: exported_string(LOCK_CHANGED),
            this: this.clone(),
        })
    }

    /// UIKit's lock notification, which every content view observes; `None`
    /// where UIKit tells no lock, and no window takes one.
    pub(crate) fn lock_notification(&self) -> Option<&NSString> {
        self.lock_changed.as_deref()
    }

    /// Whether a window can be told when the system locks its pointer.
    pub(crate) fn tells_locks(&self) -> bool {
        self.lock_changed.is_some()
    }

    /// Reach `id`'s window through its view, controller and delivery.
    pub(crate) fn register(
        &self,
        id: WindowId,
        view: objc2::rc::Weak<FluiView>,
        status_bar: std::sync::Weak<StatusBar>,
        delivery: std::sync::Weak<WindowDelivery>,
    ) {
        self.windows.borrow_mut().push(LockWindow {
            id,
            view,
            status_bar,
            delivery,
        });
    }

    /// `id`'s window is gone: it is reached no more, then lets go, so nothing
    /// of the going window is published or refreshed.
    pub(crate) fn unregister(&self, id: WindowId, mtm: MainThreadMarker) {
        self.windows.borrow_mut().retain(|window| window.id != id);
        self.withdraw(id, mtm);
    }

    /// Whether `id`'s controller prefers the lock: what `prefersPointerLocked`
    /// answers.
    pub(crate) fn prefers(&self, id: WindowId) -> bool {
        self.ledger.borrow().prefers(id)
    }

    /// Whether the system holds `id`'s scene's pointer locked: what its
    /// snapshot publishes.
    pub(crate) fn holds(&self, id: WindowId) -> bool {
        self.ledger.borrow().holds(id)
    }

    /// `id` asks for the lock: its controller prefers it, and UIKit reads it
    /// again. `locked_now` is the scene's lock as UIKit holds it now: a lock
    /// the window let go of and asks for again before UIKit read the
    /// preference in between never ended, and UIKit posts nothing of it, so
    /// the window holds it at once ([`LockLedger::ask`]).
    pub(crate) fn ask(&self, id: WindowId, locked_now: bool, mtm: MainThreadMarker) {
        let step = self.ledger.borrow_mut().ask(id, locked_now);
        self.carry_out(step, mtm);
    }

    /// `id` lets go: asked, lost the focus, or left its scene. The system's
    /// lock of its scene is not read after: the window no longer asks, so no
    /// answer of the system's could make it hold the lock again.
    pub(crate) fn withdraw(&self, id: WindowId, mtm: MainThreadMarker) {
        let step = self.ledger.borrow_mut().withdraw(id);
        self.carry_out(step, mtm);
    }

    /// The system said whether it holds `id`'s scene's pointer locked.
    pub(crate) fn reported(&self, id: WindowId, locked: bool, mtm: MainThreadMarker) {
        let step = self.ledger.borrow_mut().reported(id, locked);
        self.carry_out(step, mtm);
    }

    /// Carry out what a step changed, with nothing of the owner borrowed:
    /// the mouse handlers follow the holder, the controller reads its
    /// preference again, and each window whose grab changed publishes. A
    /// publication is the handler's code: the first panic resumes after
    /// every window published.
    fn carry_out(&self, step: LockStep<WindowId>, mtm: MainThreadMarker) {
        let remains = self.follow_holder(mtm);
        let mut failure = flui_foundation::panic::FirstPanic::new();
        if let Some(id) = step.preference
            && let Some(status_bar) = self
                .window(id, |window| window.status_bar.upgrade())
                .flatten()
        {
            failure.contain(|| status_bar.refresh_pointer_lock(mtm));
        }
        for id in step.published.into_iter().flatten() {
            if let Some(delivery) = self
                .window(id, |window| window.delivery.upgrade())
                .flatten()
            {
                failure.contain(|| delivery.publish_state());
            }
        }
        // The copies of the application's handlers go last, once the state
        // is committed, the preference refreshed and every window published:
        // their last release is the application's code (ADR-0287). Nothing
        // of the owner is borrowed here, and each is contained on its own, so
        // one copy's panic leaves the next released and the first failure
        // kept.
        for copy in remains {
            failure.contain(|| drop(copy));
        }
        failure.resume();
    }

    /// What `read` makes of `id`'s window, cloned out of the borrow.
    fn window<T>(&self, id: WindowId, read: impl FnOnce(&LockWindow) -> T) -> Option<T> {
        self.windows
            .borrow()
            .iter()
            .find(|window| window.id == id)
            .map(read)
    }

    /// The locked mouse moved: the window that holds the lock delivers it;
    /// no window does while none holds one. Called on the main thread only
    /// (`routable_delta`).
    fn route(&self, delta: (f64, f64)) {
        let Some(holder) = self.ledger.borrow().holder() else {
            return;
        };
        if let Some(view) = self.window(holder, |window| window.view.load()).flatten() {
            view.locked_motion(delta);
        }
    }

    /// Install the handlers once a window holds the lock, and take them back
    /// once none does; what is left to destroy is handed back.
    fn follow_holder(&self, mtm: MainThreadMarker) -> Remains {
        let wanted = self.ledger.borrow().holder().is_some();
        let watching = self.mice.borrow().is_some();
        let mut remains = Remains::new();
        if wanted && !watching {
            let watch = self.watch(mtm, &mut remains);
            *self.mice.borrow_mut() = Some(watch);
        } else if !wanted && watching {
            let watch = self.mice.borrow_mut().take();
            if let Some(watch) = watch {
                unwatch(watch, &mut remains);
            }
        }
        remains
    }

    /// The handler on every connected mouse, and observers that put it on a
    /// mouse that connects and forget one that goes.
    fn watch(&self, mtm: MainThreadMarker, remains: &mut Remains) -> MouseWatch {
        // The owner is reached through a main-thread binding: the blocks may
        // be run and let go of on another thread, where the `Rc` behind it is
        // neither touched nor dropped.
        let owner = NativeOwner::new(self.this.clone(), mtm);
        let moved: RcBlock<Moved> =
            RcBlock::new(move |_input: *mut AnyObject, dx: f32, dy: f32| {
                os_entry(
                    "GCMouseInput.mouseMovedHandler",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || {
                        // The handler runs on the mouse's `handlerQueue`,
                        // the application's to set: off the main thread the
                        // owner and the views are not touched, and the motion
                        // is dropped (`routable_delta`); FLUI does not set the
                        // queue over the application's.
                        let main = MainThreadMarker::new();
                        if let Some(delta) = routable_delta(main.is_some(), dx, dy)
                            && let Some(main) = main
                            && let Some(owner) = owner.get(main).upgrade()
                        {
                            owner.route(delta);
                        }
                    },
                );
            });
        let mut installed = HandlerSwaps::default();
        for mouse in mice() {
            if let HandlerSwap::Replaced(previous) = swap_moved_handler(&mouse, &moved)
                && let Err(Some(refused)) = installed.installed(Mouse(mouse), previous)
            {
                remains.push(refused);
            }
        }
        let mut observers = Vec::new();
        for (symbol, connected) in [
            (c"GCMouseDidConnectNotification", true),
            (c"GCMouseDidDisconnectNotification", false),
        ] {
            let Some(name) = exported_string(symbol) else {
                continue;
            };
            let owner = NativeOwner::new(self.this.clone(), mtm);
            let block = RcBlock::new(move |note: NonNull<NSNotification>| {
                os_entry(
                    "a GCMouse connection",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || {
                        // The main queue runs the block; a marker it lacks
                        // would mean another queue, where nothing is touched.
                        let Some(main) = MainThreadMarker::new() else {
                            return;
                        };
                        // SAFETY: the center hands the block a live
                        // notification for the call.
                        let note = unsafe { note.as_ref() };
                        if let (Some(owner), Some(mouse)) =
                            (owner.get(main).upgrade(), note.object())
                        {
                            owner.mouse_changed(mouse, connected);
                        }
                    },
                );
            });
            let queue = NSOperationQueue::mainQueue();
            // SAFETY: a `nil` object hears every mouse; the main queue runs
            // the block on the main thread, where the owner lives; the
            // observer is removed in `unwatch`.
            let observer = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(&*name),
                    None,
                    Some(&*queue),
                    &block,
                )
            };
            observers.push(observer);
        }
        MouseWatch {
            moved,
            installed,
            observers,
        }
    }

    /// A mouse connected during a lock gets the handler; one that went is
    /// forgotten. Outside a lock nothing is watched.
    fn mouse_changed(&self, mouse: Retained<AnyObject>, connected: bool) {
        let mouse = Mouse(mouse);
        // A copy of the application's handler this lets go of is destroyed
        // once the watch is committed (ADR-0287).
        if !connected {
            flui_foundation::panic::commit(&self.mice, |mice, outbox| {
                if let Some(watch) = mice.as_mut()
                    && let Some((_, Some(copy))) = watch.installed.forget(&mouse)
                {
                    outbox.release(copy);
                }
            });
            return;
        }
        // Cloned out first: no borrow is held while GameController is
        // messaged.
        let moved = self.mice.borrow().as_ref().map(|watch| watch.moved.clone());
        let Some(moved) = moved else {
            return;
        };
        let HandlerSwap::Replaced(previous) = swap_moved_handler(&mouse.0, &moved) else {
            return;
        };
        flui_foundation::panic::commit(&self.mice, |mice, outbox| {
            let refused = match mice.as_mut() {
                Some(watch) => watch.installed.installed(mouse, previous),
                None => Err(previous),
            };
            if let Err(Some(copy)) = refused {
                outbox.release(copy);
            }
        });
    }
}

/// What the owner destroys once it published: copies of the application's
/// handlers, whose last release runs the application's code, and FLUI's own
/// block.
type Remains = Vec<RcBlock<Moved>>;

/// Give each mouse that still carries FLUI's handler back the one FLUI found
/// on it (another handler set since is left alone), and stop observing; every
/// copy and FLUI's block go to `remains`.
fn unwatch(watch: MouseWatch, remains: &mut Remains) {
    let MouseWatch {
        moved,
        installed,
        observers,
    } = watch;
    let ours = |mouse: &Mouse| {
        moved_handler(&mouse.0).is_some_and(|current| std::ptr::eq(current, &raw const *moved))
    };
    let restoration = installed.restore(ours);
    for (mouse, previous) in restoration.put_back {
        set_moved_handler(&mouse.0, previous.as_deref());
        remains.extend(previous);
    }
    remains.extend(restoration.left.into_iter().filter_map(|(_, copy)| copy));
    remains.push(moved);
    let center = NSNotificationCenter::defaultCenter();
    for observer in &observers {
        let observer: &AnyObject = (**observer).as_ref();
        // SAFETY: `observer` is a token `addObserverForName:` returned.
        unsafe { center.removeObserver(observer) };
    }
}

/// The mice GameController has connected; none where it has no `GCMouse`.
fn mice() -> Vec<Retained<AnyObject>> {
    let Some(class) = AnyClass::get(c"GCMouse") else {
        return Vec::new();
    };
    // SAFETY: `+[GCMouse mice]` answers an `NSArray<GCMouse *>`.
    let mice: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class, mice] };
    mice.map_or_default(|mice| mice.to_vec())
}

/// The input of `mouse`, `nil` until it reports.
fn mouse_input(mouse: &AnyObject) -> Option<Retained<AnyObject>> {
    // SAFETY: `mouseInput` is `GCMouse`'s getter of its `GCMouseInput`.
    unsafe { msg_send![mouse, mouseInput] }
}

/// What putting FLUI's motion handler on a mouse found.
enum HandlerSwap {
    /// The handler the mouse had, copied so it can be put back; `None` for
    /// none.
    Replaced(Option<RcBlock<Moved>>),
    /// The mouse has no input to set a handler on; nothing was put on it.
    NoInput,
}

/// Put `moved` on `mouse`, answering the handler it had.
fn swap_moved_handler(mouse: &AnyObject, moved: &block2::Block<Moved>) -> HandlerSwap {
    let Some(input) = mouse_input(mouse) else {
        return HandlerSwap::NoInput;
    };
    // SAFETY: the getter of the copied block property `mouseMovedHandler`;
    // the block it answers is live for the call, and copying it keeps it.
    let previous = unsafe {
        let current: *mut block2::Block<Moved> = msg_send![&*input, mouseMovedHandler];
        RcBlock::copy(current)
    };
    set_moved_handler(mouse, Some(moved));
    HandlerSwap::Replaced(previous)
}

/// Set `mouse`'s motion handler, `None` to remove it; whether the mouse has
/// an input to set it on.
fn set_moved_handler(mouse: &AnyObject, handler: Option<&block2::Block<Moved>>) -> bool {
    let Some(input) = mouse_input(mouse) else {
        return false;
    };
    // SAFETY: `mouseMovedHandler` is a copied block property of type
    // `void (^)(GCMouseInput *, float, float)`, which `Moved`'s arguments
    // match; `nil` removes it.
    let _: () = unsafe { msg_send![&*input, setMouseMovedHandler: handler] };
    true
}

/// The motion handler `mouse` has now, by address.
fn moved_handler(mouse: &AnyObject) -> Option<*const block2::Block<Moved>> {
    let input = mouse_input(mouse)?;
    // SAFETY: the getter of the block property above; the address is only
    // compared, never called.
    let current: *const block2::Block<Moved> = unsafe { msg_send![&*input, mouseMovedHandler] };
    (!current.is_null()).then_some(current)
}

/// The name of UIKit's lock notification, read from the loaded UIKit.
const LOCK_CHANGED: &CStr = c"UIPointerLockStateDidChangeNotification";

/// The string a loaded framework exports as `symbol` (an
/// `NSString *const`), or `None` where none does.
fn exported_string(symbol: &CStr) -> Option<Retained<NSString>> {
    // SAFETY: `dlsym` with `RTLD_DEFAULT` searches the loaded images and
    // answers the address of the exported variable, or null.
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr()) };
    if address.is_null() {
        return None;
    }
    // SAFETY: the symbol is a framework's `NSString *const`, set before any
    // code of the app runs and never written; retaining it keeps the string
    // whatever its storage.
    unsafe { Retained::retain(*address.cast::<*mut NSString>()) }
}
