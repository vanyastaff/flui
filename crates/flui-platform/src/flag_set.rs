//! Small sets of named flags, declared by the contract itself.
//!
//! A Stable signature names no upstream type outside ADR-0089 §2. The `bitflags`
//! macro gives every set it declares a public `bitflags::Flags` implementation
//! and returns `bitflags::iter` types from `iter` and `iter_names`, so the
//! contract's sets are declared with this macro instead. It generates the part
//! of that API the workspace uses, under the same names, and the same `Debug`
//! form (`Name(A | B)`, `Name(0x0)` when empty).

macro_rules! flag_set {
    (
        $(#[$outer:meta])*
        pub struct $name:ident: $bits:ty {
            $(
                $(#[$inner:meta])*
                const $flag:ident = $value:expr;
            )+
        }
    ) => {
        $(#[$outer])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct $name($bits);

        impl $name {
            $(
                $(#[$inner])*
                pub const $flag: Self = Self($value);
            )+

            /// Every flag with its name, in declaration order.
            const NAMED: &'static [(&'static str, Self)] = &[$((stringify!($flag), Self::$flag)),+];

            /// The set holding no flag.
            #[must_use]
            pub const fn empty() -> Self {
                Self(0)
            }

            /// The set holding every flag.
            #[must_use]
            pub const fn all() -> Self {
                Self(0 $(| $value)+)
            }

            /// The set's bits.
            #[must_use]
            pub const fn bits(self) -> $bits {
                self.0
            }

            /// The set of `bits`, or `None` when a bit names no flag.
            #[must_use]
            pub const fn from_bits(bits: $bits) -> Option<Self> {
                if bits & !Self::all().0 == 0 { Some(Self(bits)) } else { None }
            }

            /// Whether the set holds no flag.
            #[must_use]
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Whether every flag of `other` is in the set; always for the empty set.
            #[must_use]
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// The flags in either set.
            #[must_use]
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// Adds the flags of `other`.
            pub fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            /// Removes the flags of `other`.
            pub fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }

            /// Adds the flags of `other` when `value` is true, removes them otherwise.
            pub fn set(&mut self, other: Self, value: bool) {
                if value {
                    self.insert(other);
                } else {
                    self.remove(other);
                }
            }

            /// Each flag in the set with its name, in declaration order.
            pub fn iter_names(self) -> impl Iterator<Item = (&'static str, Self)> {
                Self::NAMED.iter().copied().filter(move |&(_, flag)| self.contains(flag))
            }
        }

        impl ::core::ops::BitOr for $name {
            type Output = Self;

            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl ::core::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, other: Self) {
                self.insert(other);
            }
        }

        impl ::core::ops::BitAnd for $name {
            type Output = Self;

            fn bitand(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }
        }

        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(concat!(stringify!($name), "("))?;
                if self.is_empty() {
                    write!(f, "{:#x}", self.0)?;
                } else {
                    for (i, (name, _)) in self.iter_names().enumerate() {
                        if i > 0 {
                            f.write_str(" | ")?;
                        }
                        f.write_str(name)?;
                    }
                }
                f.write_str(")")
            }
        }
    };
}

pub(crate) use flag_set;
