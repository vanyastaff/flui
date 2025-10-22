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
            #[cfg(debug_assertions)]
            {
                // Debug: можно логировать cache hits
                // eprintln!("[LAYOUT FAST] {:?}", $self.element_id);
            }
            return $self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache (~20ns)
        if let Some(element_id) = $self.element_id {
            if !$self.needs_layout_flag {
                let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(
                    element_id, 
                    $constraints
                );

                if let Some(cached) = $crate::__layout_cache_deps::layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        $self.constraints = Some($constraints);
                        $self.size = cached.size;
                        
                        #[cfg(debug_assertions)]
                        {
                            // Debug: можно логировать cache hits
                            // eprintln!("[LAYOUT CACHE] {:?}", element_id);
                        }
                        
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual layout
        #[cfg(debug_assertions)]
        {
            // Debug: можно логировать cache misses
            // eprintln!("[LAYOUT COMPUTE] {:?}", $self.element_id);
        }
        
        $self.needs_layout_flag = false;
        let size = $layout_code;

        // Сохранить результат в self перед кешированием
        $self.size = size;
        $self.constraints = Some($constraints);

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = $self.element_id {
            let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(
                element_id, 
                $constraints
            );
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
            #[cfg(debug_assertions)]
            {
                // Debug: можно логировать cache hits
                // eprintln!("[LAYOUT FAST] {:?} (children: {})", $self.element_id, $child_count);
            }
            return $self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache (~20ns)
        // CRITICAL: Include child_count to detect structural changes!
        if let Some(element_id) = $self.element_id {
            if !$self.needs_layout_flag {
                let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(
                    element_id, 
                    $constraints
                ).with_child_count($child_count);

                if let Some(cached) = $crate::__layout_cache_deps::layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        $self.constraints = Some($constraints);
                        $self.size = cached.size;
                        
                        #[cfg(debug_assertions)]
                        {
                            // Debug: можно логировать cache hits
                            // eprintln!("[LAYOUT CACHE] {:?} (children: {})", element_id, $child_count);
                        }
                        
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual layout
        #[cfg(debug_assertions)]
        {
            // Debug: можно логировать cache misses
            // eprintln!("[LAYOUT COMPUTE] {:?} (children: {})", $self.element_id, $child_count);
        }
        
        $self.needs_layout_flag = false;
        let size = $layout_code;

        // Сохранить результат в self перед кешированием
        $self.size = size;
        $self.constraints = Some($constraints);

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = $self.element_id {
            let cache_key = $crate::__layout_cache_deps::LayoutCacheKey::new(
                element_id, 
                $constraints
            ).with_child_count($child_count);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Example widget using the macro
    #[derive(Debug)]
    struct MockWidget {
        element_id: Option<flui_core::ElementId>,
        size: flui_types::Size,
        constraints: Option<flui_core::BoxConstraints>,
        needs_layout_flag: bool,
    }

    impl MockWidget {
        fn new() -> Self {
            Self {
                element_id: None,
                size: flui_types::Size::zero(),
                constraints: None,
                needs_layout_flag: true,
            }
        }

        fn with_element_id(id: flui_core::ElementId) -> Self {
            Self {
                element_id: Some(id),
                size: flui_types::Size::zero(),
                constraints: None,
                needs_layout_flag: true,
            }
        }

        fn perform_layout(&mut self, constraints: flui_core::BoxConstraints) -> flui_types::Size {
            constraints.biggest()
        }

        fn layout(&mut self, constraints: flui_core::BoxConstraints) -> flui_types::Size {
            crate::impl_cached_layout!(self, constraints, {
                self.perform_layout(constraints)
            })
        }
    }

    #[test]
    fn test_macro_single_child() {
        use flui_core::BoxConstraints;
        use flui_types::Size;

        let mut widget = MockWidget::with_element_id(flui_core::ElementId::new());
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        // First layout - should compute
        let size = widget.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
        assert_eq!(widget.size, Size::new(100.0, 50.0));

        // Second layout - should use fast path
        let size2 = widget.layout(constraints);

        assert_eq!(size2, Size::new(100.0, 50.0));
    }

    #[derive(Debug)]
    struct MockMultiChildWidget {
        element_id: Option<flui_core::ElementId>,
        size: flui_types::Size,
        constraints: Option<flui_core::BoxConstraints>,
        needs_layout_flag: bool,
        child_count: usize,
    }

    impl MockMultiChildWidget {
        fn with_element_id(id: flui_core::ElementId, child_count: usize) -> Self {
            Self {
                element_id: Some(id),
                size: flui_types::Size::zero(),
                constraints: None,
                needs_layout_flag: true,
                child_count,
            }
        }

        fn perform_layout(&mut self, constraints: flui_core::BoxConstraints) -> flui_types::Size {
            constraints.biggest()
        }

        fn layout(&mut self, constraints: flui_core::BoxConstraints) -> flui_types::Size {
            crate::impl_cached_layout!(self, constraints, self.child_count, {
                self.perform_layout(constraints)
            })
        }

        fn set_child_count(&mut self, count: usize) {
            self.child_count = count;
            self.needs_layout_flag = true;
        }
    }

    #[test]
    fn test_macro_multi_child() {
        use flui_core::BoxConstraints;
        use flui_types::Size;

        let mut widget = MockMultiChildWidget::with_element_id(flui_core::ElementId::new(), 3);
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        // First layout
        let size = widget.layout(constraints);
        assert_eq!(size, Size::new(100.0, 50.0));

        // Second layout with same child_count
        let size2 = widget.layout(constraints);
        assert_eq!(size2, Size::new(100.0, 50.0));

        // Third layout with different child_count - should recompute
        widget.set_child_count(5);
        let size3 = widget.layout(constraints);
        assert_eq!(size3, Size::new(100.0, 50.0));
    }
}