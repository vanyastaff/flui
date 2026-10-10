//! Main-thread public owner contracts with a private portal for every case.

#[cfg(target_os = "linux")]
#[path = "support/linux_preferences.rs"]
mod fixture;

fn main() {
    #[cfg(target_os = "linux")]
    {
        let mut arguments = libtest_mimic::Arguments::from_args();
        arguments.test_threads = Some(1);
        let tests = fixture::linux::CASES
            .iter()
            .map(|&name| {
                libtest_mimic::Trial::test(name, move || {
                    if std::env::var_os("FLUI_PRIVATE_PORTAL_TEST_BUS").is_some() {
                        fixture::linux::run_case(name);
                        Ok(())
                    } else {
                        let status = std::process::Command::new("dbus-run-session")
                            .arg("--")
                            .arg(std::env::current_exe()?)
                            .args(["--exact", name, "--nocapture"])
                            .env("FLUI_PRIVATE_PORTAL_TEST_BUS", "1")
                            .status()?;
                        if status.success() {
                            Ok(())
                        } else {
                            Err(format!("isolated portal case failed: {status}").into())
                        }
                    }
                })
            })
            .collect();
        libtest_mimic::run(&arguments, tests).exit();
    }
}
