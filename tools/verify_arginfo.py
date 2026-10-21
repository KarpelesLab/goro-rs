#!/usr/bin/env python3
"""Verify the generic zpp layer (crates/goro-core/src/arginfo.rs) against a
real PHP binary, per function, and write the whitelist of functions for
which the generated arginfo reproduces PHP's argument-count and parameter
type errors exactly.

Usage: tools/verify_arginfo.py <goro-function-list.txt> [php-binary] [php-src]

<goro-function-list.txt> lists the builtins goro registers, one per line
(`get_defined_functions()['internal']` run under goro).  Each function is
probed in its own PHP process (in a scratch directory) with:
  - one argument too few / one too many            -> ArgumentCountError
  - array / null / "abc" / stdClass / 1.5 for each scalar or array typed
    parameter                                        -> TypeError / deprecation
and the first diagnostic of each probe is compared with the message the
generic layer would produce from the stub.  Writes
crates/goro-core/src/arginfo_verified.rs.
"""
import concurrent.futures
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.argv += [None] * 3
FN_LIST, PHP_BIN, PHP_SRC = sys.argv[1], sys.argv[2] or "/usr/bin/php", sys.argv[3] or "/home/magicaltux/php-src"
OUT = os.path.join(HERE, "..", "crates/goro-core/src/arginfo_verified.rs")


# Functions that must not be probed (process control, sleeping, global
# handlers, network, output control) or whose arginfo is special.
SKIP = {
    "exit", "die", "sleep", "usleep", "time_nanosleep", "time_sleep_until", "set_time_limit",
    "set_error_handler", "set_exception_handler", "restore_error_handler", "restore_exception_handler",
    "register_shutdown_function", "register_tick_function", "unregister_tick_function",
    "header_register_callback", "error_reporting", "ini_set", "ini_alter", "ini_restore",
    "ignore_user_abort", "flush", "dl", "exec", "system", "passthru", "shell_exec", "popen", "proc_open",
    "mail", "fsockopen", "pfsockopen", "stream_socket_client", "stream_socket_server",
    "gethostbyname", "gethostbynamel", "gethostbyaddr", "dns_get_record", "dns_check_record",
    "checkdnsrr", "getmxrr", "dns_get_mx", "readline", "stream_select", "socket_select",
    "fastcgi_finish_request", "func_get_args", "func_get_arg", "func_num_args", "get_defined_vars",
    "extract", "compact", "debug_backtrace", "debug_print_backtrace", "error_log", "syslog",
    "openlog", "assert", "assert_options", "call_user_func", "call_user_func_array",
    "forward_static_call", "forward_static_call_array", "spl_autoload_register", "ob_start",
    "output_add_rewrite_var", "output_reset_rewrite_vars", "chdir", "chroot", "putenv", "umask",
    "trigger_error", "user_error", "highlight_file", "show_source", "php_strip_whitespace",
    "readfile", "fpassthru", "readgzfile", "gzpassthru", "printf", "vprintf", "print_r", "var_dump",
    "var_export", "debug_zval_refcount", "session_start", "header", "setcookie", "setrawcookie",
    "http_response_code", "header_remove", "gc_enable", "gc_disable", "get_browser",
}
SKIP_PREFIX = ("pcntl_", "posix_", "proc_", "ob_", "socket_", "curl_", "mysqli_", "session_", "readline_")

T_NULL, T_FALSE, T_TRUE, T_INT, T_FLOAT, T_STRING, T_ARRAY, T_OBJECT, T_CALLABLE = 1, 2, 4, 8, 16, 32, 64, 128, 256
T_MIXED, T_CLASS = 1024, 16384
T_BOOL = T_FALSE | T_TRUE
SCALAR = T_BOOL | T_INT | T_FLOAT | T_STRING


def checkable(p):
    """How the generic layer treats a parameter (keep in sync with arginfo.rs):
    "check" (scalar/array types), "skip" (cannot fail zpp: mixed, untyped
    by-reference) or "stop" (validated by the function itself: callable,
    class/object, resource, typed by-reference -- later parameters are left
    to the function too, to keep zpp's left-to-right error order)."""
    m = p["mask"]
    if m & T_MIXED or (p["by_ref"] and m == 0):
        return "skip"
    if p["by_ref"] or m == 0 or m & (T_CLASS | T_OBJECT | T_CALLABLE):
        return "stop"
    return "check"


