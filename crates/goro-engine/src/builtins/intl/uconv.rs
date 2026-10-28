//! `UConverter` (PHP manual: UConverter). Subclasses overriding
//! `toUCallback()` / `fromUCallback()` get ICU's callback calls: a reset
//! and one call per illegal sequence or unassigned character in each of
//! the two conversion passes PHP makes, and close calls when the object
//! goes away (the order of ext/intl/tests/uconverter_oop_callback2.phpt).

use goro_ext_intl::error as ue;
use goro_ext_intl::uconv::{self as uc, Conv, Unit};
use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use super::{fail, fail_obj, has_obj, obj_err, reset, reset_obj, store, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[("uconverter", UCONV_CLASS)],
    methods: &[
        ("uconverter::__construct", u_construct),
        ("uconverter::convert", u_convert),
        ("uconverter::fromucallback", u_from_callback),
        ("uconverter::toucallback", u_to_callback),
        ("uconverter::getaliases", u_aliases),
        ("uconverter::getavailable", u_available),
        ("uconverter::getdestinationencoding", u_dest_enc),
        ("uconverter::getdestinationtype", u_dest_type),
        ("uconverter::getsourceencoding", u_src_enc),
        ("uconverter::getsourcetype", u_src_type),
        ("uconverter::geterrorcode", u_error_code),
        ("uconverter::geterrormessage", u_error_message),
        ("uconverter::getstandards", u_standards),
        ("uconverter::getsubstchars", u_get_subst),
        ("uconverter::setsubstchars", u_set_subst),
        ("uconverter::reasontext", u_reason_text),
        ("uconverter::setdestinationencoding", u_set_dest),
        ("uconverter::setsourceencoding", u_set_src),
        ("uconverter::transcode", u_transcode),
    ],
    hooks: &[("uconverter", hooks)],
    ..Module::EMPTY
};

const UCONV_CLASS: &str = r#"<?php class UConverter {
    public const int REASON_UNASSIGNED = 0;
    public const int REASON_ILLEGAL = 1;
    public const int REASON_IRREGULAR = 2;
    public const int REASON_RESET = 3;
    public const int REASON_CLOSE = 4;
    public const int REASON_CLONE = 5;
    public const int UNSUPPORTED_CONVERTER = -1;
    public const int SBCS = 0;
    public const int DBCS = 1;
    public const int MBCS = 2;
    public const int LATIN_1 = 3;
    public const int UTF8 = 4;
    public const int UTF16_BigEndian = 5;
    public const int UTF16_LittleEndian = 6;
    public const int UTF32_BigEndian = 7;
    public const int UTF32_LittleEndian = 8;
    public const int EBCDIC_STATEFUL = 9;
    public const int ISO_2022 = 10;
    public const int LMBCS_1 = 11;
    public const int LMBCS_2 = 12;
    public const int LMBCS_3 = 13;
    public const int LMBCS_4 = 14;
    public const int LMBCS_5 = 15;
    public const int LMBCS_6 = 16;
    public const int LMBCS_8 = 17;
    public const int LMBCS_11 = 18;
    public const int LMBCS_16 = 19;
    public const int LMBCS_17 = 20;
    public const int LMBCS_18 = 21;
    public const int LMBCS_19 = 22;
    public const int LMBCS_LAST = 22;
    public const int HZ = 23;
    public const int SCSU = 24;
    public const int ISCII = 25;
    public const int US_ASCII = 26;
    public const int UTF7 = 27;
    public const int BOCU1 = 28;
    public const int UTF16 = 29;
    public const int UTF32 = 30;
    public const int CESU8 = 31;
    public const int IMAP_MAILBOX = 32;
    public function __construct(?string $destination_encoding = null, ?string $source_encoding = null) {}
    public function convert(string $str, bool $reverse = false): string|false {}
    public function fromUCallback(int $reason, array $source, int $codePoint, &$error): string|int|array|null {}
    public static function getAliases(string $name): array|false|null {}
    public static function getAvailable(): array {}
    public function getDestinationEncoding(): string|false|null {}
    public function getDestinationType(): int|false|null {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): ?string {}
    public function getSourceEncoding(): string|false|null {}
    public function getSourceType(): int|false|null {}
    public static function getStandards(): ?array {}
    public function getSubstChars(): string|false|null {}
    public static function reasonText(int $reason): string {}
    public function setDestinationEncoding(string $encoding): bool {}
    public function setSourceEncoding(string $encoding): bool {}
    public function setSubstChars(string $chars): bool {}
    public function toUCallback(int $reason, string $source, string $codeUnits, &$error): string|int|array|null {}
    public static function transcode(string $str, string $toEncoding, string $fromEncoding, ?array $options = null): string|false {}
}"#;

