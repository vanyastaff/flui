use flui_sdk::animation::MotionValue;

#[derive(Clone, MotionValue)]
struct Empty;

#[derive(Clone, MotionValue)]
struct EmptyNamed {}

#[derive(Clone, MotionValue)]
struct EmptyTuple();

fn main() {}
