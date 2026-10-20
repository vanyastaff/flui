//! The three outcomes of a check.

vocabulary! {
    /// How a check came out. A check that could not run is `cannot_verify`, never `pass`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Verdict {
        /// Everything checked held.
        Pass => "pass",
        /// Something checked did not hold.
        Fail => "fail",
        /// Something asked for could not be checked.
        CannotVerify => "cannot_verify",
    }
}
