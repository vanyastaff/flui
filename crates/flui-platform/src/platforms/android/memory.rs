//! Android page-aligned memory allocator for 16KB page size support
//!
//! This module provides page-aligned memory allocation to support Android
//! devices with 16KB page sizes (Pixel 9, Galaxy S25, etc.). This is required
//! for Play Store compliance with API 35+ (Android 16).
//!
//! # Background
//!
//! Traditional Android devices use 4KB page sizes, but newer flagship devices
//! (starting with Pixel 9 in Sept 2024) use 16KB page sizes for better
//! performance. Vulkan buffer allocations must be aligned to the system page
//! size, or the app will crash with SIGBUS errors.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::platforms::android::memory::{PageAlignedVec, get_page_size};
//!
//! // Create page-aligned buffer for GPU
//! let mut buffer = PageAlignedVec::<u8>::with_capacity(8192);
//! assert_eq!(buffer.as_ptr() as usize % get_page_size(), 0);
//! ```

use std::{
    alloc::{Layout, alloc, dealloc},
    fmt,
    ptr::NonNull,
};

/// Error returned when page-aligned allocation fails.
#[derive(Debug, Clone, Copy)]
pub struct PageAllocError;

impl fmt::Display for PageAllocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "page-aligned memory allocation failed")
    }
}

impl std::error::Error for PageAllocError {}

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
        // _SC_PAGESIZE always returns a valid value on Android
        #[allow(unsafe_code)]
        unsafe {
            libc::sysconf(libc::_SC_PAGESIZE) as usize
        }
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
/// - `Err(PageAllocError)`: Allocation failed (out of memory), or `size == 0`
///
/// # Zero-size requests
///
/// `size == 0` returns `Err(PageAllocError)` rather than a dangling pointer:
/// the global allocator's `alloc` is unsafe to call with a zero-size `Layout`,
/// and this function's contract is a plain alloc/dealloc pair with no caller-
/// tracked "did this actually allocate" state to special-case later at
/// `dealloc_page_aligned` time. Callers that legitimately need a zero-byte
/// page-aligned buffer should use `PageAlignedVec::with_capacity(0)`, which
/// handles that case internally without ever calling the global allocator.
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
pub fn alloc_page_aligned(size: usize) -> Result<NonNull<u8>, PageAllocError> {
    if size == 0 {
        return Err(PageAllocError);
    }

    let page_size = get_page_size();

    // Round up to page boundary
    let aligned_size = (size + page_size - 1) & !(page_size - 1);

    // Create aligned layout
    let layout = Layout::from_size_align(aligned_size, page_size).map_err(|_| PageAllocError)?;

    // Allocate aligned memory
    // SAFETY: Layout is valid (verified above) and non-zero-size: `size != 0`
    // was just checked above, and rounding a positive size up to a page
    // boundary cannot produce 0.
    let ptr = unsafe { alloc(layout) };

    NonNull::new(ptr).ok_or(PageAllocError)
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
    // SAFETY: `page_size` is stable for the process lifetime (the kernel's page
    // size cannot change at runtime), and `aligned_size` here is computed with the
    // exact rounding formula `alloc_page_aligned` used. The caller contract above
    // requires `size` to match the value originally passed to `alloc_page_aligned`,
    // whose checked `Layout::from_size_align(aligned_size, page_size)` already
    // succeeded for this same pair — so `page_size` is confirmed a nonzero power of
    // two and `aligned_size` does not overflow `isize::MAX` when rounded to that
    // alignment, making the unchecked reconstruction valid.
    let layout = unsafe { Layout::from_size_align_unchecked(aligned_size, page_size) };
    // SAFETY: the caller contract above requires `ptr` to have been allocated by
    // `alloc_page_aligned` with this same `size` and not used after this call.
    // `layout` (reconstructed above) is therefore identical to the layout that
    // allocation used, satisfying `dealloc`'s "same allocator, same layout" contract.
    unsafe { dealloc(ptr.as_ptr(), layout) };
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
/// use flui_platform::platforms::android::memory::PageAlignedVec;
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
    /// Exact byte size passed to the allocator (page/align-rounded), stored
    /// verbatim. `Drop` and `byte_size()` must use this rather than
    /// recomputing `capacity * size_of::<T>()`, which can undershoot the real
    /// allocation size whenever `size_of::<T>()` doesn't evenly divide it —
    /// feeding `dealloc` a too-small layout is undefined behavior.
    byte_capacity: usize,
}

