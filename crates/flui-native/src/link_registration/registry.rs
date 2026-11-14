//! The registry behind [`Classes`]: the user's `HKCU\Software\Classes`,
//! read through the merged `HKEY_CLASSES_ROOT` view so a scheme the machine
//! registered counts as taken, and the process's package identity.
#![expect(
    unsafe_code,
    reason = "the registry and the package identity are Win32 calls, which have no safe wrapper"
)]

use windows::Win32::Foundation::{
    APPMODEL_ERROR_NO_PACKAGE, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
    WIN32_ERROR,
};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_CREATE_KEY_DISPOSITION,
    REG_CREATED_NEW_KEY, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::{Claim, Classes, Entry, Found, MARKER, Wrote};

/// Where the user's classes are, under `HKEY_CURRENT_USER`.
const USER_CLASSES: &str = "Software\\Classes";

/// The longest key name the registry holds, in UTF-16 units, without the
/// terminating NUL.
const MAX_KEY_NAME: usize = 255;

/// The user's classes, read through the merged view.
pub(super) struct UserClasses;

/// An open registry key, closed when it drops.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a key this value opened or created and closes
        // exactly once, here.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// `status` as a result.
fn checked(status: WIN32_ERROR) -> std::io::Result<()> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(
            i32::try_from(status.0).unwrap_or(i32::MAX),
        ))
    }
}

/// Open `path` under `root` for `access`; `None` when no key has the path.
fn open(
    root: HKEY,
    path: &str,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> std::io::Result<Option<Key>> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `key` is a stack-local the call writes the opened key to.
    let status = unsafe { RegOpenKeyExW(root, &path, None, access, &raw mut key) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(status)?;
    Ok(Some(Key(key)))
}

/// The string value `name` (`""` for the default) of `subkey` (`""` for
/// `key` itself); `None` when it is absent.
fn string(key: &Key, subkey: &str, name: &str) -> std::io::Result<Option<String>> {
    let subkey = HSTRING::from(subkey);
    let name = HSTRING::from(name);
    let subkey = if subkey.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(subkey.as_ptr())
    };
    let name = if name.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(name.as_ptr())
    };
    // A value that keeps growing between the size and the read is given up
    // on after a few tries.
    for _ in 0..4 {
        let mut size = 0_u32;
        // SAFETY: `subkey` and `name` are null or NUL-terminated strings that
        // outlive the call; with no data pointer the call only writes the
        // value's size in bytes to the stack-local `size`.
        let status = unsafe {
            RegGetValueW(
                key.0,
                subkey,
                name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&raw mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        let mut units = vec![0_u16; (size as usize).div_ceil(2)];
        let mut written = size;
        // SAFETY: as above; `units` holds `size` bytes, the capacity `written`
        // states, and `RRF_RT_REG_SZ` admits only a string, which the call
        // terminates within that capacity.
        let status = unsafe {
            RegGetValueW(
                key.0,
                subkey,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(units.as_mut_ptr().cast()),
                Some(&raw mut written),
            )
        };
        if status == ERROR_MORE_DATA {
            // The value grew between the two calls: read its new size.
            continue;
        }
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        let len = units
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(units.len());
        return Ok(Some(String::from_utf16_lossy(&units[..len])));
    }
    Err(std::io::Error::from_raw_os_error(
        i32::try_from(ERROR_MORE_DATA.0).unwrap_or(i32::MAX),
    ))
}

/// Create or open `path` under the user's hive for writing, and say whether
/// this call created it.
fn create(path: &str) -> std::io::Result<(Key, bool)> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `key` and `disposition` are stack-locals the call writes the created or
    // opened key and whether it created it to.
    checked(unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &path,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &raw mut key,
            Some(&raw mut disposition),
        )
    })?;
    Ok((Key(key), disposition == REG_CREATED_NEW_KEY))
}

