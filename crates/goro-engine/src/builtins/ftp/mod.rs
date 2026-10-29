//! ext/ftp (PHP manual: FTP): the `FTP\Connection` class and the ftp_*
//! functions over the client in conn.rs, plus the ftp:// stream wrapper
//! (wrapper.rs). Replies, warnings and checks follow ext/ftp/tests (run
//! against their fake server, tests/server.inc) and PHP 8.5's observed
//! output.

mod conn;
mod wrapper;

use std::time::Duration;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use conn::{Conn, Dest, Nb, NbDir, Open, Ty};

use crate::builtins::file::{Stream, do_seek, open_stream};
use crate::builtins::streams::NativeStream;
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "ftp",
    functions: &[
        (
            "function ftp_connect(string $hostname, int $port = 21, int $timeout = 90): FTP\\Connection|false",
            ftp_connect,
        ),
        (
            "function ftp_ssl_connect(string $hostname, int $port = 21, int $timeout = 90): FTP\\Connection|false",
            ftp_ssl_connect,
        ),
        (
            "function ftp_login(FTP\\Connection $ftp, string $username, string $password): bool",
            ftp_login,
        ),
        ("function ftp_pwd(FTP\\Connection $ftp): string|false", ftp_pwd),
        ("function ftp_cdup(FTP\\Connection $ftp): bool", ftp_cdup),
        (
            "function ftp_chdir(FTP\\Connection $ftp, string $directory): bool",
            ftp_chdir,
        ),
        (
            "function ftp_exec(FTP\\Connection $ftp, string $command): bool",
            ftp_exec,
        ),
        (
            "function ftp_raw(FTP\\Connection $ftp, string $command): ?array",
            ftp_raw,
        ),
        (
            "function ftp_mkdir(FTP\\Connection $ftp, string $directory): string|false",
            ftp_mkdir,
        ),
        (
            "function ftp_rmdir(FTP\\Connection $ftp, string $directory): bool",
            ftp_rmdir,
        ),
        (
            "function ftp_chmod(FTP\\Connection $ftp, int $permissions, string $filename): int|false",
            ftp_chmod,
        ),
        (
            "function ftp_alloc(FTP\\Connection $ftp, int $size, &$response = null): bool",
            ftp_alloc,
        ),
        (
            "function ftp_nlist(FTP\\Connection $ftp, string $directory): array|false",
            ftp_nlist,
        ),
        (
            "function ftp_rawlist(FTP\\Connection $ftp, string $directory, bool $recursive = false): array|false",
            ftp_rawlist,
        ),
        (
            "function ftp_mlsd(FTP\\Connection $ftp, string $directory): array|false",
            ftp_mlsd,
        ),
        (
            "function ftp_systype(FTP\\Connection $ftp): string|false",
            ftp_systype,
        ),
        (
            "function ftp_fget(FTP\\Connection $ftp, $stream, string $remote_filename, int $mode = FTP_BINARY, int $offset = 0): bool",
            ftp_fget,
        ),
        (
            "function ftp_nb_fget(FTP\\Connection $ftp, $stream, string $remote_filename, int $mode = FTP_BINARY, int $offset = 0): int",
            ftp_nb_fget,
        ),
        (
            "function ftp_pasv(FTP\\Connection $ftp, bool $enable): bool",
            ftp_pasv,
        ),
        (
            "function ftp_get(FTP\\Connection $ftp, string $local_filename, string $remote_filename, int $mode = FTP_BINARY, int $offset = 0): bool",
            ftp_get,
        ),
        (
            "function ftp_nb_get(FTP\\Connection $ftp, string $local_filename, string $remote_filename, int $mode = FTP_BINARY, int $offset = 0): int|false",
            ftp_nb_get,
        ),
        (
            "function ftp_nb_continue(FTP\\Connection $ftp): int",
            ftp_nb_continue,
        ),
        (
            "function ftp_fput(FTP\\Connection $ftp, string $remote_filename, $stream, int $mode = FTP_BINARY, int $offset = 0): bool",
            ftp_fput,
        ),
        (
            "function ftp_nb_fput(FTP\\Connection $ftp, string $remote_filename, $stream, int $mode = FTP_BINARY, int $offset = 0): int",
            ftp_nb_fput,
        ),
        (
            "function ftp_put(FTP\\Connection $ftp, string $remote_filename, string $local_filename, int $mode = FTP_BINARY, int $offset = 0): bool",
            ftp_put,
        ),
        (
            "function ftp_append(FTP\\Connection $ftp, string $remote_filename, string $local_filename, int $mode = FTP_BINARY): bool",
            ftp_append,
        ),
        (
            "function ftp_nb_put(FTP\\Connection $ftp, string $remote_filename, string $local_filename, int $mode = FTP_BINARY, int $offset = 0): int|false",
            ftp_nb_put,
        ),
        (
            "function ftp_size(FTP\\Connection $ftp, string $filename): int",
            ftp_size,
        ),
        (
            "function ftp_mdtm(FTP\\Connection $ftp, string $filename): int",
            ftp_mdtm,
        ),
        (
            "function ftp_rename(FTP\\Connection $ftp, string $from, string $to): bool",
            ftp_rename,
        ),
        (
            "function ftp_delete(FTP\\Connection $ftp, string $filename): bool",
            ftp_delete,
        ),
        (
            "function ftp_site(FTP\\Connection $ftp, string $command): bool",
            ftp_site,
        ),
        ("function ftp_close(FTP\\Connection $ftp): bool", ftp_close),
        ("function ftp_quit(FTP\\Connection $ftp): bool", ftp_close),
        (
            "function ftp_set_option(FTP\\Connection $ftp, int $option, $value): true",
            ftp_set_option,
        ),
        (
            "function ftp_get_option(FTP\\Connection $ftp, int $option): int|bool",
            ftp_get_option,
        ),
    ],
    classes: &[(
        "ftp\\connection",
        "<?php namespace FTP; #[\\NotSerializable] #[\\NotCloneable] final class Connection {
            public function __construct() { throw new \\Error('Cannot directly construct FTP\\\\Connection, use ftp_connect() or ftp_ssl_connect() instead'); }
        }",
    )],
    hooks: &[("ftp\\connection", hooks)],
    constants: &[
        ("FTP_ASCII", ConstVal::Int(1)),
        ("FTP_TEXT", ConstVal::Int(1)),
        ("FTP_BINARY", ConstVal::Int(2)),
        ("FTP_IMAGE", ConstVal::Int(2)),
        ("FTP_AUTORESUME", ConstVal::Int(-1)),
        ("FTP_TIMEOUT_SEC", ConstVal::Int(0)),
        ("FTP_AUTOSEEK", ConstVal::Int(1)),
        ("FTP_USEPASVADDRESS", ConstVal::Int(2)),
        ("FTP_FAILED", ConstVal::Int(0)),
        ("FTP_FINISHED", ConstVal::Int(1)),
        ("FTP_MOREDATA", ConstVal::Int(2)),
    ],
    init: Some(wrapper::init),
    listed: true,
    ..Module::EMPTY
};

