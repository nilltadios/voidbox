//! Host execution bridge for native mode
//!
//! Provides a TCP-based bridge that allows the container to execute
//! commands on the host system (like sudo) with full PTY support
//! for interactive commands.

use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::io::{Read as IoRead, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Bridge error: {0}")]
    BridgeFailed(String),
}

/// Open a PTY pair using libc directly
fn open_pty() -> Result<(OwnedFd, OwnedFd), BridgeError> {
    unsafe {
        let mut master: libc::c_int = 0;
        let mut slave: libc::c_int = 0;

        let ret = libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        if ret < 0 {
            return Err(BridgeError::BridgeFailed("openpty failed".to_string()));
        }

        Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)))
    }
}

fn generate_token() -> String {
    let mut hasher = Sha256::new();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let x = 0;
    let stack_addr = &x as *const i32 as usize;

    hasher.update(timestamp.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hasher.update(stack_addr.to_le_bytes());

    hex::encode(hasher.finalize())
}

/// Start the host bridge listener in a background thread
/// Uses port 0 to let OS assign an available port
pub fn start_host_bridge() -> Result<BridgeHandle, BridgeError> {
    // Bind to port 0 - OS will assign an available port
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let token = generate_token();

    eprintln!("[voidbox] Host bridge listening on 127.0.0.1:{}", port);

    listener.set_nonblocking(true)?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let token_clone = token.clone();

    let handle = thread::spawn(move || {
        host_bridge_loop(listener, running_clone, token_clone);
    });

    thread::sleep(Duration::from_millis(50));

    Ok(BridgeHandle {
        running,
        _thread: handle,
        port,
        token,
    })
}

pub struct BridgeHandle {
    running: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
    port: u16,
    token: String,
}

impl BridgeHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(100));
    }
}

fn host_bridge_loop(listener: TcpListener, running: Arc<AtomicBool>, token: String) {
    let listener_fd = listener.as_raw_fd();

    while running.load(Ordering::SeqCst) {
        // Use poll to wait for connection or timeout
        // This avoids busy waiting
        let mut poll_fds = [libc::pollfd {
            fd: listener_fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let ret = unsafe { libc::poll(poll_fds.as_mut_ptr(), 1, 500) };

        if ret < 0 {
            // Error in poll
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                eprintln!("[voidbox-bridge] Poll error: {}", err);
                thread::sleep(Duration::from_millis(100));
            }
            continue;
        }

        if ret == 0 {
            continue; // Timeout, check running flag
        }

        if poll_fds[0].revents & libc::POLLIN != 0 {
            match listener.accept() {
                Ok((stream, _)) => {
                    let token_clone = token.clone();
                    thread::spawn(move || {
                        if let Err(e) = handle_interactive_connection(stream, &token_clone) {
                            eprintln!("[voidbox-bridge] Connection error: {}", e);
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Should not happen with poll, but handle safely
                    continue;
                }
                Err(e) => {
                    eprintln!("[voidbox-bridge] Accept error: {}", e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

fn handle_interactive_connection(
    mut stream: TcpStream,
    expected_token: &str,
) -> Result<(), BridgeError> {
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();

    stream.set_nonblocking(false)?;

    // Helper to read a line
    let mut read_line = |out: &mut String| -> Result<bool, BridgeError> {
        out.clear();
        loop {
            let n = stream.read(&mut buf[..1])?;
            if n == 0 {
                return Ok(false); // EOF
            }
            if buf[0] == b'\n' {
                return Ok(true);
            }
            out.push(buf[0] as char);

            // Limit line length to prevent DoS
            if out.len() > 1024 {
                return Err(BridgeError::BridgeFailed("Line too long".to_string()));
            }
        }
    };

    // 1. Read and verify token
    if !read_line(&mut line_buf)? {
        return Ok(());
    }
    let received_token = line_buf.trim();
    if received_token != expected_token {
        eprintln!("[voidbox-bridge] Invalid token received. Rejecting connection.");
        return Ok(());
    }

    // 2. Read command
    if !read_line(&mut line_buf)? {
        return Ok(());
    }
    let cmd_line = line_buf.trim();
    if cmd_line.is_empty() {
        return Ok(());
    }

    let (use_sudo, cmd) = if let Some(rest) = cmd_line.strip_prefix("SUDO ") {
        (true, rest)
    } else if let Some(rest) = cmd_line.strip_prefix("EXEC ") {
        (false, rest)
    } else {
        return Ok(());
    };

    let shell_cmd = if use_sudo {
        format!("sudo {}", cmd)
    } else {
        cmd.to_string()
    };

    let (master, slave) = open_pty()?;
    let master_fd = master.as_raw_fd();
    let slave_fd = slave.as_raw_fd();

    // Fork using libc directly
    let pid = unsafe { libc::fork() };

    if pid < 0 {
        return Err(BridgeError::BridgeFailed("fork failed".to_string()));
    } else if pid == 0 {
        // Child process
        drop(master);

        unsafe {
            libc::setsid();
            libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);

            libc::dup2(slave_fd, 0);
            libc::dup2(slave_fd, 1);
            libc::dup2(slave_fd, 2);

            if slave_fd > 2 {
                libc::close(slave_fd);
            }

            let shell = CString::new("/bin/sh").unwrap();
            let arg0 = CString::new("sh").unwrap();
            let arg1 = CString::new("-c").unwrap();
            let arg2 = CString::new(shell_cmd).unwrap();

            libc::execvp(
                shell.as_ptr(),
                [
                    arg0.as_ptr(),
                    arg1.as_ptr(),
                    arg2.as_ptr(),
                    std::ptr::null(),
                ]
                .as_ptr(),
            );

            libc::_exit(1);
        }
    } else {
        // Parent process
        drop(slave);

        stream.set_nonblocking(true)?;

        // Set master to non-blocking
        unsafe {
            let flags = libc::fcntl(master_fd, libc::F_GETFL);
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        // Forward data bidirectionally using poll
        forward_pty_socket(master_fd, &mut stream)?;

        // Wait for child
        unsafe {
            let mut status: libc::c_int = 0;
            libc::waitpid(pid, &mut status, 0);
        }
    }

    Ok(())
}

fn forward_pty_socket(master_fd: i32, stream: &mut TcpStream) -> Result<(), BridgeError> {
    let socket_fd = stream.as_raw_fd();
    let mut buf = [0u8; 4096];

    loop {
        let mut poll_fds = [
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: socket_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let ret = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, 100) };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Ok(());
        }

        if ret == 0 {
            continue; // Timeout
        }

        // PTY -> socket
        if poll_fds[0].revents & libc::POLLIN != 0 {
            let n =
                unsafe { libc::read(master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n > 0 {
                if stream.write_all(&buf[..n as usize]).is_err() {
                    return Ok(());
                }
                stream.flush().ok();
            } else if n == 0 {
                return Ok(()); // PTY closed
            } else {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::WouldBlock {
                    return Ok(());
                }
            }
        }

        if poll_fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            return Ok(());
        }

        // Socket -> PTY
        if poll_fds[1].revents & libc::POLLIN != 0 {
            match stream.read(&mut buf) {
                Ok(0) => return Ok(()), // Socket closed
                Ok(n) => {
                    let written =
                        unsafe { libc::write(master_fd, buf.as_ptr() as *const libc::c_void, n) };
                    if written < 0 {
                        return Ok(());
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Ok(()),
            }
        }

        if poll_fds[1].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            return Ok(());
        }
    }
}
