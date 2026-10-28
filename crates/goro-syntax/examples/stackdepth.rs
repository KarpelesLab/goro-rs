//! Measures the parser's stack use per nesting level for a few deeply
//! nested constructs (run in release mode).

fn used_per_level(make: &dyn Fn(usize) -> String) -> f64 {
    let n = 2000;
    let src = make(n);
    // Find the smallest stack (in 64 KiB steps) that parses `n` levels.
    let mut lo = 64 * 1024;
    let mut hi = 256 * 1024 * 1024;
    while hi - lo > 64 * 1024 {
        let mid = (lo + hi) / 2;
        let s = src.clone();
        let ok = std::thread::Builder::new()
            .stack_size(mid)
            .spawn(move || {
                let arena = goro_syntax::Arena::new();
                let _ = goro_syntax::parse(&arena, s.as_bytes());
            })
            .unwrap()
            .join()
            .is_ok();
        if ok {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi as f64 / n as f64
}

fn main() {
    // A stack overflow aborts the process, so probe in child processes.
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 {
        let kind = args[1].as_str();
        let n: usize = args[2].parse().unwrap();
        let src = make_src(kind, n);
        let arena = goro_syntax::Arena::new();
        let _ = goro_syntax::parse(&arena, src.as_bytes());
        return;
    }
    for kind in ["unary", "paren", "array", "call", "assign", "if", "closure"] {
        let exe = std::env::current_exe().unwrap();
        let n = 2000;
        let mut lo: u64 = 16 * 1024;
        let mut hi: u64 = 512 * 1024 * 1024;
        while hi - lo > 16 * 1024 {
            let mid = (lo + hi) / 2;
            let ok = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!(
                    "ulimit -s {}; exec {} {} {}",
                    mid / 1024,
                    exe.display(),
                    kind,
                    n
                ))
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        println!("{:8} ~{} bytes of stack per level", kind, hi / n as u64);
    }
    let _ = used_per_level;
}

fn make_src(kind: &str, n: usize) -> String {
    let mut s = String::from("<?php $x = ");
    match kind {
        "unary" => {
            s.push_str(&"~".repeat(n));
            s.push_str("1;");
        }
        "paren" => {
            s.push_str(&"(".repeat(n));
            s.push('1');
            s.push_str(&")".repeat(n));
            s.push(';');
        }
        "array" => {
            s.push_str(&"[".repeat(n));
            s.push_str(&"]".repeat(n));
            s.push(';');
        }
        "call" => {
            s.push_str(&"f(".repeat(n));
            s.push_str(&")".repeat(n));
            s.push(';');
        }
        "assign" => {
            for _ in 0..n {
                s.push_str("$a = ");
            }
            s.push_str("1;");
        }
        "if" => {
            s = String::from("<?php ");
            s.push_str(&"if (1) {".repeat(n));
            s.push_str(&"}".repeat(n));
        }
        _ => {
            s.push_str(&"function () { return ".repeat(n));
            s.push('1');
            s.push_str(&"; }".repeat(n));
            s.push(';');
        }
    }
    s
}
