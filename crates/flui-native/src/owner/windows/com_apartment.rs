//! The owner thread's COM apartment, held as a value.
//!
//! [`ComApartment`] is one `OleInitialize` (the single-threaded apartment
//! plus the OLE state `RegisterDragDrop` and `DoDragDrop` need) whose `Drop`
//! is the matching `OleUninitialize`. Everything that uses COM objects past the platform's own
//! lifetime holds a clone of it — the platform, and each window's TSF
//! connection (the text feature's `TextServices`) — so the apartment ends only after
//! its last user has released its interfaces, in whatever order the platform
//! and its windows are dropped. `!Send`: the apartment belongs to the thread
//! that entered it, and the `Rc` count is touched there only.
//!
//! The platform itself is `Send`, so it keeps its clone in a [`ApartmentHold`],
//! which hands clones out and releases its own only on the apartment's thread;
//! a platform dropped elsewhere leaks its hold (the apartment then stays
//! entered on its thread) rather than uninitialize COM on the wrong thread.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::thread::ThreadId;

use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

/// One entry into the calling thread's single-threaded apartment.
pub(crate) struct ComApartment {
    /// `!Send` and `!Sync`: the apartment is thread-affine.
    _thread_affine: PhantomData<*const ()>,
}

impl ComApartment {
    /// Enter the calling thread's STA, with OLE.
    ///
    /// # Errors
    ///
    /// The `HRESULT` `OleInitialize` failed with (for instance
    /// `RPC_E_CHANGED_MODE` on a thread already in the multithreaded
    /// apartment); nothing is left to release then.
    pub(crate) fn enter() -> windows_core::Result<Rc<Self>> {
        // SAFETY: no pointer arguments (`None` is the reserved parameter);
        // the result is checked, and only a success is balanced by `Drop`.
        unsafe { OleInitialize(None) }?;
        Ok(Rc::new(Self {
            _thread_affine: PhantomData,
        }))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: no arguments. `enter` succeeded on this thread (the value
        // is `!Send`), and this is its single matching call.
        unsafe { OleUninitialize() };
    }
}

/// Whether the calling thread is inside an STA now: what TSF's teardown
/// requires. An implicit process MTA does not keep this thread's STA alive.
#[cfg(test)]
pub(crate) fn thread_in_apartment() -> bool {
    use windows::Win32::System::Com::{
        APTTYPE, APTTYPE_MAINSTA, APTTYPE_STA, APTTYPEQUALIFIER, CoGetApartmentType,
    };
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    // SAFETY: both out-pointers are live, writable locals.
    unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) }.is_ok()
        && matches!(kind, APTTYPE_STA | APTTYPE_MAINSTA)
}

/// The platform's clone of its [`ComApartment`], usable from the `Send`
/// platform: the clone is reached, and released, on the apartment's thread
/// only.
pub(crate) struct ApartmentHold {
    apartment: ManuallyDrop<Rc<ComApartment>>,
    thread: ThreadId,
}

// SAFETY: the `Rc` inside is cloned (`share`) and dropped (`Drop`) only after
// checking that the caller is `thread`, the thread that created it, so its
// count is never touched from two threads; off that thread the hold is inert
// and is leaked when dropped.
unsafe impl Send for ApartmentHold {}
// SAFETY: as for `Send`: `&ApartmentHold` reaches the `Rc` only through
// `share`, which refuses every thread but `thread`.
unsafe impl Sync for ApartmentHold {}

impl ApartmentHold {
    /// Hold `apartment`, created on the calling thread.
    pub(crate) fn new(apartment: Rc<ComApartment>) -> Self {
        Self {
            apartment: ManuallyDrop::new(apartment),
            thread: std::thread::current().id(),
        }
    }

    /// A clone for a COM user on the apartment's thread; `None` elsewhere.
    pub(crate) fn share(&self) -> Option<Rc<ComApartment>> {
        (std::thread::current().id() == self.thread).then(|| Rc::clone(&self.apartment))
    }
}

impl Drop for ApartmentHold {
    fn drop(&mut self) {
        if std::thread::current().id() == self.thread {
            // SAFETY: taken once, here, and never read again.
            drop(unsafe { ManuallyDrop::take(&mut self.apartment) });
        } else {
            crate::owner::panic_boundary::contain_owner_callback(|| {
                tracing::error!(
                    "COM apartment hold dropped off its thread; the apartment stays entered there"
                );
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct PanickingDiagnostic;
    impl tracing::Subscriber for PanickingDiagnostic {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            panic!("diagnostic failure");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    #[test]
    fn off_owner_apartment_retirement_contains_diagnostics() {
        for unwinding in [false, true] {
            let hold = ApartmentHold::new(ComApartment::enter().expect("STA"));
            std::thread::spawn(move || {
                tracing::subscriber::with_default(PanickingDiagnostic, || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if unwinding {
                            let _hold = hold;
                            panic!("first failure");
                        }
                        drop(hold);
                    }));
                    assert_eq!(result.is_err(), unwinding);
                    if let Err(failure) = result {
                        assert_eq!(
                            flui_foundation::panic::payload_text(failure.as_ref()),
                            Some("first failure")
                        );
                    }
                });
            })
            .join()
            .expect("retirement thread");
        }
    }
}
