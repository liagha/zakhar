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
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Interrupt {
    pub fn armed() -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let s = stop.clone();
        let join = std::thread::spawn(move || Self::scan(c, s));
        Self {
            cancel,
            stop,
            join: Some(join),
        }
    }

    pub fn is_set(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn scan(cancel: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
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
            while !stop.load(Ordering::Relaxed) {
                let n = libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len());
                if n <= 0 {
                    continue;
                }
                for &b in &buf[..n as usize] {
                    if matches!(b, 0x1b | 0x03 | 0x04 | b'q') {
                        cancel.store(true, Ordering::Relaxed);
                    }
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

            libc::close(master);
        }
    }
}