fn hooks() -> NativeHooks {
    NativeHooks {
        compare: Some(|_, x, y| match (x, y) {
            (Value::Object(a), Value::Object(b)) => {
                Some(Ok(if ObjRef::ptr_eq(a, b) { 0 } else { 1 }))
            }
            _ => None,
        }),
        ..Default::default()
    }
}

const FTP_FAILED: i64 = 0;
const FTP_FINISHED: i64 = 1;
const FTP_MOREDATA: i64 = 2;
const AUTORESUME: i64 = -1;

/// Takes the connection out of its object for the call (the VM may run
/// while it works), puts it back after.
fn with_conn<T>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Vm, &mut Conn) -> R<T>) -> R<T> {
    let o = a.obj(0);
    let taken = o.as_ref().and_then(|o| {
        let mut n = o.obj().native.borrow_mut();
        let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
        if any.downcast_ref::<Conn>().is_some_and(|c| !c.closed) {
            n.take()
        } else {
            None
        }
    });
    let Some(mut boxed) = taken else {
        return vm.throw("ValueError", "FTP\\Connection is already closed");
    };
    let r = {
        let any: &mut dyn std::any::Any = &mut *boxed;
        let c = any.downcast_mut::<Conn>().expect("an FTP connection");
        f(vm, c)
    };
    if let Some(o) = o {
        *o.obj().native.borrow_mut() = Some(boxed);
    }
    r
}

