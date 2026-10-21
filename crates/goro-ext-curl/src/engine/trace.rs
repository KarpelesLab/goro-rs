//! Verbose tracing (lib/curl_trc.c): Curl_debug(), infof()

use super::Callbacks;
use super::consts::info;

/// TRC_LINE_MAX
const TRC_LINE_MAX: usize = 2048;

#[derive(Clone, Copy)]
pub(crate) struct Tracer {
    pub verbose: bool,
    pub debug_cb: bool,
}

impl Tracer {
    /// Curl_debug()
    pub fn debug(&self, cb: &mut dyn Callbacks, kind: u32, data: &[u8]) {
        if !self.verbose {
            return;
        }
        if self.debug_cb {
            cb.debug(kind, data);
        } else {
            let prefix: &[u8] = match kind {
                info::TEXT => b"* ",
                info::HEADER_IN => b"< ",
                info::HEADER_OUT => b"> ",
                _ => return,
            };
            let mut v = Vec::with_capacity(data.len() + 2);
            v.extend_from_slice(prefix);
            v.extend_from_slice(data);
            cb.stderr(&v);
        }
    }

    /// infof()
    pub fn infof(&self, cb: &mut dyn Callbacks, msg: &str) {
        if !self.verbose {
            return;
        }
        self.text(cb, msg.as_bytes());
    }

    /// A CURLINFO_TEXT line ("\n" appended, truncated like trc_end_buf)
    pub fn text(&self, cb: &mut dyn Callbacks, msg: &[u8]) {
        if !self.verbose {
            return;
        }
        let mut v = msg.to_vec();
        if v.len() >= TRC_LINE_MAX - 2 {
            v.truncate(TRC_LINE_MAX - 5);
            v.extend_from_slice(b"...\n");
        } else {
            v.push(b'\n');
        }
        self.debug(cb, info::TEXT, &v);
    }
}
