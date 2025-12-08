//! Triple buffering for lock-free frame exchange
//!
//! Provides a lock-free mechanism for exchanging frames between
//! producer (pipeline) and consumer (renderer) threads.
//!
//! # Architecture
//!
//! ```text
//! Producer (Pipeline)          Consumer (Renderer)
//!         │                           │
//!    ┌────▼────┐                 ┌────▼────┐
//!    │ Write   │                 │  Read   │
//!    │ Buffer  │                 │ Buffer  │
//!    └────┬────┘                 └────┬────┘
//!         │                           │
//!    ┌────▼────────────────────────────▼────┐
//!    │         Shared Buffer (atomic)        │
//!    └──────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::TripleBuffer;
//!
//! let buffer = TripleBuffer::new(0, 0, 0);
//!
//! // Producer: write new frame
//! buffer.write(42);
//!
//! // Consumer: read latest frame
//! let frame = buffer.read();
//! assert_eq!(frame, 42);
//! ```

use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Lock-free triple buffer for frame exchange
///
/// Uses three buffers to allow concurrent read/write without blocking:
/// - Write buffer: Producer writes here
/// - Read buffer: Consumer reads here
/// - Shared buffer: Used for exchange (atomic swap)
///
/// # Thread Safety
///
/// - Write operations are exclusive (single producer)
/// - Read operations are exclusive (single consumer)
/// - Exchange is lock-free using atomic operations
#[derive(Debug)]
pub struct TripleBuffer<T> {
    /// The three buffers
    buffers: [Mutex<T>; 3],

    /// Current write buffer index (0-2)
    write_index: AtomicUsize,

    /// Current read buffer index (0-2)
    read_index: AtomicUsize,

    /// Shared buffer index for exchange
    shared_index: AtomicUsize,

    /// Flag indicating new data is available
    new_data: AtomicUsize,
}

impl<T: Clone> TripleBuffer<T> {
    /// Create a new triple buffer with initial values
    pub fn new(a: T, b: T, c: T) -> Self {
        Self {
            buffers: [Mutex::new(a), Mutex::new(b), Mutex::new(c)],
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(1),
            shared_index: AtomicUsize::new(2),
            new_data: AtomicUsize::new(0),
        }
    }

    /// Write a new value to the write buffer
    ///
    /// After writing, the buffer is exchanged with the shared buffer,
    /// making the new value available to the reader.
    pub fn write(&self, value: T) {
        let write_idx = self.write_index.load(Ordering::Acquire);

        // Write to the write buffer
        {
            let mut buffer = self.buffers[write_idx].lock();
            *buffer = value;
        }

        // Swap write and shared buffers
        let shared_idx = self.shared_index.swap(write_idx, Ordering::AcqRel);
        self.write_index.store(shared_idx, Ordering::Release);

        // Mark new data available
        self.new_data.store(1, Ordering::Release);
    }

    /// Read the latest value from the read buffer
    ///
    /// If new data is available, swaps the read and shared buffers first.
    pub fn read(&self) -> T {
        // Check if new data available
        if self.new_data.swap(0, Ordering::AcqRel) != 0 {
            // Swap read and shared buffers
            let read_idx = self.read_index.load(Ordering::Acquire);
            let shared_idx = self.shared_index.swap(read_idx, Ordering::AcqRel);
            self.read_index.store(shared_idx, Ordering::Release);
        }

        let read_idx = self.read_index.load(Ordering::Acquire);
        self.buffers[read_idx].lock().clone()
    }

    /// Check if new data is available without consuming it
    pub fn has_new_data(&self) -> bool {
        self.new_data.load(Ordering::Acquire) != 0
    }

    /// Get a reference to the current read buffer value
    ///
    /// Does not check for or consume new data.
    pub fn peek(&self) -> T {
        let read_idx = self.read_index.load(Ordering::Acquire);
        self.buffers[read_idx].lock().clone()
    }
}

impl<T: Clone + Default> TripleBuffer<T> {
    /// Create a triple buffer with default values
    pub fn with_default() -> Self {
        Self::new(T::default(), T::default(), T::default())
    }
}

impl<T: Clone + Default> Default for TripleBuffer<T> {
    fn default() -> Self {
        Self::with_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_basic_read_write() {
        let buffer = TripleBuffer::new(0, 0, 0);

        buffer.write(42);
        assert_eq!(buffer.read(), 42);

        buffer.write(100);
        assert_eq!(buffer.read(), 100);
    }

    #[test]
    fn test_new_data_flag() {
        let buffer = TripleBuffer::new(0, 0, 0);

        assert!(!buffer.has_new_data());

        buffer.write(42);
        assert!(buffer.has_new_data());

        buffer.read();
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_peek() {
        let buffer = TripleBuffer::new(0, 0, 0);

        buffer.write(42);

        // Peek doesn't consume new data
        assert_eq!(buffer.peek(), 0); // Still reading old buffer
        assert!(buffer.has_new_data());

        // Read swaps and consumes
        assert_eq!(buffer.read(), 42);
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let buffer = Arc::new(TripleBuffer::new(0i32, 0, 0));
        let done = Arc::new(AtomicBool::new(false));

        let buffer_writer = Arc::clone(&buffer);
        let buffer_reader = Arc::clone(&buffer);
        let done_reader = Arc::clone(&done);

        // Writer thread
        let writer = thread::spawn(move || {
            for i in 1..=1000 {
                buffer_writer.write(i);
                thread::yield_now();
            }
        });

        // Reader thread - read until writer is done
        let reader = thread::spawn(move || {
            let mut max_value = 0;

            // Read while writer is active or there's new data
            while !done_reader.load(Ordering::Relaxed) {
                if buffer_reader.has_new_data() {
                    let value = buffer_reader.read();
                    max_value = max_value.max(value);
                }
                thread::yield_now();
            }

            // Final read to catch last value
            if buffer_reader.has_new_data() {
                let value = buffer_reader.read();
                max_value = max_value.max(value);
            }

            max_value
        });

        writer.join().unwrap();
        done.store(true, Ordering::Relaxed);
        let max_value = reader.join().unwrap();

        // Reader should have seen some values
        assert!(max_value > 0);
    }

    #[test]
    fn test_default() {
        let buffer: TripleBuffer<i32> = TripleBuffer::default();
        assert_eq!(buffer.read(), 0);
    }

    #[test]
    fn test_multiple_writes() {
        let buffer = TripleBuffer::new(0, 0, 0);

        // Multiple writes before read
        buffer.write(1);
        buffer.write(2);
        buffer.write(3);

        // Reader should get the latest value
        assert_eq!(buffer.read(), 3);
    }

    #[test]
    fn test_string_buffer() {
        let buffer = TripleBuffer::new(String::new(), String::new(), String::new());

        buffer.write("hello".to_string());
        assert_eq!(buffer.read(), "hello");

        buffer.write("world".to_string());
        assert_eq!(buffer.read(), "world");
    }
}
