//! Quartz window identities independent of screen visibility or capture support.

use std::collections::{HashMap, HashSet};

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    create_description_from_array, create_window_list, kCGNullWindowID, kCGWindowListOptionAll,
    kCGWindowNumber, kCGWindowOwnerPID,
};

use crate::error::{ToolError, ToolResult};

/// All native window owners, including minimized and offscreen windows.
/// A failed or incomplete snapshot is an error, never evidence of closure.
pub fn window_owners() -> ToolResult<HashMap<u32, u32>> {
    let failed = || {
        ToolError::platform(
            "listing native window owners",
            "Quartz could not return a complete window snapshot",
        )
    };
    let windows = create_window_list(kCGWindowListOptionAll, kCGNullWindowID).ok_or_else(failed)?;
    let expected: HashSet<u32> = windows.iter().map(|id| *id).collect();
    let descriptions = create_description_from_array(windows).ok_or_else(failed)?;
    // SAFETY: Quartz exports these immutable CFString constants for the
    // process lifetime. Get-rule wrappers retain them while keys are used.
    #[expect(
        unsafe_code,
        reason = "Quartz dictionary keys are immutable process-lifetime CFString constants"
    )]
    let (number, owner) = unsafe {
        (
            CFString::wrap_under_get_rule(kCGWindowNumber),
            CFString::wrap_under_get_rule(kCGWindowOwnerPID),
        )
    };
    owners_from_descriptions(
        descriptions.iter().map(|row| (*row).clone()),
        &expected,
        &number,
        &owner,
    )
}

fn owners_from_descriptions(
    descriptions: impl IntoIterator<Item = CFDictionary<CFString, CFType>>,
    expected: &HashSet<u32>,
    number: &CFString,
    owner: &CFString,
) -> ToolResult<HashMap<u32, u32>> {
    let mut owners = HashMap::new();
    for row in descriptions {
        let read = |key: &CFString| {
            row.find(key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|value| value.to_i64())
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value != 0)
        };
        let Some((id, pid)) = read(number).zip(read(owner)) else {
            return Err(ToolError::platform(
                "listing native window owners",
                "Quartz returned an invalid window number or owner pid",
            ));
        };
        if !expected.contains(&id) || owners.insert(id, pid).is_some() {
            return Err(ToolError::platform(
                "listing native window owners",
                "Quartz returned inconsistent window identities",
            ));
        }
    }
    if owners.len() != expected.len() {
        return Err(ToolError::Busy(
            "a window disappeared while Quartz was reading its owner; retry the snapshot".into(),
        ));
    }
    Ok(owners)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_windows_still_have_owners() {
        let number = CFString::new("number");
        let owner = CFString::new("owner");
        let row = CFDictionary::from_CFType_pairs(&[
            (number.clone(), CFNumber::from(17_i32).as_CFType()),
            (owner.clone(), CFNumber::from(42_i32).as_CFType()),
            (
                CFString::new("kCGWindowIsOnscreen"),
                CFNumber::from(0_i32).as_CFType(),
            ),
        ]);
        let owners = owners_from_descriptions([row], &HashSet::from([17]), &number, &owner)
            .expect("BUG: offscreen windows retain their native identity");
        assert_eq!(owners.get(&17), Some(&42));
    }

    #[test]
    fn incomplete_and_malformed_snapshots_are_not_closure_evidence() {
        let number = CFString::new("number");
        let owner = CFString::new("owner");
        let expected = HashSet::from([17]);
        assert!(owners_from_descriptions([], &expected, &number, &owner).is_err());
        for value in [
            CFString::new("42").as_CFType(),
            CFNumber::from(-1_i32).as_CFType(),
            CFNumber::from(0_i32).as_CFType(),
            CFNumber::from(i64::from(u32::MAX) + 1).as_CFType(),
        ] {
            let row = CFDictionary::from_CFType_pairs(&[
                (number.clone(), CFNumber::from(17_i32).as_CFType()),
                (owner.clone(), value),
            ]);
            assert!(owners_from_descriptions([row], &expected, &number, &owner).is_err());
        }
        let row = CFDictionary::from_CFType_pairs(&[
            (number.clone(), CFNumber::from(17_i32).as_CFType()),
            (owner.clone(), CFNumber::from(42_i32).as_CFType()),
        ]);
        assert!(
            owners_from_descriptions([row.clone(), row.clone()], &expected, &number, &owner)
                .is_err()
        );
        assert!(owners_from_descriptions([row], &HashSet::from([18]), &number, &owner).is_err());
    }
}
