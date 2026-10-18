//! The iOS version the process runs on, read once.

use objc2_foundation::NSProcessInfo;

use crate::shared::apple_os_version::OsVersion;

/// `NSProcessInfo.operatingSystemVersion`. The platform reads it when it is
/// built and keeps the value; a feature newer than the deployment target is
/// then gated on a proof from [`OsVersion::since`].
pub(super) fn read() -> OsVersion {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    let component = |value: isize| i64::try_from(value).unwrap_or(0);
    OsVersion::from_components(
        component(version.majorVersion),
        component(version.minorVersion),
        component(version.patchVersion),
    )
}
