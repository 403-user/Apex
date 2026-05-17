use std::os::fd::RawFd;
use std::process::{Command, Child, Stdio};
use std::os::fd::FromRawFd;

pub struct PtyInstance {
    pub master_fd: RawFd,
    pub child: Option<Child>,
    pub rows: u16,
    pub cols: u16,
}

impl PtyInstance {
    pub fn new(rows: u16, cols: u16) -> anyhow::Result<Self> {
        let (master_fd, child) = unsafe {
            let mut master: RawFd = 0;
            let mut slave: RawFd = 0;
            let ret = libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), std::ptr::null());
            if ret != 0 {
                return Err(anyhow::anyhow!("Failed to open PTY: {}", std::io::Error::last_os_error()));
            }

            let stdin_fd = libc::dup(slave);
            if stdin_fd < 0 {
                libc::close(master);
                libc::close(slave);
                return Err(anyhow::anyhow!("Failed to dup slave fd for stdin: {}", std::io::Error::last_os_error()));
            }
            let stdout_fd = libc::dup(slave);
            if stdout_fd < 0 {
                libc::close(stdin_fd);
                libc::close(master);
                libc::close(slave);
                return Err(anyhow::anyhow!("Failed to dup slave fd for stdout: {}", std::io::Error::last_os_error()));
            }
            let stderr_fd = libc::dup(slave);
            if stderr_fd < 0 {
                libc::close(stdout_fd);
                libc::close(stdin_fd);
                libc::close(master);
                libc::close(slave);
                return Err(anyhow::anyhow!("Failed to dup slave fd for stderr: {}", std::io::Error::last_os_error()));
            }

            let stdin = Stdio::from_raw_fd(stdin_fd);
            let stdout = Stdio::from_raw_fd(stdout_fd);
            let stderr = Stdio::from_raw_fd(stderr_fd);

            let child = Command::new("/usr/bin/env")
                .arg("SHELL=/bin/bash")
                .arg("TERM=xterm-256color")
                .stdin(stdin)
                .stdout(stdout)
                .stderr(stderr)
                .spawn();

            match child {
                Ok(c) => {
                    libc::close(slave);
                    (master, Some(c))
                }
                Err(e) => {
                    libc::close(master);
                    libc::close(slave);
                    return Err(anyhow::anyhow!("Failed to spawn shell in PTY: {}", e));
                }
            }
        };

        Ok(PtyInstance { master_fd, child, rows, cols })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.rows = rows;
        self.cols = cols;
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let result = unsafe { libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws) };
        if result != 0 {
            return Err(anyhow::anyhow!("Failed to resize PTY: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<usize> {
        let ptr = data.as_ptr() as *const libc::c_void;
        let len = data.len();
        let ret = loop {
            let ret = unsafe { libc::write(self.master_fd, ptr, len) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
            }
            break ret;
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            Err(anyhow::anyhow!("PTY write error: {}", err))
        } else {
            Ok(ret as usize)
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let ptr = buf.as_mut_ptr() as *mut libc::c_void;
        let len = buf.len();
        let ret = loop {
            let ret = unsafe { libc::read(self.master_fd, ptr, len) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
            }
            break ret;
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            Err(anyhow::anyhow!("PTY read error: {}", err))
        } else {
            Ok(ret as usize)
        }
    }

    pub fn take_master_fd(&mut self) -> Option<RawFd> {
        if self.master_fd >= 0 {
            let fd = self.master_fd;
            self.master_fd = -1;
            Some(fd)
        } else {
            None
        }
    }
}

impl Drop for PtyInstance {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.master_fd >= 0 {
            unsafe {
                libc::close(self.master_fd);
            }
        }
    }
}
