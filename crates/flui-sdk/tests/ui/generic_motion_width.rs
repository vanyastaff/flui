use flui_sdk::animation::{Lerp, TwoWayConverter};

#[derive(Clone, TwoWayConverter)]
struct Generic<T: TwoWayConverter + Lerp>(T);

#[derive(Clone, TwoWayConverter)]
struct GenericArray<const N: usize>([f64; N]);

#[derive(Clone, TwoWayConverter)]
struct Borrowed<'a>(f64, std::marker::PhantomData<&'a ()>);

fn main() {}