#[derive(Clone)]
struct UConv {
    src: &'static Conv,
    dst: &'static Conv,
    /// The substitution bytes of the destination converter.
    subst: Option<Vec<u8>>,
    closed: bool,
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(super::numfmt::on_create),
        clone: Some(|_, from, to| {
            if let Some(c) = with_obj::<UConv, UConv>(from, |x| x.v.clone()) {
                store(to, c);
            }
        }),
        destroy: Some(on_destroy),
        ..Default::default()
    }
}

fn lookup_arg(v: Option<PhpStr>) -> Option<&'static Conv> {
    match v {
        None => uc::lookup("utf-8"),
        Some(s) => uc::lookup(&s.to_string_lossy()),
    }
}

fn u_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<UConv>(&o) {
        return super::already_constructed(vm, "UConverter");
    }
    reset(vm);
    let dst = lookup_arg(a.opt_str(0).filter(|s| !s.as_bytes().is_empty()));
    let src = lookup_arg(a.opt_str(1).filter(|s| !s.as_bytes().is_empty()));
    let (Some(dst), Some(src)) = (dst, src) else {
        let msg = format!("{}(): returned error 4: U_FILE_ACCESS_ERROR", a.fname());
        super::set_error_quiet(vm, ue::U_FILE_ACCESS_ERROR, msg.clone());
        return vm.throw("IntlException", &msg);
    };
    store(
        &o,
        UConv {
            src,
            dst,
            subst: None,
            closed: false,
        },
    );
    Ok(Value::Null)
}

fn state(vm: &mut Vm, a: &Args) -> R<(ObjRef, UConv)> {
    let o = a.this_obj().clone();
    match with_obj::<UConv, UConv>(&o, |x| x.v.clone()) {
        Some(c) => Ok((o, c)),
        None => vm.throw("Error", "UConverter object is not initialized"),
    }
}

/// Whether the object's class overrides the callbacks.
fn user_callbacks(vm: &mut Vm, o: &ObjRef) -> bool {
    let cls = vm.class_of(o);
    cls.lname.as_bytes() != b"uconverter"
        && ["toucallback", "fromucallback"].iter().any(|m| {
            cls.find_method(m.as_bytes())
                .is_some_and(|f| !f.is_native())
        })
}

/// Calls a callback method: (result, error code after the call).
fn call_cb(
    vm: &mut Vm,
    o: &ObjRef,
    name: &[u8],
    reason: i64,
    source: Value,
    unit: Value,
    err: i64,
) -> R<(Value, i64)> {
    let r = goro_runtime::PhpRef::new(Value::Int(err));
    let res = vm.call_method(
        o,
        name,
        vec![Value::Int(reason), source, unit, Value::Ref(r.clone())],
    )?;
    let e = match r.get() {
        Value::Int(i) => i,
        _ => 0,
    };
    Ok((res, e))
}

/// Appends a toU callback result (UTF-8 text, a code point or an array of
/// them) to the code points.
fn push_to_u(v: &Value, out: &mut Vec<u32>) -> R<()> {
    match v {
        Value::Str(s) => out.extend(
            String::from_utf8_lossy(s.as_bytes())
                .chars()
                .map(|c| c as u32),
        ),
        Value::Int(i) => out.push(*i as u32),
        Value::Array(arr) => {
            for (_, x) in arr.iter() {
                push_to_u(&x.deref(), out)?;
            }
        }
        Value::Ref(r) => push_to_u(&r.get(), out)?,
        _ => {}
    }
    Ok(())
}

