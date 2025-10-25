//! Sealed trait v2 - Using const discriminators for disjoint impls
//!
//! This approach uses const generics to make widget types provably disjoint,
//! allowing multiple blanket implementations without conflicts.
//!
//! # The Key Insight
//!
//! By adding a const generic discriminator to each widget trait, we make them
//! structurally different types that the compiler can distinguish:
//!
//! ```text
//! StatelessWidget   -> Widget<DISC = 0>
//! StatefulWidget    -> Widget<DISC = 1>
//! InheritedWidget   -> Widget<DISC = 2>
//! ParentDataWidget  -> Widget<DISC = 3>
//! RenderObjectWidget -> Widget<DISC = 4>
//! ```
//!
//! Now the compiler can prove these don't overlap!

use crate::element::DynElement;

/// Widget type discriminators
///
/// These const values make each widget type structurally distinct,
/// preventing blanket impl conflicts.
pub mod disc {
    pub const STATELESS: u8 = 0;
    pub const STATEFUL: u8 = 1;
    pub const INHERITED: u8 = 2;
    pub const PARENT_DATA: u8 = 3;
    pub const RENDER_OBJECT: u8 = 4;
}

/// Sealed trait with const discriminator
///
/// The discriminator makes different widget types provably disjoint,
/// allowing multiple blanket implementations without conflicts.
///
/// # Type Safety
///
/// The const discriminator is purely compile-time - there's zero runtime cost.
/// It's used only for type-level reasoning by the compiler.
pub trait Sealed {
    /// The concrete Element type created by this Widget
    type ElementType: DynElement + Send + Sync + 'static;

    /// Const discriminator that makes widget types disjoint
    ///
    /// Each widget trait gets a unique value:
    /// - StatelessWidget: 0
    /// - StatefulWidget (via Stateful): 1
    /// - InheritedWidget: 2
    /// - ParentDataWidget: 3
    /// - RenderObjectWidget: 4
    const DISCRIMINATOR: u8;
}