/// Warns with the last reply's text.
fn warn_resp(vm: &mut Vm, a: &Args, c: &Conn) -> R<()> {
    let m = String::from_utf8_lossy(&c.msg).into_owned();
    vm.fwarn(a, &m)
}

fn connect_common(vm: &mut Vm, a: &mut Args, ssl: bool) -> R<Value> {
    let host = a.str(0);
    let port = a.int_or(1, 21);
    let timeout = a.int_or(2, 90);
    if timeout <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($timeout) must be greater than 0",
        );
    }
    // The timeout is kept in microseconds (observed on PHP 8.5: "must be
    // less than 18446744073709").
    let max = u64::MAX / 1_000_000;
    if timeout as u64 >= max {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #3 ($timeout) must be less than {max}"),
        );
    }
    vm.out.flush();
    let t = Duration::from_secs(timeout as u64);
    let mut c = match Conn::open(vm, host.as_bytes(), port as u16, t) {
        Open::Ok(c) => c,
        Open::Lookup(m) => {
            vm.fwarn(a, &m)?;
            return Ok(Value::Bool(false));
        }
        Open::Failed => return Ok(Value::Bool(false)),
    };
    c.use_ssl = ssl;
    let Some(cls) = vm.find_class(b"FTP\\Connection", false)? else {
        return Err(vm.fatal("Class \"FTP\\Connection\" not found"));
    };
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(c);
    Ok(Value::Object(o))
}

fn ftp_connect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    connect_common(vm, a, false)
}

fn ftp_ssl_connect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    connect_common(vm, a, true)
}

