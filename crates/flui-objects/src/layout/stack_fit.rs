//! Stack layout types

/// How a `Stack` sizes its non-positioned children.
///
/// Mirrors Flutter's `StackFit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StackFit {
    /// The constraints passed to the stack from its parent are loosened
    /// (the default).
    ///
    /// Non-positioned children may be any size up to the stack's
    /// incoming maximum constraints.
    #[default]
    Loose,

    /// The constraints passed to the stack from its parent are tightened to the
    /// biggest size
    ///
    /// This forces the non-positioned children to be exactly as large as the
    /// stack's parent constraints.
    Expand,

    /// The non-positioned children are given unconstrained constraints
    ///
    /// This allows them to be any size they want.
    Passthrough,
}
