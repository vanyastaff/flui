//! Stack layout types

/// How to size the non-positioned children in the stack
///
/// Stack widgets can contain both positioned and non-positioned children.
/// This enum controls how the non-positioned children are sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StackFit {
    /// The constraints passed to the stack from its parent are loosened
    ///
    /// This allows the non-positioned children to be smaller than the stack's parent constraints.
    #[default]
    Loose,

    /// The constraints passed to the stack from its parent are tightened to the biggest size
    ///
    /// This forces the non-positioned children to be exactly as large as the stack's parent constraints.
    Expand,

    /// The non-positioned children are given unconstrained constraints
    ///
    /// This allows them to be any size they want.
    Passthrough,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_fit_default() {
        assert_eq!(StackFit::default(), StackFit::Loose);
    }

    #[test]
    fn test_stack_fit_variants() {
        // Just ensure all variants are accessible
        let _ = StackFit::Loose;
        let _ = StackFit::Expand;
        let _ = StackFit::Passthrough;
    }

    #[test]
    fn test_stack_fit_equality() {
        assert_eq!(StackFit::Loose, StackFit::Loose);
        assert_ne!(StackFit::Loose, StackFit::Expand);
        assert_ne!(StackFit::Expand, StackFit::Passthrough);
    }

    #[test]
    fn test_stack_fit_clone() {
        let fit = StackFit::Expand;
        let cloned = fit.clone();
        assert_eq!(fit, cloned);
    }

    #[test]
    fn test_stack_fit_copy() {
        let fit = StackFit::Passthrough;
        let copied = fit;
        assert_eq!(fit, copied);
    }
}