fn ftp_login(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (user, pass) = (a.str(1), a.str(2));
    with_conn(vm, a, |vm, c| {
        if c.login(vm, a, user.as_bytes(), pass.as_bytes())? {
            return Ok(Value::Bool(true));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

/// A command expecting one of `ok` codes: true, or a warning with the
/// reply and false.
fn simple(vm: &mut Vm, a: &Args, cmd: &str, arg: Option<&[u8]>, ok: &[i32]) -> R<Value> {
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, cmd, arg) && ok.contains(&c.code) {
            return Ok(Value::Bool(true));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

/// The text between the first two double quotes of a 257 reply.
fn quoted(msg: &[u8]) -> Option<Vec<u8>> {
    let s = msg.iter().position(|&b| b == b'"')?;
    let e = msg[s + 1..].iter().position(|&b| b == b'"')?;
    Some(msg[s + 1..s + 1 + e].to_vec())
}

fn ftp_pwd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "PWD", None)
            && c.code == 257
            && let Some(p) = quoted(&c.msg)
        {
            return Ok(Value::Str(PhpStr::from(p.as_slice())));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_cdup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    simple(vm, a, "CDUP", None, &[200, 250])
}

fn ftp_chdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    simple(vm, a, "CWD", Some(d.as_bytes()), &[250])
}

fn ftp_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut arg = b"EXEC ".to_vec();
    arg.extend_from_slice(a.bytes(1));
    simple(vm, a, "SITE", Some(&arg), &[200])
}

fn ftp_site(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arg = a.str(1);
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "SITE", Some(arg.as_bytes())) && (200..300).contains(&c.code) {
            return Ok(Value::Bool(true));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_raw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cmd = a.str(1);
    with_conn(vm, a, |vm, c| {
        let line = String::from_utf8_lossy(cmd.as_bytes()).into_owned();
        if !c.cmd(vm, &line, None) {
            return Ok(Value::Null);
        }
        let mut out = PhpArray::new();
        for l in &c.lines {
            let _ = out.push(Value::Str(PhpStr::from(l.as_slice())));
        }
        Ok(Value::Array(out))
    })
}

fn ftp_mkdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "MKD", Some(d.as_bytes())) && c.code == 257 {
            // The created path the reply quotes, else the name given (test
            // bug7216.phpt).
            let p = quoted(&c.msg).unwrap_or_else(|| d.as_bytes().to_vec());
            return Ok(Value::Str(PhpStr::from(p.as_slice())));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_rmdir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    simple(vm, a, "RMD", Some(d.as_bytes()), &[250])
}

fn ftp_delete(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    simple(vm, a, "DELE", Some(d.as_bytes()), &[250])
}

fn ftp_chmod(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.int(1);
    let file = a.str(2);
    let mut arg = format!("CHMOD {mode:o} ").into_bytes();
    arg.extend_from_slice(file.as_bytes());
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "SITE", Some(&arg)) && c.code == 200 {
            return Ok(Value::Int(mode));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_alloc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = a.int(1);
    let r = with_conn(vm, a, |vm, c| {
        let ok = c.cmd(vm, "ALLO", Some(size.to_string().as_bytes()));
        Ok((ok && (c.code == 200 || c.code == 202), c.msg.clone()))
    })?;
    if a.ref_at(2).is_some() {
        a.set_ref(2, Value::Str(PhpStr::from(r.1.as_slice())));
    }
    Ok(Value::Bool(r.0))
}

fn lines_value(lines: Vec<Vec<u8>>) -> Value {
    let mut out = PhpArray::new();
    for l in lines {
        let _ = out.push(Value::Str(PhpStr::from(l.as_slice())));
    }
    Value::Array(out)
}

fn ftp_nlist(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    with_conn(vm, a, |vm, c| {
        let arg = if d.is_empty() {
            None
        } else {
            Some(d.as_bytes())
        };
        Ok(match c.list(vm, a, "NLST", arg)? {
            Some(l) => lines_value(l),
            None => Value::Bool(false),
        })
    })
}

fn ftp_rawlist(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    let rec = a.bool_or(2, false);
    with_conn(vm, a, |vm, c| {
        let mut arg = Vec::new();
        if rec {
            arg.extend_from_slice(b"-R");
            if !d.is_empty() {
                arg.push(b' ');
            }
        }
        arg.extend_from_slice(d.as_bytes());
        let arg = if arg.is_empty() { None } else { Some(&arg[..]) };
        Ok(match c.list(vm, a, "LIST", arg)? {
            Some(l) => lines_value(l),
            None => Value::Bool(false),
        })
    })
}

/// One MLSD line (RFC 3659 section 7: "fact=value;...; name"), or the
/// warning it gets.
fn mlsd_entry(line: &[u8]) -> Result<PhpArray, &'static str> {
    let Some(sp) = line.iter().position(|&b| b == b' ') else {
        return Err("Missing pathname in MLSD response");
    };
    let (facts, name) = (&line[..sp], &line[sp + 1..]);
    let mut m = PhpArray::new();
    m.insert(Key::from("name"), Value::Str(PhpStr::from(name)));
    let mut rest = facts;
    while !rest.is_empty() {
        let Some(semi) = rest.iter().position(|&b| b == b';') else {
            return Err("Malformed fact in MLSD response");
        };
        let fact = &rest[..semi];
        let Some(eq) = fact.iter().position(|&b| b == b'=') else {
            return Err("Malformed fact in MLSD response");
        };
        m.insert(
            Key::from_bytes(&fact[..eq]),
            Value::Str(PhpStr::from(&fact[eq + 1..])),
        );
        rest = &rest[semi + 1..];
    }
    Ok(m)
}

fn ftp_mlsd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(1);
    let lines = with_conn(vm, a, |vm, c| {
        let arg = if d.is_empty() {
            None
        } else {
            Some(d.as_bytes())
        };
        c.list(vm, a, "MLSD", arg)
    })?;
    let Some(lines) = lines else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for l in lines {
        match mlsd_entry(&l) {
            Ok(m) => {
                let _ = out.push(Value::Array(m));
            }
            Err(w) => vm.fwarn(a, w)?,
        }
    }
    Ok(Value::Array(out))
}

fn ftp_systype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "SYST", None) && c.code == 215 {
            let t: Vec<u8> = c
                .msg
                .iter()
                .copied()
                .skip_while(|&b| b == b' ')
                .take_while(|&b| b != b' ')
                .collect();
            return Ok(Value::Str(PhpStr::from(t.as_slice())));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_pasv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let on = a.bool(1);
    with_conn(vm, a, |_vm, c| {
        c.pasv = on;
        Ok(Value::Bool(true))
    })
}

