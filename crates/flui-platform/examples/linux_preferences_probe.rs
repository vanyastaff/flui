//! Linux preferences through the public owner, with an isolated fixture portal.
//!
//! Run inside dbus-run-session and xvfb-run:
//! `cargo run -p flui-platform --features winit-backend --example linux_preferences_probe`.

#[cfg(target_os = "linux")]
#[path = "../tests/support/linux_preferences.rs"]
mod fixture;

fn main() {
    #[cfg(target_os = "linux")]
    {
        if let Some(case) = std::env::args().nth(1) {
            assert!(
                fixture::linux::CASES.contains(&case.as_str()),
                "unknown case"
            );
            fixture::linux::run_case(&case);
        } else {
            for case in fixture::linux::CASES {
                let status =
                    std::process::Command::new(std::env::current_exe().expect("probe executable"))
                        .arg(case)
                        .status()
                        .expect("isolated owner process");
                assert!(status.success(), "Linux preference case {case} failed");
            }
            println!("LINUX_PREFERENCES_PASS");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("linux_preferences_probe requires Linux");
        std::process::exit(1);
    }
}
