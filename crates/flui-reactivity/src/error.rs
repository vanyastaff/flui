//! Error types for flui-reactivity.
//!
//! This module provides error types for signal operations, hook management,
//! and runtime errors.

use std::any::TypeId;
use thiserror::Error;

use crate::computed::ComputedId;
use crate::context::{ComponentId, HookId};
use crate::signal::SignalId;

/// Result type alias for reactivity operations.
pub type Result<T> = std::result::Result<T, ReactivityError>;

/// Main error type for reactivity operations.
#[derive(Error, Debug, Clone)]
pub enum ReactivityError {
    /// Signal-related errors
    #[error(transparent)]
    Signal(#[from] SignalError),

    /// Hook-related errors
    #[error(transparent)]
    Hook(#[from] HookError),

    /// Runtime errors
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Errors related to signal operations.
#[derive(Error, Debug, Clone)]
pub enum SignalError {
    /// Signal not found in runtime
    #[error("Signal with ID {0:?} not found in runtime")]
    NotFound(SignalId),

    /// Type mismatch when accessing signal
    #[error("Type mismatch for signal {signal_id:?}: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        signal_id: SignalId,
        expected: TypeId,
        actual: TypeId,
    },

    /// Subscription not found
    #[error("Subscription with ID {0} not found")]
    SubscriptionNotFound(u64),

    /// Signal already disposed
    #[error("Signal {0:?} has been disposed and cannot be accessed")]
    Disposed(SignalId),

    /// Circular dependency detected in signals
    #[error("Circular dependency detected in signal graph involving {0:?}")]
    CircularDependency(SignalId),

    /// Circular dependency detected in computed signals
    #[error("Circular dependency detected in Computed({0:?}). Computed signals cannot form dependency cycles.")]
    ComputedCircularDependency(ComputedId),

    /// Maximum subscribers exceeded
    #[error("Maximum number of subscribers ({max}) exceeded for signal {signal_id:?}")]
    TooManySubscribers { signal_id: SignalId, max: usize },

    /// Maximum pending notifications exceeded
    #[error("Maximum number of pending notifications ({max}) exceeded in batch mode")]
    TooManyPendingNotifications { max: usize },

    /// Deadlock detected (lock acquisition timeout)
    #[error("Potential deadlock detected in {resource}: failed to acquire lock within {timeout_secs} seconds. This likely indicates circular dependencies across threads.")]
    DeadlockDetected { resource: String, timeout_secs: u64 },
}

/// Errors related to hook operations.
#[derive(Error, Debug, Clone)]
pub enum HookError {
    /// Hook called in wrong order
    #[error("Hook order violation: hooks must be called in the same order every render")]
    OrderViolation,

    /// Hook called conditionally
    #[error("Conditional hook call detected: hooks cannot be called inside conditionals")]
    ConditionalCall,

    /// Hook called in loop with variable iterations
    #[error("Hook called in loop with variable iterations")]
    VariableLoopCall,

    /// Hook state type mismatch
    #[error("Hook state type mismatch at index {index}: expected {expected:?}, got {actual:?}")]
    StateMismatch {
        index: usize,
        expected: TypeId,
        actual: TypeId,
    },

    /// No active component context
    #[error("No active component context: hooks must be called during component rendering")]
    NoActiveComponent,

    /// Component not found
    #[error("Component with ID {0:?} not found")]
    ComponentNotFound(ComponentId),

    /// Hook not found
    #[error("Hook with ID {0:?} not found")]
    HookNotFound(HookId),

    /// Too many hooks for component
    #[error("Too many hooks ({count}) for component {component_id:?} (max: {max})")]
    TooManyHooks {
        component_id: ComponentId,
        count: usize,
        max: usize,
    },

    /// Hook called outside of render phase
    #[error("Hook called outside of render phase")]
    OutsideRenderPhase,
}

/// Errors related to runtime operations.
#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    /// Runtime already initialized
    #[error("SignalRuntime already initialized")]
    AlreadyInitialized,

    /// Runtime not initialized
    #[error("SignalRuntime not initialized")]
    NotInitialized,

    /// Memory limit exceeded
    #[error("Memory limit exceeded: {current} bytes used, limit is {limit} bytes")]
    MemoryLimitExceeded { current: usize, limit: usize },

    /// Lock acquisition failed
    #[error("Failed to acquire lock for {resource} after {attempts} attempts")]
    LockFailed { resource: String, attempts: usize },

    /// Internal consistency error
    #[error("Internal consistency error: {0}")]
    Inconsistency(String),

    /// Overflow error
    #[error("Counter overflow: {counter_name} reached maximum value")]
    CounterOverflow { counter_name: String },
}

/// Extension trait for Result types to provide additional context.
pub trait ResultExt<T> {
    /// Add context to an error.
    fn context(self, msg: impl Into<String>) -> Result<T>;