def placeholder(p):
    m = p["mask"]
    if m & T_STRING or m & T_MIXED:
        return "'a'"
    if m & T_INT:
        return "1"
    if m & T_FLOAT:
        return "1.5"
    if m & T_BOOL:
        return "true" if m & T_TRUE else "false"
    if m & T_ARRAY:
        return "[]"
    if m & T_CALLABLE:
        return "'strlen'"
    if p["by_ref"] and m == 0:
        return "null"
    return None


def arity_msg(name, req, npar, variadic, n):
    mx = None if variadic else npar
    if mx is not None and req == mx:
        kind, cnt = "exactly", req
    elif n < req:
        kind, cnt = "at least", req
    else:
        kind, cnt = "at most", mx
    return "X: ArgumentCountError: %s() expects %s %d argument%s, %d given" % (name, kind, cnt, "" if cnt == 1 else "s", n)


def build_probes(f):
    name = f["name"]
    params = [p for p in f["params"] if not p["variadic"]]
    variadic = any(p["variadic"] for p in f["params"])
    req = f["required"]
    probes = []  # (kind, args, expected_first_line)

    def args_with(n, at=None, probe=None):
        out = []
        for j in range(n):
            if j == at:
                out.append(probe)
                continue
            if j >= len(params):
                out.append("1")
                continue
            ph = placeholder(params[j])
            if ph is None:
                return None
            out.append(ph)
        return out

    if req > 0:
        a = args_with(req - 1)
        if a is not None:
            probes.append(("arity", a, arity_msg(name, req, len(params), variadic, req - 1)))
    if not variadic:
        a = args_with(len(params) + 1)
        if a is not None:
            probes.append(("arity", a, arity_msg(name, req, len(params), variadic, len(params) + 1)))
    for i, p in enumerate(params):
        c = checkable(p)
        if c == "stop":
            break
        if c == "skip":
            continue
        m = p["mask"]
        k = i + 1
        pre = "X: TypeError: %s(): Argument #%d ($%s) must be of type %s, " % (name, k, p["name"], p["ty"])
        cases = []
        if not m & T_ARRAY:
            cases.append(("[]", pre + "array given"))
        if not m & T_NULL:
            if m & SCALAR:
                cases.append(("null", "E8192: %s(): Passing null to parameter #%d ($%s) of type %s is deprecated" % (name, k, p["name"], p["ty"])))
            else:
                cases.append(("null", pre + "null given"))
        if m & (T_INT | T_FLOAT) and not m & T_STRING:
            cases.append(("'abc'", pre + "string given"))
        if m & T_STRING:
            cases.append(("new stdClass", pre + "stdClass given"))
        if m & T_INT and not m & (T_FLOAT | T_STRING):
            cases.append(("1.5", "E8192: Implicit conversion from float 1.5 to int loses precision"))
        for val, exp in cases:
            # Every argument count: some functions' zpp depends on it (strtr)
            for n in range(max(req, k), len(params) + 1):
                a = args_with(n, i, val)
                if a is not None:
                    probes.append(("types", a, exp))
    return probes


def probe_script(f, probes):
    lines = ["<?php",
             "set_error_handler(function($no, $str) { echo \"\\nE$no: $str\\n\"; return true; });",
             "function __p($id, $f) { echo \"\\n#P$id\\n\"; try { $f(); } catch (\\Throwable $e) { echo \"\\nX: \", get_class($e), \": \", $e->getMessage(), \"\\n\"; } }"]
    for idx, (_, args, _) in enumerate(probes):
        pre, call = [], []
        # Pass everything through variables so nothing is evaluated at compile time.
        for j, a in enumerate(args):
            pre.append("$v%d = %s;" % (j, a))
            call.append("$v%d" % j)
        lines.append("__p(%d, function() { %s %s(%s); });" % (idx, " ".join(pre), f["name"], ", ".join(call)))
    lines.append("echo \"\\n#END\\n\";")
    return "\n".join(lines) + "\n"


def run_one(item):
    f, probes = item
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "probe.php")
        with open(path, "w") as fo:
            fo.write(probe_script(f, probes))
        try:
            r = subprocess.run([PHP_BIN, "-d", "display_errors=1", "-d", "error_reporting=-1", "-d", "memory_limit=256M", path],
                               cwd=d, capture_output=True, timeout=10, stdin=subprocess.DEVNULL)
            out = r.stdout.decode("utf-8", "replace")
        except subprocess.TimeoutExpired:
            return f, probes, None
    first = {}
    cur = None
    for line in out.split("\n"):
        if line.startswith("#P"):
            cur = int(line[2:])
            first[cur] = None
            continue
        if line.startswith("#END"):
            cur = None
            continue
        if cur is not None and first[cur] is None and line.strip():
            first[cur] = line
    return f, probes, first


