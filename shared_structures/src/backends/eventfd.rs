// src/backends/eventfd.rs
#![cfg(feature = "eventfd")]

use super::common::SyncBackend;
use nix::fcntl::{fcntl, FcntlArg};
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::eventfd::{self, EfdFlags};
use nix::unistd;
use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

#[repr(C, align(8))]
pub struct EventFdHeader {
    message_fd: AtomicI32,
    command_fd: AtomicI32,
}

pub struct EventFdBackend {
    header: *mut EventFdHeader,
    // Creator holds the actual EventFd objects to manage their lifetime
    local_message_fd: Option<eventfd::EventFd>,
    local_command_fd: Option<eventfd::EventFd>,
    // Log warnings only once
    msg_warned: AtomicBool,
    cmd_warned: AtomicBool,
}

unsafe impl Send for EventFdBackend {}
unsafe impl Sync for EventFdBackend {}

impl EventFdBackend {
    pub fn new() -> Self {
        Self {
            header: std::ptr::null_mut(),
            local_message_fd: None,
            local_command_fd: None,
            msg_warned: AtomicBool::new(false),
            cmd_warned: AtomicBool::new(false),
        }
    }

    fn wait_on_eventfd(
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

        let fd = unsafe {
            if is_message {
                (*self.header).message_fd.load(Ordering::Acquire)
            } else {
                (*self.header).command_fd.load(Ordering::Acquire)
            }
        };

        if fd < 0 || !fd_is_valid(fd) {
            let warned_flag = if is_message {
                &self.msg_warned
            } else {
                &self.cmd_warned
            };
            if !warned_flag.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "eventfd {} is not valid in this process. Falling back to polling.",
                    fd
                );
            }
            // Fallback: sleep a little and check again.
            std::thread::sleep(timeout.unwrap_or(Duration::from_millis(1)));
            return Ok(has_data());
        }

        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let poll_fd = PollFd::new(borrowed_fd, PollFlags::POLLIN);

        let timeout_ms = timeout.map_or(-1, |d| d.as_millis() as i32);

        use nix::poll::PollTimeout;
        use nix::sys::epoll::EpollTimeout;
        // (todo) fix here
        match poll(&mut [poll_fd], timeout_ms as u16) {
            Ok(0) => Ok(has_data()), // Timeout
            Ok(_) => {
                let mut buf = [0u8; 8];
                let _ = unistd::read(borrowed_fd, &mut buf); // Drain the eventfd
                Ok(true)
            }
            Err(e) => {
                log::warn!("poll(eventfd) error: {}. Fallback to check state.", e);
                Ok(has_data())
            }
        }
    }
}

impl SyncBackend for EventFdBackend {
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()> {
        self.header = backend_ptr as *mut EventFdHeader;
        if is_creator {
            let flags = EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC;
            let msg_efd =
                eventfd::EventFd::from_flags(flags).map_err(|e| Error::new(ErrorKind::Other, e))?;
            let cmd_efd =
                eventfd::EventFd::from_flags(flags).map_err(|e| Error::new(ErrorKind::Other, e))?;

            unsafe {
                (*self.header)
                    .message_fd
                    .store(msg_efd.as_raw_fd(), Ordering::Release);
                (*self.header)
                    .command_fd
                    .store(cmd_efd.as_raw_fd(), Ordering::Release);
            }
            self.local_message_fd = Some(msg_efd);
            self.local_command_fd = Some(cmd_efd);
        } else {
            unsafe {
                (*self.header).message_fd.store(-1, Ordering::Release);
                (*self.header).command_fd.store(-1, Ordering::Release);
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
        self.wait_on_eventfd(true, has_data, spins, timeout)
    }

    fn wait_for_command(
        &self,
        has_data: impl Fn() -> bool,
        spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        self.wait_on_eventfd(false, has_data, spins, timeout)
    }

    fn signal_message(&self) -> Result<()> {
        if let Some(efd) = &self.local_message_fd {
            match efd.write(1) {
                Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
                Err(e) => Err(Error::new(ErrorKind::Other, e)),
            }
        } else {
            Ok(()) // Non-creator can't signal
        }
    }

    fn signal_command(&self) -> Result<()> {
        if let Some(efd) = &self.local_command_fd {
            match efd.write(1) {
                Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
                Err(e) => Err(Error::new(ErrorKind::Other, e)),
            }
        } else {
            Ok(()) // Non-creator can't signal
        }
    }

    fn cleanup(&mut self, _is_creator: bool) {
        // The local_..._fd fields being Option<EventFd> will automatically
        // close the file descriptors when the EventFdBackend is dropped.
        // No explicit cleanup is needed here for the FDs themselves.
    }
}

fn fd_is_valid(fd: i32) -> bool {
    if fd < 0 {
        return false;
    }
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    fcntl(borrowed_fd, FcntlArg::F_GETFD).is_ok()
}
