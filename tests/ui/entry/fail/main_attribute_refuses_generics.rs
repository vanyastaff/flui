// A generic entry has no one function for `android_main` to call.
#[flui::main]
pub fn run<T: Default>() -> Result<(), flui::LaunchError> {
    let _ = T::default();
    Ok(())
}

// A clause without a predicate has only its keyword to point at.
#[flui::main]
pub fn start() -> Result<(), flui::LaunchError> where {
    Ok(())
}

fn main() {}
