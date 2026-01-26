//! Android page-aligned memory allocator for 16KB page size support
//!
//! This module provides page-aligned memory allocation to support Android devices
//! with 16KB page sizes (Pixel 9, Galaxy S25, etc.). This is required for Play Store
//! compliance with API 35+ (Android 16).
//!
//! # Background
//!
//! Traditional Android devices use 4KB page sizes, but newer flagship devices
//! (starting with Pixel 9 in Sept 2024) use 16KB page sizes for better performance.
//! Vulkan buffer allocations must be aligned to the system page size, or the app
//! will crash with SIGBUS errors.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::android::memory::{PageAlignedVec, get_page_size};
//!
//! // Create page-aligned buffer for GPU
//! let mut buffer = PageAlignedVec::<u8>::with_capacity(8192);
//! assert_eq!(buffer.as_ptr() as usize % get_page_size(), 0);
//! ```

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

// ============================================================================
// Page Size Detection
// ============================================================================

/// Get system page size at runtime.
///
/// Returns the actual page size configured by the kernel:
/// - 4096 bytes on traditional Android devices
/// - 16384 bytes on Pixel 9, Galaxy S25, and newer flagship devices
///
/// # Platform Support
///
/// - Android: Queries `sysconf(_SC_PAGESIZE)`
/// - Other platforms: Returns 4096 as default
pub fn get_page_size() -> usize {
    #[cfg(target_os = "android")]
    {
        // SAFETY: sysconf is a standard POSIX function
        // _SC_PAGESIZE always returns a valid value
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }

    #[cfg(not(target_os = "android"))]
    {
        4096 // Default 4KB pages for non-Android platforms
    }
}

/// Check if the current device uses 16KB page size.
///
/// This is useful for logging and analytics to understand device distribution.
pub fn is_16kb_page_size() -> bool {
    get_page_size() == 16384
}

// ============================================================================
// Low-Level Page-Aligned Allocation
// ============================================================================

/// Allocate page-aligned memory.
///
/// This function allocates memory aligned to the system page size,
/// ensuring compatibility with Vulkan buffer requirements.
///
/// # Parameters
///
/// - `size`: Number of bytes to allocate (will be rounded up to page boundary)
///
/// # Returns
///
/// - `Ok(NonNull<u8>)`: Pointer to page-aligned memory
/// - `Err(AllocError)`: Allocation failed (out of memory)
///
/// # Safety
///
/// The returned pointer must be deallocated with `dealloc_page_aligned`
/// using the same size parameter.
///
/// # Example
///
/// ```rust,ignore
/// let ptr = alloc_page_aligned(8192)?;
/// // Use memory...
/// unsafe { dealloc_page_aligned(ptr, 8192); }
/// ```
pub fn alloc_page_aligned(size: usize) -> Result<NonNull<u8>, std::alloc::AllocError> {
    let page_size = get_page_size();

    // Round up to page boundary
    let aligned_size = (size + page_size - 1) & !(page_size - 1);

    // Create aligned layout
    let layout =
        Layout::from_size_align(aligned_size, page_size).map_err(|_| std::alloc::AllocError)?;

    // Allocate aligned memory
    // SAFETY: Layout is valid (verified above)
    let ptr = unsafe { alloc(layout) };

    NonNull::new(ptr).ok_or(std::alloc::AllocError)
}

/// Deallocate page-aligned memory.
///
/// # Safety
///
/// - `ptr` must have been allocated with `alloc_page_aligned`
/// - `size` must match the size passed to `alloc_page_aligned`
/// - `ptr` must not be used after calling this function
pub unsafe fn dealloc_page_aligned(ptr: NonNull<u8>, size: usize) {
    let page_size = get_page_size();
    let aligned_size = (size + page_size - 1) & !(page_size - 1);
    let layout = Layout::from_size_align_unchecked(aligned_size, page_size);
    dealloc(ptr.as_ptr(), layout);
}

// ============================================================================
// Page-Aligned Vector
// ============================================================================

/// Page-aligned vector for GPU buffers.
///
/// This is a Vec-like container that guarantees page alignment,
/// suitable for use with Vulkan buffers on 16KB page size devices.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::android::memory::PageAlignedVec;
///
/// // Create buffer for 1024 vertices
/// let mut vertices = PageAlignedVec::<Vertex>::with_capacity(1024);
///
/// // Use like a normal slice
/// unsafe {
///     std::ptr::copy_nonoverlapping(
///         vertex_data.as_ptr(),
///         vertices.as_mut_ptr(),
///         1024,
///     );
/// }
///
/// // Pass to Vulkan
/// let buffer = device.create_buffer_init(&BufferInitDescriptor {
///     contents: vertices.as_slice(),
///     usage: BufferUsages::VERTEX,
/// });
/// ```
pub struct PageAlignedVec<T> {
    ptr: NonNull<T>,
    len: usize,
    capacity: usize,
}

