//! The limits an extension's program runs under: a Windows Job Object of its own. The program dies when the notch lets go
//! of it (also when the notch itself is killed), and it cannot start other programs (no `cmd`, no `powershell`, no
//! scripts), unless its manifest says `"processes": "allow"`. It is not a sandbox: the program still has the rights of the
//! user, so it can read the user's files and use the network. What this stops is an extension that brings other programs
//! along.
//!
//! The program is made suspended, put in the job, and only then let go, so there is no moment in which it could start
//! something before the limit holds.

use std::ffi::c_void;
use windows::core::{s, w};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// `CREATE_SUSPENDED`: the process exists but has not run a single instruction.
pub(crate) const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// The job. Dropping it ends the program in it.
pub(crate) struct Job(isize);

impl Job {
    /// A job whose program dies with it, and that may not start other programs unless `allow_children`.
    pub(crate) fn new(allow_children: bool) -> Option<Job> {
        unsafe {
            let h = CreateJobObjectW(None, w!("")).ok().filter(|h| !h.is_invalid())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | if allow_children { Default::default() } else { JOB_OBJECT_LIMIT_ACTIVE_PROCESS };
            // the program itself is the one process the job may hold
            info.BasicLimitInformation.ActiveProcessLimit = 1;
            let set = SetInformationJobObject(h, JobObjectExtendedLimitInformation, &info as *const _ as *const c_void, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
            if set.is_err() {
                let _ = CloseHandle(h);
                return None;
            }
            Some(Job(h.0 as isize))
        }
    }

    /// Put a process in the job.
    pub(crate) fn assign(&self, process: HANDLE) -> bool {
        unsafe { AssignProcessToJobObject(HANDLE(self.0 as *mut c_void), process).is_ok() }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(HANDLE(self.0 as *mut c_void));
        }
    }
}

/// Let a process that was made suspended run (all its threads; at the start there is only one).
pub(crate) fn resume(process: HANDLE) -> bool {
    unsafe {
        let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else { return false };
        let Some(f) = GetProcAddress(ntdll, s!("NtResumeProcess")) else { return false };
        let f: extern "system" fn(HANDLE) -> i32 = std::mem::transmute(f);
        f(process) >= 0
    }
}
