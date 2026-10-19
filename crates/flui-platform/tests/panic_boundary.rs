//! The boundary an entry the OS calls runs under: a panic answers the OS
//! with a failure and its payload never runs user code there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_platform::shared::panic_boundary::contain_os_entry;

/// A panic payload whose destructor is user code: it records that it ran.
struct Payload(Arc<AtomicBool>);

impl Drop for Payload {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// An entry's answer passes through; a panic is `None` and an opaque
/// payload is retained, never destroyed inside the OS's call. Red-check:
/// drop the payload instead of retaining it.
#[test]
fn an_os_entry_contains_its_panic_and_retains_its_payload() {
    assert_eq!(contain_os_entry("a healthy entry", || 7), Some(7));
    assert_eq!(
        contain_os_entry("a formatted panic", || -> u8 { panic!("entry {}", 7) }),
        None
    );

    let destroyed = Arc::new(AtomicBool::new(false));
    let payload = Payload(Arc::clone(&destroyed));
    let answered = contain_os_entry("an opaque panic", move || -> u8 {
        std::panic::panic_any(payload)
    });
    assert_eq!(answered, None);
    assert!(
        !destroyed.load(Ordering::SeqCst),
        "the payload's destructor ran inside the entry"
    );
}
