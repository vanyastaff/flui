// The entry attribute writes `include_bytes!` by its `::core` path, so an
// application's own macro of that name is not what it expands. The `cfg!` of a
// feature's declarations is written the same way; a trybuild manifest declares
// nothing, so flui-macros' `a_features_part_tests_its_feature_through_core`
// reads that expansion.
#[allow(unused_macros)]
macro_rules! include_bytes {
    ($($tokens:tt)*) => {
        compile_error!("the application's own `include_bytes!` was expanded")
    };
}

#[flui::main]
pub fn run() {}

fn main() {
    run();
}