/// Appends a fromU callback result (bytes, a code point or an array).
fn push_from_u(c: &Conv, v: &Value, out: &mut Vec<u8>) -> R<()> {
    match v {
        Value::Str(s) => out.extend_from_slice(s.as_bytes()),
        Value::Int(i) => {
            if let Some(b) = uc::encode_cp(c, *i as u32) {
                out.extend(b);
            }
        }
        Value::Array(arr) => {
            for (_, x) in arr.iter() {
                push_from_u(c, &x.deref(), out)?;
            }
        }
        Value::Ref(r) => push_from_u(c, &r.get(), out)?,
        _ => {}
    }
    Ok(())
}

/// Converts `input` from `src` to `dst`; None when a callback left an
/// error.
fn run(
    vm: &mut Vm,
    o: Option<&ObjRef>,
    src: &'static Conv,
    dst: &'static Conv,
    subst: &[u8],
    input: &[u8],
) -> R<Option<Vec<u8>>> {
    let units = uc::decode(src, input);
    let callbacks = o.is_some();
    let src_val = Value::from(input);
    let mut cps: Vec<u32> = Vec::new();
    for pass in 0..2 {
        let mut out = Vec::new();
        if let Some(o) = o {
            call_cb(
                vm,
                o,
                b"toucallback",
                uc::REASON_RESET,
                Value::from(""),
                Value::from(""),
                0,
            )?;
        }
        for u in &units {
            match u {
                Unit::Cp(c) => out.push(*c),
                Unit::Bad(bytes) => {
                    if let Some(o) = o {
                        let (r, e) = call_cb(
                            vm,
                            o,
                            b"toucallback",
                            uc::REASON_ILLEGAL,
                            src_val.clone(),
                            Value::from(bytes.as_slice()),
                            ue::U_ILLEGAL_CHAR_FOUND,
                        )?;
                        if e > 0 {
                            return Ok(None);
                        }
                        push_to_u(&r, &mut out)?;
                    } else {
                        out.push(0xFFFD);
                    }
                }
            }
        }
        if pass == 0 || !callbacks {
            cps = out;
        }
        if !callbacks {
            break;
        }
    }
    let stateful = matches!(
        dst.ty,
        uc::UTF7 | uc::ISO_2022 | uc::HZ | uc::IMAP_MAILBOX | uc::UTF16 | uc::UTF32
    );
    let mut result = Vec::new();
    for pass in 0..2 {
        let mut out = Vec::new();
        if let Some(o) = o {
            call_cb(
                vm,
                o,
                b"fromucallback",
                uc::REASON_RESET,
                Value::Array(PhpArray::new()),
                Value::Int(0),
                0,
            )?;
        }
        let mut run_cps: Vec<u32> = Vec::new();
        for &cp in &cps {
            let ok = uc::encode_cp(dst, cp);
            match ok {
                Some(b) => {
                    if stateful {
                        run_cps.push(cp);
                    } else {
                        out.extend(b);
                    }
                }
                None => {
                    if stateful && !run_cps.is_empty() {
                        out.extend(uc::encode_all(dst, &run_cps));
                        run_cps.clear();
                    }
                    if let Some(o) = o {
                        let source = Value::Array(PhpArray::from_values(
                            cps.iter().map(|c| Value::Int(*c as i64)),
                        ));
                        let (r, e) = call_cb(
                            vm,
                            o,
                            b"fromucallback",
                            uc::REASON_UNASSIGNED,
                            source,
                            Value::Int(cp as i64),
                            ue::U_INVALID_CHAR_FOUND,
                        )?;
                        if e > 0 {
                            return Ok(None);
                        }
                        push_from_u(dst, &r, &mut out)?;
                    } else {
                        out.extend_from_slice(subst);
                    }
                }
            }
        }
        if !run_cps.is_empty() {
            out.extend(uc::encode_all(dst, &run_cps));
        }
        if pass == 0 || !callbacks {
            result = out;
        }
        if !callbacks {
            break;
        }
    }
    Ok(Some(result))
}

