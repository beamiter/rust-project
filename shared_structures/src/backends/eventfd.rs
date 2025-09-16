// src/backends/eventfd.rs
#![cfg(feature = "eventfd")]

use super::common::SyncBackend;
use log::info;
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, FdFlag, OFlag};
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::socket::{
    accept, bind, connect, listen, recvmsg, sendmsg, socket, AddressFamily, Backlog,
    ControlMessage, ControlMessageOwned, MsgFlags, SockFlag, SockType, UnixAddr,
};
use nix::unistd;
use std::ffi::CString;
use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::io::{IoSlice, IoSliceMut};
use std::os::fd::FromRawFd;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// UNIX sockaddr_un 路径上限为 108 字节
const UNIX_SOCK_MAX: usize = 108;

#[repr(C, align(8))]
pub struct EventFdHeader {
    // 创建者写入路径后，把 ready 置为 true，打开者据此读取路径
    is_ready: AtomicBool,
    // 以 0 结尾的 C 字节串，未用完则补 0
    sock_path: [u8; UNIX_SOCK_MAX],
}

pub struct EventFdBackend {
    header: *mut EventFdHeader,

    // 本进程可用的 eventfd（消息、命令）
    local_message_fd: Option<OwnedFd>,
    local_command_fd: Option<OwnedFd>,

    // 仅创建者持有：用于关闭监听线程
    is_creator: bool,
    listener_stop: Option<Arc<AtomicBool>>,
    sock_path: Option<PathBuf>,
}

unsafe impl Send for EventFdBackend {}
unsafe impl Sync for EventFdBackend {}

impl EventFdBackend {
    pub fn new() -> Self {
        Self {
            header: std::ptr::null_mut(),
            local_message_fd: None,
            local_command_fd: None,
            is_creator: false,
            listener_stop: None,
            sock_path: None,
        }
    }

