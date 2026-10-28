//! The operator-overloading hooks of internal classes (`do_operation`,
//! `do_unary`), driven by a test class.

use goro_engine::class::{NativeHooks, UnaryOp};
use goro_engine::{Module, R, RunOptions, Script, Vm};
use goro_runtime::ops::BinOp;
use goro_runtime::{ObjRef, Value};

fn op(_vm: &mut Vm, op: BinOp, a: &Value, b: &Value) -> Option<R<Value>> {
    let side = if matches!(a, Value::Object(_)) {
        "L"
    } else {
        "R"
    };
    let other = if side == "L" { b } else { a };
    let o = match other {
        Value::Int(i) => i.to_string(),
        Value::Str(s) => s.to_string_lossy(),
        Value::Object(_) => "obj".into(),
        _ => "?".into(),
    };
    match op {
        BinOp::Add | BinOp::Mod | BinOp::Mul | BinOp::Shl => Some(Ok(Value::from(
            format!("{}{}{}", side, op.symbol(), o).as_str(),
        ))),
        _ => None,
    }
}

fn unary(_vm: &mut Vm, op: UnaryOp, _o: &ObjRef) -> Option<R<Value>> {
    match op {
        UnaryOp::Neg => Some(Ok(Value::from("neg"))),
        UnaryOp::Inc => Some(Ok(Value::Int(100))),
        _ => None,
    }
}

fn hooks() -> NativeHooks {
    NativeHooks {
        do_operation: Some(op),
        do_unary: Some(unary),
        ..Default::default()
    }
}

static MODULE: Module = Module {
    name: "ophook_test",
    classes: &[("opbox", "<?php final class OpBox {}")],
    hooks: &[("opbox", hooks)],
    listed: false,
    ..Module::EMPTY
};

#[test]
fn operators_reach_the_hooks() {
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("native_hooks.out");
    let _ = std::fs::remove_file(&out);
    let code = format!(
        r#"$o = new OpBox;
$r = [$o + 1, 2 + $o, $o % 3, "4" % $o, -$o, +$o, 5 << $o];
try {{ $x = "abc" % $o; }} catch (TypeError $e) {{ $r[] = $e->getMessage(); }}
try {{ $x = $o - 1; }} catch (TypeError $e) {{ $r[] = $e->getMessage(); }}
try {{ $x = ~$o; }} catch (TypeError $e) {{ $r[] = $e->getMessage(); }}
$p = $o; $p++; $r[] = $p;
file_put_contents({:?}, implode("|", $r));"#,
        out.to_string_lossy()
    );
    let status = goro_engine::run(RunOptions {
        script: Script::Code(code.into_bytes()),
        ini: Vec::new(),
        args: Vec::new(),
        modules: vec![&MODULE],
        lines: None,
    });
    assert_eq!(status, 0);
    let got = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        got,
        "L+1|R+2|L%3|R%4|neg|L*1|R<<5|Unsupported operand types: string % OpBox|Unsupported operand types: OpBox - int|Cannot perform bitwise not on OpBox|100"
    );
}

// ---------------------------------------------------------------- streams

use goro_engine::builtins::file::{Backing, Stream};
use goro_engine::builtins::streams::{self, FilterSide, NativeStream, NativeWrapper, StreamFilter};

/// Reads "abc" repeated `n` times.
struct Abc(usize);

impl NativeStream for Abc {
    fn read(&mut self, _vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.0 == 0 || buf.len() < 3 {
            return Ok(0);
        }
        self.0 -= 1;
        buf[..3].copy_from_slice(b"abc");
        Ok(3)
    }
}

fn abc_open(
    _vm: &mut Vm,
    _a: &goro_engine::Args,
    url: &[u8],
    mode: &[u8],
    _quiet: bool,
) -> R<Option<Stream>> {
    let n = url.len() - b"abc://".len();
    Ok(Some(Stream::new(
        Backing::Native(Box::new(Abc(n))),
        mode,
        url,
        "abc",
        "abc",
    )))
}

static ABC: NativeWrapper = NativeWrapper { open: abc_open };

/// Doubles every byte.
struct Double;

impl StreamFilter for Double {
    fn filter(&mut self, _vm: &mut Vm, input: &[u8], _c: bool, out: &mut Vec<u8>) -> R<bool> {
        for &b in input {
            out.push(b);
            out.push(b);
        }
        Ok(true)
    }
}

fn double(
    _vm: &mut Vm,
    _name: &[u8],
    _p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    Ok(Some(Box::new(Double)))
}

fn streams_init(vm: &mut Vm) {
    streams::register_wrapper(vm, "abc", &ABC);
    streams::register_filter(vm, "test.double", double);
}

static STREAMS: Module = Module {
    name: "streams_test",
    init: Some(streams_init),
    listed: false,
    ..Module::EMPTY
};

#[test]
fn native_wrappers_and_filters() {
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("native_streams.out");
    let _ = std::fs::remove_file(&out);
    let code = format!(
        r#"$r = [file_get_contents("abc://xx"), in_array("abc", stream_get_wrappers())];
$f = fopen("abc://x", "r");
stream_filter_append($f, "test.double");
$r[] = fread($f, 100);
$r[] = in_array("test.double", stream_get_filters());
file_put_contents({:?}, json_encode($r));"#,
        out.to_string_lossy()
    );
    let status = goro_engine::run(RunOptions {
        script: Script::Code(code.into_bytes()),
        ini: Vec::new(),
        args: Vec::new(),
        modules: vec![&STREAMS],
        lines: None,
    });
    assert_eq!(status, 0);
    let got = std::fs::read_to_string(&out).unwrap();
    assert_eq!(got, r#"["abcabc",true,"aabbcc",true]"#);
}
