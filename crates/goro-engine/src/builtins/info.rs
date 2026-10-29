//! phpinfo() and phpcredits() in the CLI's plain-text form (PHP manual:
//! phpinfo, phpcredits; layout as observed on PHP 8.5's CLI), or as HTML
//! under a web SAPI (info_html.rs).

use goro_runtime::{PhpStr, Value};

use super::info_html::{self as html, Table};
use super::info_text;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        ("function phpinfo(int $flags = INFO_ALL): true", phpinfo),
        (
            "function phpcredits(int $flags = CREDITS_ALL): true",
            phpcredits,
        ),
    ],
    constants: &[
        ("INFO_GENERAL", ConstVal::Int(1)),
        ("INFO_CREDITS", ConstVal::Int(2)),
        ("INFO_CONFIGURATION", ConstVal::Int(4)),
        ("INFO_MODULES", ConstVal::Int(8)),
        ("INFO_ENVIRONMENT", ConstVal::Int(16)),
        ("INFO_VARIABLES", ConstVal::Int(32)),
        ("INFO_LICENSE", ConstVal::Int(64)),
        ("INFO_ALL", ConstVal::Int(4294967295)),
        ("CREDITS_GROUP", ConstVal::Int(1)),
        ("CREDITS_GENERAL", ConstVal::Int(2)),
        ("CREDITS_SAPI", ConstVal::Int(4)),
        ("CREDITS_MODULES", ConstVal::Int(8)),
        ("CREDITS_DOCS", ConstVal::Int(16)),
        ("CREDITS_FULLPAGE", ConstVal::Int(32)),
        ("CREDITS_QA", ConstVal::Int(64)),
        ("CREDITS_ALL", ConstVal::Int(4294967295)),
    ],
    listed: false,
    ..Module::EMPTY
};

const HR: &str = "\n _______________________________________________________________________\n\n";

fn credits_text(flags: i64) -> String {
    let mut s = String::from("PHP Credits\n");
    for (f, text) in info_text::SECTIONS {
        if flags & f != 0 {
            s.push('\n');
            s.push_str(text);
        }
    }
    s
}

fn phpcredits(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(0, 4294967295);
    let s = if vm.out.sapi.web() {
        html::credits_page(flags)
    } else {
        credits_text(flags)
    };
    vm.echo(s.as_bytes());
    Ok(Value::Bool(true))
}

/// Calls a builtin by name, if it exists.
fn call_named(vm: &mut Vm, name: &str, args: Vec<Value>) -> R<Option<Value>> {
    let f = Value::Str(PhpStr::from(name));
    if !vm.is_callable_value(&f, false) {
        return Ok(None);
    }
    vm.call_value(&f, args).map(Some)
}