    fn write_u64(fd: BorrowedFd<'_>, v: u64) -> Result<()> {
        let bytes = v.to_ne_bytes();
        match unistd::write(fd, &bytes) {
            Ok(_) => Ok(()),
            Err(Errno::EAGAIN) => Ok(()),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }

    fn drain_eventfd(fd: BorrowedFd<'_>) {
        let mut buf = [0u8; 8];
        let _ = unistd::read(fd, &mut buf);
    }

    fn poll_fd(fd: RawFd, timeout: Option<Duration>) -> Result<bool> {
        use nix::poll::PollTimeout;
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let pfd = PollFd::new(borrowed_fd, PollFlags::POLLIN);
        let to = timeout.map_or(PollTimeout::NONE, |d| {
            // 尽量转换，否则用 NONE
            nix::poll::PollTimeout::try_from(d.as_millis()).unwrap_or(PollTimeout::NONE)
        });
        match poll(&mut [pfd], to) {
            Ok(0) => Ok(false),
            Ok(_) => Ok(true),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }

    fn set_header_sock_path(header: *mut EventFdHeader, path: &Path) -> Result<()> {
        let cstr = CString::new(
            path.as_os_str()
                .to_str()
                .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Invalid socket path"))?,
        )
        .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("Bad path: {e}")))?;
        let bytes = cstr.as_bytes_with_nul();
        if bytes.len() > UNIX_SOCK_MAX {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Socket path too long for sockaddr_un",
            ));
        }
        unsafe {
            (*header).sock_path.fill(0);
            (&mut (*header).sock_path)[..bytes.len()].copy_from_slice(bytes);
            (*header).is_ready.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn get_header_sock_path(header: *mut EventFdHeader) -> Result<PathBuf> {
        unsafe {
            if !(*header).is_ready.load(Ordering::Acquire) {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "Backend not ready (socket path not published)",
                ));
            }
            let buf = &(*header).sock_path;
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let s = std::str::from_utf8(&buf[..len])
                .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            Ok(PathBuf::from(s))
        }
    }

    fn generate_socket_path() -> PathBuf {
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        PathBuf::from(format!("/tmp/srb-{}-{}.sock", pid, ts))
    }

    fn create_eventfd_owned() -> Result<OwnedFd> {
        // 用 libc 直接创建 eventfd，避免中间类型转换问题
        let flags = libc::EFD_NONBLOCK | libc::EFD_CLOEXEC;
        let fd = unsafe { libc::eventfd(0, flags) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        // SAFETY: 来自内核的有效 fd，交由 OwnedFd 接管
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn spawn_listener_thread(
        sock_path: PathBuf,
        msg_fd: OwnedFd,
        cmd_fd: OwnedFd,
        stop: Arc<AtomicBool>,
    ) -> Result<()> {
        // 删除残留
        if sock_path.exists() {
            let _ = std::fs::remove_file(&sock_path);
        }

        let srv = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_CLOEXEC, // 监听 socket 本身 CLOEXEC 即可
            None,
        )
        .map_err(|e| Error::new(ErrorKind::Other, e))?;

        let addr = UnixAddr::new(&sock_path).map_err(|e| Error::new(ErrorKind::Other, e))?;
        bind(srv.as_raw_fd(), &addr).map_err(|e| Error::new(ErrorKind::Other, e))?;
        listen(&srv, Backlog::new(8)?).map_err(|e| Error::new(ErrorKind::Other, e))?;

        // 监听 socket 非阻塞；CLOEXEC 应该通过 F_SETFD 设置，而非 F_SETFL
        let _ = fcntl(&srv, FcntlArg::F_SETFL(OFlag::O_NONBLOCK));
        let _ = fcntl(&srv, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC));

        // 把 OwnedFd 移入线程闭包，保证其生命周期覆盖整个线程
        std::thread::Builder::new()
            .name("srb_eventfd_fdpass".to_string())
            .spawn(move || {
                // 在闭包内部再取 raw fd
                let msg_fd_raw = msg_fd.as_raw_fd();
                let cmd_fd_raw = cmd_fd.as_raw_fd();

                while !stop.load(Ordering::Relaxed) {
                    match accept(srv.as_raw_fd()) {
                        Ok(cli_fd) => {
                            let iov = [IoSlice::new(&[0xE5])];
                            let fds = [msg_fd_raw, cmd_fd_raw];
                            let cmsg = [ControlMessage::ScmRights(&fds)];

                            // 检查返回值，记录错误，必要时可考虑重试一次
                            if let Err(e) = sendmsg::<nix::sys::socket::UnixAddr>(
                                cli_fd,
                                &iov,
                                &cmsg,
                                MsgFlags::empty(),
                                None,
                            ) {
                                log::warn!("sendmsg(SCM_RIGHTS) failed: {e}");
                            }

                            let _ = unistd::close(cli_fd);
                        }
                        Err(Errno::EAGAIN) => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => {
                            log::warn!("eventfd listener accept error: {e}");
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }

                // 退出时自动 drop srv 和 msg_fd/cmd_fd，并删除 socket 文件
                let _ = std::fs::remove_file(&sock_path);
            })
            .map_err(|e| Error::new(ErrorKind::Other, e))?;

        Ok(())
    }

    fn receive_fds_from_server(sock_path: &Path) -> Result<(OwnedFd, OwnedFd)> {
        // 连接重试：最多 200ms，避免 creator 刚发布路径但尚未 listen 的窗口
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        let cli = loop {
            match socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            ) {
                Ok(cli) => {
                    let addr =
                        UnixAddr::new(sock_path).map_err(|e| Error::new(ErrorKind::Other, e))?;
                    match connect(cli.as_raw_fd(), &addr) {
                        Ok(()) => break cli,
                        Err(e) => {
                            if std::time::Instant::now() >= deadline {
                                return Err(Error::new(ErrorKind::Other, e));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    }
                }
                Err(e) => return Err(Error::new(ErrorKind::Other, e)),
            }
        };

        let mut buf = [0u8; 1];
        let mut iov = [IoSliceMut::new(&mut buf)];
        let mut cmsgspace = nix::cmsg_space!([RawFd; 2]);

        let msg = recvmsg::<UnixAddr>(
            cli.as_raw_fd(),
            &mut iov,
            Some(&mut cmsgspace),
            MsgFlags::empty(),
        )
        .map_err(|e| Error::new(ErrorKind::Other, e))?;

        // 校验：必须读到 1 字节 payload，且没有控制消息截断
        if msg.bytes == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "server closed before sending fds",
            ));
        }
        if msg.flags.contains(MsgFlags::MSG_CTRUNC) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "ancillary data truncated",
            ));
        }

        let mut fds: Vec<RawFd> = Vec::new();
        if let Ok(mut cmsg) = msg.cmsgs() {
            while let Some(ControlMessageOwned::ScmRights(recv_fds)) = cmsg.next() {
                fds.extend(recv_fds);
            }
        }
        info!("fds: {:?}", fds);

        if fds.len() < 2 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Did not receive 2 fds from server",
            ));
        }

        // SAFETY: 来自 SCM_RIGHTS 的 fd 由本进程接管
        let owned_msg = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let owned_cmd = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        Ok((owned_msg, owned_cmd))
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

        let opt_fd = if is_message {
            self.local_message_fd.as_ref()
        } else {
            self.local_command_fd.as_ref()
        };
        let Some(ofd) = opt_fd else {
            std::thread::sleep(timeout.unwrap_or(Duration::from_millis(1)));
            return Ok(has_data());
        };

        match Self::poll_fd(ofd.as_raw_fd(), timeout) {
            Ok(true) => {
                let borrowed = unsafe { BorrowedFd::borrow_raw(ofd.as_raw_fd()) };
                Self::drain_eventfd(borrowed);
                Ok(true)
            }
            Ok(false) => Ok(has_data()),
            Err(e) => {
                log::warn!("poll(eventfd) error: {e}. Fallback to check state.");
                Ok(has_data())
            }
        }
    }
}