    /// Add context to an error using a closure (lazy evaluation).
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;

    /// Convert error to a panic message with context.
    fn expect_reactivity(self, msg: &str) -> T;

    /// Unwrap or log the error.
    fn unwrap_or_log(self) -> Option<T>
    where
        Self: Sized;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: Into<ReactivityError>,
{
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|e| {
            let base_error = e.into();
            // Wrap in Inconsistency for context
            ReactivityError::Runtime(RuntimeError::Inconsistency(format!(
                "{}: {}",
                msg.into(),
                base_error
            )))
        })
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| {
            let base_error = e.into();
            ReactivityError::Runtime(RuntimeError::Inconsistency(format!(
                "{}: {}",
                f(),
                base_error
            )))
        })
    }

    fn expect_reactivity(self, msg: &str) -> T {
        self.unwrap_or_else(|e| panic!("{}: {}", msg, e.into()))
    }

    fn unwrap_or_log(self) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::error!("Reactivity error: {}", e.into());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_error_display() {
        let error = SignalError::NotFound(SignalId::new());
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn test_hook_error_display() {
        let error = HookError::OrderViolation;
        assert!(error.to_string().contains("order"));
    }

    #[test]
    fn test_runtime_error_display() {
        let error = RuntimeError::NotInitialized;
        assert!(error.to_string().contains("not initialized"));
    }

    #[test]
    fn test_error_conversion() {
        let signal_err = SignalError::NotFound(SignalId::new());
        let reactivity_err: ReactivityError = signal_err.into();
        assert!(matches!(reactivity_err, ReactivityError::Signal(_)));
    }

    #[test]
    fn test_result_context() {
        let result: std::result::Result<(), SignalError> =
            Err(SignalError::NotFound(SignalId::new()));

        let with_context = result.context("Failed to get signal");
        assert!(with_context.is_err());
        let err_msg = with_context.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to get signal"));
    }

    #[test]
    fn test_result_with_context() {
        let result: std::result::Result<(), SignalError> =
            Err(SignalError::NotFound(SignalId::new()));

        let with_context = result.with_context(|| format!("Context: signal lookup failed"));
        assert!(with_context.is_err());
        let err_msg = with_context.unwrap_err().to_string();
        assert!(err_msg.contains("Context: signal lookup failed"));
    }

    #[test]
    fn test_type_mismatch_error() {
        let error = SignalError::TypeMismatch {
            signal_id: SignalId::new(),
            expected: TypeId::of::<i32>(),
            actual: TypeId::of::<String>(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Type mismatch"));
        assert!(msg.contains("expected"));
        assert!(msg.contains("got"));
    }

    #[test]
    fn test_hook_state_mismatch() {
        let error = HookError::StateMismatch {
            index: 5,
            expected: TypeId::of::<i32>(),
            actual: TypeId::of::<String>(),
        };
        let msg = error.to_string();
        assert!(msg.contains("index 5"));
        assert!(msg.contains("mismatch"));
    }

    #[test]
    fn test_memory_limit_exceeded() {
        let error = RuntimeError::MemoryLimitExceeded {
            current: 1024 * 1024 * 100, // 100MB
            limit: 1024 * 1024 * 50,    // 50MB
        };
        let msg = error.to_string();
        assert!(msg.contains("Memory limit exceeded"));
        assert!(msg.contains("bytes"));
    }

    #[test]
    fn test_too_many_subscribers() {
        let error = SignalError::TooManySubscribers {
            signal_id: SignalId::new(),
            max: 1000,
        };
        let msg = error.to_string();
        assert!(msg.contains("Maximum number of subscribers"));
        assert!(msg.contains("1000"));
    }

    #[test]
    fn test_too_many_pending_notifications() {
        let error = SignalError::TooManyPendingNotifications { max: 10_000 };
        let msg = error.to_string();
        assert!(msg.contains("Maximum number of pending notifications"));
        assert!(msg.contains("10000"));
    }

    #[test]
    fn test_counter_overflow() {
        let error = RuntimeError::CounterOverflow {
            counter_name: "SignalId".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Counter overflow"));
        assert!(msg.contains("SignalId"));
    }
}
