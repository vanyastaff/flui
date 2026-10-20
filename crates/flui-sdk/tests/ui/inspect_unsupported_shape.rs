use flui_sdk::foundation::inspect::Inspect;

#[derive(Inspect)]
union Bits {
    int: u32,
    float: f32,
}

#[derive(Inspect)]
struct SkippedWithUnit {
    #[inspect(skip, unit = logical_px)]
    width: f64,
}

#[derive(Inspect)]
struct DebugWithDefault {
    #[inspect(debug, default)]
    height: f64,
}

#[derive(Inspect)]
struct UnknownUnit {
    #[inspect(unit = inches)]
    depth: f64,
}

#[derive(Inspect)]
enum Shape {
    Circle { variant: f64 },
}

#[derive(Inspect)]
struct Renamed {
    width: f64,
    #[inspect(name = "width")]
    height: f64,
}

fn main() {}
