//! Web executor implementation

use crate::traits::PlatformExecutor;

pub struct WebExecutor;

unsafe impl Send for WebExecutor {}
unsafe impl Sync for WebExecutor {}

impl WebExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformExecutor for WebExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        task();
    }

    fn is_on_executor(&self) -> bool {
        true
    }
}
