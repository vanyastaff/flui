use flui_sdk::{animation::MotionValue, geometry::Lerp};

#[derive(Clone, MotionValue)]
struct Generic<T: MotionValue + Lerp>(T);

#[derive(Clone, MotionValue)]
struct GenericArray<const N: usize>([f64; N]);

#[derive(Clone, MotionValue)]
struct Borrowed<'a>(f64, std::marker::PhantomData<&'a ()>);

fn main() {}
