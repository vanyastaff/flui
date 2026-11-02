//! macOS secure event input (`EnableSecureEventInput`), held exactly while a
//! protected field has focus in the key window, as `NSSecureTextField` does:
//! a password typed in FLUI reaches no other process's event tap, and
//! another application's keyboard utilities work while this one is in the
//! background.
//!
//! The system counts holds: each accepted hold has one matching release,
//! and an acquisition the system refused is never released. The owner makes
//! one final release attempt on drop after a refused ordinary release; it
//! cannot force the OS to accept it. Pure Rust over the call that changes
//! it, so its rules run on every host.

use std::cell::Cell;

/// Whether this view holds secure event input.
#[derive(Debug)]
pub(crate) struct SecureHold<F: Fn(bool) -> bool> {
    held: Cell<bool>,
    change: F,
}

impl<F: Fn(bool) -> bool> SecureHold<F> {
    /// Own the operation that takes or releases a hold, answering whether
    /// the system accepted it.
    pub(crate) fn new(change: F) -> Self {
        Self {
            held: Cell::new(false),
            change,
        }
    }

    /// Match `wanted`, asking the system only when the held state differs.
    pub(crate) fn sync(&self, wanted: bool) {
        if wanted != self.held.get() && (self.change)(wanted) {
            self.held.set(wanted);
        }
    }
}

impl<F: Fn(bool) -> bool> Drop for SecureHold<F> {
    fn drop(&mut self) {
        // The tracker is retiring: withdraw its claim before the system
        // callback, and make one final release attempt after a prior refusal.
        // A permanent OS refusal is reported by the platform callback.
        if self.held.replace(false) {
            let _released = (self.change)(false);
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
        let holds = Cell::new(0_isize);
        let refuse = Cell::new(false);
        let change = |wanted: bool| {
            if refuse.replace(false) {
                return false;
            }
            holds.set(holds.get() + if wanted { 1 } else { -1 });
            true
        };
        let hold = SecureHold::new(change);
        hold.sync(true);
        hold.sync(true);
        assert_eq!(holds.get(), 1, "wanted twice, taken once");
        hold.sync(false);
        hold.sync(false);
        assert_eq!(holds.get(), 0, "given back once");
        refuse.set(true);
        hold.sync(true);
        assert_eq!(holds.get(), 0, "the system refused");
        hold.sync(false);
        assert_eq!(holds.get(), 0, "nothing to give back");
    }

    #[test]
    fn dropping_a_hold_retries_one_refused_release() {
        let holds = Cell::new(0_isize);
        let refuse = Cell::new(false);
        {
            let hold = SecureHold::new(|wanted| {
                if refuse.replace(false) {
                    return false;
                }
                holds.set(holds.get() + if wanted { 1 } else { -1 });
                true
            });
            hold.sync(true);
            refuse.set(true);
            hold.sync(false);
            assert_eq!(holds.get(), 1, "the ordinary release was refused");
        }
        assert_eq!(
            holds.get(),
            0,
            "the owned release survives context retirement"
        );
    }
}