fn u_convert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, c) = state(vm, a)?;
    reset_obj::<UConv>(vm, &o);
    let reverse = a.bool_or(1, false);
    let (src, dst) = if reverse {
        (c.dst, c.src)
    } else {
        (c.src, c.dst)
    };
    let subst = c.subst.clone().unwrap_or_else(|| uc::default_subst(dst));
    let cb = user_callbacks(vm, &o);
    match run(vm, cb.then_some(&o), src, dst, &subst, a.bytes(0))? {
        Some(b) => Ok(Value::from(b.as_slice())),
        None => {
            fail_obj::<UConv>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "Error converting string",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

/// The converters close when the object goes: user callbacks see it
/// (UConverter has no `__destruct`; an exception they throw surfaces at
/// the statement that released the object).
fn on_destroy(vm: &mut Vm, o: &ObjRef) {
    if let Err(e) = close(vm, o)
        && vm.err.deferred.is_none()
    {
        vm.err.deferred = Some(e);
    }
}

fn close(vm: &mut Vm, o: &ObjRef) -> R<Value> {
    let o = o.clone();
    let Some(closed) = with_obj::<UConv, bool>(&o, |x| std::mem::replace(&mut x.v.closed, true))
    else {
        return Ok(Value::Null);
    };
    if !closed && user_callbacks(vm, &o) {
        // the source and the destination converters close
        for _ in 0..2 {
            call_cb(
                vm,
                &o,
                b"toucallback",
                uc::REASON_CLOSE,
                Value::from(""),
                Value::from(""),
                0,
            )?;
            call_cb(
                vm,
                &o,
                b"fromucallback",
                uc::REASON_CLOSE,
                Value::Array(PhpArray::new()),
                Value::Int(0),
                0,
            )?;
        }
    }
    Ok(Value::Null)
}

fn set_err_ref(vm: &mut Vm, a: &Args, i: usize, v: i64) -> R<()> {
    super::timezone::set_ref_typed(vm, a, i, Value::Int(v))
}

/// The default toU callback: the error is cleared and nothing is
/// substituted beyond ICU's U+FFFD.
fn u_to_callback(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let reason = a.int(0);
    if matches!(
        reason,
        uc::REASON_UNASSIGNED | uc::REASON_ILLEGAL | uc::REASON_IRREGULAR
    ) {
        set_err_ref(vm, a, 3, 0)?;
        return Ok(Value::from("\u{FFFD}"));
    }
    Ok(Value::Null)
}

/// The default fromU callback: the destination's substitution.
fn u_from_callback(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let reason = a.int(0);
    if matches!(
        reason,
        uc::REASON_UNASSIGNED | uc::REASON_ILLEGAL | uc::REASON_IRREGULAR
    ) {
        set_err_ref(vm, a, 3, 0)?;
        let (_, c) = state(vm, a)?;
        let s = c.subst.clone().unwrap_or_else(|| uc::default_subst(c.dst));
        return Ok(Value::from(s.as_slice()));
    }
    Ok(Value::Null)
}

fn u_aliases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let name = a.str(0).to_string_lossy();
    match uc::lookup(&name) {
        Some(c) => Ok(Value::Array(PhpArray::from_values(
            c.aliases.iter().map(|s| Value::from(*s)),
        ))),
        None => {
            fail(
                vm,
                a,
                ue::U_FILE_ACCESS_ERROR,
                "ucnv_getAliases() returned error 4: U_FILE_ACCESS_ERROR",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn u_available(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    Ok(Value::Array(PhpArray::from_values(
        uc::CONVERTERS
            .iter()
            .filter(|c| uc::available(c))
            .map(|c| Value::from(c.name)),
    )))
}

fn u_standards(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    Ok(Value::Array(PhpArray::from_values(
        uc::STANDARDS.iter().map(|s| Value::from(*s)),
    )))
}

fn u_src_enc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c) = state(vm, a)?;
    Ok(Value::from(c.src.name))
}

fn u_dest_enc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c) = state(vm, a)?;
    Ok(Value::from(c.dst.name))
}

fn u_src_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c) = state(vm, a)?;
    Ok(Value::Int(c.src.ty))
}

