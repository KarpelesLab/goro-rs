//! Debugging aid: rx <php-pattern> <subject> [-p]
//! Prints preg_match_all-style results (and the program with -p).
//! A subject starting with "hex:" is hex-decoded.

use goro_regex::Limits;
use goro_regex::php::{self, GlobalMatcher};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pat = args[1].as_bytes();
    let subj: Vec<u8> = if let Some(h) = args[2].strip_prefix("hex:") {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    } else {
        args[2].as_bytes().to_vec()
    };
    let re = match php::compile(pat) {
        Ok(r) => r,
        Err(e) => {
            println!("ERR {}", e.message());
            return;
        }
    };
    if args.iter().any(|a| a == "-p") {
        for (i, ins) in re.prog().insts.iter().enumerate() {
            println!("{i:4} {ins:?}");
        }
        println!("{:?}", re.prog().start);
    }
    let mut g = GlobalMatcher::new(&re, &subj, 0, Limits::new(1_000_000, 100_000));
    loop {
        match g.next_match() {
            Ok(true) => {
                let c = &g.caps;
                let v: Vec<String> = (0..c.highest_set())
                    .map(|k| match c.get(k) {
                        Some((s, e)) => format!("{k}={s},{e}"),
                        None => format!("{k}=-"),
                    })
                    .collect();
                println!(
                    "match {} mark={:?}",
                    v.join(" "),
                    c.mark().map(String::from_utf8_lossy)
                );
            }
            Ok(false) => break,
            Err(e) => {
                println!("error {e:?}");
                break;
            }
        }
    }
}
