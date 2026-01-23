//! Edge utilities for box model calculations.
//!
//! This module provides [`Edges`] for representing values associated with
//! the four edges of a rectangle (top, right, bottom, left). Common uses
//! include padding, margin, borders, and insets.

use std::fmt::{self, Debug};
use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Edges<T = f32> {
    /// The top edge value.
    pub top: T,
    /// The right edge value.
    pub right: T,
    /// The bottom edge value.
    pub bottom: T,
    /// The left edge value.
    pub left: T,
}

#[inline]
pub const fn edges<T>(top: T, right: T, bottom: T, left: T) -> Edges<T> {
    Edges {
        top,
        right,
        bottom,
        left,
    }
}

impl<T> Edges<T> {
    #[inline]
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    #[inline]
    pub fn all(value: T) -> Self
    where
        T: Clone,
    {
        Self {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }

    #[inline]
    pub fn symmetric(vertical: T, horizontal: T) -> Self
    where
        T: Clone,
    {
        Self {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical,
            left: horizontal,
        }
    }

    #[inline]
    pub fn horizontal(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top: T::default(),
            right: value.clone(),
            bottom: T::default(),
            left: value,
        }
    }

    #[inline]
    pub fn vertical(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top: value.clone(),
            right: T::default(),
            bottom: value,
            left: T::default(),
        }
    }

    #[inline]
    pub fn horizontal_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.left + self.right
    }

    #[inline]
    pub fn vertical_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.top + self.bottom
    }

    #[must_use]
    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Edges<U> {
        Edges {
            top: f(&self.top),
            right: f(&self.right),
            bottom: f(&self.bottom),
            left: f(&self.left),
        }
    }

    #[must_use]
    pub fn any(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) || f(&self.right) || f(&self.bottom) || f(&self.left)
    }

    #[must_use]
    pub fn all_satisfy(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) && f(&self.right) && f(&self.bottom) && f(&self.left)
    }
}

// ============================================================================
// f32-specific methods
// ============================================================================

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Edges<super::units::Pixels> {
    #[must_use]
    pub fn scale(&self, factor: f32) -> Edges<super::units::ScaledPixels> {
        Edges {
            top: self.top.scale(factor),
            right: self.right.scale(factor),
            bottom: self.bottom.scale(factor),
            left: self.left.scale(factor),
        }
    }
}

// Arithmetic operators
impl<T> Add for Edges<T>
where
    T: Add<Output = T>,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top + rhs.top,
            right: self.right + rhs.right,
            bottom: self.bottom + rhs.bottom,
            left: self.left + rhs.left,
        }
    }
}

impl<T> AddAssign for Edges<T>
where
    T: AddAssign + Copy,
{
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.top += rhs.top;
        self.right += rhs.right;
        self.bottom += rhs.bottom;
        self.left += rhs.left;
    }
}

impl<T> Sub for Edges<T>
where
    T: Sub<Output = T>,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top - rhs.top,
            right: self.right - rhs.right,
            bottom: self.bottom - rhs.bottom,
            left: self.left - rhs.left,
        }
    }
}

impl<T> SubAssign for Edges<T>
where
    T: SubAssign + Copy,
{
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.top -= rhs.top;
        self.right -= rhs.right;
        self.bottom -= rhs.bottom;
        self.left -= rhs.left;
    }
}

impl<T> Mul for Edges<T>
where
    T: Mul<Output = T> + Clone,
{
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top.clone() * rhs.top,
            right: self.right.clone() * rhs.right,
            bottom: self.bottom.clone() * rhs.bottom,
            left: self.left * rhs.left,
        }
    }
}

impl<T, S> MulAssign<S> for Edges<T>
where
    T: Mul<S, Output = T> + Clone,
    S: Clone,
{
    #[inline]
    fn mul_assign(&mut self, rhs: S) {
        self.top = self.top.clone() * rhs.clone();
        self.right = self.right.clone() * rhs.clone();
        self.bottom = self.bottom.clone() * rhs.clone();
        self.left = self.left.clone() * rhs;
    }
}

// ============================================================================
// Along trait - Axis-based access
// ============================================================================

impl<T: Clone> super::traits::Along for Edges<T> {
    type Unit = (T, T);

    #[inline]
    fn along(&self, axis: super::traits::Axis) -> Self::Unit {
        match axis {
            super::traits::Axis::Horizontal => (self.left.clone(), self.right.clone()),
            super::traits::Axis::Vertical => (self.top.clone(), self.bottom.clone()),
        }
    }

    #[inline]
    fn apply_along(
        &self,
        axis: super::traits::Axis,
        f: impl FnOnce(Self::Unit) -> Self::Unit,
    ) -> Self {
        match axis {
            super::traits::Axis::Horizontal => {
                let (left, right) = f((self.left.clone(), self.right.clone()));
                Self {
                    top: self.top.clone(),
                    right,
                    bottom: self.bottom.clone(),
                    left,
                }
            }
            super::traits::Axis::Vertical => {
                let (top, bottom) = f((self.top.clone(), self.bottom.clone()));
                Self {
                    top,
                    right: self.right.clone(),
                    bottom,
                    left: self.left.clone(),
                }
            }
        }
    }
}

// ============================================================================
// Debug formatting
// ============================================================================

impl<T: Debug> Debug for Edges<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Edges")
            .field("top", &self.top)
            .field("right", &self.right)
            .field("bottom", &self.bottom)
            .field("left", &self.left)
            .finish()
    }
}

