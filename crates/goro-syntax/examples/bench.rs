//! Lexer and parser throughput on real PHP code: the `--FILE--` sections
//! of the php-src tests plus their `.php` / `.inc` helper files.
//!
//! Usage: bench [DIR] [ROUNDS]   (release build: see tools/syntax-bench.sh)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Counts live heap bytes, to report the parser's peak memory.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let n = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        PEAK.fetch_max(n, Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { std::alloc::System.dealloc(p, l) }
    }
}
#[global_allocator]
static GA: Counting = Counting;

use goro_syntax::{Arena, Lexer, LexerOptions, ParseOptions, parse_with};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut v: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    v.sort_by_key(|e| e.file_name());
    for e in v {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => collect(&p, out),
            Ok(_) => out.push(p),
            _ => {}
        }
    }
}

/// The `--FILE--` section of a .phpt file.
fn file_section(data: &[u8]) -> Option<Vec<u8>> {
    let start = data.windows(9).position(|w| w == b"--FILE--\n")? + 9;
    let rest = &data[start..];
    let mut end = rest.len();
    let mut i = 0;
    while i < rest.len() {
        if rest[i..].starts_with(b"\n--") {
            let line_end = rest[i + 1..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|p| i + 1 + p)
                .unwrap_or(rest.len());
            let line = &rest[i + 1..line_end];
            if line.len() > 4
                && line.ends_with(b"--")
                && line[2..line.len() - 2]
                    .iter()
                    .all(|c| c.is_ascii_uppercase() || *c == b'_')
            {
                end = i + 1;
                break;
            }
        }
        i += 1;
    }
    Some(rest[..end].to_vec())
}

fn main() {
    let mut dir = PathBuf::from("/tmp/claude-1000/php-tests");
    let mut rounds = 5usize;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut concat: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--file" => files.push(PathBuf::from(args.next().expect("--file FILE"))),
            "--concat" => concat = Some(PathBuf::from(args.next().expect("--concat OUT"))),
            _ => match a.parse() {
                Ok(n) => rounds = n,
                Err(_) => dir = PathBuf::from(a),
            },
        }
    }
    let mut sources = Vec::new();
    if files.is_empty() {
        let mut paths = Vec::new();
        collect(&dir, &mut paths);
        for p in &paths {
            let Ok(data) = std::fs::read(p) else { continue };
            match p.extension().and_then(|e| e.to_str()) {
                Some("phpt") => {
                    if let Some(s) = file_section(&data) {
                        sources.push(s);
                    }
                }
                Some("php") | Some("inc") => sources.push(data),
                _ => {}
            }
        }
    } else {
        for f in &files {
            sources.push(std::fs::read(f).expect("read"));
        }
    }
    if let Some(out) = concat {
        // One large file from test sources that parse alone, end in HTML
        // mode and do not stop at __halt_compiler.
        let mut big = Vec::new();
        for s in &sources {
            let t = s.trim_ascii_end();
            if !t.ends_with(b"?>")
                || s.windows(15)
                    .any(|w| w.eq_ignore_ascii_case(b"__halt_compiler"))
                || s.windows(8).any(|w| w.eq_ignore_ascii_case(b"encoding"))
            {
                continue;
            }
            if parse_with(&Arena::new(), s, ParseOptions::default()).is_err() {
                continue;
            }
            big.extend_from_slice(t);
            big.push(b'\n');
            if big.len() > 4 << 20 {
                break;
            }
        }
        std::fs::write(&out, &big).expect("write");
        println!(
            "wrote {} bytes; parses: {}",
            big.len(),
            parse_with(&Arena::new(), &big, ParseOptions::default()).is_ok()
        );
        return;
    }
    let bytes: usize = sources.iter().map(|s| s.len()).sum();
    let mb = bytes as f64 / 1e6;
    let opts = LexerOptions {
        short_open_tag: true,
        parse_mode: false,
    };
    let mut best_lex = f64::MAX;
    let mut best_parse = f64::MAX;
    let mut best_drop = f64::MAX;
    let mut arena_bytes = 0usize;
    let mut peak = 0usize;
    let mut live = 0usize;
    let mut tokens = 0usize;
    let mut ok = 0usize;
    for _ in 0..rounds {
        let t0 = Instant::now();
        tokens = 0;
        for s in &sources {
            let mut lx = Lexer::new(s, opts);
            while let Some(t) = lx.next_token() {
                std::hint::black_box(t);
                tokens += 1;
            }
        }
        best_lex = best_lex.min(t0.elapsed().as_secs_f64());
        ok = 0;
        let arenas: Vec<Arena> = sources.iter().map(|_| Arena::new()).collect();
        let base = LIVE.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let t0 = Instant::now();
        let mut keep = Vec::with_capacity(sources.len());
        for (s, arena) in sources.iter().zip(&arenas) {
            match parse_with(arena, s, ParseOptions::default()) {
                Err(e) if std::env::var_os("BENCH_ERRORS").is_some() => {
                    eprintln!(
                        "{} @{} | {:?}",
                        e.message,
                        e.line,
                        String::from_utf8_lossy(&s[..s.len().min(80)])
                    )
                }
                Err(_) => {}
                Ok(p) => {
                    keep.push(p);
                    ok += 1;
                }
            }
        }
        best_parse = best_parse.min(t0.elapsed().as_secs_f64());
        peak = PEAK.load(Ordering::Relaxed) - base;
        live = LIVE.load(Ordering::Relaxed) - base;
        let t0 = Instant::now();
        drop(keep);
        arena_bytes = arenas
            .iter()
            .map(|a| a.allocated_bytes() - a.chunk_capacity())
            .sum();
        drop(arenas);
        best_drop = best_drop.min(t0.elapsed().as_secs_f64());
    }
    println!(
        "{} sources, {:.1} MB, {} tokens ({} parse)",
        sources.len(),
        mb,
        tokens,
        ok
    );
    println!(
        "lexer:          {:.3} s  {:6.1} MB/s  {:5.1} M tokens/s",
        best_lex,
        mb / best_lex,
        tokens as f64 / 1e6 / best_lex
    );
    println!(
        "lexer + parser: {:.3} s  {:6.1} MB/s",
        best_parse,
        mb / best_parse
    );
    println!("AST drop:       {:.3} s", best_drop);
    println!(
        "memory: arena chunks {:.1} MB ({:.1} MB used), parse peak {:.1} MB",
        live as f64 / 1e6,
        arena_bytes as f64 / 1e6,
        peak as f64 / 1e6
    );
}
