//! Windows process isolation via Win32 Job Objects and integrity limits.

#[derive(Debug)]
pub struct WindowsJobObject {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    max_memory_mb: Option<u64>,
}

unsafe impl Send for WindowsJobObject {}
unsafe impl Sync for WindowsJobObject {}

impl WindowsJobObject {
    pub fn max_memory_mb(&self) -> Option<u64> {
        self.max_memory_mb
    }

    #[cfg(windows)]
    pub fn assign_process(&self, process_handle: windows_sys::Win32::Foundation::HANDLE) -> bool {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        unsafe { AssignProcessToJobObject(self.handle, process_handle) != 0 }
    }

    #[cfg(not(windows))]
    pub fn assign_process(&self, _pid: u32) -> bool {
        false
    }
}

#[cfg(windows)]
impl Drop for WindowsJobObject {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

pub fn is_windows_job_object_supported() -> bool {
    cfg!(windows)
}

pub fn create_confined_job_object(max_memory_mb: Option<u64>) -> Option<WindowsJobObject> {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use std::ptr::null;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_JOB_MEMORY,
        };

        unsafe {
            let handle = CreateJobObjectW(null(), null());
            if handle.is_null() {
                return None;
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            if let Some(mb) = max_memory_mb {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                info.JobMemoryLimit = (mb * 1024 * 1024) as usize;
            }

            let res = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if res == 0 {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                return None;
            }

            Some(WindowsJobObject {
                handle,
                max_memory_mb,
            })
        }
    }
    #[cfg(not(windows))]
    {
        let _ = max_memory_mb;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_job_object_limits_and_support() {
        let supported = is_windows_job_object_supported();
        if cfg!(windows) {
            assert!(supported);
            let job = create_confined_job_object(Some(512));
            assert!(job.is_some());
            assert_eq!(job.unwrap().max_memory_mb(), Some(512));
        } else {
            assert!(!supported);
            assert!(create_confined_job_object(Some(512)).is_none());
        }
    }
}
