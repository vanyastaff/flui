//! Change tracking for undo/redo and dirty state

use ahash::AHasher;
use instant::Instant;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

/// Tracks changes and provides undo/redo capability
pub struct ChangeTracker {
    /// Hash of last known value
    last_value_hash: u64,
    /// Whether value has changed
    is_dirty: bool,
    /// History of snapshots
    history: VecDeque<Snapshot>,
    /// Maximum history size
    max_history: usize,
    /// Current position in history (-1 means at latest)
    history_position: Option<usize>,
    /// Change listeners
    listeners: Vec<Box<dyn Fn(&ChangeEvent)>>,
}

/// A snapshot of state at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Serialized state
    pub data: serde_json::Value,
    /// When snapshot was taken
    #[serde(skip)]
    pub timestamp: Option<Instant>,
}

/// Change event
#[derive(Debug, Clone)]
pub enum ChangeEvent {
    /// Value changed
    ValueChanged {
        old_hash: u64,
        new_hash: u64,
    },
    /// Reverted to previous state
    Reverted,
    /// Reset to initial state
    Reset,
    /// Snapshot saved
    SnapshotSaved,
}

impl Default for ChangeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeTracker {
    /// Create new change tracker
    pub fn new() -> Self {
        Self::with_max_history(10)
    }

    /// Create with custom history size
    pub fn with_max_history(max_history: usize) -> Self {
        Self {
            last_value_hash: 0,
            is_dirty: false,
            history: VecDeque::with_capacity(max_history),
            max_history,
            history_position: None,
            listeners: Vec::new(),
        }
    }

    /// Check if value changed and update hash
    pub fn check_changes<T: Hash>(&mut self, value: &T) -> bool {
        let mut hasher = AHasher::default();
        value.hash(&mut hasher);
        let current_hash = hasher.finish();

        if current_hash != self.last_value_hash {
            let old_hash = self.last_value_hash;
            self.last_value_hash = current_hash;
            self.is_dirty = true;

            self.notify(ChangeEvent::ValueChanged {
                old_hash,
                new_hash: current_hash,
            });

            true
        } else {
            false
        }
    }

    /// Save a snapshot
    pub fn save_snapshot(&mut self, data: serde_json::Value) {
        // If we're not at the end of history, remove everything after current position
        if let Some(pos) = self.history_position {
            self.history.truncate(pos + 1);
        }

        // Add new snapshot
        self.history.push_back(Snapshot {
            data,
            timestamp: Some(Instant::now()),
        });

        // Limit history size
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }

        // Reset position to latest
        self.history_position = None;

        self.notify(ChangeEvent::SnapshotSaved);
    }

    /// Undo to previous snapshot
    pub fn undo(&mut self) -> Option<Snapshot> {
        let current_pos = self.history_position.unwrap_or(self.history.len().saturating_sub(1));

        if current_pos > 0 {
            self.history_position = Some(current_pos - 1);
            self.is_dirty = true;
            self.notify(ChangeEvent::Reverted);
            self.history.get(current_pos - 1).cloned()
        } else {
            None
        }
    }

    /// Redo to next snapshot
    pub fn redo(&mut self) -> Option<Snapshot> {
        if let Some(pos) = self.history_position {
            if pos + 1 < self.history.len() {
                self.history_position = Some(pos + 1);
                self.is_dirty = true;
                self.notify(ChangeEvent::Reverted);
                return self.history.get(pos + 1).cloned();
            }
        }
        None
    }

    /// Check if can undo
    pub fn can_undo(&self) -> bool {
        let current_pos = self.history_position.unwrap_or(self.history.len().saturating_sub(1));
        current_pos > 0
    }

    /// Check if can redo
    pub fn can_redo(&self) -> bool {
        if let Some(pos) = self.history_position {
            pos + 1 < self.history.len()
        } else {
            false
        }
    }

    /// Check if dirty
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// Mark as clean
    pub fn mark_clean(&mut self) {
        self.is_dirty = false;
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_position = None;
        self.notify(ChangeEvent::Reset);
    }

    /// Get history size
    pub fn history_size(&self) -> usize {
        self.history.len()
    }

    /// Add change listener
    pub fn add_listener(&mut self, listener: impl Fn(&ChangeEvent) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    /// Notify listeners
    fn notify(&self, event: ChangeEvent) {
        for listener in &self.listeners {
            listener(&event);
        }
    }
}

impl Snapshot {
    /// Create new snapshot
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            data,
            timestamp: Some(Instant::now()),
        }
    }
}