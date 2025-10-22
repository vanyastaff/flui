//! Macros for implementing cached layout logic
//!
//! Provides reusable patterns for implementing layout() methods with caching support.

/// Implements cached layout logic with fast path, global cache, and result caching.
///
/// This macro reduces code duplication by providing a standard pattern for layout
/// implementations that support ElementId-based caching.
///
/// # Usage for single-child widgets (no child_count)
///
/// ```rust,ignore
/// impl DynRenderObject for MyWidget {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         impl_cached_layout!(self, constraints, {
///             // Your layout logic here
///             self.perform_layout(constraints)
///         })
///     }
/// }
/// ```
///
/// # Usage for multi-child widgets (with child_count)
///
/// ```rust,ignore
/// impl DynRenderObject for MyMultiChildWidget {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         impl_cached_layout!(self, constraints, self.children.len(), {
///             // Your layout logic here
///             self.perform_layout(constraints)
///         })
///     }
/// }
/// ```
///
/// # What it generates
///
/// The macro expands to:
/// 1. ⚡ **Fast path**: Early return if layout not needed (~2ns)
/// 2. 🔍 **Global cache**: Check layout cache for hit (~20ns)
/// 3. 🐌 **Compute layout**: Execute your layout code (~1000ns+)
/// 4. 💾 **Cache result**: Store result for future use
///
/// # Performance
///
/// - Fast path hit: ~2ns (same constraints, no dirty flag)
/// - Cache hit: ~20ns (hash lookup)
/// - Cache miss: ~1000ns+ (full layout computation)
/// - Overall: 10x-100x speedup for repeated layouts
#[macro_export]
macro_rules! impl_cached_layout {
    // Single-child variant (no child_count)
    ($self:ident, $constraints:ident, $layout_code:block) => {{
        // ⚡ FAST PATH: Early return if layout not needed (~2ns)
        if !$self.needs_layout_flag && $self.constraints == Some($constraints) {
            return $self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache (~20ns)
        if let Some(element_id) = $self.element_id {
            if !$self.needs_layout_flag {
                let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(element_id, $constraints);

                if let Some(cached) = $crate::__layout_cache_deps::layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        $self.constraints = Some($constraints);
                        $self.size = cached.size;
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual layout
        $self.needs_layout_flag = false;
        let size = $layout_code;

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = $self.element_id {
            let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(element_id, $constraints);
            $crate::__layout_cache_deps::layout_cache().insert(
                cache_key,
                $crate::__layout_cache_deps::LayoutResult::new(size)
            );
        }

        size
    }};

    // Multi-child variant (with child_count)
    ($self:ident, $constraints:ident, $child_count:expr, $layout_code:block) => {{
        // ⚡ FAST PATH: Early return if layout not needed (~2ns)
        if !$self.needs_layout_flag && $self.constraints == Some($constraints) {
            return $self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache (~20ns)
        // CRITICAL: Include child_count to detect structural changes!
        if let Some(element_id) = $self.element_id {
            if !$self.needs_layout_flag {
                let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(element_id, $constraints)
                    .with_child_count($child_count);

                if let Some(cached) = $crate::__layout_cache_deps::layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        $self.constraints = Some($constraints);
                        $self.size = cached.size;
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual layout
        $self.needs_layout_flag = false;
        let size = $layout_code;

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = $self.element_id {
            let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(element_id, $constraints)
                .with_child_count($child_count);
            $crate::__layout_cache_deps::layout_cache().insert(
                cache_key,
                $crate::__layout_cache_deps::LayoutResult::new(size)
            );
        }

        size
    }};
}

// Re-export dependencies for the macro
#[doc(hidden)]
pub mod __layout_cache_deps {
    pub use flui_core::cache::{layout_cache, LayoutCacheKey, LayoutResult};
}