impl SyncBackend for EventFdBackend {
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()> {
        self.header = backend_ptr as *mut EventFdHeader;
        self.is_creator = is_creator;

        if is_creator {
            info!("is creator");
            // 1) 创建一对 eventfd（本进程留用）
            let msg_fd = Self::create_eventfd_owned()?;
            let cmd_fd = Self::create_eventfd_owned()?;

            // 2) 为“监听线程用于发送给对端”复制一份句柄（指向同一个内核对象）
            let msg_fd_for_send =
                nix::unistd::dup(&msg_fd).map_err(|e| Error::new(ErrorKind::Other, e))?;
            let cmd_fd_for_send =
                nix::unistd::dup(&cmd_fd).map_err(|e| Error::new(ErrorKind::Other, e))?;

            // 3) 生成 socket 路径、写 header 并启动监听线程（用复制出来的那对句柄）
            let sock_path = Self::generate_socket_path();
            Self::set_header_sock_path(self.header, &sock_path)?;
            let stop = Arc::new(AtomicBool::new(false));
            Self::spawn_listener_thread(
                sock_path.clone(),
                msg_fd_for_send,
                cmd_fd_for_send,
                stop.clone(),
            )?;

            // 4) 本进程继续使用原始的那对 eventfd
            self.local_message_fd = Some(msg_fd);
            self.local_command_fd = Some(cmd_fd);
            self.listener_stop = Some(stop);
            self.sock_path = Some(sock_path);
        } else {
            info!("is not creator");
            // 打开者：等待路径 ready，再连接接收 2 个 fd
            // 自旋等待 ready（通常很快）
            for _ in 0..10_000 {
                unsafe {
                    if (*self.header).is_ready.load(Ordering::Acquire) {
                        break;
                    }
                }
                hint::spin_loop();
            }
            if unsafe { !(*self.header).is_ready.load(Ordering::Acquire) } {
                std::thread::sleep(Duration::from_millis(5));
            }

            let sock_path = Self::get_header_sock_path(self.header)?;
            let (msg_fd, cmd_fd) = Self::receive_fds_from_server(&sock_path)?;
            self.local_message_fd = Some(msg_fd);
            self.local_command_fd = Some(cmd_fd);
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
        if let Some(fd) = &self.local_message_fd {
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd.as_raw_fd()) };
            Self::write_u64(borrowed, 1)
        } else {
            Ok(())
        }
    }

    fn signal_command(&self) -> Result<()> {
        if let Some(fd) = &self.local_command_fd {
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd.as_raw_fd()) };
            Self::write_u64(borrowed, 1)
        } else {
            Ok(())
        }
    }

    fn cleanup(&mut self, is_creator: bool) {
        if is_creator {
            if let Some(stop) = &self.listener_stop {
                stop.store(true, Ordering::Relaxed);
                // 让监听线程尽快退出
                std::thread::sleep(Duration::from_millis(20));
            }
            if let Some(path) = &self.sock_path {
                let _ = std::fs::remove_file(path);
            }
        }
        // OwnedFd 会在 drop 时自动关闭
    }
}
