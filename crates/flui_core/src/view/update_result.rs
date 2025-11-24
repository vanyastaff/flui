//! Update result for render object updates.

/// Result of render object update operation.
///
/// Indicates what invalidation is needed after updating properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateResult {
    /// No properties changed - skip invalidation entirely.
    ///
    /// Most efficient - no layout or paint needed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn update_render_object(&self, render: &mut RenderPadding) -> UpdateResult {
    ///     if render.padding == self.padding {
    ///         return UpdateResult::Unchanged;  // Skip work!
    ///     }
    ///     // ... update
    /// }
    /// ```
    Unchanged,

    /// Properties affecting layout changed.
    ///
    /// Triggers layout + paint phases.
    ///
    /// # Examples
    ///
    /// Properties that affect layout:
    /// - Padding, margins, size
    /// - Alignment, flex values
    /// - Width/height factors
    ///
    /// ```rust,ignore
    /// render.padding = self.padding;
    /// UpdateResult::NeedsLayout
    /// ```
    NeedsLayout,

    /// Only visual properties changed.
    ///
    /// Triggers paint phase only (skips layout).
    ///
    /// # Examples
    ///
    /// Properties that only affect paint:
    /// - Color, opacity
    /// - Decorations, shadows
    /// - Text style (color, etc)
    ///
    /// ```rust,ignore
    /// render.color = self.color;
    /// UpdateResult::NeedsPaint  // Skip layout!
    /// ```
    NeedsPaint,
}

impl UpdateResult {
    /// Returns true if any work is needed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::render::UpdateResult;
    ///
    /// assert!(!UpdateResult::Unchanged.needs_work());
    /// assert!(UpdateResult::NeedsLayout.needs_work());
    /// assert!(UpdateResult::NeedsPaint.needs_work());
    /// ```
    #[inline]
    pub fn needs_work(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// Returns true if layout is needed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::render::UpdateResult;
    ///
    /// assert!(!UpdateResult::Unchanged.needs_layout());
    /// assert!(UpdateResult::NeedsLayout.needs_layout());
    /// assert!(!UpdateResult::NeedsPaint.needs_layout());
    /// ```
    #[inline]
    pub fn needs_layout(self) -> bool {
        matches!(self, Self::NeedsLayout)
    }

    /// Returns true if paint is needed.
    ///
    /// Note: `NeedsLayout` also needs paint (layout implies paint).
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::render::UpdateResult;
    ///
    /// assert!(!UpdateResult::Unchanged.needs_paint());
    /// assert!(UpdateResult::NeedsLayout.needs_paint());  // Layout implies paint
    /// assert!(UpdateResult::NeedsPaint.needs_paint());
    /// ```
    #[inline]
    pub fn needs_paint(self) -> bool {
        matches!(self, Self::NeedsLayout | Self::NeedsPaint)
    }

    /// Combine two update results (takes the most severe).
    ///
    /// Severity order: `NeedsLayout` > `NeedsPaint` > `Unchanged`
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::render::UpdateResult;
    ///
    /// let result1 = UpdateResult::NeedsPaint;
    /// let result2 = UpdateResult::NeedsLayout;
    /// assert_eq!(result1.combine(result2), UpdateResult::NeedsLayout);
    /// ```
    pub fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::NeedsLayout, _) | (_, Self::NeedsLayout) => Self::NeedsLayout,
            (Self::NeedsPaint, _) | (_, Self::NeedsPaint) => Self::NeedsPaint,
            _ => Self::Unchanged,
        }
    }
}

impl Default for UpdateResult {
    fn default() -> Self {
        Self::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_work() {
        assert!(!UpdateResult::Unchanged.needs_work());
        assert!(UpdateResult::NeedsLayout.needs_work());
        assert!(UpdateResult::NeedsPaint.needs_work());
    }

    #[test]
    fn test_needs_layout() {
        assert!(!UpdateResult::Unchanged.needs_layout());
        assert!(UpdateResult::NeedsLayout.needs_layout());
        assert!(!UpdateResult::NeedsPaint.needs_layout());
    }

    #[test]
    fn test_needs_paint() {
        assert!(!UpdateResult::Unchanged.needs_paint());
        assert!(UpdateResult::NeedsLayout.needs_paint());
        assert!(UpdateResult::NeedsPaint.needs_paint());
    }

    #[test]
    fn test_combine() {
        assert_eq!(
            UpdateResult::Unchanged.combine(UpdateResult::Unchanged),
            UpdateResult::Unchanged
        );
        assert_eq!(
            UpdateResult::Unchanged.combine(UpdateResult::NeedsPaint),
            UpdateResult::NeedsPaint
        );
        assert_eq!(
            UpdateResult::NeedsPaint.combine(UpdateResult::NeedsLayout),
            UpdateResult::NeedsLayout
        );
        assert_eq!(
            UpdateResult::NeedsLayout.combine(UpdateResult::Unchanged),
            UpdateResult::NeedsLayout
        );
    }
}
