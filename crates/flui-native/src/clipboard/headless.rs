//! The headless host's clipboard: a [`MemoryClipboard`], one per loop.

use std::{rc::Rc, sync::Arc};

use crate::host;
use crate::host::{MemoryClipboard, NativeClipboard};
use crate::owner::headless::HeadlessLoop;

impl host::Clipboard<HeadlessLoop> for MemoryClipboard {
    fn clipboard(owner: &HeadlessLoop) -> Rc<dyn NativeClipboard> {
        Rc::new(owner.with_state(|state| Arc::clone(&state.clipboard)))
    }
}