impl<T> PageAlignedVec<T> {
    /// Create a new page-aligned vector with the given capacity.
    ///
    /// The actual allocated capacity will be rounded up to the nearest
    /// page boundary (or to `align_of::<T>()`, if that exceeds the page size).
    ///
    /// # Panics
    ///
    /// - Panics if allocation fails (out of memory).
    /// - Panics if the capacity/page-rounding arithmetic overflows `usize`.
    ///
    /// # Compile-time errors
    ///
    /// Fails to compile if `T` is a zero-sized type. Capacity accounting
    /// (`capacity = aligned_bytes / size_of::<T>()`) and the GPU-buffer
    /// contract (`byte_size()` reflecting real device-visible bytes) both need
    /// a nonzero element stride. `Vec<ZST>` sidesteps the same issue by
    /// special-casing `capacity()` as `usize::MAX` with no backing allocation
    /// at all — that shortcut doesn't fit a type whose entire purpose is
    /// handing real, page-aligned byte spans to Vulkan.
    pub fn with_capacity(capacity: usize) -> Self {
        const {
            assert!(
                std::mem::size_of::<T>() != 0,
                "PageAlignedVec<T> does not support zero-sized T (see doc comment)"
            );
        }

        let page_size = get_page_size();
        // The allocation must satisfy both the page-alignment contract this
        // type advertises and `T`'s own alignment requirement.
        let align = page_size.max(std::mem::align_of::<T>());

        let requested_bytes = capacity.checked_mul(std::mem::size_of::<T>()).expect(
            "BUG: PageAlignedVec capacity overflow: capacity * size_of::<T>() > usize::MAX",
        );
        let byte_capacity = requested_bytes
            .checked_add(align - 1)
            .expect("BUG: PageAlignedVec capacity overflow: page-rounding overflowed usize::MAX")
            & !(align - 1);

        let ptr = if byte_capacity == 0 {
            // SAFETY: `align` is a nonzero power of two — `page_size` is a
            // power of two (from `get_page_size`/`sysconf`) and
            // `align_of::<T>()` is always a power of two, so their max is
            // too. Using `align` directly as a pointer address therefore
            // yields a non-null pointer whose address is a multiple of
            // `align`, satisfying both `align_of::<T>()` and the page-
            // alignment contract `is_page_aligned` checks. No storage backs
            // this pointer, but `capacity` is 0 here so `push` can never
            // write through it, and `as_slice`/`as_mut_slice`/`clear` only
            // ever read through it with `len == 0` — a zero-length
            // `slice::from_raw_parts(_mut)` never dereferences its data
            // pointer — so the lack of a real allocation is never observed.
            NonNull::new(std::ptr::without_provenance_mut(align))
                .expect("BUG: page size/alignment is never zero")
        } else {
            // SAFETY: `byte_capacity` is nonzero here and rounded up to
            // `align`, and `align` is a nonzero power of two (see above), so
            // `Layout::from_size_align` succeeds and `alloc` receives a
            // valid, non-zero-size layout.
            let layout = Layout::from_size_align(byte_capacity, align)
                .expect("Invalid layout for page-aligned allocation");
            let raw = unsafe { alloc(layout) as *mut T };
            NonNull::new(raw).expect("Allocation failed")
        };

        Self {
            ptr,
            len: 0,
            capacity: byte_capacity / std::mem::size_of::<T>(),
            byte_capacity,
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
        // SAFETY: `self.ptr` was allocated (or, for a zero-capacity vector,
        // set to a dangling sentinel — see `with_capacity`) for `self.capacity`
        // elements of `T`, and is non-null and aligned to
        // `page_size.max(align_of::<T>())`, which is always >= `align_of::<T>()`
        // — so it is aligned for `T` regardless of `T`'s own alignment
        // requirement. The type invariant maintained by `push`/`set_len` is
        // `self.len <= self.capacity` with elements `0..self.len` initialized,
        // so the first `self.len` elements starting at `self.ptr` are live,
        // initialized `T` values. The returned reference borrows `self`
        // immutably, so it cannot alias a concurrent `&mut` access to the same
        // elements for its lifetime.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Get a mutable slice view of the initialized elements.
    ///
    /// # Safety
    ///
    /// Only the first `len` elements are guaranteed to be initialized.
    #[inline]
    pub unsafe fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: as with `as_slice`, `self.ptr` is a valid, non-null pointer
        // aligned to `page_size.max(align_of::<T>())` (so aligned for `T`
        // regardless of `T`'s own alignment requirement) for `self.capacity`
        // elements, with the first `self.len` elements initialized. The
        // `&mut self` borrow gives this call exclusive access to the vector
        // for the returned slice's lifetime, so no other reference to these
        // elements can be alive concurrently, satisfying
        // `from_raw_parts_mut`'s aliasing requirement.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
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

        // SAFETY: `self.len < self.capacity` was just asserted, so
        // `self.ptr.add(self.len)` lands within the allocation's `capacity`
        // elements and denotes an as-yet-uninitialized slot (this type's
        // invariant is that elements `self.len..capacity` are never
        // initialized), so writing `value` there does not drop or alias a
        // live `T`.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(value);
        }
        self.len += 1;
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
        self.byte_capacity
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

        if self.byte_capacity == 0 {
            // `with_capacity(0)` never called the allocator (see there) — the
            // pointer is a dangling sentinel, so there is nothing to free.
            return;
        }

        let page_size = get_page_size();
        let align = page_size.max(std::mem::align_of::<T>());
        // SAFETY: `self.byte_capacity` is the exact size that was passed to
        // `Layout::from_size_align` in `with_capacity` (stored verbatim in the
        // `byte_capacity` field, never recomputed via `capacity *
        // size_of::<T>()`, which can undershoot the real allocation size when
        // `size_of::<T>()` doesn't evenly divide the page/align-rounded size).
        // `align` is recomputed identically from `page_size` (process-constant)
        // and `align_of::<T>()` (a `T`-level constant), so this layout matches
        // the allocation's layout exactly, satisfying `dealloc`'s "same
        // allocator, same layout" contract.
        let layout = Layout::from_size_align(self.byte_capacity, align).expect("Invalid layout");

        unsafe {
            dealloc(self.ptr.as_ptr() as *mut u8, layout);
        }
    }
}

