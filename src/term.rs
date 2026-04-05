use std::sync::atomic::{AtomicBool, Ordering};

static RESIZED: AtomicBool = AtomicBool::new(true);

// ── libc FFI ──────────────────────────────────────────────────────────────

const STDIN: i32 = 0;
const STDOUT: i32 = 1;
const TCSAFLUSH: i32 = 2;
const TIOCGWINSZ: u64 = 0x5413;
const SIGWINCH: i32 = 28;
const POLLIN: i16 = 0x0001;

// c_lflag bits
const ECHO: u32 = 0o10;
const ICANON: u32 = 0o2;
const ISIG: u32 = 0o1;
const IEXTEN: u32 = 0o100000;

// c_iflag bits
const ICRNL: u32 = 0o400;
const IXON: u32 = 0o2000;
const BRKINT: u32 = 0o2;
const INPCK: u32 = 0o20;
const ISTRIP: u32 = 0o40;

// c_oflag bits
const OPOST: u32 = 0o1;

// c_cflag bits
const CS8: u32 = 0o60;

// c_cc indices
const VMIN: usize = 6;
const VTIME: usize = 5;

#[repr(C)]
#[derive(Copy, Clone)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

extern "C" {
    fn tcgetattr(fd: i32, t: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, t: *const Termios) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn signal(signum: i32, handler: usize) -> usize;
}

extern "C" fn handle_winch(_: i32) {
    RESIZED.store(true, Ordering::SeqCst);
}

// ── Raw mode guard ────────────────────────────────────────────────────────

static mut ORIG_TERMIOS: Option<Termios> = None;

pub struct RawMode;

impl RawMode {
    pub fn enter() -> Self {
        unsafe {
            let mut orig: Termios = std::mem::zeroed();
            tcgetattr(STDIN, &mut orig);
            ORIG_TERMIOS = Some(orig);

            let mut raw = orig;
            raw.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
            raw.c_oflag &= !OPOST;
            raw.c_cflag |= CS8;
            raw.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
            raw.c_cc[VMIN] = 0;
            raw.c_cc[VTIME] = 0;
            tcsetattr(STDIN, TCSAFLUSH, &raw);

            signal(SIGWINCH, handle_winch as *const () as usize);
        }
        // Alternate screen, hide cursor
        write_str("\x1b[?1049h\x1b[?25l");
        RawMode
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // Show cursor, leave alternate screen
        write_str("\x1b[?25h\x1b[?1049l");
        unsafe {
            if let Some(ref orig) = ORIG_TERMIOS {
                tcsetattr(STDIN, TCSAFLUSH, orig);
            }
        }
    }
}

// ── Terminal queries ──────────────────────────────────────────────────────

pub fn get_size() -> (usize, usize) {
    unsafe {
        let mut ws: Winsize = std::mem::zeroed();
        if ioctl(STDOUT, TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (80, 24)
        }
    }
}

pub fn take_resized() -> bool {
    RESIZED.swap(false, Ordering::SeqCst)
}

// ── Input ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum Key {
    Char(u8),
    CtrlC,
    Enter,
    Backspace,
    Escape,
}

pub fn poll_key(timeout_ms: i32) -> Option<Key> {
    unsafe {
        let mut pfd = PollFd { fd: STDIN, events: POLLIN, revents: 0 };
        let ret = poll(&mut pfd, 1, timeout_ms);
        if ret <= 0 || (pfd.revents & POLLIN) == 0 {
            return None;
        }
        let mut byte: u8 = 0;
        let n = read(STDIN, &mut byte, 1);
        if n != 1 {
            return None;
        }
        Some(match byte {
            3 => Key::CtrlC,
            13 => Key::Enter,
            27 => {
                // Drain any escape sequence bytes quickly
                let mut pfd2 = PollFd { fd: STDIN, events: POLLIN, revents: 0 };
                while poll(&mut pfd2, 1, 20) > 0 && (pfd2.revents & POLLIN) != 0 {
                    let mut discard: u8 = 0;
                    if read(STDIN, &mut discard, 1) <= 0 { break; }
                }
                Key::Escape
            }
            8 | 127 => Key::Backspace,
            b => Key::Char(b),
        })
    }
}

// ── Output helpers ────────────────────────────────────────────────────────

pub fn write_bytes(buf: &[u8]) {
    unsafe {
        let mut off = 0;
        while off < buf.len() {
            let n = write(STDOUT, buf.as_ptr().add(off), buf.len() - off);
            if n <= 0 { break; }
            off += n as usize;
        }
    }

    extern "C" {
        fn write(fd: i32, buf: *const u8, count: usize) -> isize;
    }
}

fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}
