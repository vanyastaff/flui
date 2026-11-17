//! The owner thread's COM apartment, held as a value.
//!
//! [`ComApartment`] is one `OleInitialize` (the single-threaded apartment
//! plus the OLE state `RegisterDragDrop` and `DoDragDrop` need) whose `Drop`
//! is the matching `OleUninitialize`. Everything that uses COM objects past the platform's own
//! lifetime holds an `Rc` of it — the platform, and each window's TSF
//! connection (the text feature's `TextServices`) and touchpad viewport — so
//! the apartment ends only after its last user has released its interfaces,
//! in whatever order the platform and its windows are dropped.
//!
//! `!Send`, like the platform and the windows that hold it (ADR-0327 §5): the
//! apartment belongs to the thread that entered it, so every clone and the
//! last drop happen there, and the compiler refuses any other thread.

use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

/// One entry into the calling thread's single-threaded apartment.
pub(crate) struct ComApartment {
    /// `!Send` and `!Sync`: the apartment is thread-affine.
    _thread_affine: PhantomData<*const ()>,
}

// `Drop` calls `OleUninitialize` with no thread check: only the type keeps it
// on the thread that entered.
static_assertions::assert_not_impl_any!(ComApartment: Send, Sync);

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