fn ftp_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(1);
    with_conn(vm, a, |vm, c| {
        if !c.set_type(vm, Ty::Image) || !c.cmd(vm, "SIZE", Some(f.as_bytes())) || c.code != 213 {
            return Ok(Value::Int(-1));
        }
        let s = String::from_utf8_lossy(&c.msg);
        Ok(Value::Int(s.trim().parse::<i64>().unwrap_or(-1)))
    })
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn ftp_mdtm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(1);
    with_conn(vm, a, |vm, c| {
        if !c.cmd(vm, "MDTM", Some(f.as_bytes())) || c.code != 213 {
            return Ok(Value::Int(-1));
        }
        // RFC 3659 time-val: YYYYMMDDHHMMSS[.sss], in UTC.
        let s: Vec<u8> = c.msg.iter().copied().skip_while(|&b| b == b' ').collect();
        if s.len() < 14 || !s[..14].iter().all(u8::is_ascii_digit) {
            return Ok(Value::Int(-1));
        }
        let n = |r: std::ops::Range<usize>| {
            s[r].iter()
                .fold(0i64, |acc, &b| acc * 10 + (b - b'0') as i64)
        };
        let t = days_from_civil(n(0..4), n(4..6), n(6..8)) * 86400
            + n(8..10) * 3600
            + n(10..12) * 60
            + n(12..14);
        Ok(Value::Int(t))
    })
}

fn ftp_rename(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (from, to) = (a.str(1), a.str(2));
    with_conn(vm, a, |vm, c| {
        if c.cmd(vm, "RNFR", Some(from.as_bytes()))
            && c.code == 350
            && c.cmd(vm, "RNTO", Some(to.as_bytes()))
            && c.code == 250
        {
            return Ok(Value::Bool(true));
        }
        warn_resp(vm, a, c)?;
        Ok(Value::Bool(false))
    })
}

fn ftp_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_conn(vm, a, |vm, c| Ok(Value::Bool(c.quit(vm))))
}

const OPT_ERR: &str =
    "Argument #2 ($option) must be one of FTP_TIMEOUT_SEC, FTP_AUTOSEEK, or FTP_USEPASVADDRESS";

fn ftp_set_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opt = a.int(1);
    let v = a.val(2);
    match opt {
        0 => {
            let Value::Int(n) = v else {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #3 ($value) must be of type int for the FTP_TIMEOUT_SEC option, {} given",
                        Vm::type_name_given(&v)
                    ),
                );
            };
            if n <= 0 {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($value) must be greater than 0 for the FTP_TIMEOUT_SEC option",
                );
            }
            with_conn(vm, a, |_vm, c| {
                c.timeout = Duration::from_secs(n as u64);
                c.ctl.timeout = Some(c.timeout);
                Ok(())
            })?;
        }
        1 | 2 => {
            let Value::Bool(b) = v else {
                let name = if opt == 1 {
                    "FTP_AUTOSEEK"
                } else {
                    "FTP_USEPASVADDRESS"
                };
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #3 ($value) must be of type bool for the {name} option, {} given",
                        Vm::type_name_given(&v)
                    ),
                );
            };
            with_conn(vm, a, |_vm, c| {
                if opt == 1 {
                    c.autoseek = b;
                } else {
                    c.usepasvaddress = b;
                }
                Ok(())
            })?;
        }
        _ => return vm.fthrow(a, "ValueError", OPT_ERR),
    }
    Ok(Value::Bool(true))
}

fn ftp_get_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opt = a.int(1);
    if !(0..=2).contains(&opt) {
        return vm.fthrow(a, "ValueError", OPT_ERR);
    }
    with_conn(vm, a, |_vm, c| {
        Ok(match opt {
            0 => Value::Int(c.timeout.as_secs() as i64),
            1 => Value::Bool(c.autoseek),
            _ => Value::Bool(c.usepasvaddress),
        })
    })
}

// ------------------------------------------------------------ transfers

fn check_mode(vm: &mut Vm, a: &Args, i: usize) -> R<Ty> {
    match a.int(i) {
        1 => Ok(Ty::Ascii),
        2 => Ok(Ty::Image),
        _ => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($mode) must be either FTP_ASCII or FTP_BINARY",
                i + 1
            ),
        ),
    }
}

