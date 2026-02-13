//! Application lifecycle management.
//!
//! Re-exports platform lifecycle types as the canonical lifecycle API.
//! Previously this module defined its own `AppLifecycle` enum, which
//! duplicated `LifecycleState` from `flui-platform`. Now we delegate
//! entirely to the platform crate's lifecycle model.

pub use flui_platform::traits::{
    DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle,
};
