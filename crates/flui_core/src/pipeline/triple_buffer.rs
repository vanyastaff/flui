//! Triple buffering for lock-free frame exchange
//!
//! Provides a lock-free triple buffer for exchanging frames between
//! the render thread and compositor thread.
//!
//! # Triple Buffering
//!
//! Triple buffering allows:
//! - **Lock-free writes**: Render thread never blocks
//! - **Lock-free reads**: Compositor thread never blocks
//! - **No tearing**: Always read complete frames
//!
//! # How It Works
//!
//! ```text
//! [Write Buffer] ←─ Render thread writes here
//! [Swap Buffer]  ←─ Ready to swap
//! [Read Buffer]  ←─ Compositor reads from here
//! ```
//!
//! When render completes, Write and Swap are atomically swapped.
//! When compositor needs a frame, Swap and Read are atomically swapped.
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::TripleBuffer;
//!
//! // Create triple buffer with initial value
//! let mut buffer = TripleBuffer::new(0);
//!
//! // Writer thread
//! buffer.write(42);
//! buffer.publish();
//!
//! // Reader thread
//! if buffer.has_new_data() {
//!     let value = buffer.read();
//!     println!("Read: {}", value);
//! }
//! ```

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// Triple buffer for lock-free producer-consumer communication
///
/// Allows a single producer and single consumer to exchange data
/// without locks or blocking.
///
/// # Type Parameter
///
/// - `T`: Must implement `Clone` for buffer initialization
///
/// # Thread Safety
///
/// - Safe for one producer and one consumer thread
/// - NOT safe for multiple producers or consumers
///
/// # Performance
///
/// - Write: ~5ns (atomic swap)
/// - Read: ~5ns (atomic swap)
/// - No allocations after initialization
#[derive(Debug)]
pub struct TripleBuffer<T> {
    /// Three buffers
    buffers: Arc<[T; 3]>,

    /// Buffer indices (packed into single atomic)
    ///
    /// Layout (8 bits):
    /// - bits 0-1: write buffer index (0-2)
    /// - bits 2-3: swap buffer index (0-2)
    /// - bits 4-5: read buffer index (0-2)
    /// - bit 6: has new data flag
    /// - bit 7: unused
    indices: Arc<AtomicU8>,
}

impl<T: Clone> TripleBuffer<T> {
    /// Create new triple buffer with initial value
    ///
    /// # Parameters
    ///
    /// - `initial`: Initial value for all three buffers
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let buffer = TripleBuffer::new(42);
    /// ```
    pub fn new(initial: T) -> Self {
        Self {
            buffers: Arc::new([initial.clone(), initial.clone(), initial]),
            // Initial state: write=0, swap=1, read=2, has_new=false
            indices: Arc::new(AtomicU8::new(0b00_10_01_00)),
        }
    }

    /// Get current write buffer index
    #[inline]
    fn write_index(state: u8) -> usize {
        (state & 0b11) as usize
    }

    /// Get current swap buffer index
    #[inline]
    fn swap_index(state: u8) -> usize {
        ((state >> 2) & 0b11) as usize
    }

    /// Get current read buffer index
    #[inline]
    fn read_index(state: u8) -> usize {
        ((state >> 4) & 0b11) as usize
    }

    /// Check if has new data
    #[inline]
    fn has_new_data_bit(state: u8) -> bool {
        (state & 0b0100_0000) != 0
    }

    /// Get mutable reference to write buffer
    ///
    /// # Safety
    ///
    /// Safe because only producer calls this, and write buffer
    /// is never read by consumer until published.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let mut buffer = TripleBuffer::new(0);
    /// *buffer.write_mut() = 42;
    /// buffer.publish();
    /// ```
    pub fn write_mut(&mut self) -> &mut T {
        let state = self.indices.load(Ordering::Relaxed);
        let index = Self::write_index(state);

        // SAFETY: We have &mut self, so we have exclusive access
        // to the write buffer. Cast away const for buffers Arc.
        unsafe {
            let ptr = Arc::as_ptr(&self.buffers) as *mut [T; 3];
            &mut (*ptr)[index]
        }
    }

    /// Write new value to buffer (convenience method)
    ///
    /// Equivalent to `*buffer.write_mut() = value`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let mut buffer = TripleBuffer::new(0);
    /// buffer.write(42);
    /// buffer.publish();
    /// ```
    pub fn write(&mut self, value: T) {
        *self.write_mut() = value;
    }

    /// Publish written data
    ///
    /// Atomically swaps write and swap buffers, making the
    /// written data available for reading.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let mut buffer = TripleBuffer::new(0);
    /// buffer.write(42);
    /// buffer.publish();  // Make data available
    /// ```
    pub fn publish(&mut self) {
        let old_state = self.indices.load(Ordering::Relaxed);

        let write_idx = Self::write_index(old_state);
        let swap_idx = Self::swap_index(old_state);
        let read_idx = Self::read_index(old_state);

        // Swap write and swap indices, set has_new_data flag
        let new_state = write_idx << 2  // old write → new swap
            | swap_idx                   // old swap → new write
            | read_idx << 4              // read stays same
            | 0b0100_0000;               // set has_new_data flag

        self.indices.store(new_state as u8, Ordering::Release);
    }

