use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use apex_pty::PtyInstance;

pub async fn run_dumb_terminal() -> anyhow::Result<()> {
    let (rows, cols) = terminal_size().unwrap_or((24, 80));

    let mut pty = PtyInstance::new(rows, cols)?;
    let master_fd = pty.master_fd;

    let raw = set_raw_mode().ok().flatten();
    let guard = raw.map(RawModeGuard);

    let mut sigwinch = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::from_raw(libc::SIGWINCH),
    )?;

    let running = Arc::new(AtomicBool::new(true));

    let r_stdin = running.clone();
    let stdin_task = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut buf = [0u8; 4096];
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        while r_stdin.load(Ordering::Relaxed) {
            let n = match input.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => n,
                Err(e) => return Err(anyhow::anyhow!("stdin read: {}", e)),
            };
            pty_write_all(master_fd, &buf[..n])?;
        }
        Ok(())
    });

    let r_pty = running.clone();
    let pty_task = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut buf = [0u8; 65536];
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        while r_pty.load(Ordering::Relaxed) {
            let n = match unsafe {
                libc::read(master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            } {
                -1 => {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(anyhow::anyhow!("PTY read: {}", e));
                }
                0 => return Ok(()),
                n => n as usize,
            };
            if out.write_all(&buf[..n]).is_err() {
                return Err(anyhow::anyhow!(
                    "stdout write: {}",
                    io::Error::last_os_error()
                ));
            }
            if out.flush().is_err() {
                return Err(anyhow::anyhow!(
                    "stdout flush: {}",
                    io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    });

    tokio::pin!(stdin_task);
    tokio::pin!(pty_task);

    let mut stdin_done = false;

    loop {
        if stdin_done {
            tokio::select! {
                _ = sigwinch.recv() => {
                    if let Some((r, c)) = terminal_size() {
                        let _ = pty.resize(r, c);
                    }
                }
                _ = &mut pty_task => break,
            }
        } else {
            tokio::select! {
                _ = sigwinch.recv() => {
                    if let Some((r, c)) = terminal_size() {
                        let _ = pty.resize(r, c);
                    }
                }
                result = &mut stdin_task => {
                    stdin_done = true;
                    if let Err(e) = result {
                        tracing::warn!("stdin task error: {:?}", e);
                    }
                }
                _ = &mut pty_task => break,
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    drop(guard);

    Ok(())
}

fn set_raw_mode() -> anyhow::Result<Option<libc::termios>> {
    let mut orig = std::mem::MaybeUninit::<libc::termios>::uninit();
    let ret = unsafe { libc::tcgetattr(libc::STDIN_FILENO, orig.as_mut_ptr()) };
    if ret < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOTTY) {
            return Ok(None);
        }
        return Err(anyhow::anyhow!("tcgetattr: {}", err));
    }
    let orig = unsafe { orig.assume_init() };
    let mut raw = orig;
    unsafe { libc::cfmakeraw(&mut raw); }
    let ret = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
    if ret < 0 {
        return Err(anyhow::anyhow!("tcsetattr raw: {}", io::Error::last_os_error()));
    }
    Ok(Some(orig))
}

fn terminal_size() -> Option<(u16, u16)> {
    let mut ws = std::mem::MaybeUninit::<libc::winsize>::uninit();
    let ret = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, ws.as_mut_ptr()) };
    if ret == 0 {
        let ws = unsafe { ws.assume_init() };
        if ws.ws_row > 0 && ws.ws_col > 0 {
            return Some((ws.ws_row, ws.ws_col));
        }
    }
    None
}

fn pty_write_all(fd: i32, buf: &[u8]) -> anyhow::Result<usize> {
    let ptr = buf.as_ptr() as *const libc::c_void;
    let len = buf.len();
    let mut total = 0;
    while total < len {
        let ret = unsafe { libc::write(fd, ptr.add(total), len - total) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if err.kind() == io::ErrorKind::WouldBlock {
                break;
            }
            return Err(anyhow::anyhow!("PTY write: {}", err));
        }
        if ret == 0 {
            break;
        }
        total += ret as usize;
    }
    Ok(total)
}

struct RawModeGuard(libc::termios);

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0); }
    }
}
