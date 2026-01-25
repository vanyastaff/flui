# Android 16KB Page Size Support Guide
**Status:** 🚨 **URGENT - Deadline Passed (August 2025)**  
**Date:** 2026-01-25  
**Target:** API 35+ (Android 16)

---

## Executive Summary

Google Play Store now **requires** all apps targeting API 35+ to support 16KB page sizes. The deadline passed in August 2025, making this a critical compliance issue. Apps that don't support 16KB pages will be rejected from the Play Store.

### Impact on FLUI

FLUI's GPU rendering engine uses Vulkan and wgpu, which perform extensive memory allocations. These allocations **must be page-aligned** on 16KB page size devices, or the app will crash.

### Affected Devices

- **Pixel 9, 9 Pro, 9 Pro XL** (released Sept 2024)
- **Samsung Galaxy S25 series** (released Jan 2025)
- All future Android flagship devices (16KB is the new standard)

---

## Problem Overview

### What Changed?

Traditional Android devices use **4KB page sizes** for virtual memory. New devices use **16KB page sizes** for:
- Better performance (fewer TLB misses)
- Improved security (larger guard pages)
- Alignment with ARM64 architecture recommendations

### Why This Matters

Memory allocations that work on 4KB devices can fail on 16KB devices:

```rust
// ❌ BREAKS on 16KB devices
let buffer = vec![0u8; 8192];  // 8KB allocation, not 16KB-aligned

// ✅ WORKS on both 4KB and 16KB devices
let buffer = vec![0u8; 16384];  // 16KB allocation, properly aligned
```

### Vulkan-Specific Issues

Vulkan buffer allocations must respect `VkPhysicalDeviceLimits::minMemoryMapAlignment`:
- **4KB devices:** `minMemoryMapAlignment = 4096`
- **16KB devices:** `minMemoryMapAlignment = 16384`

**Our wgpu code must query this value and use it.**

---

## Testing Requirements

### Required Hardware

You **must** test on actual 16KB devices. Emulators are insufficient because they don't catch all alignment issues.

**Recommended Test Devices:**
1. **Google Pixel 9** (most common 16KB device)
2. **Samsung Galaxy S25** (flagship with 16KB)
3. **Any device with Snapdragon 8 Gen 3 or later**

### Required Software

