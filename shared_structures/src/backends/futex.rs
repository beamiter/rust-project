// src/backends/futex.rs
#![cfg(feature = "futex")]

use super::common::SyncBackend;
use libc::timespec;
use std::hint;
use std::io::{Error, Result};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[repr(C, align(8))]
pub struct FutexHeader {
    message_seq: AtomicU32,
    message_waiters: AtomicU32,
    command_seq: AtomicU32,
    command_waiters: AtomicU32,
}

pub struct FutexBackend {
    header: *mut FutexHeader,
}

unsafe impl Send for FutexBackend {}
unsafe impl Sync for FutexBackend {}

impl FutexBackend {
    pub fn new() -> Self {
        Self {
            header: std::ptr::null_mut(),
        }
    }

    fn wait_on_futex(
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

        unsafe {
            let (seq, waiters) = if is_message {
                (&(*self.header).message_seq, &(*self.header).message_waiters)
            } else {
                (&(*self.header).command_seq, &(*self.header).command_waiters)
            };

            waiters.fetch_add(1, Ordering::AcqRel);
            if has_data() {
                waiters.fetch_sub(1, Ordering::AcqRel);
                return Ok(true);
            }

            let snapshot = seq.load(Ordering::Acquire);
            let res = futex_wait(seq, snapshot, timeout);
            waiters.fetch_sub(1, Ordering::AcqRel);

            res.map(|_| has_data()).or_else(|e| {
                log::warn!("futex_wait error: {}. Fallback to check state", e);
                Ok(has_data())
            })
        }
    }
}

impl SyncBackend for FutexBackend {
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()> {
        self.header = backend_ptr as *mut FutexHeader;
        if is_creator {
            unsafe {
                self.header.write(FutexHeader {
                    message_seq: AtomicU32::new(0),
                    message_waiters: AtomicU32::new(0),
                    command_seq: AtomicU32::new(0),
                    command_waiters: AtomicU32::new(0),
                });
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
        self.wait_on_futex(true, has_data, spins, timeout)
    }

    fn wait_for_command(
        &self,
        has_data: impl Fn() -> bool,
        spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        self.wait_on_futex(false, has_data, spins, timeout)
    }

    fn signal_message(&self) -> Result<()> {
        unsafe {
            if (*self.header).message_waiters.load(Ordering::Acquire) > 0 {
                (*self.header).message_seq.fetch_add(1, Ordering::Release);
                let _ = futex_wake(&(*self.header).message_seq, 1);
            }
        }
        Ok(())
    }

    fn signal_command(&self) -> Result<()> {
        unsafe {
            if (*self.header).command_waiters.load(Ordering::Acquire) > 0 {
                (*self.header).command_seq.fetch_add(1, Ordering::Release);
                let _ = futex_wake(&(*self.header).command_seq, 1);
            }
        }
        Ok(())
    }

    fn cleanup(&mut self, _is_creator: bool) {
        unsafe {
            (*self.header).message_seq.fetch_add(1, Ordering::Release);
            let _ = futex_wake(&(*self.header).message_seq, i32::MAX);
            (*self.header).command_seq.fetch_add(1, Ordering::Release);
            let _ = futex_wake(&(*self.header).command_seq, i32::MAX);
        }
    }
}

#[inline]
fn futex_wait(addr: &AtomicU32, expected: u32, timeout: Option<Duration>) -> Result<bool> {
    let mut ts = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let ts_ptr = if let Some(dur) = timeout {
        ts.tv_sec = dur.as_secs() as libc::time_t;
        ts.tv_nsec = dur.subsec_nanos() as libc::c_long;
        &mut ts as *mut timespec
    } else {
        std::ptr::null_mut()
    };
    let uaddr = addr as *const AtomicU32 as *const i32;
    let ret = unsafe {
        libc::syscall(
            libc::SYS_futex,
            uaddr,
            libc::FUTEX_WAIT,
            expected as i32,
            ts_ptr,
        )
    };
    if ret == 0 {
        Ok(true)
    } else {
        let err = Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EAGAIN) | Some(libc::EINTR) | Some(libc::ETIMEDOUT) => Ok(false),
            _ => Err(err),
        }
    }
}

#[inline]
fn futex_wake(addr: &AtomicU32, n: i32) -> Result<i32> {
    let uaddr = addr as *const AtomicU32 as *const i32;
    let ret = unsafe { libc::syscall(libc::SYS_futex, uaddr, libc::FUTEX_WAKE, n) };
    if ret < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(ret as i32)
    }
}