/// Set the string value `name` (`""` for the default) of `key` to `data`.
fn set_string(key: &Key, name: &str, data: &str) -> std::io::Result<()> {
    let name = HSTRING::from(name);
    let name = if name.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(name.as_ptr())
    };
    let data: Vec<u8> = data
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: `name` is null or a NUL-terminated string that outlives the
    // call, and `data` is the value's UTF-16 with its terminating NUL, the
    // form `REG_SZ` holds.
    checked(unsafe { RegSetValueExW(key.0, name, None, REG_SZ, Some(&data)) })
}

impl Classes for UserClasses {
    fn find(&self, name: &str) -> std::io::Result<Found> {
        let Some(key) = open(HKEY_CLASSES_ROOT, name, KEY_READ)? else {
            return Ok(Found::Absent);
        };
        Ok(match string(&key, "", MARKER)? {
            Some(app_id) => Found::Marked {
                app_id,
                command: string(&key, super::COMMAND, "")?,
            },
            None => Found::Unmarked,
        })
    }

    fn write(&mut self, name: &str, entries: &[Entry], claim: Claim) -> std::io::Result<Wrote> {
        let root = format!("{USER_CLASSES}\\{name}");
        let (key, created) = create(&root)?;
        if claim == Claim::New && !created {
            // Another process made the key since it was read: it is theirs.
            return Ok(Wrote::Taken);
        }
        let written = entries.iter().try_for_each(|entry| {
            if entry.subkey.is_empty() {
                set_string(&key, entry.name, &entry.data)
            } else {
                let (subkey, _) = create(&format!("{root}\\{}", entry.subkey))?;
                set_string(&subkey, entry.name, &entry.data)
            }
        });
        drop(key);
        match written {
            Ok(()) => Ok(Wrote::Written),
            Err(error) => {
                // A key this write created goes with it, so no empty or
                // unmarked key is left for the next run to refuse.
                if created && let Err(rollback) = self.delete(name) {
                    tracing::warn!(%rollback, key = %name, "a key left by a failed registration cannot be deleted");
                }
                Err(error)
            }
        }
    }

    fn delete(&mut self, name: &str) -> std::io::Result<()> {
        let path = HSTRING::from(format!("{USER_CLASSES}\\{name}"));
        // SAFETY: `path` is a NUL-terminated string that outlives the call.
        // Named by a subkey, the call deletes that key with its subkeys and
        // values.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &path) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        checked(status)
    }

    fn marked_by(&self, app_id: &str) -> std::io::Result<Vec<String>> {
        let Some(classes) = open(HKEY_CURRENT_USER, USER_CLASSES, KEY_READ)? else {
            return Ok(Vec::new());
        };
        let mut marked = Vec::new();
        let mut name = [0_u16; MAX_KEY_NAME + 1];
        for index in 0_u32.. {
            let mut len = u32::try_from(name.len()).expect("BUG: a key name buffer is 256 units");
            // SAFETY: `name` holds `len` UTF-16 units, the capacity the call
            // is told, and the call writes the key's name and its length
            // without the NUL there.
            let status = unsafe {
                RegEnumKeyExW(
                    classes.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            checked(status)?;
            let key_name = String::from_utf16_lossy(&name[..len as usize]);
            // A key whose marker cannot be read as a string (another
            // program's value of that name, a key it guards) is not this
            // application's.
            if string(&classes, &key_name, MARKER)
                .ok()
                .flatten()
                .is_some_and(|marker| marker == app_id)
            {
                marked.push(key_name);
            }
        }
        Ok(marked)
    }
}

/// Whether the process has a package identity: `GetCurrentPackageFullName`
/// answers `APPMODEL_ERROR_NO_PACKAGE` for one without.
pub(super) fn runs_packaged() -> bool {
    let mut len = 0_u32;
    // SAFETY: with no buffer the call only writes the name's length to the
    // stack-local `len`.
    let status = unsafe { GetCurrentPackageFullName(&raw mut len, None) };
    status != APPMODEL_ERROR_NO_PACKAGE
}
