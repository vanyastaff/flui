use flui_sdk::animation::TwoWayConverter;

#[derive(Clone, TwoWayConverter)]
struct Empty;

#[derive(Clone, TwoWayConverter)]
struct EmptyNamed {}

#[derive(Clone, TwoWayConverter)]
struct EmptyTuple();

fn main() {}