# --- Static check against php-src's C implementation -----------------------
#
# The runtime probes use whatever PHP binary is installed (possibly older
# than the stubs), so additionally require that the function body in
# php-src parses its parameters in a single zpp block whose per-parameter
# types match the stub (no argument-count dependent signatures like
# strtr(), no Z_PARAM_ZVAL where the stub declares a scalar type, ...).

ZP = {  # Z_PARAM_* -> mask
    "STR": T_STRING, "STRING": T_STRING, "PATH": T_STRING, "PATH_STR": T_STRING,
    "LONG": T_INT, "DOUBLE": T_FLOAT, "BOOL": T_BOOL, "NUMBER": T_INT | T_FLOAT,
    "ARRAY": T_ARRAY, "ARRAY_HT": T_ARRAY, "ARRAY_EX": T_ARRAY, "ARRAY_HT_EX": T_ARRAY,
    "ARRAY_HT_OR_STR": T_ARRAY | T_STRING, "STR_OR_LONG": T_STRING | T_INT,
    "ARRAY_HT_OR_LONG": T_ARRAY | T_INT, "NUMBER_OR_STR": T_INT | T_FLOAT | T_STRING,
}
FMT = {"s": T_STRING, "S": T_STRING, "p": T_STRING, "P": T_STRING, "l": T_INT, "d": T_FLOAT,
       "b": T_BOOL, "a": T_ARRAY, "A": T_ARRAY, "h": T_ARRAY, "H": T_ARRAY, "n": T_INT | T_FLOAT}

_C_BODIES = None


def c_bodies():
    """name -> body of every PHP_FUNCTION / ZEND_FUNCTION in php-src."""
    global _C_BODIES
    if _C_BODIES is not None:
        return _C_BODIES
    _C_BODIES = {}
    pat = re.compile(r"^(?:PHP|ZEND)_FUNCTION\((\w+)\)\s*(?:/\*.*?\*/)?\s*\{(.*?)^\}", re.M | re.S)
    # Helpers taking INTERNAL_FUNCTION_PARAMETERS (zpp often lives there)
    hpat = re.compile(r"^[\w \t\*]*?\b(\w+)\s*\(\s*INTERNAL_FUNCTION_PARAMETERS[^)]*\)\s*(?:/\*.*?\*/)?\s*\{(.*?)^\}", re.M | re.S)
    for root, dirs, files in os.walk(PHP_SRC):
        if "/tests" in root:
            continue
        for fn in files:
            if fn.endswith(".c"):
                try:
                    txt = open(os.path.join(root, fn), errors="replace").read()
                except OSError:
                    continue
                for m in pat.finditer(txt):
                    _C_BODIES.setdefault(m.group(1).lower(), []).append(m.group(2))
                for m in hpat.finditer(txt):
                    _C_HELPERS.setdefault(m.group(1), []).append(m.group(2))
    return _C_BODIES


_C_HELPERS = {}


def zpp_body(body, depth=0):
    """The body itself, or the INTERNAL_FUNCTION_PARAM_PASSTHRU helper it
    delegates to when it has no zpp of its own."""
    if re.search(r"ZEND_PARSE_PARAMETERS|zend_parse_parameters|zend_parse_method_parameters", body) or depth > 2:
        return body
    calls = re.findall(r"\b(\w+)\s*\(\s*INTERNAL_FUNCTION_PARAM_PASSTHRU", body)
    if len(set(calls)) != 1:
        return body
    helpers = _C_HELPERS.get(calls[0], [])
    if len(helpers) != 1:
        return body
    return zpp_body(helpers[0], depth + 1)


