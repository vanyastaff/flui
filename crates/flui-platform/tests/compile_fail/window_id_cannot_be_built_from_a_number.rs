use std::num::NonZeroU64;

use flui_platform::WindowId;

fn main() {
    let raw = NonZeroU64::MIN;
    let _ = WindowId(raw);
    let _: WindowId = raw.into();
    let _: WindowId = 1u64.into();
    let _ = WindowId::new(1);
}
