//! The headless host's clipboard: the contract's in-memory clipboard, one per
//! loop.

use std::sync::Arc;

use flui_platform::{Clipboard, InMemoryClipboard};

use crate::host;
use crate::owner::headless::HeadlessLoop;

impl host::Clipboard<HeadlessLoop> for InMemoryClipboard {
    fn clipboard(owner: &HeadlessLoop) -> Arc<dyn Clipboard> {
        owner.with_state(|state| state.clipboard.clone())
    }
}
