use std::io::Read;

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
