//! String interning for O(1) comparison (using lasso crate)

use lasso::{Spur, ThreadedRodeo};
use once_cell::sync::Lazy;

/// Global thread-safe string interner
static INTERNER: Lazy<ThreadedRodeo> = Lazy::new(ThreadedRodeo::default);

/// Interned string handle (4 bytes, Copy, O(1) comparison)
pub type InternedString = Spur;

/// Intern string (O(1) amortized, returns handle)
pub fn intern(s: &str) -> InternedString {
    INTERNER.get_or_intern(s)
}

/// Resolve handle to &str
pub fn resolve(key: InternedString) -> &'static str {
    // SAFETY: ThreadedRodeo guarantees that interned strings live for the lifetime of the interner,
    // which is 'static since INTERNER is a static Lazy. The returned &str is valid for 'static.
    unsafe {
        let rodeo = &*INTERNER;
        let s = rodeo.resolve(&key);
        // Extend lifetime to 'static (safe because interner is static)
        std::mem::transmute::<&str, &'static str>(s)
    }
}

/// Check if string already interned (returns handle if yes)
pub fn try_get(s: &str) -> Option<InternedString> {
    INTERNER.get(s)
}

/// Get number of interned strings
pub fn len() -> usize {
    INTERNER.len()
}

/// Check if the interner is empty
pub fn is_empty() -> bool {
    INTERNER.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_string_twice() {
        let s1 = intern("test");
        let s2 = intern("test");
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_intern_different_strings() {
        let s1 = intern("foo");
        let s2 = intern("bar");
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_resolve() {
        let handle = intern("MyWidget");
        let s = resolve(handle);
        assert_eq!(s, "MyWidget");
    }

    #[test]
    fn test_try_get_existing() {
        let handle = intern("existing");
        assert_eq!(try_get("existing"), Some(handle));
    }

    #[test]
    fn test_try_get_non_existing() {
        // Use a unique string that's unlikely to have been interned before
        let unique = format!("non_existing_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        assert_eq!(try_get(&unique), None);
    }

    #[test]
    fn test_comparison_performance() {
        // Intern some widget type names
        let container = intern("Container");
        let row = intern("Row");
        let column = intern("Column");
        let text = intern("Text");

        // Fast O(1) comparisons
        assert_eq!(container, intern("Container"));
        assert_ne!(container, row);
        assert_ne!(row, column);
        assert_eq!(text, intern("Text"));
    }

    #[test]
    fn test_widget_type_names() {
        // Simulate widget type name interning
        let widget_types = vec![
            "Container", "Row", "Column", "Stack", "Text",
            "Image", "Button", "TextField", "Checkbox", "Radio"
        ];

        let interned: Vec<_> = widget_types.iter().map(|s| intern(s)).collect();

        // Verify all strings are interned
        for (i, &s) in widget_types.iter().enumerate() {
            assert_eq!(interned[i], intern(s));
        }
    }
}