impl<T> PageAlignedVec<T> {
    /// Create a new page-aligned vector with the given capacity.
    ///
    /// The actual allocated capacity will be rounded up to the nearest
    /// page boundary.
    ///
    /// # Panics
    ///
    /// Panics if allocation fails (out of memory).
    pub fn with_capacity(capacity: usize) -> Self {
        let page_size = get_page_size();
        let byte_capacity = capacity * std::mem::size_of::<T>();
        let aligned_capacity = (byte_capacity + page_size - 1) & !(page_size - 1);

        let layout = Layout::from_size_align(aligned_capacity, page_size)
            .expect("Invalid layout for page-aligned allocation");

        // SAFETY: Layout is valid (verified above)
        let ptr = unsafe { alloc(layout) as *mut T };
        let ptr = NonNull::new(ptr).expect("Allocation failed");

        Self {
            ptr,
            len: 0,
            capacity: aligned_capacity / std::mem::size_of::<T>(),
        }
    }

    /// Create an empty page-aligned vector.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Get a raw pointer to the buffer.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }

    /// Get a mutable raw pointer to the buffer.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Get a slice view of the initialized elements.
    ///
    /// # Safety
    ///
    /// Only the first `len` elements are guaranteed to be initialized.
    #[inline]
    pub unsafe fn as_slice(&self) -> &[T] {
        std::slice::from_raw_parts(self.ptr.as_ptr(), self.len)
    }

    /// Get a mutable slice view of the initialized elements.
    ///
    /// # Safety
    ///
    /// Only the first `len` elements are guaranteed to be initialized.
    #[inline]
    pub unsafe fn as_mut_slice(&mut self) -> &mut [T] {
        std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len)
    }

    /// Get the number of initialized elements.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Get the allocated capacity (in elements).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if the vector is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Set the length of initialized elements.
    ///
    /// # Safety
    ///
    /// - `new_len` must be <= capacity
    /// - All elements 0..new_len must be properly initialized
    #[inline]
    pub unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.capacity);
        self.len = new_len;
    }

    /// Push an element to the end of the vector.
    ///
    /// # Panics
    ///
    /// Panics if capacity is exceeded (no automatic reallocation).
    pub fn push(&mut self, value: T) {
        assert!(self.len < self.capacity, "PageAlignedVec capacity exceeded");

        // SAFETY: len < capacity, so this is valid
        unsafe {
            self.ptr.as_ptr().add(self.len).write(value);
            self.len += 1;
        }
    }

    /// Clear all elements without deallocating.
    pub fn clear(&mut self) {
        // SAFETY: Dropping initialized elements
        unsafe {
            std::ptr::drop_in_place(std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len));
        }
        self.len = 0;
    }

    /// Get the byte size of the allocation.
    pub fn byte_size(&self) -> usize {
        self.capacity * std::mem::size_of::<T>()
    }

    /// Verify that the allocation is page-aligned.
    ///
    /// This is useful for debugging and testing.
    pub fn is_page_aligned(&self) -> bool {
        let page_size = get_page_size();
        (self.ptr.as_ptr() as usize) % page_size == 0
    }
}

impl<T> Default for PageAlignedVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for PageAlignedVec<T> {
    fn drop(&mut self) {
        // Drop all initialized elements
        self.clear();

        // Deallocate memory
        let page_size = get_page_size();
        let byte_capacity = self.capacity * std::mem::size_of::<T>();
        let layout = Layout::from_size_align(byte_capacity, page_size).expect("Invalid layout");

        // SAFETY: ptr was allocated with the same layout
        unsafe {
            dealloc(self.ptr.as_ptr() as *mut u8, layout);
        }
    }
}

// SAFETY: PageAlignedVec can be sent between threads if T is Send
unsafe impl<T: Send> Send for PageAlignedVec<T> {}

// SAFETY: PageAlignedVec can be shared between threads if T is Sync
unsafe impl<T: Sync> Sync for PageAlignedVec<T> {}

// ============================================================================
// Buffer Size Alignment
// ============================================================================

/// Round a size up to the nearest page boundary.
///
/// This is useful for ensuring Vulkan buffer sizes are page-aligned.
///
/// # Example
///
/// ```rust,ignore
/// let size = 12345;
/// let aligned = align_to_page_size(size);
/// assert_eq!(aligned % get_page_size(), 0);
/// assert!(aligned >= size);
/// ```
#[inline]
pub fn align_to_page_size(size: usize) -> usize {
    let page_size = get_page_size();
    (size + page_size - 1) & !(page_size - 1)
}

