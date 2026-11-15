//! An application whose window options ask for numbers a window command
//! would be refused for does not start: its main window is refused with the
//! error that names the option and its numbers, on the headless platform as
//! on every host.

use flui_app::app::{AppConfig, AppWindowError, Application, LaunchError};
use flui_app::testing::HeadlessRun;
use flui_native::OpenWindowError;
use flui_platform::{InvalidGeometry, WindowExtent, WindowOption};
use flui_widgets::SizedBox;

/// What starting an application with `config` fails with: the option the
/// main window's open refused, and why.
fn refused_start(config: AppConfig) -> (WindowOption, InvalidGeometry, String) {
    let run = HeadlessRun::start(Application::new().config(config), SizedBox::new(10.0, 10.0));
    let error = run.join().expect_err("the application does not start");
    let message = error.to_string();
    let LaunchError::InitialWindow(AppWindowError::Native { source }) = error else {
        panic!("the main window's open is refused: {message}");
    };
    let Some(OpenWindowError::InvalidOption(refused)) = source.downcast_ref::<OpenWindowError>()
    else {
        panic!("refused for its options: {message}");
    };
    (refused.option, refused.refused, message)
}

fn extent(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(flui_foundation::geometry::Size::new(width, height)).expect("an extent")
}

pub(crate) fn a_minimum_with_a_zero_side_refuses_the_start() {
    let (option, refused, message) = refused_start(AppConfig::new().min_size(0, 600));
    assert_eq!(option, WindowOption::MinSize);
    assert_eq!(
        refused,
        InvalidGeometry::Empty {
            width: 0.0,
            height: 600.0
        }
    );
    assert!(
        message.contains("`min_size`") && message.contains("0x600"),
        "the error names the option and its numbers: {message}"
    );
}

pub(crate) fn a_minimum_above_the_maximum_refuses_the_start() {
    let (option, refused, message) =
        refused_start(AppConfig::new().min_size(900, 700).max_size(400, 300));
    assert_eq!(option, WindowOption::Limits);
    assert_eq!(
        refused,
        InvalidGeometry::Limits {
            min: extent(900.0, 700.0),
            max: extent(400.0, 300.0)
        }
    );
    assert!(
        message.contains("`min_size and max_size`"),
        "the error names the options: {message}"
    );
}
