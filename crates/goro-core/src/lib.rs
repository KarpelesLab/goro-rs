pub mod anon_class;
pub mod arginfo;
pub mod array;
pub mod ast_export;
pub mod ast_walk;
pub mod basedir;
pub mod callable;
pub mod class_table;
pub mod clone;
pub mod closure;
pub mod compiler;
pub mod const_order;
pub mod crypto;
pub mod csv;
pub mod custom_fiber;
pub mod errdisplay;
pub mod ext_order;
pub mod ext_stream;
pub mod fastpath;
pub mod fetch_w;
pub mod fiber;
pub mod foreach_obj;
pub mod function;
pub mod fxhash;
pub mod gc;
pub mod generator;
pub mod image_handlers;
pub mod include_path;
pub mod ini_quantity;
pub mod initcheck;
pub mod internal_frames;
pub mod internal_iterator;
pub mod ob;
pub mod object;
pub mod opcode;
pub mod preload;
pub mod proptable;
pub mod random;
pub mod rcstr;
pub mod recv_fast;
pub mod reflection;
pub mod rfc1867;
pub mod sapi_headers;
pub mod script_cache;
pub mod smalloc;
pub mod spl;
pub mod spl_array;
pub mod spl_dir;
pub mod spl_dll;
pub mod spl_file;
pub mod spl_fixed;
pub mod spl_heap;
pub mod spl_iter;
pub mod spl_storage;
pub mod spl_stubs;
pub mod stack;
pub mod string;
pub mod stub_sigs;
pub mod stubs;
pub mod symtab;
pub mod tokenizer;
pub mod uncaught;
pub mod uri;
pub mod value;
pub mod vm;

pub use compiler::Compiler;
pub use value::Value;
pub use vm::Vm;

/// php_build_date: "Mon DD YYYY HH:MM:SS" of the binary.
pub fn php_build_date() -> String {
    let secs = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {:2} {} {:02}:{:02}:{:02}",
        MONTHS[(m - 1) as usize],
        d,
        y,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
