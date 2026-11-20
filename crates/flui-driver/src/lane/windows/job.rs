//! The kill-on-close job object a launched app runs in: when the driver's handle to the job
//! closes, Windows ends every process in it, the app and whatever it started. The handle closes
//! when the session drops it and when the driver's process ends however it ends, so an app never
//! outlives the driver that launched it.
//!
//! The app starts suspended ([`KillOnCloseJob::spawn_suspended`]), joins the job, and only then
//! runs its first instruction ([`SuspendedChild::resume`]): nothing it starts can be outside the
//! job, however fast it starts it.
#![expect(
    unsafe_code,
    reason = "Win32 job objects and thread resumption; each call takes an owned handle or a local"
)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, ExitStatus};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::BOOL;

use crate::error::{Error, Result};

/// A job whose processes end when it is dropped: see the [module docs](self).
#[derive(Debug)]
pub struct KillOnCloseJob {
    job: OwnedHandle,
}

/// A process the job holds that has not run yet: [`Self::resume`] starts it; dropped instead, it
/// is ended before it ever ran.
#[derive(Debug)]
#[must_use = "a suspended process runs only once resumed"]
pub struct SuspendedChild {
    child: Option<Child>,
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

    /// Start `command` suspended and put it in the job: see the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when it cannot start, or Windows refuses to put it in the job, which then
    /// ends it before it ran.
    pub fn spawn_suspended(&self, command: &mut Command) -> Result<SuspendedChild> {
        let child = command.creation_flags(CREATE_SUSPENDED.0).spawn()?;
        let suspended = SuspendedChild { child: Some(child) };
        self.assign(suspended.child())?;
        Ok(suspended)
    }

    /// Put `child` in the job; a child already in it (started by a process in the job) counts.
    fn assign(&self, child: &Child) -> Result<()> {
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

impl SuspendedChild {
    fn child(&self) -> &Child {
        self.child
            .as_ref()
            .expect("BUG: a suspended child is held until it is resumed or dropped")
    }

    /// The process id.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child().id()
    }

    /// Whether the process has ended, without waiting: a suspended one has not.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when its state cannot be read.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self
            .child
            .as_mut()
            .expect("BUG: a suspended child is held until it is resumed or dropped")
            .try_wait()?)
    }

    /// Let the process run its first instruction.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when its thread cannot be resumed; the process is ended then.
    pub fn resume(mut self) -> Result<Child> {
        let mut child = self
            .child
            .take()
            .expect("BUG: a suspended child is held until it is resumed or dropped");
        if let Err(error) = resume_threads(child.id()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(child)
    }
}

impl Drop for SuspendedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Resume every thread of the process `pid`: a process started suspended has one, its first.
fn resume_threads(pid: u32) -> Result<()> {
    // SAFETY: a snapshot of the system's threads; the handle is owned below.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(io)?;
    // SAFETY: a handle the call just returned, owned by nothing else.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).expect("BUG: a thread entry is small"),
        ..THREADENTRY32::default()
    };
    let mut resumed = 0_usize;
    // SAFETY: a live snapshot and a local entry with its size set, as the call requires.
    let mut next = unsafe { Thread32First(HANDLE(snapshot.as_raw_handle()), &raw mut entry) };
    while next.is_ok() {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: a thread id the snapshot listed; the handle is owned below.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                .map_err(io)?;
            // SAFETY: a handle `OpenThread` just returned, owned by nothing else.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread.0) };
            // SAFETY: a live thread handle with the right to resume it.
            if unsafe { ResumeThread(HANDLE(thread.as_raw_handle())) } == u32::MAX {
                return Err(io(windows::core::Error::from_thread()));
            }
            resumed += 1;
        }
        // SAFETY: as above.
        next = unsafe { Thread32Next(HANDLE(snapshot.as_raw_handle()), &raw mut entry) };
    }
    if resumed == 0 {
        return Err(Error::Io(std::io::Error::other(format!(
            "process {pid} has no thread to resume"
        ))));
    }
    Ok(())
}

fn io(error: windows::core::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}
