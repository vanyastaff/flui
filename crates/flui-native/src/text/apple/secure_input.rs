//! macOS secure event input (`EnableSecureEventInput`), held exactly while a
//! protected field has focus in the key window, as `NSSecureTextField` does:
//! a password typed in FLUI reaches no other process's event tap, and
//! another application's keyboard utilities work while this one is in the
//! background.
//!
//! The system counts holds, so what was taken is given back exactly once,
//! and a hold the system refused is never given back. Pure Rust over the
//! call that changes it, so its rules run on every host.

use std::cell::Cell;

/// Whether this view holds secure event input.
#[derive(Debug, Default)]
pub(crate) struct SecureHold {
    held: Cell<bool>,
}

impl SecureHold {
    /// Take or give back the hold so it matches `wanted`, through `change`,
    /// which asks the system for it and answers whether the system did:
    /// `change` runs only when the hold differs from `wanted`.
    pub(crate) fn sync(&self, wanted: bool, change: impl FnOnce(bool) -> bool) {
        if wanted != self.held.get() && change(wanted) {
            self.held.set(wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::SecureHold;

    /// A hold is taken once, given back once, and one the system refused is
    /// neither counted nor given back.
    #[test]
    fn a_hold_is_taken_and_given_back_once() {
        let hold = SecureHold::default();
        let holds = Cell::new(0_isize);
        let refuse = Cell::new(false);
        let change = |wanted: bool| {
            if refuse.replace(false) {
                return false;
            }
            holds.set(holds.get() + if wanted { 1 } else { -1 });
            true
        };
        hold.sync(true, change);
        hold.sync(true, change);
        assert_eq!(holds.get(), 1, "wanted twice, taken once");
        hold.sync(false, change);
        hold.sync(false, change);
        assert_eq!(holds.get(), 0, "given back once");
        refuse.set(true);
        hold.sync(true, change);
        assert_eq!(holds.get(), 0, "the system refused");
        hold.sync(false, change);
        assert_eq!(holds.get(), 0, "nothing to give back");
    }
}
