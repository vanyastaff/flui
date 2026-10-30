//! A clipboard handle is owner-local: it never reaches another thread
//! (ADR-0152 §3).

use flui_interaction::ClipboardHandle;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<ClipboardHandle>();
}
