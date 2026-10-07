use flui_platform::OwnerPlatform;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<OwnerPlatform>();
}
