//! Dev-time utilities (layer 1: build orchestration).
//!
//! This module is only needed by tooling (`flui-cli`, `flui-devtools`) that watches
//! source files and triggers rebuilds. Runtime hosts use [`crate::HotReloadDriver`]
//! instead.

#[cfg(feature = "source-watch")]
mod source_watch;

#[cfg(feature = "source-watch")]
pub use source_watch::{SourceWatcher, WatchError};
