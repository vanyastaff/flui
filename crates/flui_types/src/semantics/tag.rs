//! Semantic tags for categorizing semantic nodes

use std::fmt;

/// A tag that can be applied to a semantics node
///
/// Similar to Flutter's `SemanticsTag`. Tags are used to categorize
/// semantics nodes for testing and debugging purposes.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::SemanticsTag;
///
/// let tag = SemanticsTag::new("button");
/// assert_eq!(tag.name(), "button");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticsTag {
    name: String,
}

impl SemanticsTag {
    /// Creates a new semantics tag with the given name
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsTag;
    ///
    /// let tag = SemanticsTag::new("custom_tag");
    /// assert_eq!(tag.name(), "custom_tag");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Returns the name of this tag
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for SemanticsTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SemanticsTag({})", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_tag_new() {
        let tag = SemanticsTag::new("button");
        assert_eq!(tag.name(), "button");
    }

    #[test]
    fn test_semantics_tag_equality() {
        let tag1 = SemanticsTag::new("button");
        let tag2 = SemanticsTag::new("button");
        let tag3 = SemanticsTag::new("link");

        assert_eq!(tag1, tag2);
        assert_ne!(tag1, tag3);
    }

    #[test]
    fn test_semantics_tag_display() {
        let tag = SemanticsTag::new("button");
        assert_eq!(tag.to_string(), "SemanticsTag(button)");
    }
}
