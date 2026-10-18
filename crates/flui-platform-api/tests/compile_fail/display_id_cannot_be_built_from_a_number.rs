use std::num::NonZeroU64;

use flui_platform_api::DisplayId;

fn main() {
    let raw = NonZeroU64::MIN;
    let _ = DisplayId(raw);
    let _ = DisplayId::from(raw);
    let _: DisplayId = raw.into();
    let _: DisplayId = 1u64.into();
    let _ = DisplayId::new(1);
}
