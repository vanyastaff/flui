//! String attributes for rich text semantics

/// Base trait for string attributes
///
/// Similar to Flutter's `StringAttribute`. Attributes that can be applied
/// to ranges of text in semantic strings.
pub trait StringAttribute: std::fmt::Debug {
    /// Returns the range this attribute applies to
    fn range(&self) -> (usize, usize);

    /// Returns the type name of this attribute
    fn attribute_type(&self) -> &str;
}

/// A string with attached semantic attributes
///
/// Similar to Flutter's `AttributedString`. This allows associating
/// semantic information with ranges of text.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::AttributedString;
///
/// let text = AttributedString::new("Hello, World!");
/// assert_eq!(text.string(), "Hello, World!");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributedString {
    string: String,
    // Attributes would go here in a full implementation
    // For now we keep it simple
}

impl AttributedString {
    /// Creates a new attributed string
    pub fn new(string: impl Into<String>) -> Self {
        Self {
            string: string.into(),
        }
    }

    /// Returns the string content
    pub fn string(&self) -> &str {
        &self.string
    }

    /// Returns the length of the string
    pub fn len(&self) -> usize {
        self.string.len()
    }

    /// Returns true if the string is empty
    pub fn is_empty(&self) -> bool {
        self.string.is_empty()
    }
}

/// An attribute that specifies the locale for a range of text
///
/// Similar to Flutter's `LocaleStringAttribute`. This helps screen readers
/// pronounce text correctly in different languages.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::{LocaleStringAttribute, StringAttribute};
///
/// let attr = LocaleStringAttribute::new(0, 5, "en-US");
/// assert_eq!(attr.range(), (0, 5));
/// assert_eq!(attr.locale(), "en-US");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LocaleStringAttribute {
    start: usize,
    end: usize,
    locale: String,
}

impl LocaleStringAttribute {
    /// Creates a new locale attribute
    pub fn new(start: usize, end: usize, locale: impl Into<String>) -> Self {
        Self {
            start,
            end,
            locale: locale.into(),
        }
    }

    /// Returns the locale
    pub fn locale(&self) -> &str {
        &self.locale
    }
}

impl StringAttribute for LocaleStringAttribute {
    fn range(&self) -> (usize, usize) {
        (self.start, self.end)
    }

    fn attribute_type(&self) -> &str {
        "locale"
    }
}

/// An attribute that indicates text should be spelled out character by character
///
/// Similar to Flutter's `SpellOutStringAttribute`. Useful for things like
/// acronyms or codes that should be pronounced letter-by-letter.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::{SpellOutStringAttribute, StringAttribute};
///
/// let attr = SpellOutStringAttribute::new(0, 3); // For "FBI"
/// assert_eq!(attr.range(), (0, 3));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpellOutStringAttribute {
    start: usize,
    end: usize,
}

impl SpellOutStringAttribute {
    /// Creates a new spell-out attribute
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

impl StringAttribute for SpellOutStringAttribute {
    fn range(&self) -> (usize, usize) {
        (self.start, self.end)
    }

    fn attribute_type(&self) -> &str {
        "spell_out"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attributed_string_new() {
        let text = AttributedString::new("Hello");
        assert_eq!(text.string(), "Hello");
        assert_eq!(text.len(), 5);
        assert!(!text.is_empty());
    }

    #[test]
    fn test_attributed_string_empty() {
        let text = AttributedString::new("");
        assert!(text.is_empty());
        assert_eq!(text.len(), 0);
    }

    #[test]
    fn test_locale_attribute() {
        let attr = LocaleStringAttribute::new(0, 5, "fr-FR");
        assert_eq!(attr.range(), (0, 5));
        assert_eq!(attr.locale(), "fr-FR");
        assert_eq!(attr.attribute_type(), "locale");
    }

    #[test]
    fn test_spell_out_attribute() {
        let attr = SpellOutStringAttribute::new(10, 15);
        assert_eq!(attr.range(), (10, 15));
        assert_eq!(attr.attribute_type(), "spell_out");
    }
}