- **NDK r26 or later** (r25 and earlier don't support 16KB)
- **Android API 35** target
- **Rust 1.75+** for Android compilation

---

## Implementation Checklist

### 1. Update NDK Version

```toml
# android/build.gradle or cargo-ndk config
android {
    ndkVersion = "26.1.10909125"  // Or later
}
```

**Check current NDK:**
```bash
$ANDROID_NDK_ROOT/ndk-build --version
# Should output 26.x.x or higher
```

### 2. Update Android Target API

```toml
# android/app/build.gradle
android {
    compileSdk = 35
    targetSdk = 35  // Required for Play Store
    minSdk = 24     // Keep backwards compatibility
}
```

### 3. Add Page-Aligned Allocator

Create a new file for Android-specific memory management:

**File:** `crates/flui_engine/src/android/memory.rs`

```rust
//! Android page-aligned memory allocator for 16KB page size support

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

/// Get system page size at runtime
pub fn get_page_size() -> usize {
    #[cfg(target_os = "android")]
    {
        // Query actual page size from system
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }
    
    #[cfg(not(target_os = "android"))]
    {
        4096  // Default for non-Android
    }
}

/// Allocate page-aligned memory
pub fn alloc_page_aligned(size: usize) -> Result<NonNull<u8>, std::alloc::AllocError> {
    let page_size = get_page_size();
    
    // Round up to page boundary
    let aligned_size = (size + page_size - 1) & !(page_size - 1);
    
    // Create aligned layout
    let layout = Layout::from_size_align(aligned_size, page_size)
        .map_err(|_| std::alloc::AllocError)?;
    
    // Allocate aligned memory
    let ptr = unsafe { alloc(layout) };
    
    NonNull::new(ptr).ok_or(std::alloc::AllocError)
}

/// Deallocate page-aligned memory
pub unsafe fn dealloc_page_aligned(ptr: NonNull<u8>, size: usize) {
    let page_size = get_page_size();
    let aligned_size = (size + page_size - 1) & !(page_size - 1);
    let layout = Layout::from_size_align_unchecked(aligned_size, page_size);
    dealloc(ptr.as_ptr(), layout);
}

/// Page-aligned vector for GPU buffers
pub struct PageAlignedVec<T> {
    ptr: NonNull<T>,
    len: usize,
    capacity: usize,
}

impl<T> PageAlignedVec<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let page_size = get_page_size();
        let byte_capacity = capacity * std::mem::size_of::<T>();
        let aligned_capacity = (byte_capacity + page_size - 1) & !(page_size - 1);
        
        let layout = Layout::from_size_align(aligned_capacity, page_size)
            .expect("Invalid layout for page-aligned allocation");
        
        let ptr = unsafe { alloc(layout) as *mut T };
        let ptr = NonNull::new(ptr).expect("Allocation failed");
        
        Self {
            ptr,
            len: 0,
            capacity: aligned_capacity / std::mem::size_of::<T>(),
        }
    }
    
    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }
    
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.as_ptr()
    }
    
    pub fn len(&self) -> usize {
        self.len
    }
    
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T> Drop for PageAlignedVec<T> {
    fn drop(&mut self) {
        let page_size = get_page_size();
        let byte_capacity = self.capacity * std::mem::size_of::<T>();
        let layout = Layout::from_size_align(byte_capacity, page_size)
            .expect("Invalid layout");
        
        unsafe {
            dealloc(self.ptr.as_ptr() as *mut u8, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_page_size_detection() {
        let page_size = get_page_size();
        assert!(page_size >= 4096);
        assert!(page_size <= 65536);
        
        // Page size must be power of 2
        assert!(page_size.is_power_of_two());
    }
    
    #[test]
    fn test_page_aligned_alloc() {
        let ptr = alloc_page_aligned(8192).expect("Allocation failed");
        let page_size = get_page_size();
        
        // Verify alignment
        assert_eq!(ptr.as_ptr() as usize % page_size, 0);
        
        unsafe {
            dealloc_page_aligned(ptr, 8192);
        }
    }
    
    #[test]
    fn test_page_aligned_vec() {
        let vec = PageAlignedVec::<u8>::with_capacity(1024);
        let page_size = get_page_size();
        
        // Verify alignment
        assert_eq!(vec.as_ptr() as usize % page_size, 0);
        
        // Verify capacity is page-aligned
        assert!(vec.capacity() * std::mem::size_of::<u8>() >= page_size);
    }
}
```

### 4. Update wgpu Buffer Creation

**File:** `crates/flui_engine/src/wgpu/buffers.rs` (modify existing)

```rust
#[cfg(target_os = "android")]
use crate::android::memory::{get_page_size, PageAlignedVec};

impl BufferManager {
    pub fn create_buffer(&mut self, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        #[cfg(target_os = "android")]
        {
            let page_size = get_page_size() as u64;
            // Round up to page boundary on Android
            let aligned_size = (size + page_size - 1) & !(page_size - 1);
            
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Page-aligned buffer"),
                size: aligned_size,
                usage,
                mapped_at_creation: false,
            })
        }
        
        #[cfg(not(target_os = "android"))]
        {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Buffer"),
                size,
                usage,
                mapped_at_creation: false,
            })
        }
    }
}
```

### 5. Query Vulkan Alignment Requirements

**File:** `crates/flui_engine/src/wgpu/vulkan.rs` (add to existing)

```rust
/// Query Vulkan memory alignment requirements
pub fn get_min_memory_map_alignment(adapter: &wgpu::Adapter) -> u64 {
    #[cfg(target_os = "android")]
    {
        // TODO: Query VkPhysicalDeviceLimits::minMemoryMapAlignment
        // For now, use conservative 16KB on Android
        16384
    }
    
    #[cfg(not(target_os = "android"))]
    {
        4096  // Standard 4KB alignment
    }
}

/// Ensure size is aligned to Vulkan requirements
pub fn align_buffer_size(size: u64, adapter: &wgpu::Adapter) -> u64 {
    let alignment = get_min_memory_map_alignment(adapter);
    (size + alignment - 1) & !(alignment - 1)
}
```

---

## Testing Procedure

### Step 1: Build for Android with NDK r26

```bash
# Set up Android environment
export ANDROID_NDK_ROOT=/path/to/ndk/26.1.10909125
export ANDROID_HOME=/path/to/android-sdk

# Install Android targets
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi

# Build FLUI for Android
cargo ndk --target aarch64-linux-android --platform 35 build --release
```

### Step 2: Run on 16KB Device

```bash
# Install on Pixel 9 or Galaxy S25
adb install -r target/aarch64-linux-android/release/flui_app.apk

# Enable detailed logging
adb shell setprop debug.flui.log 1

# Run app and monitor logs
adb logcat | grep -E "FLUI|wgpu|Vulkan"
```

### Step 3: Look for These Errors

**Common 16KB page size errors:**

```
E/Vulkan: vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY
E/wgpu: Buffer mapping failed: alignment mismatch
E/libc: SIGBUS (Bus error) at address 0x7f8a4c2000
F/libc: Fatal signal 7 (SIGBUS), code 1 (BUS_ADRALN)
```

**If you see these, your allocations are not page-aligned!**

### Step 4: Verify Alignment

Add this debug logging:

```rust
#[cfg(target_os = "android")]
{
    let page_size = get_page_size();
    tracing::info!(
        page_size = page_size,
        buffer_size = size,
        aligned_size = aligned_size,
        "Android buffer allocation"
    );
    
    assert_eq!(aligned_size % page_size as u64, 0, "Buffer not page-aligned!");
}
```

---

## Performance Testing

### Benchmark Memory Allocation

```rust
#[cfg(test)]
mod benches {
    use super::*;
    
    #[bench]
    fn bench_page_aligned_alloc(b: &mut Bencher) {
        b.iter(|| {
            let ptr = alloc_page_aligned(65536).unwrap();
            unsafe { dealloc_page_aligned(ptr, 65536); }
        });
    }
    
    #[bench]
    fn bench_standard_alloc(b: &mut Bencher) {
        b.iter(|| {
            let vec = vec![0u8; 65536];
            drop(vec);
        });
    }
}
```

**Expected Results:**
- Page-aligned allocation: ~5-10% slower than standard allocation
- This is acceptable for the correctness gain

---

## Cargo Configuration

### Update Cargo.toml

```toml
[target.'cfg(target_os = "android")'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "android")'.build-dependencies]
ndk-build = "0.7"

# Ensure minimum API level
[package.metadata.android]
min_sdk_version = 24
target_sdk_version = 35
```

### Add Android Module

```rust
// crates/flui_engine/src/lib.rs

#[cfg(target_os = "android")]
pub mod android;
```

---

## Compliance Checklist

Before submitting to Play Store:

- [ ] NDK version is r26 or later
- [ ] `targetSdkVersion = 35` in build.gradle
- [ ] All buffer allocations are page-aligned
- [ ] Tested on actual 16KB device (Pixel 9 or Galaxy S25)
- [ ] No SIGBUS crashes in stress testing
- [ ] No VK_ERROR_OUT_OF_DEVICE_MEMORY errors
- [ ] App runs smoothly for 30+ minutes on 16KB device
- [ ] Memory profiler shows no alignment issues

---

## Risk Assessment

### High Risk Areas

1. **Vulkan Buffer Allocation** 🔴
   - All `wgpu::Buffer` creation
   - Staging buffers for texture uploads
   - Uniform buffers
   - Vertex/index buffers

2. **Memory-Mapped Files** 🟡
   - Asset loading via mmap
   - Shader cache files
   - Font file loading

3. **Native Code Interop** 🟡
   - JNI buffer passing
   - NDK bitmap allocation
   - Native activity window buffers

### Mitigation Strategy

1. Use `PageAlignedVec` for all GPU buffers
2. Query `minMemoryMapAlignment` from Vulkan
3. Round all buffer sizes up to page boundaries
4. Add assertions to catch misalignment in debug builds

---

## Tools and Debugging

### Android Studio Profiler

```bash
# Launch profiler
adb shell am start -n com.flui.app/.MainActivity --attach-agent 'instrument:profile_memory'

# Look for:
# - Abnormal memory spikes
# - Allocation failures
# - Bus errors in native code
```

### Vulkan Validation Layers

```bash
# Enable validation
adb shell setprop debug.vulkan.layers VK_LAYER_KHRONOS_validation

# Check for alignment warnings
adb logcat | grep "VUID-vkAllocateMemory-pAllocateInfo"
```

### Memory Sanitizer

```bash
# Build with address sanitizer
RUSTFLAGS="-Z sanitizer=address" cargo ndk build --target aarch64-linux-android

# Install and run - crashes will show exact alignment issues
```

---

## References

- **Google Documentation:** [Support 16 KB page sizes](https://developer.android.com/guide/practices/page-sizes)
- **NDK r26 Release:** [Release Notes](https://github.com/android/ndk/wiki/Changelog-r26)
- **Vulkan Page Size:** [VkPhysicalDeviceLimits](https://registry.khronos.org/vulkan/specs/1.3/html/chap39.html#VkPhysicalDeviceLimits)

---

## Timeline

### Immediate (This Week)
1. Update NDK to r26
2. Implement `PageAlignedVec` allocator
3. Update all buffer creation code
4. Add alignment assertions

### Short-term (Next 2 Weeks)
1. Acquire Pixel 9 or Galaxy S25 for testing
2. Run full test suite on 16KB device
3. Fix any alignment issues discovered
4. Performance testing and optimization

### Deployment (Week 3)
1. Final testing on multiple 16KB devices
2. Update Play Store listing with API 35 target
3. Submit updated app for review
4. Monitor crash reports closely

---

## Success Criteria

✅ **App runs without crashes on Pixel 9**  
✅ **No SIGBUS errors in 24-hour stress test**  
✅ **All Vulkan buffer allocations properly aligned**  
✅ **Play Store accepts API 35 submission**  
✅ **Zero alignment-related crash reports from users**

---

**URGENT: This must be completed before next Play Store submission!**