// SAFETY: `PageAlignedVec<T>` exclusively owns its buffer — all access goes
// through `&self`/`&mut self`, which the borrow checker already serializes —
// and it holds no thread-affine state of its own, so moving it across
// threads is sound whenever moving its `T` elements across threads is sound.
unsafe impl<T: Send> Send for PageAlignedVec<T> {}

// SAFETY: shared access through `&PageAlignedVec<T>` only ever exposes `&T`
// (via `as_ptr`/`as_slice`), and mutation always requires `&mut self`,
// matching ordinary `&T` aliasing rules exactly, so sharing it across
// threads is sound whenever `T` is `Sync`.
unsafe impl<T: Sync> Sync for PageAlignedVec<T> {}

// ============================================================================
// Buffer Size Alignment
// ============================================================================

/// Round a size up to the nearest page boundary.
///
/// This is useful for ensuring Vulkan buffer sizes are page-aligned.
///
/// Note: `align_to_page_size(0)` returns `0`. This is a pure rounding helper
/// (`(size + page_size - 1) & !(page_size - 1)`), not an allocator, so "0
/// bytes is already page-aligned" is the coherent answer. This differs from
/// [`alloc_page_aligned`], which rejects `size == 0` outright because it must
/// hand back a real allocation.
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

        for i in 0..100u32 {
            vec.push(i);
        }

        assert_eq!(vec.len(), 100);

        unsafe {
            let slice = vec.as_slice();
            for i in 0..100u32 {
                assert_eq!(slice[i as usize], i);
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
            // `align_to_page_size` is a pure rounding formula, not an
            // allocator: 0 bytes rounds to 0 bytes (already page-aligned).
            (0, 0),
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
        let page_size = get_page_size();
        // `u8`'s stride (1) evenly divides the page size, so `with_capacity`
        // rounds the requested capacity up to exactly one page's worth of
        // elements regardless of the small `10` argument below — pushing
        // `page_size` elements fills it exactly, and one more must exceed the
        // real (rounded) capacity.
        let mut vec = PageAlignedVec::<u8>::with_capacity(10);
        assert_eq!(vec.capacity(), page_size);

        for _ in 0..=page_size {
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

    #[test]
    fn test_alloc_page_aligned_rejects_zero_size() {
        assert!(alloc_page_aligned(0).is_err());
    }

    #[test]
    fn test_page_aligned_vec_uneven_stride_drop() {
        // `size_of::<[u8; 3]>() == 3` does not evenly divide the page-rounded
        // allocation size, so `Drop` must use the stored `byte_capacity`
        // rather than recomputing `capacity * size_of::<T>()` (the latter
        // would hand `dealloc` a smaller-than-allocated layout — this is the
        // scenario miri caught as "incorrect layout on deallocation" before
        // `byte_capacity` was introduced).
        let mut vec = PageAlignedVec::<[u8; 3]>::with_capacity(1);
        vec.push([1, 2, 3]);
        drop(vec);
    }

    #[test]
    fn test_page_aligned_vec_zero_capacity_roundtrip() {
        // `with_capacity(0)` (and `new()`/`Default`) must not call the global
        // allocator with a zero-size layout; exercises the dangling
        // page-aligned sentinel path end-to-end, including `Drop` skipping
        // `dealloc` for it.
        let vec = PageAlignedVec::<u32>::with_capacity(0);
        assert_eq!(vec.capacity(), 0);
        assert_eq!(vec.byte_size(), 0);
        assert!(vec.is_page_aligned());
        drop(vec);
    }
}
