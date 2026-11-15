//! The keyboard layout callback (`Platform::on_keyboard_layout_change`), as
//! every loop keeps it: one slot, reached through the call point.
//!
//! A registration replaces the callback before it, which is dropped outside
//! the slot with its panic contained, so a capture's destructor may call the
//! platform; a change calls the newest one with no borrow held, so it may
//! register another. Its panic is the failure of the turn that heard the
//! change, which the loop folds into that turn as it does a window handler's
//! (`crates/flui-native/ARCHITECTURE.md`, "A turn after a handler's panic").

use std::cell::RefCell;

use super::call_point::{Called, HandlerSlot, call, install};

/// A registered keyboard layout callback.
type Callback = Box<dyn FnMut()>;

/// Where a loop keeps the keyboard layout callback: owner-thread state, one
/// callback at a time.
#[derive(Default)]
pub(crate) struct LayoutCallback(RefCell<Option<Callback>>);

impl LayoutCallback {
    /// Keep `callback`, dropping the one it replaces outside the slot.
    pub(crate) fn register(&self, callback: Callback) {
        install(self, callback);
    }

    /// The layout changed: call the callback, if one is registered. Its panic
    /// is the caller's to fold into its turn.
    #[must_use = "a callback's panic is the turn's failure: resume or contain it"]
    pub(crate) fn changed(&self) -> Called<()> {
        call(self, |callback| callback())
    }

    /// Drop the callback, contained: the loop let go of what the application
    /// gave it.
    #[cfg(any(wayland_host, all(test, not(target_arch = "wasm32"))))]
    pub(crate) fn release(&self) {
        let callback = self.0.borrow_mut().take();
        super::call_point::retire(callback);
    }
}

impl HandlerSlot for LayoutCallback {
    type Handler = Callback;

    fn take(&self) -> Option<Callback> {
        self.0.borrow_mut().take()
    }

    fn restore(&self, callback: Callback) -> Option<Callback> {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(callback);
            None
        } else {
            Some(callback)
        }
    }

    fn replace(&self, callback: Callback) -> Option<Callback> {
        self.0.borrow_mut().replace(callback)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use super::{Called, LayoutCallback};

    /// Counts its calls and its drop.
    fn counting(calls: &Rc<Cell<u32>>, dropped: &Rc<Cell<bool>>) -> Box<dyn FnMut()> {
        struct OnDrop(Rc<Cell<bool>>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let calls = Rc::clone(calls);
        let guard = OnDrop(Rc::clone(dropped));
        Box::new(move || {
            let _ = &guard;
            calls.set(calls.get() + 1);
        })
    }

    /// A second registration replaces the first: the first is dropped at once
    /// and never called again, the newest hears every change.
    #[test]
    fn a_registration_replaces_the_one_before_it() {
        let slot = LayoutCallback::default();
        let (first_calls, first_dropped) = (Rc::default(), Rc::default());
        let (second_calls, second_dropped) = (Rc::default(), Rc::default());
        slot.register(counting(&first_calls, &first_dropped));
        assert!(matches!(slot.changed(), Called::Returned(())));
        slot.register(counting(&second_calls, &second_dropped));
        assert!(first_dropped.get(), "the replaced callback was kept");
        assert!(matches!(slot.changed(), Called::Returned(())));
        assert!(matches!(slot.changed(), Called::Returned(())));
        assert_eq!((first_calls.get(), second_calls.get()), (1, 2));
        slot.release();
        assert!(second_dropped.get());
        assert!(matches!(slot.changed(), Called::Vacant));
    }

    /// A callback that panics stays registered, its panic comes back to the
    /// caller, and the next change calls it again; one that registers another
    /// from inside its call is replaced by it.
    #[test]
    fn a_panicking_callback_is_the_callers_failure_and_stays() {
        let slot = Rc::new(LayoutCallback::default());
        let calls = Rc::new(Cell::new(0_u32));
        let counted = Rc::clone(&calls);
        slot.register(Box::new(move || {
            counted.set(counted.get() + 1);
            if counted.get() == 1 {
                std::panic::panic_any("layout");
            }
        }));
        let Called::Panicked(failure) = slot.changed() else {
            panic!("the callback's panic was not returned");
        };
        let payload = catch_unwind(AssertUnwindSafe(|| failure.resume())).expect_err("resumed");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"layout"));
        assert!(matches!(slot.changed(), Called::Returned(())));
        assert_eq!(calls.get(), 2);

        let replacing = Rc::downgrade(&slot);
        let (newest_calls, newest_dropped) = (Rc::default(), Rc::default());
        let (calls_in, dropped_in) = (Rc::clone(&newest_calls), Rc::clone(&newest_dropped));
        slot.register(Box::new(move || {
            if let Some(slot) = replacing.upgrade() {
                slot.register(counting(&calls_in, &dropped_in));
            }
        }));
        assert!(matches!(slot.changed(), Called::Returned(())));
        assert!(matches!(slot.changed(), Called::Returned(())));
        assert_eq!(
            newest_calls.get(),
            1,
            "the one registered inside the call stays"
        );
    }
}
