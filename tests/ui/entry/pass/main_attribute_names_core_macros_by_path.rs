// The entry attribute writes `include_bytes!` and `cfg!` by their `::core`
// paths, so an application's own macros of those names are not what it expands.
#[allow(unused_macros)]
macro_rules! include_bytes {
    ($($tokens:tt)*) => {
        compile_error!("the application's own `include_bytes!` was expanded")
    };
}

#[allow(unused_macros)]
macro_rules! cfg {
    ($($tokens:tt)*) => {
        compile_error!("the application's own `cfg!` was expanded")
    };
}

#[flui::main]
pub fn run() {}

fn main() {
    run();
}
