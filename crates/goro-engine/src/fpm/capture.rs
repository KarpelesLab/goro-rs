//! Workers' stdout and stderr as the master logs them with
//! catch_workers_output (PHP manual: catch_workers_output,
//! decorate_workers_output, log_limit, log_buffering).
//!
//! Output is cut into lines at newlines and at the flush mark a worker
//! writes after each request; a line longer than the limit continues on
//! the next one. Decorated lines read
//! `[time] WARNING: [pool NAME] child PID said into stderr: "text"`, the
//! last one of a closed pipe ending with `, pipe is closed`; plain lines
//! are the text alone. Each line, with its newline, is at most log_limit
//! bytes (tests sapi/fpm/tests/log-bw*.phpt, log-dw*.phpt).

use std::os::fd::RawFd;

use super::log::{self, Level};

/// What a worker writes on its stdout and stderr after each request: the
/// master ends the current line there (test
/// sapi/fpm/tests/log-bwp-msg-flush-split-real.phpt).
pub const FLUSH_MARK: &[u8] = b"\0fscf\0";

const CLOSED_SUFFIX: &str = ", pipe is closed";

/// How lines are written.
#[derive(Clone)]
pub struct Style {
    pub pool: String,
    pub pid: i32,
    pub decorate: bool,
    pub limit: usize,
}

/// One captured stream.
pub struct Capture {
    pub fd: RawFd,
    pub stderr: bool,
    buf: Vec<u8>,
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

impl Capture {
    pub fn new(fd: RawFd, stderr: bool) -> Capture {
        Capture {
            fd,
            stderr,
            buf: Vec::new(),
        }
    }

    fn prefix(&self, st: &Style) -> String {
        format!(
            "{} {}: [pool {}] child {} said into {}: ",
            log::stamp(),
            Level::Warning.name(),
            st.pool,
            st.pid,
            if self.stderr { "stderr" } else { "stdout" }
        )
    }

    /// Text bytes that fit on one line.
    fn room(&self, st: &Style) -> usize {
        if st.decorate {
            st.limit.saturating_sub(self.prefix(st).len() + 3).max(16)
        } else {
            st.limit.saturating_sub(1).max(16)
        }
    }

    fn emit(&self, st: &Style, text: &[u8], suffix: &str) {
        if log::level() > Level::Warning {
            return;
        }
        let mut line = Vec::with_capacity(text.len() + 128);
        if st.decorate {
            line.extend_from_slice(self.prefix(st).as_bytes());
            line.push(b'"');
            line.extend_from_slice(text);
            line.push(b'"');
            line.extend_from_slice(suffix.as_bytes());
        } else {
            line.extend_from_slice(text);
        }
        line.push(b'\n');
        log::write_fd(log::fd(), &line);
    }

    /// Writes `text` as one or more lines.
    fn emit_text(&self, st: &Style, mut text: &[u8], suffix: &str) {
        let room = self.room(st);
        while text.len() > room {
            self.emit(st, &text[..room], "");
            text = &text[room..];
        }
        if text.len() + suffix.len() <= room {
            self.emit(st, text, suffix);
        } else {
            // the suffix goes on a line of its own
            self.emit(st, text, "");
            if log::level() <= Level::Warning {
                let line = format!("{}{}\n", self.prefix(st), suffix.trim_start_matches(", "));
                log::write_fd(log::fd(), line.as_bytes());
            }
        }
    }

    /// Takes new bytes and logs the complete lines.
    pub fn push(&mut self, st: &Style, data: &[u8]) {
        self.buf.extend_from_slice(data);
        self.process(st);
    }

    fn process(&mut self, st: &Style) {
        loop {
            let nl = self.buf.iter().position(|&c| c == b'\n');
            let mk = find(&self.buf, FLUSH_MARK);
            let cut = match (nl, mk) {
                (Some(a), Some(b)) if b < a => Some((b, FLUSH_MARK.len())),
                (Some(a), _) => Some((a, 1)),
                (None, Some(b)) => Some((b, FLUSH_MARK.len())),
                (None, None) => None,
            };
            match cut {
                Some((pos, len)) => {
                    let text: Vec<u8> = self.buf[..pos].to_vec();
                    self.buf.drain(..pos + len);
                    if !text.is_empty() {
                        self.emit_text(st, &text, "");
                    }
                }
                None => {
                    let room = self.room(st);
                    while self.buf.len() >= room {
                        // a part of the flush mark at the end may complete
                        // with the next read
                        if self.buf.len() < room + FLUSH_MARK.len() && self.ends_in_mark_start() {
                            break;
                        }
                        let part: Vec<u8> = self.buf[..room].to_vec();
                        self.buf.drain(..room);
                        self.emit(st, &part, "");
                    }
                    return;
                }
            }
        }
    }

    fn ends_in_mark_start(&self) -> bool {
        (1..FLUSH_MARK.len()).any(|k| self.buf.ends_with(&FLUSH_MARK[..k]))
    }

    /// The pipe closed: what is left is logged.
    pub fn close(&mut self, st: &Style) {
        self.process(st);
        if !self.buf.is_empty() {
            let text = std::mem::take(&mut self.buf);
            let suffix = if st.decorate { CLOSED_SUFFIX } else { "" };
            self.emit_text(st, &text, suffix);
        }
    }

    /// Reads what is available (non-blocking descriptor); false at end of
    /// file.
    pub fn read(&mut self, st: &Style) -> bool {
        let mut chunk = [0u8; 8192];
        loop {
            // SAFETY: reading into a local buffer.
            let n = unsafe {
                libc::read(
                    self.fd,
                    chunk.as_mut_ptr() as *mut libc::c_void,
                    chunk.len(),
                )
            };
            if n > 0 {
                self.push(st, &chunk[..n as usize]);
                continue;
            }
            if n == 0 {
                return false;
            }
            let e = std::io::Error::last_os_error().raw_os_error();
            if e == Some(libc::EINTR) {
                continue;
            }
            return e == Some(libc::EAGAIN);
        }
    }
}