/// The stream resource argument `i` (TypeError otherwise).
fn stream_arg(vm: &mut Vm, a: &Args, i: usize) -> R<goro_runtime::ResRef> {
    let v = a.val(i);
    if let Value::Resource(r) = &v
        && crate::builtins::file::is_stream_res(r)
    {
        return Ok(r.clone());
    }
    let msg = match &v {
        Value::Resource(_) => format!(
            "Argument #{} ($stream) must be an open stream resource",
            i + 1
        ),
        _ => format!(
            "Argument #{} ($stream) must be of type resource, {} given",
            i + 1,
            Vm::type_name_given(&v)
        ),
    };
    vm.fthrow(a, "TypeError", &msg)
}

/// The size of a local stream (FTP_AUTORESUME on downloads).
fn local_size(vm: &mut Vm, s: &mut Stream) -> i64 {
    if !do_seek(vm, s, 0, 2) {
        return 0;
    }
    match &mut s.backing {
        crate::builtins::file::Backing::File(f) => {
            f.metadata().map(|m| m.len() as i64).unwrap_or(0)
        }
        crate::builtins::file::Backing::Memory { data, .. } => data.len() as i64,
        _ => 0,
    }
}

/// Starts a download into `dest` (REST for a resume position); the data
/// connection, or None after the failing reply.
fn start_get(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    dest: &mut Dest,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<Option<crate::builtins::sockets::Sock>> {
    c.abort_nb(vm);
    let mut pos = offset;
    if pos == AUTORESUME {
        pos = if c.autoseek {
            dest.with(|s| local_size(vm, s)).unwrap_or(0)
        } else {
            0
        };
    } else if pos > 0 && c.autoseek {
        dest.with(|s| do_seek(vm, s, pos, 0));
    }
    if !c.set_type(vm, ty) {
        return Ok(None);
    }
    if pos > 0 && (!c.cmd(vm, "REST", Some(pos.to_string().as_bytes())) || c.code != 350) {
        return Ok(None);
    }
    c.data_cmd(vm, a, "RETR", Some(remote))
}

/// Writes received bytes to the destination.
fn put_local(vm: &mut Vm, dest: &mut Dest, data: &[u8]) {
    dest.with(|s| {
        let _ = s.write_all(vm, data);
    });
}

/// Finishes a download: the final reply (226/250).
fn end_transfer(vm: &mut Vm, a: &Args, c: &mut Conn) -> R<bool> {
    if c.getresp(vm) && (c.code == 226 || c.code == 250) {
        return Ok(true);
    }
    warn_resp(vm, a, c)?;
    Ok(false)
}

fn get_into(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    mut dest: Dest,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<bool> {
    let Some(mut data) = start_get(vm, a, c, &mut dest, remote, ty, offset)? else {
        warn_resp(vm, a, c)?;
        return Ok(false);
    };
    let mut buf = vec![0u8; 16384];
    let mut cr = false;
    loop {
        match data.read(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = if ty == Ty::Ascii {
                    conn::from_ascii(&buf[..n], &mut cr, false)
                } else {
                    buf[..n].to_vec()
                };
                put_local(vm, &mut dest, &chunk);
            }
        }
    }
    if cr {
        put_local(vm, &mut dest, b"\r");
    }
    Conn::close_data(vm, data);
    end_transfer(vm, a, c)
}

fn ftp_fget(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = stream_arg(vm, a, 1)?;
    let remote = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        Ok(Value::Bool(get_into(
            vm,
            a,
            c,
            Dest::Res(r),
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

/// Opens the local file of ftp_get(): read/write at the resume position
/// when autoseek resumes, else truncated.
fn open_local_get(vm: &mut Vm, a: &Args, c: &Conn, local: &[u8], offset: i64) -> R<Option<Stream>> {
    if c.autoseek && offset != 0 {
        if let Some(s) = open_stream(vm, a, local, b"rb+", true)? {
            return Ok(Some(s));
        }
    }
    open_stream(vm, a, local, b"wb", false)
}

fn ftp_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let local = a.str(1);
    let remote = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        let Some(s) = open_local_get(vm, a, c, local.as_bytes(), offset)? else {
            return Ok(Value::Bool(false));
        };
        let ok = get_into(
            vm,
            a,
            c,
            Dest::Own(Box::new(s)),
            remote.as_bytes(),
            ty,
            offset,
        )?;
        Ok(Value::Bool(ok))
    })
}

/// One step of a non-blocking download: a chunk read (MOREDATA), or the
/// end of the transfer.
fn nb_step(vm: &mut Vm, a: &Args, c: &mut Conn) -> R<i64> {
    let Some(mut nb) = c.nb.take() else {
        vm.fwarn(a, "No non-blocking transfer to continue")?;
        return Ok(FTP_FAILED);
    };
    match &mut nb.dir {
        NbDir::Get(dest) => {
            let mut buf = vec![0u8; 16384];
            match nb.data.read(vm, &mut buf) {
                Ok(n) if n > 0 => {
                    let chunk = if nb.ascii {
                        conn::from_ascii(&buf[..n], &mut nb.cr, false)
                    } else {
                        buf[..n].to_vec()
                    };
                    put_local(vm, dest, &chunk);
                    c.nb = Some(nb);
                    Ok(FTP_MOREDATA)
                }
                _ => {
                    if nb.cr {
                        put_local(vm, dest, b"\r");
                    }
                    Conn::close_data(vm, nb.data);
                    Ok(if end_transfer(vm, a, c)? {
                        FTP_FINISHED
                    } else {
                        FTP_FAILED
                    })
                }
            }
        }
        NbDir::Put(src) => {
            let mut buf = vec![0u8; 4096];
            let n = src
                .with(|s| s.read_some(vm, &mut buf).unwrap_or(0))
                .unwrap_or(0);
            if n > 0 {
                let chunk = if nb.ascii {
                    conn::to_ascii(&buf[..n], &mut nb.cr)
                } else {
                    buf[..n].to_vec()
                };
                if !write_data(vm, &mut nb.data, &chunk) {
                    Conn::close_data(vm, nb.data);
                    let _ = end_transfer(vm, a, c)?;
                    return Ok(FTP_FAILED);
                }
            }
            let at_end = n == 0 || src.with(|s| s.eof || stream_at_end(s)).unwrap_or(true);
            if !at_end {
                c.nb = Some(nb);
                return Ok(FTP_MOREDATA);
            }
            Conn::close_data(vm, nb.data);
            Ok(if end_transfer(vm, a, c)? {
                FTP_FINISHED
            } else {
                FTP_FAILED
            })
        }
    }
}

/// Whether a file stream's position reached its size.
fn stream_at_end(s: &mut Stream) -> bool {
    use std::io::Seek;
    match &mut s.backing {
        crate::builtins::file::Backing::File(f) => {
            let pos = f.stream_position().unwrap_or(0);
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            pos >= len
        }
        crate::builtins::file::Backing::Memory { data, pos } => *pos >= data.len(),
        _ => false,
    }
}

fn nb_get_start(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    mut dest: Dest,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<i64> {
    let Some(data) = start_get(vm, a, c, &mut dest, remote, ty, offset)? else {
        warn_resp(vm, a, c)?;
        return Ok(FTP_FAILED);
    };
    c.nb = Some(Nb {
        data,
        dir: NbDir::Get(dest),
        ascii: ty == Ty::Ascii,
        cr: false,
    });
    nb_step(vm, a, c)
}

fn ftp_nb_fget(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = stream_arg(vm, a, 1)?;
    let remote = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        Ok(Value::Int(nb_get_start(
            vm,
            a,
            c,
            Dest::Res(r),
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

fn ftp_nb_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let local = a.str(1);
    let remote = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        let Some(s) = open_local_get(vm, a, c, local.as_bytes(), offset)? else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Int(nb_get_start(
            vm,
            a,
            c,
            Dest::Own(Box::new(s)),
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

fn ftp_nb_continue(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_conn(vm, a, |vm, c| Ok(Value::Int(nb_step(vm, a, c)?)))
}

fn write_data(vm: &mut Vm, data: &mut crate::builtins::sockets::Sock, mut b: &[u8]) -> bool {
    while !b.is_empty() {
        match data.write(vm, b) {
            Ok(0) | Err(_) => return false,
            Ok(n) => b = &b[n..],
        }
    }
    true
}

/// Starts an upload (STOR or APPE) from `src`.
fn start_put(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    src: &mut Dest,
    cmd: &str,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<Option<crate::builtins::sockets::Sock>> {
    c.abort_nb(vm);
    let mut pos = offset;
    if pos == AUTORESUME {
        pos = 0;
        if c.autoseek
            && c.set_type(vm, Ty::Image)
            && c.cmd(vm, "SIZE", Some(remote))
            && c.code == 213
        {
            pos = String::from_utf8_lossy(&c.msg).trim().parse().unwrap_or(0);
        }
    }
    if pos > 0 && c.autoseek {
        src.with(|s| do_seek(vm, s, pos, 0));
    }
    if !c.set_type(vm, ty) {
        return Ok(None);
    }
    if pos > 0 && (!c.cmd(vm, "REST", Some(pos.to_string().as_bytes())) || c.code != 350) {
        return Ok(None);
    }
    c.data_cmd(vm, a, cmd, Some(remote))
}

fn put_from(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    mut src: Dest,
    cmd: &str,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<bool> {
    let Some(mut data) = start_put(vm, a, c, &mut src, cmd, remote, ty, offset)? else {
        warn_resp(vm, a, c)?;
        return Ok(false);
    };
    let mut buf = vec![0u8; 4096];
    let mut cr = false;
    loop {
        let n = src
            .with(|s| s.read_some(vm, &mut buf).unwrap_or(0))
            .unwrap_or(0);
        if n == 0 {
            break;
        }
        let chunk = if ty == Ty::Ascii {
            conn::to_ascii(&buf[..n], &mut cr)
        } else {
            buf[..n].to_vec()
        };
        if !write_data(vm, &mut data, &chunk) {
            break;
        }
    }
    Conn::close_data(vm, data);
    end_transfer(vm, a, c)
}

fn ftp_fput(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.str(1);
    let r = stream_arg(vm, a, 2)?;
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        Ok(Value::Bool(put_from(
            vm,
            a,
            c,
            Dest::Res(r),
            "STOR",
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

fn open_local_put(vm: &mut Vm, a: &Args, local: &[u8]) -> R<Option<Stream>> {
    open_stream(vm, a, local, b"rb", false)
}

fn ftp_put(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.str(1);
    let local = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        let Some(s) = open_local_put(vm, a, local.as_bytes())? else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(put_from(
            vm,
            a,
            c,
            Dest::Own(Box::new(s)),
            "STOR",
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

fn ftp_append(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.str(1);
    let local = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    with_conn(vm, a, |vm, c| {
        let Some(s) = open_local_put(vm, a, local.as_bytes())? else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(put_from(
            vm,
            a,
            c,
            Dest::Own(Box::new(s)),
            "APPE",
            remote.as_bytes(),
            ty,
            0,
        )?))
    })
}

fn nb_put_start(
    vm: &mut Vm,
    a: &Args,
    c: &mut Conn,
    mut src: Dest,
    remote: &[u8],
    ty: Ty,
    offset: i64,
) -> R<i64> {
    let Some(data) = start_put(vm, a, c, &mut src, "STOR", remote, ty, offset)? else {
        warn_resp(vm, a, c)?;
        return Ok(FTP_FAILED);
    };
    c.nb = Some(Nb {
        data,
        dir: NbDir::Put(src),
        ascii: ty == Ty::Ascii,
        cr: false,
    });
    nb_step(vm, a, c)
}

fn ftp_nb_fput(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.str(1);
    let r = stream_arg(vm, a, 2)?;
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        Ok(Value::Int(nb_put_start(
            vm,
            a,
            c,
            Dest::Res(r),
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}

fn ftp_nb_put(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let remote = a.str(1);
    let local = a.str(2);
    let ty = check_mode(vm, a, 3)?;
    let offset = a.int_or(4, 0);
    with_conn(vm, a, |vm, c| {
        let Some(s) = open_local_put(vm, a, local.as_bytes())? else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Int(nb_put_start(
            vm,
            a,
            c,
            Dest::Own(Box::new(s)),
            remote.as_bytes(),
            ty,
            offset,
        )?))
    })
}