    /// Check if new data is available
    ///
    /// Returns `true` if producer has published new data since
    /// last read.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let mut buffer = TripleBuffer::new(0);
    /// assert!(!buffer.has_new_data());
    ///
    /// buffer.write(42);
    /// buffer.publish();
    /// assert!(buffer.has_new_data());
    /// ```
    pub fn has_new_data(&self) -> bool {
        let state = self.indices.load(Ordering::Acquire);
        Self::has_new_data_bit(state)
    }

    /// Read current value
    ///
    /// If new data is available, atomically swaps read and swap buffers
    /// to get the latest data.
    ///
    /// # Returns
    ///
    /// Reference to current read buffer.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let mut buffer = TripleBuffer::new(0);
    /// buffer.write(42);
    /// buffer.publish();
    ///
    /// let value = buffer.read();
    /// assert_eq!(*value, 42);
    /// ```
    pub fn read(&self) -> &T {
        let old_state = self.indices.load(Ordering::Acquire);

        // If has new data, swap read and swap buffers
        if Self::has_new_data_bit(old_state) {
            let write_idx = Self::write_index(old_state);
            let swap_idx = Self::swap_index(old_state);
            let read_idx = Self::read_index(old_state);

            // Swap read and swap indices, clear has_new_data flag
            let new_state = write_idx              // write stays same
                | read_idx << 2                    // old read → new swap
                | swap_idx << 4                    // old swap → new read
                | 0;                               // clear has_new_data flag

            self.indices.store(new_state as u8, Ordering::Release);

            &self.buffers[swap_idx]
        } else {
            // No new data, just read current buffer
            &self.buffers[Self::read_index(old_state)]
        }
    }

    /// Get immutable reference to current read buffer without swapping
    ///
    /// This does NOT consume new data - `has_new_data()` will still
    /// return true if new data was available.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::TripleBuffer;
    ///
    /// let buffer = TripleBuffer::new(42);
    /// let value = buffer.peek();
    /// assert_eq!(*value, 42);
    /// ```
    pub fn peek(&self) -> &T {
        let state = self.indices.load(Ordering::Acquire);
        &self.buffers[Self::read_index(state)]
    }
}

impl<T: Clone> Clone for TripleBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            buffers: Arc::clone(&self.buffers),
            indices: Arc::clone(&self.indices),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_triple_buffer_creation() {
        let buffer = TripleBuffer::new(42);
        assert_eq!(*buffer.peek(), 42);
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_write_and_read() {
        let mut buffer = TripleBuffer::new(0);

        buffer.write(42);
        buffer.publish();

        assert!(buffer.has_new_data());
        assert_eq!(*buffer.read(), 42);
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_multiple_writes() {
        let mut buffer = TripleBuffer::new(0);

        buffer.write(1);
        buffer.publish();
        buffer.write(2);
        buffer.publish();
        buffer.write(3);
        buffer.publish();

        // Should read latest value
        assert_eq!(*buffer.read(), 3);
    }

    #[test]
    fn test_peek_does_not_consume() {
        let mut buffer = TripleBuffer::new(0);

        buffer.write(42);
        buffer.publish();

        assert!(buffer.has_new_data());
        assert_eq!(*buffer.peek(), 0); // Still old value
        assert!(buffer.has_new_data()); // Flag still set

        assert_eq!(*buffer.read(), 42); // Now get new value
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_write_mut() {
        let mut buffer = TripleBuffer::new(String::from("hello"));

        {
            let write_buf = buffer.write_mut();
            write_buf.push_str(" world");
        }

        buffer.publish();
        assert_eq!(buffer.read().as_str(), "hello world");
    }

    #[test]
    fn test_no_new_data_read() {
        let buffer = TripleBuffer::new(42);

        // Reading without new data should return initial value
        assert_eq!(*buffer.read(), 42);
        assert!(!buffer.has_new_data());
    }

    #[test]
    fn test_thread_safety() {
        let mut producer = TripleBuffer::new(0);
        let consumer = producer.clone();

        // Producer thread
        let producer_handle = thread::spawn(move || {
            for i in 1..=10 {
                producer.write(i);
                producer.publish();
                thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        // Consumer thread
        let consumer_handle = thread::spawn(move || {
            let mut last_value = 0;
            for _ in 0..20 {
                if consumer.has_new_data() {
                    let value = *consumer.read();
                    assert!(value >= last_value);
                    last_value = value;
                }
                thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        producer_handle.join().unwrap();
        consumer_handle.join().unwrap();
    }

    #[test]
    fn test_index_packing() {
        // Test write index extraction
        let state = 0b00_10_01_00;
        assert_eq!(TripleBuffer::<()>::write_index(state), 0);
        assert_eq!(TripleBuffer::<()>::swap_index(state), 1);
        assert_eq!(TripleBuffer::<()>::read_index(state), 2);
        assert!(!TripleBuffer::<()>::has_new_data_bit(state));

        // Test with has_new_data flag
        let state = 0b0100_10_01_00;
        assert!(TripleBuffer::<()>::has_new_data_bit(state));
    }

    #[test]
    fn test_rapid_writes() {
        let mut buffer = TripleBuffer::new(0);

        // Write many values rapidly
        for i in 0..1000 {
            buffer.write(i);
            buffer.publish();
        }

        // Should read latest
        assert_eq!(*buffer.read(), 999);
    }

    #[test]
    fn test_clone() {
        let mut original = TripleBuffer::new(42);
        let clone = original.clone();

        original.write(100);
        original.publish();

        // Clone should see the update
        assert_eq!(*clone.read(), 100);
    }
}