fn joined(vm: &mut Vm, name: &str) -> R<String> {
    Ok(match call_named(vm, name, Vec::new())? {
        Some(Value::Array(a)) => a
            .iter()
            .map(|(_, v)| match v.deref() {
                Value::Str(s) => s.to_string_lossy(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    })
}

fn ini_display(v: Option<&PhpStr>) -> String {
    match v {
        Some(s) if !s.is_empty() => s.to_string_lossy(),
        _ => "no value".to_string(),
    }
}

/// The settings, sorted: (name, local value, master value).
fn directive_values(vm: &Vm, names: &[&str]) -> Vec<(String, String, String)> {
    let mut names: Vec<&str> = names.to_vec();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .map(|n| {
            let k = PhpStr::from(n);
            let local = vm.ini.values.get(&k).cloned();
            let master = match vm.ini.originals.get(&k) {
                Some(Some(o)) => Some(o.clone()),
                _ => local.clone(),
            };
            (
                n.to_string(),
                ini_display(local.as_ref()),
                ini_display(master.as_ref()),
            )
        })
        .collect()
}

fn directives(vm: &Vm, names: &[&str], out: &mut String) {
    let rows = directive_values(vm, names);
    if rows.is_empty() {
        return;
    }
    out.push_str("\nDirective => Local Value => Master Value\n");
    for (n, l, m) in rows {
        out.push_str(&format!("{n} => {l} => {m}\n"));
    }
}

fn directives_html(vm: &Vm, names: &[&str], out: &mut String) {
    let rows = directive_values(vm, names);
    if rows.is_empty() {
        return;
    }
    let mut t = Table::new();
    t.header(&["Directive", "Local Value", "Master Value"]);
    for (n, l, m) in &rows {
        t.row(&[n, l, m]);
    }
    out.push_str(&t.finish());
}

type InfoFn = Option<fn(&mut Vm) -> String>;

/// The modules by name (several registry modules can share one), the
/// unlisted ones' settings folded into Core.
fn module_list(
    vm: &Vm,
) -> (
    Vec<&'static str>,
    Vec<(&'static str, Vec<&'static str>, InfoFn)>,
) {
    let mut core_ini: Vec<&'static str> = Vec::new();
    let mut mods: Vec<(&'static str, Vec<&'static str>, InfoFn)> = Vec::new();
    for m in vm.modules.iter() {
        let inis = m.ini.iter().map(|(k, _)| *k);
        if !m.listed || m.name == "Core" {
            core_ini.extend(inis);
        } else if let Some(e) = mods.iter_mut().find(|(n, _, _)| *n == m.name) {
            e.1.extend(inis);
            e.2 = e.2.or(m.info);
        } else {
            mods.push((m.name, inis.collect(), m.info));
        }
    }
    mods.sort_by_key(|(n, _, _)| n.to_ascii_lowercase());
    (core_ini, mods)
}

/// The general information block (INFO_GENERAL), as `a => b` lines.
fn general(vm: &mut Vm, version: &str, sapi: &str) -> R<String> {
    let uname = match call_named(vm, "php_uname", Vec::new())? {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    let build = match vm.constants.get(&PhpStr::from("PHP_BUILD_DATE")) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    let streams = joined(vm, "stream_get_wrappers")?;
    let mut transports = joined(vm, "stream_get_transports")?;
    if transports.is_empty() {
        transports = "tcp, udp, unix, udg".to_string();
    }
    let mut filters = joined(vm, "stream_get_filters")?;
    if filters.is_empty() {
        filters = "string.rot13, string.toupper, string.tolower, convert.*".to_string();
    }
    Ok(format!(
        "PHP Version => {version}\n\nSystem => {uname}\nBuild Date => {build}\nConfigure Command => goro\n\
         Server API => {sapi}\nVirtual Directory Support => disabled\n\
         Configuration File (php.ini) Path => (none)\nLoaded Configuration File => (none)\n\
         Scan this dir for additional .ini files => (none)\n\
         Additional .ini files parsed => (none)\nPHP API => 20250925\n\
         PHP Extension => 20250925\nZend Extension => 420250925\n\
         Zend Extension Build => API420250925,NTS\nPHP Extension Build => API20250925,NTS\n\
         PHP Integer Size => 64 bits\nDebug Build => no\nThread Safety => disabled\n\
         Zend Signal Handling => disabled\nZend Memory Manager => disabled\n\
         Zend Multibyte Support => disabled\nZend Max Execution Timers => disabled\n\
         IPv6 Support => enabled\nDTrace Support => disabled\n\n\
         Registered PHP Streams => {streams}\n\
         Registered Stream Socket Transports => {transports}\n\
         Registered Stream Filters => {filters}\n\n\
         This program makes use of the Zend Scripting Language Engine:\n\
         Zend Engine v4.5.4, Copyright (c) Zend Technologies\n"
    ))
}

/// phpinfo() under a web SAPI: the same content as an HTML page.
fn phpinfo_html(vm: &mut Vm, flags: i64, version: &str) -> R<Value> {
    let mut out = html::head(&format!("PHP {version} - phpinfo()"));
    if flags & 1 != 0 {
        out.push_str(&format!(
            "<table>\n<tr class=\"h\"><td>\n<h1 class=\"p\">PHP Version {version}</h1>\n</td></tr>\n</table>\n"
        ));
        let sapi = if vm.out.sapi.server {
            "Built-in HTTP server"
        } else {
            "CGI/FastCGI"
        };
        let text = general(vm, version, sapi)?;
        let mut parts = text.split("\n\n");
        parts.next(); // the version, shown above
        for part in parts {
            let mut t = Table::new();
            t.lines(part);
            out.push_str(&t.finish());
        }
    }
    let (core_ini, mods) = module_list(vm);
    if flags & 4 != 0 {
        out.push_str("<h1>Configuration</h1>\n");
        if flags & 8 == 0 {
            out.push_str(&html::module_heading("PHP Core"));
            directives_html(vm, &core_ini, &mut out);
        }
    }
    if flags & 8 != 0 {
        out.push_str(&html::module_heading("Core"));
        let mut t = Table::new();
        t.row(&["PHP Version", version]);
        out.push_str(&t.finish());
        directives_html(vm, &core_ini, &mut out);
        for (name, inis, info) in &mods {
            out.push_str(&html::module_heading(name));
            let text = match info {
                Some(f) => f(vm),
                None if *name == "gettext" => "GetText Support => enabled\n".to_string(),
                None => format!("{name} support => enabled\n"),
            };
            let mut t = Table::new();
            t.lines(&text);
            out.push_str(&t.finish());
            directives_html(vm, inis, &mut out);
        }
        out.push_str("<h2>Additional Modules</h2>\n");
        let mut t = Table::new();
        t.header(&["Module Name"]);
        out.push_str(&t.finish());
    }
    if flags & 16 != 0 {
        out.push_str("<h2>Environment</h2>\n");
        let mut t = Table::new();
        t.header(&["Variable", "Value"]);
        for (k, v) in std::env::vars_os() {
            t.row(&[&k.to_string_lossy(), &v.to_string_lossy()]);
        }
        out.push_str(&t.finish());
    }
    if flags & 32 != 0 {
        out.push_str("<h2>PHP Variables</h2>\n");
        let mut t = Table::new();
        t.header(&["Variable", "Value"]);
        html::variables(vm, &mut t)?;
        out.push_str(&t.finish());
    }
    if flags & 2 != 0 {
        out.push_str("<hr />\n");
        out.push_str(&html::credits_body(4294967295));
    }
    if flags & 64 != 0 {
        out.push_str(&html::license());
    }
    out.push_str(html::FOOT);
    vm.echo(out.as_bytes());
    Ok(Value::Bool(true))
}

fn phpinfo(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(0, 4294967295);
    let mut out = String::from("phpinfo()\n");
    let version = match vm.constants.get(&PhpStr::from("PHP_VERSION")) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    if vm.out.sapi.web() {
        return phpinfo_html(vm, flags, &version);
    }
    if flags & 1 != 0 {
        let text = general(vm, &version, "Command Line Interface")?;
        out.push_str(&text);
    }
    let (core_ini, mods) = module_list(vm);
    if flags & 4 != 0 {
        out.push('\n');
        out.push_str(HR);
        out.push_str("\nConfiguration\n");
        if flags & 8 == 0 {
            out.push_str("\nPHP Core\n");
            directives(vm, &core_ini, &mut out);
        }
    }
    if flags & 8 != 0 {
        out.push_str(&format!("\nCore\n\nPHP Version => {version}\n"));
        directives(vm, &core_ini, &mut out);
        for (name, inis, info) in &mods {
            match info {
                Some(f) => {
                    out.push_str(&format!("\n{name}\n\n"));
                    out.push_str(&f(vm));
                }
                // ext/gettext's line (test gettext_phpinfo.phpt).
                None if *name == "gettext" => {
                    out.push_str(&format!("\n{name}\n\nGetText Support => enabled\n"))
                }
                None => out.push_str(&format!("\n{name}\n\n{name} support => enabled\n")),
            }
            directives(vm, inis, &mut out);
        }
        out.push_str("\nAdditional Modules\n\nModule Name\n");
    }
    if flags & 16 != 0 {
        out.push_str("\nEnvironment\n\nVariable => Value\n");
        for (k, v) in std::env::vars_os() {
            out.push_str(&format!(
                "{} => {}\n",
                k.to_string_lossy(),
                v.to_string_lossy()
            ));
        }
    }
    let mut bytes = out.into_bytes();
    if flags & 32 != 0 {
        bytes.extend_from_slice(b"\nPHP Variables\n\nVariable => Value\n");
        for g in [
            "_REQUEST", "_GET", "_POST", "_COOKIE", "_FILES", "_SERVER", "_ENV",
        ] {
            let Some(p) = vm.global_var_ptr(&PhpStr::from(g), false) else {
                continue;
            };
            // SAFETY: a live global slot; read before any user code runs.
            let v = unsafe { (*p).deref() };
            let Value::Array(arr) = v else { continue };
            for (k, v) in arr.iter() {
                let k = match k {
                    goro_runtime::KeyRef::Int(i) => i.to_string().into_bytes(),
                    goro_runtime::KeyRef::Str(s) => s.to_vec(),
                };
                bytes.extend_from_slice(format!("${}['", g).as_bytes());
                bytes.extend_from_slice(&k);
                bytes.extend_from_slice(b"'] => ");
                let v = v.deref();
                if matches!(v, Value::Array(_)) {
                    vm.print_r_into(&mut bytes, &v, 0)?;
                    bytes.push(b'\n');
                } else {
                    let mut s = Vec::new();
                    vm.append_to_string(&mut s, &v)?;
                    bytes.extend_from_slice(&s);
                    bytes.push(b'\n');
                }
            }
        }
    }
    if flags & 2 != 0 {
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(HR.as_bytes());
        bytes.extend_from_slice(credits_text(4294967295).as_bytes());
    }
    if flags & 64 != 0 {
        bytes.extend_from_slice(b"\nPHP License\n");
        bytes.extend_from_slice(info_text::LICENSE.as_bytes());
    }
    vm.echo(&bytes);
    Ok(Value::Bool(true))
}