fn u_dest_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c) = state(vm, a)?;
    Ok(Value::Int(c.dst.ty))
}

fn set_enc(vm: &mut Vm, a: &Args, dest: bool) -> R<Value> {
    let (o, _) = state(vm, a)?;
    reset_obj::<UConv>(vm, &o);
    match uc::lookup(&a.str(0).to_string_lossy()) {
        Some(c) => {
            with_obj::<UConv, ()>(&o, |x| {
                if dest {
                    x.v.dst = c;
                    x.v.subst = None;
                } else {
                    x.v.src = c;
                }
            });
            Ok(Value::Bool(true))
        }
        None => {
            fail_obj::<UConv>(
                vm,
                a,
                &o,
                ue::U_FILE_ACCESS_ERROR,
                "returned error 4: U_FILE_ACCESS_ERROR",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn u_set_dest(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_enc(vm, a, true)
}

fn u_set_src(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_enc(vm, a, false)
}

fn u_get_subst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c) = state(vm, a)?;
    let s = c.subst.clone().unwrap_or_else(|| uc::default_subst(c.dst));
    Ok(Value::from(s.as_slice()))
}

fn u_set_subst(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, c) = state(vm, a)?;
    reset_obj::<UConv>(vm, &o);
    let s = a.bytes(0).to_vec();
    if !uc::valid_subst(c.dst, &s) || !uc::valid_subst(c.src, &s) {
        fail_obj::<UConv>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "ucnv_setSubstChars() returned error 1: U_ILLEGAL_ARGUMENT_ERROR",
        )?;
        return Ok(Value::Bool(false));
    }
    with_obj::<UConv, ()>(&o, |x| x.v.subst = Some(s));
    Ok(Value::Bool(true))
}

fn u_reason_text(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(uc::reason_text(a.int(0))))
}

fn u_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let _ = vm;
    Ok(Value::Int(obj_err::<UConv>(&o).code))
}

fn u_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let _ = vm;
    let e = obj_err::<UConv>(&o);
    if e.code == 0 && e.msg.is_empty() {
        return Ok(Value::Null);
    }
    Ok(Value::from(e.full_message().as_str()))
}

fn u_transcode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let to = uc::lookup(&a.str(1).to_string_lossy());
    let from = uc::lookup(&a.str(2).to_string_lossy());
    let (Some(to), Some(from)) = (to, from) else {
        fail(
            vm,
            a,
            ue::U_FILE_ACCESS_ERROR,
            "Error setting encoding: 4 - U_FILE_ACCESS_ERROR",
        )?;
        return Ok(Value::Bool(false));
    };
    let opts = a.array(3);
    let get = |k: &[u8]| opts.get_bytes(k).map(|v| v.deref());
    let mut subst = uc::default_subst(to);
    for (key, conv) in [(&b"from_subst"[..], from), (&b"to_subst"[..], to)] {
        if let Some(v) = get(key) {
            let s = match &v {
                Value::Str(s) => s.as_bytes().to_vec(),
                other => vm.to_php_str(other)?.as_bytes().to_vec(),
            };
            if !uc::valid_subst(conv, &s) {
                fail(
                    vm,
                    a,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "returned error 1: U_ILLEGAL_ARGUMENT_ERROR",
                )?;
                return Ok(Value::Bool(false));
            }
            if key == b"to_subst" {
                subst = s;
            }
        }
    }
    match run(vm, None, from, to, &subst, a.bytes(0))? {
        Some(b) => Ok(Value::from(b.as_slice())),
        None => Ok(Value::Bool(false)),
    }
}
