//! Host execution bridge for native mode
//!
//! Provides a TCP-based bridge that allows the container to execute
//! commands on the host system (like sudo) with full PTY support
//! for interactive commands.

use std::ffi::CString;
use std::io::{Read as IoRead, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
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

/// Start the host bridge listener in a background thread
/// Uses port 0 to let OS assign an available port
pub fn start_host_bridge() -> Result<BridgeHandle, BridgeError> {
    // Bind to port 0 - OS will assign an available port
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();

    eprintln!("[voidbox] Host bridge listening on 127.0.0.1:{}", port);

    listener.set_nonblocking(true)?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let handle = thread::spawn(move || {
        host_bridge_loop(listener, running_clone);
    });

    thread::sleep(Duration::from_millis(50));

    Ok(BridgeHandle {
        running,
        _thread: handle,
        port,
    })
}

pub struct BridgeHandle {
    running: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
    port: u16,
}

impl BridgeHandle {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(100));
    }
}

fn host_bridge_loop(listener: TcpListener, running: Arc<AtomicBool>) {
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                thread::spawn(move || {
                    if let Err(e) = handle_interactive_connection(stream) {
                        eprintln!("[voidbox-bridge] Connection error: {}", e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("[voidbox-bridge] Accept error: {}", e);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_interactive_connection(mut stream: TcpStream) -> Result<(), BridgeError> {
    let mut buf = [0u8; 4096];
    let mut cmd_line = String::new();

    stream.set_nonblocking(false)?;

    // Read command line until newline
    loop {
        let n = stream.read(&mut buf[..1])?;
        if n == 0 {
            return Ok(());
        }
        if buf[0] == b'\n' {
            break;
        }
        cmd_line.push(buf[0] as char);
    }

    let cmd_line = cmd_line.trim();
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
                [arg0.as_ptr(), arg1.as_ptr(), arg2.as_ptr(), std::ptr::null()].as_ptr(),
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
            let n = unsafe { libc::read(master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
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
                    let written = unsafe {
                        libc::write(master_fd, buf.as_ptr() as *const libc::c_void, n)
                    };
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
