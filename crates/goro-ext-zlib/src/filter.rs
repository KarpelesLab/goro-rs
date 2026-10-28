//! The `zlib.deflate` and `zlib.inflate` stream filters (PHP manual:
//! Compression Filters), as transformations of the bytes passing through.
//! The engine feeds each bucket's data with the stream's flush state and
//! emits the returned bytes.
//!
//! Parameters, warnings and flushing were observed on PHP 8.5
//! (tests/fixtures/behaviour_php85.txt): `zlib.deflate` takes `level`,
//! `window` and `memory` (a non-array parameter is the level), defaults to
//! raw deflate at the default level with memory level 9, sync-flushes on
//! `fflush()` and finishes the stream when the filter is closed or
//! removed; `zlib.inflate` takes `window` (raw deflate by default), stops
//! at the end of the first stream and ignores what follows, and reports
//! corrupt data with a notice ("zlib: data error").

use crate::api::Diag;
use crate::deflate::{DeflateParams, DeflateStatus, Deflater, Flush, Strategy, Wrap};
use crate::inflate::{InflateFlush, InflateStatus, InflateWrap, Inflater};

/// Filter parameters after the engine's conversions: the integer values of
/// the recognised keys of an array or object, or the integer value of a
/// scalar parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FilterParams {
    #[default]
    None,
    /// A parameter that is neither an array nor an object, as `(int)`.
    Scalar(i64),
    Map {
        level: Option<i64>,
        window: Option<i64>,
        memory: Option<i64>,
    },
}

/// How a filter call is flushing (PHP's `PSFS_FLAG_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFlush {
    /// Ordinary data.
    None,
    /// `fflush()`.
    Inc,
    /// The stream is closing or the filter is removed.
    Close,
}

/// Output buffer of PHP's zlib filters (zlib calls see this much output
/// space at a time).
const FILTER_BUF: usize = 0x8000;

fn window_ok(w: i64) -> bool {
    (-15..=31).contains(&w)
}

/// `zlib.deflate`.
pub struct DeflateFilter {
    d: Deflater,
    done: bool,
}

/// Creates a `zlib.deflate` filter; `None` when zlib rejects the
/// parameters (the engine then warns "Unable to create or locate filter").
/// Out-of-range values warn and fall back to the defaults.
pub fn deflate_filter(params: &FilterParams, diags: &mut Vec<Diag>) -> Option<DeflateFilter> {
    let mut level = -1i64;
    let mut window = -15i64;
    let mut memory = 9i64;
    let mut set_level = |v: i64, diags: &mut Vec<Diag>| {
        if (-1..=9).contains(&v) {
            level = v;
        } else {
            diags.push(Diag::Warning(format!(
                "Invalid compression level specified. ({v})"
            )));
        }
    };
    match params {
        FilterParams::None => {}
        FilterParams::Scalar(v) => set_level(*v, diags),
        FilterParams::Map {
            level: l,
            window: w,
            memory: m,
        } => {
            if let Some(m) = m {
                if (1..=9).contains(m) {
                    memory = *m;
                } else {
                    diags.push(Diag::Warning(format!(
                        "Invalid parameter given for memory level ({m})"
                    )));
                }
            }
            if let Some(w) = w {
                if window_ok(*w) {
                    window = *w;
                } else {
                    diags.push(Diag::Warning(format!(
                        "Invalid parameter given for window size ({w})"
                    )));
                }
            }
            if let Some(l) = l {
                set_level(*l, diags);
            }
        }
    }
    let (wrap, bits) = match window {
        w if w < 0 => (Wrap::Raw, -w),
        w if w > 15 => (Wrap::Gzip, w - 16),
        w => (Wrap::Zlib, w),
    };
    let d = Deflater::new(DeflateParams {
        level: level as i32,
        window_bits: bits as u32,
        mem_level: memory as u32,
        strategy: Strategy::Default,
        wrap,
    })
    .ok()?;
    Some(DeflateFilter { d, done: false })
}

impl DeflateFilter {
    /// Compresses one bucket's worth of data.
    pub fn filter(&mut self, input: &[u8], flush: FilterFlush) -> Vec<u8> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        let mode = match flush {
            FilterFlush::None => Flush::None,
            FilterFlush::Inc => Flush::Sync,
            FilterFlush::Close => Flush::Finish,
        };
        // The data goes through in pieces of the filter's buffer size, each
        // zlib call seeing a fresh output buffer of that size (observed on
        // PHP 8.5 through level 0's stored block sizes); then the flush is
        // repeated until zlib has nothing more to say.
        let mut bin = 0;
        while bin < input.len() {
            let desired = (input.len() - bin).min(FILTER_BUF);
            let mut inp = &input[bin..bin + desired];
            let mut avail = FILTER_BUF;
            self.d
                .deflate_step(&mut inp, Flush::None, &mut out, &mut avail);
            let used = desired - inp.len();
            if used == 0 && avail == FILTER_BUF {
                break;
            }
            bin += used;
        }
        if mode != Flush::None {
            loop {
                let mut avail = FILTER_BUF;
                let mut empty: &[u8] = &[];
                let st = self.d.deflate_step(&mut empty, mode, &mut out, &mut avail);
                if st == DeflateStatus::StreamEnd {
                    self.done = true;
                }
                if st != DeflateStatus::Ok {
                    break;
                }
            }
        }
        out
    }
}

/// `zlib.inflate`.
pub struct InflateFilter {
    inf: Inflater,
    done: bool,
    failed: Option<&'static str>,
}

/// Creates a `zlib.inflate` filter; `None` when zlib rejects the window.
pub fn inflate_filter(params: &FilterParams, diags: &mut Vec<Diag>) -> Option<InflateFilter> {
    let mut window = -15i64;
    if let FilterParams::Map {
        window: Some(w), ..
    } = params
    {
        if window_ok(*w) || (32..=47).contains(w) {
            window = *w;
        } else {
            diags.push(Diag::Warning(format!(
                "Invalid parameter given for window size ({w})"
            )));
        }
    }
    let (wrap, bits) = InflateWrap::from_window_bits(window)?;
    Some(InflateFilter {
        inf: Inflater::new(wrap, bits),
        done: false,
        failed: None,
    })
}

impl InflateFilter {
    /// Decompresses one bucket's worth of data. An error is the notice
    /// PHP raises (without the `fread(): ` prefix); the filter then fails
    /// every later call too.
    pub fn filter(&mut self, input: &[u8], _flush: FilterFlush) -> Result<Vec<u8>, Diag> {
        if let Some(m) = self.failed {
            return Err(Diag::Notice(format!("zlib: {m}")));
        }
        let mut out = Vec::new();
        if self.done || input.is_empty() {
            return Ok(out);
        }
        let (_, st) = self
            .inf
            .inflate(input, &mut out, usize::MAX, InflateFlush::None);
        match st {
            InflateStatus::StreamEnd => self.done = true,
            InflateStatus::DataError(_) => {
                self.failed = Some("data error");
                return Err(Diag::Notice("zlib: data error".into()));
            }
            InflateStatus::NeedDict => {
                self.failed = Some("need dictionary");
                return Err(Diag::Notice("zlib: need dictionary".into()));
            }
            _ => {}
        }
        Ok(out)
    }
}
