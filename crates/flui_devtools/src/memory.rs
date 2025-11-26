//! Memory profiling for FLUI applications
//!
//! Tracks memory usage over time and provides insights into memory allocation patterns.
//! Useful for detecting memory leaks and optimizing memory usage.
//!
//! # Example
//!
//! ```rust
//! use flui_devtools::memory::MemoryProfiler;
//!
//! let mut profiler = MemoryProfiler::new();
//!
//! // Take snapshots periodically
//! profiler.snapshot();
//!
//! // Get current memory usage
//! let stats = profiler.current_stats();
//! println!("Memory: {:.2} MB", stats.total_mb());
//!
//! // Get memory history
//! let history = profiler.history();
//! for snapshot in history {
//!     println!("Time: {:?}, Memory: {:.2} MB",
//!         snapshot.timestamp, snapshot.total_mb());
//! }
//! ```

use web_time::Instant;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum number of snapshots to keep in history
const DEFAULT_MAX_HISTORY: usize = 300; // 5 minutes at 1 snapshot/second

/// Memory statistics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Timestamp when snapshot was taken
    pub timestamp: u64,

    /// Total allocated memory in bytes (approximation)
    pub allocated_bytes: usize,

    /// Relative time in milliseconds since profiler creation
    pub relative_time_ms: u64,
}

impl MemorySnapshot {
    /// Get allocated memory in megabytes
    pub fn total_mb(&self) -> f64 {
        self.allocated_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get allocated memory in kilobytes
    pub fn total_kb(&self) -> f64 {
        self.allocated_bytes as f64 / 1024.0
    }
}

/// Memory profiler for tracking memory usage
#[derive(Debug)]
pub struct MemoryProfiler {
    /// Start time for relative timestamps
    start_time: Instant,

    /// History of memory snapshots
    history: VecDeque<MemorySnapshot>,

    /// Maximum history size
    max_history: usize,

    /// Total snapshots taken
    total_snapshots: u64,
}

impl MemoryProfiler {
    /// Create a new memory profiler
    pub fn new() -> Self {
        Self::with_max_history(DEFAULT_MAX_HISTORY)
    }

    /// Create a profiler with custom history size
    pub fn with_max_history(max_history: usize) -> Self {
        Self {
            start_time: Instant::now(),
            history: VecDeque::with_capacity(max_history),
            max_history,
            total_snapshots: 0,
        }
    }

    /// Take a memory snapshot
    ///
    /// This captures current memory usage and adds it to history.
    pub fn snapshot(&mut self) {
        let allocated = Self::get_memory_usage();
        let relative_time = self.start_time.elapsed().as_millis() as u64;

        let snapshot = MemorySnapshot {
            timestamp: self.total_snapshots,
            allocated_bytes: allocated,
            relative_time_ms: relative_time,
        };

        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }

        self.history.push_back(snapshot);
        self.total_snapshots += 1;
    }

    /// Get current memory statistics
    pub fn current_stats(&self) -> MemorySnapshot {
        if let Some(latest) = self.history.back() {
            latest.clone()
        } else {
            MemorySnapshot {
                timestamp: 0,
                allocated_bytes: Self::get_memory_usage(),
                relative_time_ms: 0,
            }
        }
    }

    /// Get memory usage history
    pub fn history(&self) -> Vec<MemorySnapshot> {
        self.history.iter().cloned().collect()
    }

    /// Get peak memory usage from history
    pub fn peak_memory(&self) -> Option<MemorySnapshot> {
        self.history
            .iter()
            .max_by_key(|s| s.allocated_bytes)
            .cloned()
    }

    /// Get average memory usage
    pub fn average_memory_mb(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }

        let sum: usize = self.history.iter().map(|s| s.allocated_bytes).sum();

        (sum as f64 / self.history.len() as f64) / (1024.0 * 1024.0)
    }

    /// Detect potential memory leak
    ///
    /// Returns true if memory usage is consistently increasing.
    pub fn is_leaking(&self) -> bool {
        if self.history.len() < 10 {
            return false;
        }

        // Check if memory is trending upward
        let recent = &self.history.iter().rev().take(10).collect::<Vec<_>>();
        if recent.len() < 10 {
            return false;
        }

        // Simple heuristic: check if last 3 samples are all higher than first 3
        let first_avg: usize = recent[7..10]
            .iter()
            .map(|s| s.allocated_bytes)
            .sum::<usize>()
            / 3;
        let last_avg: usize = recent[0..3]
            .iter()
            .map(|s| s.allocated_bytes)
            .sum::<usize>()
            / 3;

        // Consider it a leak if growth is > 20%
        last_avg > first_avg && (last_avg - first_avg) as f64 / first_avg as f64 > 0.2
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.total_snapshots = 0;
        self.start_time = Instant::now();
    }

    /// Get memory usage in bytes (platform-specific approximation)
    ///
    /// This is a best-effort implementation. On some platforms, this might
    /// not be available and will return 0.
    #[cfg(target_os = "linux")]
    fn get_memory_usage() -> usize {
        // On Linux, read from /proc/self/statm
        if let Ok(content) = std::fs::read_to_string("/proc/self/statm") {
            // statm format: size resident shared text lib data dt
            // We want resident set size (RSS) in pages
            if let Some(rss_pages) = content.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages.parse::<usize>() {
                    // Page size is typically 4096 bytes
                    return pages * 4096;
                }
            }
        }
        0
    }

    #[cfg(target_os = "macos")]
    fn get_memory_usage() -> usize {
        // On macOS, we could use task_info but it requires unsafe code
        // For now, return 0 as placeholder
        // TODO: Implement using libc::mach_task_self() and task_info
        0
    }

    #[cfg(target_os = "windows")]
    fn get_memory_usage() -> usize {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        unsafe {
            let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

            let result = GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut pmc,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            );

            if result != 0 {
                // Return working set size (physical memory in use)
                pmc.WorkingSetSize
            } else {
                0
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn get_memory_usage() -> usize {
        0
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_profiler_creation() {
        let profiler = MemoryProfiler::new();
        assert_eq!(profiler.history().len(), 0);
    }

    #[test]
    fn test_snapshot() {
        let mut profiler = MemoryProfiler::new();
        profiler.snapshot();
        assert_eq!(profiler.history().len(), 1);

        profiler.snapshot();
        assert_eq!(profiler.history().len(), 2);
    }

    #[test]
    fn test_history_limit() {
        let mut profiler = MemoryProfiler::with_max_history(3);

        for _ in 0..5 {
            profiler.snapshot();
        }

        assert_eq!(profiler.history().len(), 3);
    }

    #[test]
    fn test_current_stats() {
        let mut profiler = MemoryProfiler::new();
        profiler.snapshot();

        let stats = profiler.current_stats();
        assert!(stats.total_mb() >= 0.0);
    }
}