/// Round a size up to the nearest page boundary (u64 version).
#[inline]
pub fn align_to_page_size_u64(size: u64) -> u64 {
    let page_size = get_page_size() as u64;
    (size + page_size - 1) & !(page_size - 1)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_size_detection() {
        let page_size = get_page_size();
        println!("Detected page size: {} bytes", page_size);

        // Page size must be at least 4KB
        assert!(page_size >= 4096);

        // Page size must not exceed 64KB (reasonable upper bound)
        assert!(page_size <= 65536);

        // Page size must be power of 2
        assert!(page_size.is_power_of_two());

        // Common page sizes: 4KB, 8KB, 16KB, 64KB
        assert!(matches!(page_size, 4096 | 8192 | 16384 | 65536));
    }

    #[test]
    fn test_16kb_detection() {
        let is_16kb = is_16kb_page_size();
        let page_size = get_page_size();
        assert_eq!(is_16kb, page_size == 16384);
    }

    #[test]
    fn test_page_aligned_alloc() {
        let ptr = alloc_page_aligned(8192).expect("Allocation failed");
        let page_size = get_page_size();

        // Verify alignment
        assert_eq!(ptr.as_ptr() as usize % page_size, 0);

        // Verify we can write to the memory
        unsafe {
            std::ptr::write_bytes(ptr.as_ptr(), 0xAA, page_size);
        }

        unsafe {
            dealloc_page_aligned(ptr, 8192);
        }
    }

    #[test]
    fn test_page_aligned_vec_empty() {
        let vec = PageAlignedVec::<u8>::new();
        assert_eq!(vec.len(), 0);
        assert!(vec.is_empty());
    }

    #[test]
    fn test_page_aligned_vec_alignment() {
        let vec = PageAlignedVec::<u8>::with_capacity(1024);
        let page_size = get_page_size();

        // Verify alignment
        assert_eq!(vec.as_ptr() as usize % page_size, 0);
        assert!(vec.is_page_aligned());

        // Verify capacity is at least what we requested
        assert!(vec.capacity() >= 1024);

        // Verify capacity is page-aligned
        assert!(vec.byte_size() >= page_size);
        assert_eq!(vec.byte_size() % page_size, 0);
    }

    #[test]
    fn test_page_aligned_vec_push() {
        let mut vec = PageAlignedVec::<u32>::with_capacity(100);

        for i in 0..100 {
            vec.push(i);
        }

        assert_eq!(vec.len(), 100);

        unsafe {
            let slice = vec.as_slice();
            for i in 0..100 {
                assert_eq!(slice[i], i);
            }
        }
    }

    #[test]
    fn test_page_aligned_vec_clear() {
        let mut vec = PageAlignedVec::<u32>::with_capacity(100);

        vec.push(1);
        vec.push(2);
        vec.push(3);
        assert_eq!(vec.len(), 3);

        vec.clear();
        assert_eq!(vec.len(), 0);
        assert!(vec.is_empty());
    }

    #[test]
    fn test_align_to_page_size() {
        let page_size = get_page_size();

        // Test various sizes
        let test_cases = vec![
            (0, page_size),
            (1, page_size),
            (page_size - 1, page_size),
            (page_size, page_size),
            (page_size + 1, page_size * 2),
            (page_size * 2, page_size * 2),
            (12345, ((12345 + page_size - 1) / page_size) * page_size),
        ];

        for (input, expected) in test_cases {
            let aligned = align_to_page_size(input);
            assert_eq!(aligned, expected);
            assert_eq!(aligned % page_size, 0);
            assert!(aligned >= input);
        }
    }

    #[test]
    fn test_align_to_page_size_u64() {
        let page_size = get_page_size() as u64;

        let aligned = align_to_page_size_u64(12345);
        assert_eq!(aligned % page_size, 0);
        assert!(aligned >= 12345);
    }

    #[test]
    #[should_panic(expected = "capacity exceeded")]
    fn test_page_aligned_vec_push_overflow() {
        let mut vec = PageAlignedVec::<u8>::with_capacity(10);

        for _ in 0..11 {
            vec.push(0);
        }
    }

    #[test]
    fn test_page_aligned_vec_large_type() {
        #[derive(Clone, Copy)]
        struct LargeType {
            data: [u8; 256],
        }

        let vec = PageAlignedVec::<LargeType>::with_capacity(100);
        assert!(vec.is_page_aligned());
        assert!(vec.capacity() >= 100);
    }
}