def zpp_signature(body):
    """(min, max, [mask or None per parameter]) of the function's single zpp
    block, or None when it does not have exactly one."""
    starts = re.findall(r"ZEND_PARSE_PARAMETERS_START\(\s*(\d+)\s*,\s*(-?\d+)\s*\)(.*?)ZEND_PARSE_PARAMETERS_END", body, re.S)
    none = len(re.findall(r"ZEND_PARSE_PARAMETERS_NONE\(\)", body))
    old = re.findall(r"zend_parse_parameters(?:_ex)?\s*\((?:[^,]*,)?\s*ZEND_NUM_ARGS\(\)\s*,\s*\"([^\"]*)\"", body)
    allold = len(re.findall(r"zend_parse_parameters|zend_parse_method_parameters", body))
    if len(starts) + none + allold != 1:
        return None
    if none:
        return (0, 0, [])
    if starts:
        mn, mx, inner = starts[0]
        masks = []
        for m in re.finditer(r"Z_PARAM_(\w+)\s*\(([^;]*?)\)\s*(?:\n|$)", inner):
            k, a = m.group(1), m.group(2)
            if k == "OPTIONAL":
                continue
            if k.startswith("VARIADIC"):
                masks.append(None)
                break
            null = 0
            if k.endswith("_OR_NULL"):
                k, null = k[:-8], T_NULL
            if k in ("ARRAY_EX", "ARRAY_HT_EX"):
                parts = [x.strip() for x in a.split(",")]
                if len(parts) > 1 and parts[1] in ("1", "true"):
                    null = T_NULL
            masks.append(ZP[k] | null if k in ZP else None)
        return (int(mn), int(mx), masks)
    if len(old) != 1:
        return None
    fmt = old[0]
    masks, mn, opt, variadic = [], None, False, False
    for ch in fmt:
        if ch == "|":
            mn = len(masks)
        elif ch == "!":
            if masks and masks[-1] is not None:
                masks[-1] |= T_NULL
        elif ch == "/":
            continue
        elif ch in "*+":
            masks[-1:] = [None]
            variadic = True
        else:
            masks.append(FMT.get(ch))
    if mn is None:
        mn = len(masks)
    return (mn, -1 if variadic else len(masks), masks)


def source_flags(f):
    """CHECK_* flags allowed by php-src's implementation of `f`."""
    name = (f.get("alias") or f["name"]).lower()
    bodies = c_bodies().get(name, [])
    if len(bodies) != 1:
        return 0
    sig = zpp_signature(zpp_body(bodies[0]))
    if sig is None:
        return 0
    mn, mx, masks = sig
    params = [p for p in f["params"] if not p["variadic"]]
    variadic = any(p["variadic"] for p in f["params"])
    flags = 0
    if mn == f["required"] and mx == (-1 if variadic else len(params)):
        flags |= 1
    ok = True
    for i, p in enumerate(params):
        c = checkable(p)
        if c == "stop":
            break
        if c == "skip":
            continue
        if i >= len(masks) or masks[i] != p["mask"]:
            ok = False
            break
    if ok:
        flags |= 2
    return flags


def rs_unescape(lit):
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(m.group(1), m.group(1)), lit)


def stub_functions():
    """Internal functions from crates/goro-core/src/stub_sigs.rs (generated
    by tools/gen_stub_sigs.php), keyed by lowercase name."""
    txt = open(os.path.join(HERE, "..", "crates/goro-core/src/stub_sigs.rs")).read()
    start = txt.index("pub static FUNCTIONS: &[StubFunc] = &[")
    txt = txt[start:txt.index("\n];\n", start)]
    lit = r'"((?:[^"\\]|\\.)*)"'
    funcs = {}
    for block in re.split(r"\n    StubFunc \{", txt)[1:]:
        name = rs_unescape(re.search(r"name: " + lit, block).group(1))
        flags = int(re.search(r"flags: (0x[0-9a-f]+|\d+)", block).group(1), 0)
        args = []
        for a in re.finditer(r"StubArg \{\s*name: " + lit + r",\s*ty: " + lit + r",\s*default: (None|Some\(" + lit + r"\)),\s*flags: (\d+),", block):
            af = int(a.group(5))
            args.append(dict(name=a.group(1), type=rs_unescape(a.group(2)),
                             default=None if a.group(3) == "None" else rs_unescape(a.group(4)),
                             by_ref=2 if af & 2 else (1 if af & 1 else 0), variadic=bool(af & 4),
                             unknown_default=bool(af & 16)))
        funcs[name.lower()] = dict(name=name, params=args, deprecated=bool(flags & 0x20000))
    return funcs


