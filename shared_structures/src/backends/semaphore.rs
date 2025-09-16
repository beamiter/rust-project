// src/backends/semaphore.rs
#![cfg(feature = "semaphore")]

use super::common::SyncBackend;
use libc::{sem_destroy, sem_init, sem_post, sem_t, sem_timedwait, sem_wait};
use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[repr(C)]
pub struct SemaphoreHeader {
    message_sem: sem_t,
    command_sem: sem_t,
}

pub struct SemaphoreBackend {
    header: *mut SemaphoreHeader,
}

unsafe impl Send for SemaphoreBackend {}
unsafe impl Sync for SemaphoreBackend {}

impl SemaphoreBackend {
    pub fn new() -> Self {
        Self {
            header: std::ptr::null_mut(),
        }
    }

    fn wait_on_semaphore(
        &self,
        is_message: bool,
        has_data: impl Fn() -> bool,
        adaptive_poll_spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        for _ in 0..adaptive_poll_spins {
            if has_data() {
                return Ok(true);
            }
            hint::spin_loop();
        }
        if has_data() {
            return Ok(true);
        }

        let sem_ptr = unsafe {
            if is_message {
                &mut (*self.header).message_sem
            } else {
                &mut (*self.header).command_sem
            }
        };

        match wait_timeout(sem_ptr, timeout) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(has_data()),
            Err(e) => {
                log::warn!("semaphore wait error: {}. Fallback to check state.", e);
                Ok(has_data())
            }
        }
    }
}

impl SyncBackend for SemaphoreBackend {
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()> {
        self.header = backend_ptr as *mut SemaphoreHeader;
        if is_creator {
            unsafe {
                if sem_init(&mut (*self.header).message_sem, 1, 0) != 0 {
                    return Err(Error::last_os_error());
                }
                if sem_init(&mut (*self.header).command_sem, 1, 0) != 0 {
                    sem_destroy(&mut (*self.header).message_sem);
                    return Err(Error::last_os_error());
                }
            }
        }
        Ok(())
    }

    fn wait_for_message(
        &self,
        has_data: impl Fn() -> bool,
        spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        self.wait_on_semaphore(true, has_data, spins, timeout)
    }

    fn wait_for_command(
        &self,
        has_data: impl Fn() -> bool,
        spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        self.wait_on_semaphore(false, has_data, spins, timeout)
    }

    fn signal_message(&self) -> Result<()> {
        unsafe {
            if sem_post(&mut (*self.header).message_sem) != 0 {
                let err = Error::last_os_error();
                if err.raw_os_error() != Some(libc::EOVERFLOW) {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn signal_command(&self) -> Result<()> {
        unsafe {
            if sem_post(&mut (*self.header).command_sem) != 0 {
                let err = Error::last_os_error();
                if err.raw_os_error() != Some(libc::EOVERFLOW) {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn cleanup(&mut self, is_creator: bool) {
        if is_creator && !self.header.is_null() {
            unsafe {
                sem_destroy(&mut (*self.header).message_sem);
                sem_destroy(&mut (*self.header).command_sem);
            }
        }
    }
}

fn wait_timeout(sem: *mut sem_t, timeout: Option<Duration>) -> Result<bool> {
    unsafe {
        match timeout {
            Some(duration) => {
                let deadline = SystemTime::now() + duration;
                let ts = deadline
                    .duration_since(UNIX_EPOCH)
                    .map(|d| libc::timespec {
                        tv_sec: d.as_secs() as libc::time_t,
                        tv_nsec: d.subsec_nanos() as libc::c_long,
                    })
                    .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid time"))?;
                if sem_timedwait(sem, &ts) == 0 {
                    Ok(true)
                } else {
                    let err = Error::last_os_error();
                    match err.raw_os_error() {
                        Some(libc::ETIMEDOUT) | Some(libc::EINTR) => Ok(false),
                        _ => Err(err),
                    }
                }
            }
            None => {
                if sem_wait(sem) == 0 {
                    Ok(true)
                } else {
                    let err = Error::last_os_error();
                    if err.raw_os_error() == Some(libc::EINTR) {
                        Ok(false)
                    } else {
                        Err(err)
                    }
                }
            }
        }
    }
}
