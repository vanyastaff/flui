//! Overlay manager - manages the overlay stack.

use std::collections::HashMap;

use super::entry::{OverlayEntry, OverlayId, OverlayPriority};

/// Manages a stack of overlay entries.
///
/// Overlays are sorted by priority, with higher priority overlays
/// rendered on top of lower priority ones.
#[derive(Debug)]
pub struct OverlayManager {
    entries: HashMap<OverlayId, OverlayEntry>,
    /// Sorted list of overlay IDs by priority (lowest to highest).
    order: Vec<OverlayId>,
}

impl OverlayManager {
    /// Create a new empty overlay manager.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Insert an overlay entry, returning its ID.
    pub fn insert(&mut self, entry: OverlayEntry) -> OverlayId {
        let id = entry.id();
        let priority = entry.priority();
        self.entries.insert(id, entry);
        self.insert_sorted(id, priority);
        id
    }

    /// Remove an overlay by ID.
    pub fn remove(&mut self, id: OverlayId) -> Option<OverlayEntry> {
        if let Some(entry) = self.entries.remove(&id) {
            self.order.retain(|&oid| oid != id);
            Some(entry)
        } else {
            None
        }
    }

    /// Get an overlay by ID.
    pub fn get(&self, id: OverlayId) -> Option<&OverlayEntry> {
        self.entries.get(&id)
    }

    /// Get a mutable reference to an overlay by ID.
    pub fn get_mut(&mut self, id: OverlayId) -> Option<&mut OverlayEntry> {
        self.entries.get_mut(&id)
    }

    /// Find overlays by tag.
    pub fn find_by_tag(&self, tag: &str) -> Vec<OverlayId> {
        self.entries
            .iter()
            .filter(|(_, e)| e.tag() == Some(tag))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Remove all overlays with a given tag.
    pub fn remove_by_tag(&mut self, tag: &str) -> Vec<OverlayEntry> {
        let ids: Vec<_> = self.find_by_tag(tag);
        ids.into_iter().filter_map(|id| self.remove(id)).collect()
    }

    /// Get the number of overlays.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if there are no overlays.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if there are any modal overlays.
    pub fn has_modal(&self) -> bool {
        self.entries
            .values()
            .any(|e| e.is_modal() && e.is_visible())
    }

    /// Get the topmost modal overlay, if any.
    pub fn topmost_modal(&self) -> Option<OverlayId> {
        self.order.iter().rev().copied().find(|id| {
            self.entries
                .get(id)
                .map(|e| e.is_modal() && e.is_visible())
                .unwrap_or(false)
        })
    }

    /// Iterate over visible overlays in render order (bottom to top).
    pub fn visible_overlays(&self) -> impl Iterator<Item = &OverlayEntry> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|e| e.is_visible())
    }

    /// Iterate over visible overlay IDs in render order.
    pub fn visible_overlay_ids(&self) -> impl Iterator<Item = OverlayId> + '_ {
        self.order.iter().copied().filter(|id| {
            self.entries
                .get(id)
                .map(|e| e.is_visible())
                .unwrap_or(false)
        })
    }

    /// Hide an overlay.
    pub fn hide(&mut self, id: OverlayId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.set_visible(false);
        }
    }

    /// Show an overlay.
    pub fn show(&mut self, id: OverlayId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.set_visible(true);
        }
    }

    /// Clear all overlays.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Clear all overlays with a specific priority.
    pub fn clear_priority(&mut self, priority: OverlayPriority) {
        let ids: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.priority() == priority)
            .map(|(id, _)| *id)
            .collect();

        for id in ids {
            self.remove(id);
        }
    }

    /// Insert an ID into the sorted order list.
    fn insert_sorted(&mut self, id: OverlayId, priority: OverlayPriority) {
        // Find insertion point - overlays with same priority are ordered by insertion time
        let pos = self.order.partition_point(|&oid| {
            self.entries
                .get(&oid)
                .map(|e| e.priority() <= priority)
                .unwrap_or(false)
        });
        self.order.insert(pos, id);
    }
}

impl Default for OverlayManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::entry::OverlayEntryBuilder;

    #[test]
    fn test_insert_and_remove() {
        let mut manager = OverlayManager::new();

        let entry = OverlayEntry::new();
        let id = manager.insert(entry);

        assert_eq!(manager.len(), 1);
        assert!(manager.get(id).is_some());

        let removed = manager.remove(id);
        assert!(removed.is_some());
        assert!(manager.is_empty());
    }

    #[test]
    fn test_priority_ordering() {
        let mut manager = OverlayManager::new();

        let low = OverlayEntryBuilder::new()
            .priority(OverlayPriority::Normal)
            .build();
        let high = OverlayEntryBuilder::new()
            .priority(OverlayPriority::Modal)
            .build();
        let debug = OverlayEntryBuilder::new()
            .priority(OverlayPriority::Debug)
            .build();

        let low_id = manager.insert(low);
        let high_id = manager.insert(high);
        let debug_id = manager.insert(debug);

        let ids: Vec<_> = manager.visible_overlay_ids().collect();
        assert_eq!(ids, vec![low_id, high_id, debug_id]);
    }

    #[test]
    fn test_find_by_tag() {
        let mut manager = OverlayManager::new();

        let entry1 = OverlayEntryBuilder::new().tag("dialog").build();
        let entry2 = OverlayEntryBuilder::new().tag("tooltip").build();
        let entry3 = OverlayEntryBuilder::new().tag("dialog").build();

        manager.insert(entry1);
        manager.insert(entry2);
        manager.insert(entry3);

        let dialogs = manager.find_by_tag("dialog");
        assert_eq!(dialogs.len(), 2);

        let tooltips = manager.find_by_tag("tooltip");
        assert_eq!(tooltips.len(), 1);
    }

    #[test]
    fn test_modal_detection() {
        let mut manager = OverlayManager::new();

        let normal = OverlayEntry::new();
        manager.insert(normal);
        assert!(!manager.has_modal());

        let modal = OverlayEntryBuilder::new().modal(true).build();
        let modal_id = manager.insert(modal);
        assert!(manager.has_modal());
        assert_eq!(manager.topmost_modal(), Some(modal_id));

        manager.hide(modal_id);
        assert!(!manager.has_modal());
    }
}
