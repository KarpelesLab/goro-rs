//! Library-level timing of common filter_var() calls, for comparison with
//! tests/fixtures/filter_bench.php on PHP 8.5:
//!
//!   cargo run --release -p goro-ext-filter --example filter_bench

use std::hint::black_box;
use std::time::Instant;

use goro_ext_filter::consts::*;
use goro_ext_filter::{FilterHost, Options, filter_var};
use goro_runtime::{Ctx, Key, Level, ObjRef, PhpArray, PhpError, PhpStr, Value};

struct Host;

impl Ctx for Host {
    fn notice(&mut self, _: Level, _: String) {}
}

impl FilterHost for Host {
    type Exception = ();
    fn object_string(&mut self, _: &ObjRef) -> Result<Option<PhpStr>, ()> {
        Ok(None)
    }
    fn is_callable(&mut self, _: &Value) -> bool {
        false
    }
    fn call(&mut self, _: &Value, _: Value) -> Result<Value, ()> {
        Err(())
    }
    fn throw_error(&mut self, _: PhpError) {}
    fn filter_failed(&mut self, _: PhpStr) {}
}

const N: u32 = 1_000_000;

fn bench(name: &str, value: &str, filter: i64, options: Options<'_>) {
    let mut h = Host;
    let v = Value::Str(PhpStr::from(value));
    // warm up
    for _ in 0..10_000 {
        black_box(filter_var(&mut h, black_box(&v), filter, options).ok());
    }
    let t = Instant::now();
    for _ in 0..N {
        black_box(filter_var(&mut h, black_box(&v), filter, options).ok());
    }
    let ns = t.elapsed().as_nanos() as f64 / N as f64;
    println!("{name:<28} {ns:7.1} ns/call");
}

fn main() {
    let range = PhpArray::from_pairs([(
        Key::from_bytes(b"options"),
        Value::Array(PhpArray::from_pairs([
            (Key::from_bytes(b"min_range"), Value::Int(1)),
            (Key::from_bytes(b"max_range"), Value::Int(100_000)),
        ])),
    )]);
    bench("int", "12345", FILTER_VALIDATE_INT, Options::Int(0));
    bench(
        "int min/max",
        "12345",
        FILTER_VALIDATE_INT,
        Options::Array(&range),
    );
    bench("float", "12345.678", FILTER_VALIDATE_FLOAT, Options::Int(0));
    bench("bool", "yes", FILTER_VALIDATE_BOOL, Options::Int(0));
    bench(
        "email",
        "user.name@example.com",
        FILTER_VALIDATE_EMAIL,
        Options::Int(0),
    );
    bench("ipv4", "192.168.1.1", FILTER_VALIDATE_IP, Options::Int(0));
    bench(
        "ipv6",
        "2001:db8::ff00:42:8329",
        FILTER_VALIDATE_IP,
        Options::Int(0),
    );
    bench(
        "url",
        "http://www.example.com/path?q=1",
        FILTER_VALIDATE_URL,
        Options::Int(0),
    );
    bench(
        "domain hostname",
        "www.example.com",
        FILTER_VALIDATE_DOMAIN,
        Options::Int(FILTER_FLAG_HOSTNAME),
    );
    bench(
        "special_chars",
        "<b>hello</b> & world",
        FILTER_SANITIZE_SPECIAL_CHARS,
        Options::Int(0),
    );
}
