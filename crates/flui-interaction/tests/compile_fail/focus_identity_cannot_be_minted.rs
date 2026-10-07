use std::num::NonZeroU64;

use flui_interaction::FocusNodeId;

fn main() {
    let raw = NonZeroU64::MIN;
    let _ = FocusNodeId::new(1);
    let _ = FocusNodeId::try_new(1);
    let _ = FocusNodeId::from_non_zero(raw);
    let _ = FocusNodeId::from(raw);
    let _: FocusNodeId = raw.into();
    let _: Result<FocusNodeId, _> = raw.try_into();
}
