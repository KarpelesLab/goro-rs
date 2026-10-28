//! A stand-in "php" for goro-phpt's end-to-end tests. It accepts the CLI
//! options the runner passes (`-n`, `-d`, `-f`, `--`, `-m`, `-r`) and runs a
//! tiny line-based script language instead of PHP:
//!
//! - `echo TEXT`, `stderr TEXT`: print a line to stdout / stderr
//! - `ini KEY`, `env KEY`: print a `-d` setting / environment variable
//! - `args`, `cwd`, `file`: print the script arguments, working directory,
//!   script path
//! - `stdin`: copy standard input to standard output
//! - `sleep SECS`, `exit CODE`, `crash` (abort)
//! - `touch PATH`, `rm PATH`, `exists PATH`: files relative to the script
//! - `flaky PATH`: prints "first" and creates PATH, or "second" if it exists
//!
//! Other lines are ignored.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    let mut ini: Vec<(String, String)> = Vec::new();
    let mut script: Option<PathBuf> = None;
    let mut code: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-n" => {}
            "-d" => {
                let kv = it.next().unwrap_or_default();
                let (k, v) = kv.split_once('=').unwrap_or((&kv, "1"));
                ini.push((k.to_string(), v.to_string()));
            }
            "-m" => {
                println!("[PHP Modules]\nCore\nstandard\nfakeext\n\n[Zend Modules]\n");
                return;
            }
            "-r" => code = it.next(),
            "-f" => script = it.next().map(PathBuf::from),
            "--" => {
                args.extend(it.by_ref());
            }
            _ => {
                if script.is_none() && code.is_none() {
                    script = Some(PathBuf::from(&a));
                } else {
                    args.push(a);
                }
            }
        }
    }
    if let Some(code) = code {
        // The runner's shared-extension probe asks for extension_dir.
        if code.contains("extension_dir") {
            println!("sharedext");
        }
        return;
    }
    let Some(script) = script else {
        eprintln!("fake-php: no script");
        std::process::exit(1);
    };
    let text = std::fs::read_to_string(&script).unwrap_or_default();
    let dir = script.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut out = std::io::stdout();
    for line in text.lines() {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        match cmd {
            "echo" => println!("{rest}"),
            "stderr" => eprintln!("{rest}"),
            "ini" => {
                let v = ini
                    .iter()
                    .rev()
                    .find(|(k, _)| k == rest)
                    .map(|(_, v)| v.clone());
                println!("{}", v.unwrap_or_else(|| "(unset)".into()));
            }
            "env" => println!(
                "{}",
                std::env::var(rest).unwrap_or_else(|_| "(unset)".into())
            ),
            "args" => {
                for a in &args {
                    println!("{a}");
                }
            }
            "cwd" => println!("{}", std::env::current_dir().unwrap().display()),
            "file" => println!("{}", script.display()),
            "stdin" => {
                let mut s = Vec::new();
                std::io::stdin().read_to_end(&mut s).unwrap();
                out.write_all(&s).unwrap();
            }
            "sleep" => std::thread::sleep(std::time::Duration::from_secs_f64(
                rest.parse().unwrap_or(1.0),
            )),
            "exit" => std::process::exit(rest.parse().unwrap_or(0)),
            "crash" => std::process::abort(),
            "touch" => std::fs::write(dir.join(rest), b"").unwrap(),
            "rm" => {
                let _ = std::fs::remove_file(dir.join(rest));
            }
            "exists" => println!("{}", if dir.join(rest).exists() { "yes" } else { "no" }),
            "flaky" => {
                let p = dir.join(rest);
                if p.exists() {
                    println!("second");
                } else {
                    std::fs::write(&p, b"").unwrap();
                    println!("first");
                }
            }
            _ => {}
        }
        out.flush().unwrap();
    }
}
