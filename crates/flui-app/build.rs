//! Names the build that serves the driver agent (ADR-0239, ADR-0288 §7) once:
//! `cfg(driver_agent)` is a debug build, or one with the `driver` feature, on a
//! system whose transport is built (a local socket: a pipe on Windows, a Unix
//! socket elsewhere, Android's abstract one included; the web transport is not
//! built yet). The sites that compile the agent name this one cfg instead of
//! spelling the condition again.
//!
//! Read from what Cargo tells a build script about the target: it sets
//! `CARGO_CFG_DEBUG_ASSERTIONS` exactly when the profile has debug assertions,
//! so a custom profile that turns them off is read right (`cfg_aliases` falls
//! back to the profile's name there, and would not be).

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(driver_agent)");
    let set = |name: &str| std::env::var_os(name).is_some();
    let families = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let local_socket = families
        .split(',')
        .any(|family| family == "unix" || family == "windows");
    let built = set("CARGO_CFG_DEBUG_ASSERTIONS") || set("CARGO_FEATURE_DRIVER");
    if built && local_socket {
        println!("cargo::rustc-cfg=driver_agent");
    }
}
