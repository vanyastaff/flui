//! A `Completion` settles at its owner's turn: it never reaches another
//! thread (ADR-0152 §3).

use flui_platform::capability::Completion;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<Completion<u32>>();
}
