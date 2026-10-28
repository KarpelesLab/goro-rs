use std::ffi::OsString;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = match args.first().map(|a| a.to_string_lossy()) {
        Some(a) if a == "--test" => goro_phpt::cli::test_main(&args[1..]),
        Some(a) if a == "--test-compare" => goro_phpt::cli::compare_main(&args[1..]),
        _ => {
            eprintln!("goro: not implemented yet");
            1
        }
    };
    std::process::exit(code);
}
