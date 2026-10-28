//! Throughput of decode/encode on a JSON document (the counterpart of
//! tests/fixtures/json_bench.php, which writes the document):
//!
//!     php tests/fixtures/json_bench.php /tmp/doc
//!     cargo run --release -p goro-ext-json --example json_bench -- /tmp/doc.*.json

use std::rc::Rc;
use std::time::Instant;

use goro_ext_json::consts::*;
use goro_ext_json::{JsonError, JsonHost, ObjectKind, json_decode, json_encode, json_validate};
use goro_runtime::{ClassInfo, ObjRef, PhpArray, PhpError, PhpStr, Value};

struct Std(PhpStr);

impl ClassInfo for Std {
    fn name(&self) -> &PhpStr {
        &self.0
    }
}

struct Host {
    std: Rc<dyn ClassInfo>,
}

impl JsonHost for Host {
    type Exception = String;
    fn object_kind(&mut self, _: &ObjRef) -> ObjectKind {
        ObjectKind::Plain
    }
    fn json_serialize(&mut self, _: &ObjRef) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn object_properties(&mut self, o: &ObjRef) -> Result<PhpArray, String> {
        Ok(o.obj().props.borrow().clone())
    }
    fn new_object(&mut self, props: PhpArray) -> Value {
        let o = ObjRef::new(self.std.clone(), Vec::new());
        *o.obj().props.borrow_mut() = props;
        Value::Object(o)
    }
    fn json_exception(&mut self, e: JsonError) -> String {
        e.message().to_string()
    }
    fn throw_error(&mut self, e: PhpError) -> String {
        e.message.clone()
    }
}

fn bench(label: &str, mb: f64, mut f: impl FnMut()) {
    f();
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("{label:<28} {:8.1} MB/s ({:.2} ms)", mb / best, best * 1e3);
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: json_bench <doc.json>...");
    for path in paths {
        run(&path);
    }
}

fn run(path: &str) {
    let doc = std::fs::read(path).expect("read document");
    let mb = doc.len() as f64 / 1e6;
    println!("{path}: {mb:.2} MB");
    let mut h = Host {
        std: Rc::new(Std(PhpStr::from("stdClass"))),
    };
    let arr = json_decode(&mut h, &doc, Some(true), 512, 0).unwrap();
    let obj = json_decode(&mut h, &doc, Some(false), 512, 0).unwrap();
    assert_eq!(goro_ext_json::json_last_error(), 0);
    // Round trip check: re-encoding gives the document back.
    match json_encode(&mut h, &arr, 0, 512).unwrap() {
        Value::Str(s) => assert!(s.as_bytes() == &doc[..], "round trip differs"),
        _ => panic!("encode failed"),
    }
    bench("decode assoc", mb, || {
        std::hint::black_box(json_decode(&mut h, &doc, Some(true), 512, 0).unwrap());
    });
    bench("decode objects", mb, || {
        std::hint::black_box(json_decode(&mut h, &doc, Some(false), 512, 0).unwrap());
    });
    bench("validate", mb, || {
        std::hint::black_box(json_validate(&mut h, &doc, 512, 0).unwrap());
    });
    bench("encode arrays", mb, || {
        std::hint::black_box(json_encode(&mut h, &arr, 0, 512).unwrap());
    });
    bench("encode objects", mb, || {
        std::hint::black_box(json_encode(&mut h, &obj, 0, 512).unwrap());
    });
    bench("encode unescaped", mb, || {
        std::hint::black_box(
            json_encode(
                &mut h,
                &arr,
                JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES,
                512,
            )
            .unwrap(),
        );
    });
    bench("encode pretty", mb, || {
        std::hint::black_box(json_encode(&mut h, &arr, JSON_PRETTY_PRINT, 512).unwrap());
    });
}