def type_mask(t):
    """Mask of a stub type (keep in sync with arginfo.rs type_mask)."""
    t = t.strip()
    if not t:
        return 0
    mask = 0
    if t.startswith("?"):
        mask |= T_NULL
        t = t[1:]
    names = {"null": T_NULL, "false": T_FALSE, "true": T_TRUE, "bool": T_BOOL, "int": T_INT, "float": T_FLOAT,
             "string": T_STRING, "array": T_ARRAY, "object": T_OBJECT, "callable": T_CALLABLE,
             "mixed": T_MIXED | T_NULL, "void": 2048, "never": 4096, "static": 8192, "self": 8192,
             "iterable": T_ARRAY | T_CLASS}
    for part in t.split("|"):
        part = part.strip().strip("()")
        mask |= T_CLASS if "&" in part else names.get(part.lower(), T_CLASS)
    return mask


def type_display(t):
    """zend_type_to_string() of a stub type (keep in sync with arginfo.rs)."""
    orig = t.strip()
    if not orig:
        return ""
    t = orig[1:] if orig.startswith("?") else orig
    builtin = {"null", "false", "true", "bool", "int", "float", "string", "array", "object", "callable",
               "mixed", "void", "never", "static", "self"}
    parts = []
    for part in t.split("|"):
        part = part.strip().strip("()")
        if "&" in part:
            parts.append(part)
        elif part.lower() == "iterable":
            parts.append("Traversable")
        elif part.lower() not in builtin:
            parts.append(part.lstrip("\\"))
    mask = type_mask(orig)
    if mask & T_MIXED:
        return "|".join(parts + ["mixed"])
    for bit, name in ((8192, "static"), (T_CALLABLE, "callable"), (T_OBJECT, "object"), (T_ARRAY, "array"),
                      (T_STRING, "string"), (T_INT, "int"), (T_FLOAT, "float")):
        if mask & bit:
            parts.append(name)
    if mask & T_BOOL == T_BOOL:
        parts.append("bool")
    elif mask & T_FALSE:
        parts.append("false")
    elif mask & T_TRUE:
        parts.append("true")
    if mask & 2048:
        parts.append("void")
    if mask & 4096:
        parts.append("never")
    if mask & T_NULL:
        if len(parts) == 1 and "&" not in parts[0]:
            return "?" + parts[0]
        parts.append("null")
    return "|".join(parts)


def main():
    funcs = stub_functions()
    wanted = [l.strip().lower() for l in open(FN_LIST) if l.strip()]
    items = []
    for name in wanted:
        f = funcs.get(name)
        if f is None or f["deprecated"] or name in SKIP or name.startswith(SKIP_PREFIX):
            continue
        params = [dict(p, mask=type_mask(p["type"]), ty=type_display(p["type"])) for p in f["params"]]
        req = 0
        for p in params:
            if p["default"] is not None or p["variadic"] or p["unknown_default"]:
                break
            req += 1
        f = dict(f, params=params, required=req)
        items.append((f, build_probes(f)))
    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:
        for f, probes, first in ex.map(run_one, items):
            if first is None or not first:
                continue
            flags = 0
            ok = {"arity": True, "types": True}
            seen = {"arity": False, "types": False}
            for idx, (kind, _, exp) in enumerate(probes):
                seen[kind] = True
                got = first.get(idx)
                if got != exp:
                    ok[kind] = False
                    if os.environ.get("VERBOSE"):
                        print("%s: %s\n   expected %r\n   got      %r" % (f["name"], kind, exp, got))
            if ok["arity"]:
                flags |= 1
            if ok["types"]:
                flags |= 2
            sf = source_flags(f)
            if os.environ.get("VERBOSE") and flags & ~sf:
                print("%s: source check drops flags %d" % (f["name"], flags & ~sf))
            results[f["name"].lower()] = flags & sf
    with open(OUT, "w") as fo:
        fo.write("// Generated by tools/verify_arginfo.py (probing %s); do not edit.\n" % os.path.basename(PHP_BIN))
        fo.write("// Functions whose generated arginfo reproduces PHP's zpp errors: 1 = argument count, 2 = parameter types.\n")
        fo.write("#![cfg_attr(rustfmt, rustfmt_skip)]\n")
        fo.write("pub static VERIFIED: &[(&str, u8)] = &[\n")
        for k in sorted(results):
            if results[k]:
                fo.write("    (\"%s\", %d),\n" % (k, results[k]))
        fo.write("];\n")
    n1 = sum(1 for v in results.values() if v & 1)
    n2 = sum(1 for v in results.values() if v & 2)
    print("probed %d functions: %d arity-verified, %d type-verified" % (len(results), n1, n2))


main()
