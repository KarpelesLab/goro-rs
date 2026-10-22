//! Runs a document through the library the way PHP's ext/tidy does
//! (`new tidy` + parseString + cleanRepair + diagnose), printing what
//! tidy_get_output() and tidy_get_error_buffer() would return. Used to
//! compare against PHP builds linked with libtidy.
//!
//! usage: phptidy [-o name=value]... [-c] [-d] [-e encoding] file

use goro_tidy::{Doc, OptId, tidylib};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut opts: Vec<(String, String)> = Vec::new();
    let mut clean = false;
    let mut diag = false;
    let mut enc: Option<String> = None;
    let mut file = None;
    let mut cfgfile: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => {
                let kv = args.next().expect("-o name=value");
                let (k, v) = kv.split_once('=').expect("-o name=value");
                opts.push((k.to_string(), v.to_string()));
            }
            "-f" => cfgfile = args.next(),
            "-c" => clean = true,
            "-d" => diag = true,
            "-e" => enc = args.next(),
            _ => file = Some(a),
        }
    }
    let data = std::fs::read(file.expect("file")).expect("read");
    let mut doc = Doc::new();
    tidylib::set_error_buffer(&mut doc);
    tidylib::opt_set_bool(&mut doc, OptId::ForceOutput, true);
    tidylib::opt_set_bool(&mut doc, OptId::Mark, false);
    if let Some(f) = &cfgfile {
        tidylib::load_config(&mut doc, f.as_bytes());
    }
    for (k, v) in &opts {
        if !tidylib::opt_parse_value(&mut doc, k.as_bytes(), v.as_bytes()) {
            eprintln!("bad option {k}={v}");
        }
    }
    // php_tidy_parse_string(): a bad encoding leaves the document
    // unparsed (and uninitialized: diagnose() does nothing)
    let mut initialized = true;
    if let Some(e) = enc {
        if tidylib::set_char_encoding(&mut doc, e.as_bytes()) < 0 {
            initialized = false;
        }
    }
    // every PHP operation ends with tidy_doc_update_properties(), which
    // saves the document
    if initialized {
        tidylib::parse_buffer(&mut doc, data);
        tidylib::save_buffer(&mut doc);
    }
    if clean {
        tidylib::clean_and_repair(&mut doc);
        tidylib::save_buffer(&mut doc);
    }
    if diag && initialized {
        tidylib::run_diagnostics(&mut doc);
        tidylib::save_buffer(&mut doc);
    }
    let (_, out) = tidylib::save_buffer(&mut doc);
    use std::io::Write;
    let mut so = std::io::stdout();
    let _ = so.write_all(b"=== OUTPUT\n");
    let _ = so.write_all(&out[..out.len().saturating_sub(1)]);
    let _ = so.write_all(b"\n=== ERRORS\n");
    let eb = doc.errout.bytes();
    let _ = so.write_all(&eb[..eb.len().saturating_sub(1)]);
    let _ = so.write_all(b"\n");
}
