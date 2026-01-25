//! Stack layout types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StackFit {
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
