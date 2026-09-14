use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VMIN: usize = 6;
const VTIME: usize = 5;

pub fn read_key() -> char {
    unsafe {
        let mut orig: libc::termios = std::mem::zeroed();
        libc::tcgetattr(libc::STDIN_FILENO, &mut orig);
        let mut raw = orig;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[VMIN] = 1;
        raw.c_cc[VTIME] = 0;
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);

        let mut buf = [0u8; 1];
        std::io::stdin().read_exact(&mut buf).ok();

        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &orig);

        buf[0] as char
    }
}

pub struct Interrupt {
    cancel: Arc<AtomicBool>,
    expand: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Interrupt {
    pub fn armed() -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let expand = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let e = expand.clone();
        let s = stop.clone();
        let join = std::thread::spawn(move || Self::scan(c, e, s));
        Self {
            cancel,
            expand,
            stop,
            join: Some(join),
        }
    }

    pub fn is_set(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// ctrl+t (0x14) during a live stream toggles the thinking block; it is
    /// latched so the caller can consume it once. Returns whether it fired.
    pub fn take_expand(&self) -> bool {
        self.expand.swap(false, Ordering::Relaxed)
    }

    fn scan(cancel: Arc<AtomicBool>, expand: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
        unsafe {
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut orig) != 0 {
                return;
            }
            let mut raw = orig;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            raw.c_cc[VMIN] = 0;
            raw.c_cc[VTIME] = 1;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);

            let mut buf = [0u8; 64];
            let mut in_escape = false;
            while !stop.load(Ordering::Relaxed) {
                let n = libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len());
                if n <= 0 {
                    continue;
                }
                let mut i = 0usize;
                while i < n as usize {
                    let b = buf[i];
                    if in_escape {
                        // CSI/OSC sequences run until a final byte (0x40..0x7e).
                        if (0x40..=0x7e).contains(&b) {
                            in_escape = false;
                        }
                        i += 1;
                        continue;
                    }
                    if b == 0x1b {
                        // Lone esc = cancel. If followed by '[' or 'O' it starts
                        // a multi-byte sequence (arrow keys etc.) — consume it
                        // without cancelling.
                        let next = buf.get(i + 1).copied();
                        cancel.store(true, Ordering::Relaxed);
                        if next == Some(0x5b) || next == Some(0x4f) {
                            in_escape = true;
                            i += 2;
                        } else {
                            i += 1;
                        }
                        continue;
                    }
                    if b == 0x14 {
                        // ctrl+t: expand thinking
                        expand.store(true, Ordering::Relaxed);
                        i += 1;
                        continue;
                    }
                    if matches!(b, 0x03 | 0x04 | b'q') {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    i += 1;
                }
            }

            libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &orig);
        }
    }
}

impl Default for Interrupt {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            expand: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            join: None,
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory;

    #[test]
    fn esc_sets_cancel_flag() {
        let _g = memory::lock();
        unsafe {
            let mut master = 0;
            let mut slave = 0;
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                ),
                0
            );
            assert_eq!(
                libc::dup2(slave, libc::STDIN_FILENO),
                libc::STDIN_FILENO
            );
            libc::close(slave);

            for key in [0x1b, 0x03, b'q'] {
                let watch = Interrupt::armed();
                std::thread::sleep(std::time::Duration::from_millis(80));

                let buf = [key];
                assert_eq!(
                    libc::write(master, buf.as_ptr().cast(), 1),
                    1
                );

                let mut tries = 0;
                while !watch.is_set() && tries < 200 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    tries += 1;
                }
                assert!(watch.is_set());
                drop(watch);
            }

            // ctrl+t latches the expand flag without cancelling the stream.
            {
                let watch = Interrupt::armed();
                std::thread::sleep(std::time::Duration::from_millis(80));

                let buf = [0x14u8];
                assert_eq!(
                    libc::write(master, buf.as_ptr().cast(), 1),
                    1
                );

                let mut saw = false;
                let mut tries = 0;
                while !saw && tries < 200 {
                    if watch.take_expand() {
                        saw = true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    tries += 1;
                }
                assert!(saw, "ctrl+t should latch the expand flag");
                // consumed by the take above:
                assert!(!watch.take_expand(), "expand flag should be cleared once taken");
                assert!(!watch.is_set(), "ctrl+t must not cancel the stream");
                drop(watch);
            }

            libc::close(master);
        }
    }
}
