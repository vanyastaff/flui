//! The kill-on-close job object a launched app runs in: when the driver's handle to the job
//! closes, Windows ends every process in it, the app and whatever it started. The handle closes
//! when the session drops it and when the driver's process ends however it ends, so an app never
//! outlives the driver that launched it.
#![expect(
    unsafe_code,
    reason = "Win32 job objects; each call takes an owned handle or a local"
)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::core::BOOL;

use crate::error::{Error, Result};

/// A job whose processes end when it is dropped: see the [module docs](self).
#[derive(Debug)]
pub struct KillOnCloseJob {
    job: OwnedHandle,
}

impl KillOnCloseJob {
    /// A fresh job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when Windows refuses to create or configure it.
    pub fn try_new() -> Result<Self> {
        // SAFETY: an anonymous job with default security; the handle is owned below.
        let raw = unsafe { CreateJobObjectW(None, None) }.map_err(io)?;
        // SAFETY: a handle `CreateJobObjectW` just returned, owned by nothing else.
        let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the information class's struct, with its exact size, on a live job.
        unsafe {
            SetInformationJobObject(
                HANDLE(job.as_raw_handle()),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .expect("BUG: the limit information is a few hundred bytes"),
            )
        }
        .map_err(io)?;
        Ok(Self { job })
    }

    /// Put `child` in the job; a child already in it (started by a process in the job) counts.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when Windows refuses, and the child is in no job of this one's.
    pub fn assign(&self, child: &Child) -> Result<()> {
        let job = HANDLE(self.job.as_raw_handle());
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: both handles are open for the call: the job is owned by `self`, the process
        // by `child`.
        let assigned = unsafe { AssignProcessToJobObject(job, process) };
        if assigned.is_ok() {
            return Ok(());
        }
        let mut inside = BOOL::default();
        // SAFETY: as above; `inside` is a local the call writes.
        let checked = unsafe { IsProcessInJob(process, Some(job), &raw mut inside) };
        if checked.is_ok() && inside.as_bool() {
            return Ok(());
        }
        assigned.map_err(io)
    }
}

fn io(error: windows::core::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}
