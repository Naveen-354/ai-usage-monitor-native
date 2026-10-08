#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};

#[derive(Debug)]
pub struct AlreadyRunning;

pub struct Guard {
    #[cfg(windows)]
    handle: HANDLE,
}

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn acquire() -> Result<Guard, AlreadyRunning> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        
        let name = "Local\\AIUsageMonitorSingleInstanceMutex";
        let mut wide_name: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
        wide_name.push(0);
        
        // Define CreateMutexW directly to bypass feature issues
        extern "system" {
            fn CreateMutexW(
                lpMutexAttributes: *const std::ffi::c_void,
                bInitialOwner: i32,
                lpName: *const u16,
            ) -> HANDLE;
        }

        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide_name.as_ptr()) };
        if handle.is_null() {
            return Err(AlreadyRunning);
        }
        
        let err = unsafe { GetLastError() };
        if err == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err(AlreadyRunning);
        }
        
        Ok(Guard { handle })
    }
    #[cfg(not(windows))]
    {
        Ok(Guard {}) // Mock for other platforms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_instance() {
        let guard1 = acquire().expect("First should succeed");
        let guard2 = acquire();
        assert!(guard2.is_err(), "Second should fail");
        drop(guard1);
        let _guard3 = acquire().expect("Third should succeed after first dropped");
    }
}
