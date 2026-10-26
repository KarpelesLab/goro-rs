#!/usr/bin/env python3
"""Verify the generic zpp layer (crates/goro-core/src/arginfo.rs) against a
real PHP binary, per function, and write the whitelist of functions for
which the generated arginfo reproduces PHP's argument-count and parameter
type errors exactly.

Usage: tools/verify_arginfo.py <goro-function-list.txt> [php-binary] [php-src]

<goro-function-list.txt> lists the builtins goro registers, one per line
(`get_defined_functions()['internal']` run under goro).  Each function, and
each public method of the internal classes in the stubs (called on an
instance made by RECIPES, statically, or through `new`), is probed in its
own PHP process (in a scratch directory) with:
  - one argument too few / one too many            -> ArgumentCountError
  - array / null / "abc" / stdClass / 1.5 for each scalar or array typed
    parameter                                        -> TypeError / deprecation
and the first diagnostic of each probe is compared with the message the
generic layer would produce from the stub.  The C implementation in php-src
must also parse its parameters in a single zpp block matching the stub (see
source_flags()).  Functions and methods the PHP binary does not have (newer
than it) are verified by that C-source check alone.  Writes
crates/goro-core/src/arginfo_verified.rs.

With GORO=<goro binary> in the environment, the probes are also run under
goro, and every verified function/method whose diagnostics differ from PHP's
is listed (to GORO_REPORT, default stdout): a check of the call paths the
generic layer is wired into.
"""
import concurrent.futures
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.argv += [None] * 3
FN_LIST, PHP_BIN, PHP_SRC = sys.argv[1], sys.argv[2] or "/usr/bin/php8.3", sys.argv[3] or "/home/magicaltux/php-src"
GORO = os.environ.get("GORO")
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

# Methods: an instance to call the methods declared by each internal class
# on (lowercase class -> PHP expression; abstract classes via a concrete
# subclass). Classes without a recipe only have their static methods and
# constructor probed.
RECIPES = {
    "arrayobject": "new ArrayObject([1])",
    "arrayiterator": "new ArrayIterator([1])",
    "recursivearrayiterator": "new RecursiveArrayIterator([1])",
    "appenditerator": "new AppendIterator()",
    "cachingiterator": "new CachingIterator(new ArrayIterator([1]))",
    "recursivecachingiterator": "new RecursiveCachingIterator(new RecursiveArrayIterator([1]))",
    "callbackfilteriterator": "new CallbackFilterIterator(new ArrayIterator([1]), fn() => true)",
    "recursivecallbackfilteriterator": "new RecursiveCallbackFilterIterator(new RecursiveArrayIterator([1]), fn() => true)",
    "filteriterator": "new CallbackFilterIterator(new ArrayIterator([1]), fn() => true)",
    "recursivefilteriterator": "new ParentIterator(new RecursiveArrayIterator([1]))",
    "parentiterator": "new ParentIterator(new RecursiveArrayIterator([1]))",
    "iteratoriterator": "new IteratorIterator(new ArrayIterator([1]))",
    "infiniteiterator": "new InfiniteIterator(new ArrayIterator([1]))",
    "norewinditerator": "new NoRewindIterator(new ArrayIterator([1]))",
    "limititerator": "new LimitIterator(new ArrayIterator([1]))",
    "regexiterator": "new RegexIterator(new ArrayIterator(['a']), '/a/')",
    "recursiveregexiterator": "new RecursiveRegexIterator(new RecursiveArrayIterator(['a']), '/a/')",
    "recursiveiteratoriterator": "new RecursiveIteratorIterator(new RecursiveArrayIterator([1]))",
    "recursivetreeiterator": "new RecursiveTreeIterator(new RecursiveArrayIterator([1]))",
    "multipleiterator": "new MultipleIterator()",
    "emptyiterator": "new EmptyIterator()",
    "spldoublylinkedlist": "new SplDoublyLinkedList()",
    "splqueue": "new SplQueue()",
    "splstack": "new SplStack()",
    "splfixedarray": "new SplFixedArray(2)",
    "splobjectstorage": "new SplObjectStorage()",
    "splheap": "new SplMinHeap()",
    "splminheap": "new SplMinHeap()",
    "splmaxheap": "new SplMaxHeap()",
    "splpriorityqueue": "new SplPriorityQueue()",
    "splfileinfo": "new SplFileInfo(__DIR__ . '/data.txt')",
    "splfileobject": "new SplFileObject(__DIR__ . '/data.txt', 'r+')",
    "spltempfileobject": "new SplTempFileObject()",
    "directoryiterator": "new DirectoryIterator(__DIR__)",
    "filesystemiterator": "new FilesystemIterator(__DIR__)",
    "recursivedirectoryiterator": "new RecursiveDirectoryIterator(__DIR__)",
    "globiterator": "new GlobIterator(__DIR__ . '/*')",
    "datetime": "new DateTime('2020-01-01 00:00:00 UTC')",
    "datetimeimmutable": "new DateTimeImmutable('2020-01-01 00:00:00 UTC')",
    "datetimezone": "new DateTimeZone('UTC')",
    "dateinterval": "new DateInterval('P1D')",
    "dateperiod": "new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2)",
    "directory": "dir(__DIR__)",
    "closure": "function() { return 1; }",
    "generator": "pgen()",
    "fiber": "new Fiber(function() { return 1; })",
    "weakmap": "new WeakMap()",
    "weakreference": "WeakReference::create($GLOBALS['__keep'])",
    "exception": "new Exception('m')",
    "errorexception": "new ErrorException('m')",
    "error": "new Error('m')",
    "random\\randomizer": "new Random\\Randomizer(new Random\\Engine\\Mt19937(1))",
    "random\\engine\\mt19937": "new Random\\Engine\\Mt19937(1)",
    "random\\engine\\pcgoneseq128xslrr64": "new Random\\Engine\\PcgOneseq128XslRr64(1)",
    "random\\engine\\xoshiro256starstar": "new Random\\Engine\\Xoshiro256StarStar(1)",
    "random\\engine\\secure": "new Random\\Engine\\Secure()",
    "reflectionclass": "new ReflectionClass('PC')",
    "reflectionobject": "new ReflectionObject(new PC)",
    "reflectionmethod": "new ReflectionMethod('PC', 'm')",
    "reflectionfunction": "new ReflectionFunction('pfn')",
    "reflectionfunctionabstract": "new ReflectionFunction('pfn')",
    "reflectionproperty": "new ReflectionProperty('PC', 'p')",
    "reflectionparameter": "new ReflectionParameter('pfn', 0)",
    "reflectionclassconstant": "new ReflectionClassConstant('PC', 'K')",
    "reflectionenum": "new ReflectionEnum('PE')",
    "reflectionenumunitcase": "new ReflectionEnumUnitCase('PE', 'A')",
    "reflectionenumbackedcase": "new ReflectionEnumBackedCase('PB', 'A')",
    "reflectionextension": "new ReflectionExtension('standard')",
    "reflectiongenerator": "new ReflectionGenerator(pgen())",
    "reflectionfiber": "new ReflectionFiber(new Fiber(fn() => 1))",
    "reflectiontype": "(new ReflectionFunction('pfn'))->getParameters()[0]->getType()",
    "reflectionnamedtype": "(new ReflectionFunction('pfn'))->getParameters()[0]->getType()",
    "reflectionuniontype": "(new ReflectionFunction('pfu'))->getParameters()[0]->getType()",
    "reflectionintersectiontype": "(new ReflectionFunction('pfi'))->getParameters()[0]->getType()",
    "reflectionattribute": "(new ReflectionClass('PC'))->getAttributes()[0]",
    "phptoken": "new PhpToken(1, 'x')",
    "simplexmlelement": "new SimpleXMLElement('<a><b>x</b></a>')",
    "sensitiveparametervalue": "new SensitiveParameterValue(1)",
}

# Declarations the recipes use (probe scripts of methods only).
PREAMBLE = r"""
#[AllowDynamicProperties] class PC { const K = 1; public $p = 1; function m($a) {} }
function pfn(int $a) {}
function pfu(int|string $a) {}
interface PI1 {} interface PI2 {}
function pfi(PI1&PI2 $a) {}
enum PE { case A; }
enum PB: int { case A = 1; }
function pgen() { yield 1; }
$__keep = new stdClass;
file_put_contents(__DIR__ . '/data.txt', "a,b\nc,d\n");
"""

# Methods not to probe (lowercase "class::method").
SKIP_METHODS = {"closure::__invoke", "fiber::suspend"}
# Classes whose methods are not probed (network, session state).
SKIP_CLASSES = {"mysqli", "mysqli_result", "mysqli_stmt", "mysqli_driver", "mysqli_warning",
                "mysqli_sql_exception", "sessionhandler", "php_user_filter"}

T_NULL, T_FALSE, T_TRUE, T_INT, T_FLOAT, T_STRING, T_ARRAY, T_OBJECT, T_CALLABLE = 1, 2, 4, 8, 16, 32, 64, 128, 256
T_MIXED, T_CLASS = 1024, 16384
T_BOOL = T_FALSE | T_TRUE
# zpp_signature() markers: Z_PARAM_ZVAL / "z" and Z_PARAM_RESOURCE / "r"
ZVAL_MARK, RESOURCE_MARK = 1 << 20, 1 << 21
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
    name = f["disp"]
    params = [p for p in f["params"] if not p["variadic"]]
    variadic = any(p["variadic"] for p in f["params"])
    req = f["required"]
    probes = []  # (kind, args, expected_first_line)
    # (the parameters after a "stop" one are probed too: kind "past")
    sf = source_flags(f)
    past = sf & 4 != 0

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
            if ph is None and past and checkable(params[j]) == "stop":
                ph = stop_placeholder(params[j])
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
    kind = "types"
    for i, p in enumerate(params):
        c = checkable(p)
        if c == "stop":
            if not past:
                break
            kind = "past"
            if sf & 8 and p["mask"] == 0 and not p["by_ref"]:
                exp = "X: TypeError: %s(): Argument #%d ($%s) must be of type resource%s, string given" % (
                    name, i + 1, p["name"], " or null" if p["default"] == "null" else "")
                for n in range(max(req, i + 1), len(params) + 1):
                    a = args_with(n, i, "'abc'")
                    if a is not None:
                        probes.append((kind, a, exp))
            continue
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
                    probes.append((kind, a, exp))
    return probes


def probe_script(f, probes):
    lines = ["<?php",
             "set_error_handler(function($no, $str) { echo \"\\nE$no: $str\\n\"; return true; });",
             "function __p($id, $f) { echo \"\\n#P$id\\n\"; try { $f(); } catch (\\Throwable $e) { echo \"\\nX: \", get_class($e), \": \", $e->getMessage(), \"\\n\"; } }"]
    if f.get("setup") is not None:
        lines.append(PREAMBLE)
    for idx, (_, args, _) in enumerate(probes):
        pre, call = [], []
        # Pass everything through variables so nothing is evaluated at compile time.
        for j, a in enumerate(args):
            pre.append("$v%d = %s;" % (j, a))
            call.append("$v%d" % j)
        lines.append("__p(%d, function() { %s %s %s; });" % (idx, f.get("setup") or "", " ".join(pre), f["call"] % ", ".join(call)))
    lines.append("echo \"\\n#END\\n\";")
    return "\n".join(lines) + "\n"


def run_probes(binary, f, probes):
    """{probe index: [diagnostic lines]} of the probes run under `binary`
    (None on timeout)."""
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "probe.php")
        with open(path, "w") as fo:
            fo.write(probe_script(f, probes))
        cmd = [binary]
        if binary == PHP_BIN:
            cmd += ["-d", "display_errors=1", "-d", "error_reporting=-1", "-d", "memory_limit=256M"]
        try:
            r = subprocess.run(cmd + [path], cwd=d, capture_output=True, timeout=20, stdin=subprocess.DEVNULL)
            out = r.stdout.decode("utf-8", "replace")
        except subprocess.TimeoutExpired:
            return None
    lines = {}
    cur = None
    for line in out.split("\n"):
        if line.startswith("#P"):
            cur = int(line[2:])
            lines[cur] = []
            continue
        if line.startswith("#END"):
            cur = None
            continue
        if cur is not None and line.strip():
            lines[cur].append(line)
    return lines


def run_one(item):
    f, probes = item
    if not f.get("exists", True):
        return f, probes, {}, None
    out = run_probes(PHP_BIN, f, probes)
    if out is None:
        return f, probes, None, None
    first = {k: (v[0] if v else None) for k, v in out.items()}
    return f, probes, first, out


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
    # (parameters zpp accepts anything for / only resources for: see source_flags())
    "ZVAL": ZVAL_MARK, "RESOURCE": RESOURCE_MARK, "STREAM": RESOURCE_MARK,
}
FMT = {"s": T_STRING, "S": T_STRING, "p": T_STRING, "P": T_STRING, "l": T_INT, "d": T_FLOAT,
       "b": T_BOOL, "a": T_ARRAY, "A": T_ARRAY, "h": T_ARRAY, "H": T_ARRAY, "n": T_INT | T_FLOAT,
       "z": ZVAL_MARK, "r": RESOURCE_MARK}

_C_BODIES = None


def c_bodies():
    """C handler name (zif_<function>, zim_<Class>_<method>, lowercase) ->
    bodies of every PHP_FUNCTION / ZEND_FUNCTION / PHP_METHOD / ZEND_METHOD
    in php-src."""
    global _C_BODIES
    if _C_BODIES is not None:
        return _C_BODIES
    _C_BODIES = {}
    pat = re.compile(r"^(?:PHP|ZEND)_(?:FUNCTION\((\w+)|METHOD\((\w+),\s*(\w+))\)\s*(?:/\*.*?\*/)?\s*\{(.*?)^\}", re.M | re.S)
    # Helpers taking INTERNAL_FUNCTION_PARAMETERS (zpp often lives there)
    hpat = re.compile(r"^[\w \t\*]*?\b(\w+)\s*\(\s*INTERNAL_FUNCTION_PARAMETERS[^)]*\)\s*(?:/\*.*?\*/)?\s*\{(.*?)^\}", re.M | re.S)
    for root, dirs, files in os.walk(PHP_SRC):
        if "/tests" in root:
            continue
        consts = None
        for fn in files:
            if fn.endswith(".c"):
                try:
                    txt = open(os.path.join(root, fn), errors="replace").read()
                except OSError:
                    continue
                for m in pat.finditer(txt):
                    key = "zif_" + m.group(1) if m.group(1) else "zim_%s_%s" % (m.group(2), m.group(3))
                    _C_BODIES.setdefault(key.lower(), []).append(m.group(4))
                for m in hpat.finditer(txt):
                    _C_HELPERS.setdefault(m.group(1), []).append(m.group(2))
                if MACRO_FUNC.search(txt):
                    if consts is None:
                        consts = dir_int_macros(root, files)
                    for name, body in macro_functions(txt, consts):
                        _C_BODIES.setdefault("zif_" + name.lower(), []).append(body)
    return _C_BODIES


# A function-like macro whose expansion is a PHP_FUNCTION definition.
MACRO_FUNC = re.compile(r"^#define\s+\w+\([^)]*\)\s*\\\s*\n\s*(?:static\s+)?(?:PHP|ZEND)_FUNCTION\(", re.M)


def dir_int_macros(root, files):
    """Integer-valued object-like macros (`#define NAME 0x1f`) of the .c/.h
    files of a directory."""
    out = {}
    for fn in files:
        if fn.endswith((".c", ".h")):
            try:
                txt = open(os.path.join(root, fn), errors="replace").read()
            except OSError:
                continue
            for m in re.finditer(r"^#define\s+(\w+)[ \t]+\(?\s*(-?(?:0[xX][0-9a-fA-F]+|\d+))\s*\)?[ \t]*(?:/\*.*?\*/)?[ \t]*$", txt, re.M):
                out[m.group(1)] = int(m.group(2), 0)
    return out


def macro_functions(txt, consts):
    """(function name, body) of the PHP_FUNCTIONs that function-like macros
    define, e.g. ext/zlib's `#define PHP_ZLIB_ENCODE_FUNC(name,
    default_encoding) PHP_FUNCTION(name) { ... }` invoked as
    `PHP_ZLIB_ENCODE_FUNC(gzdeflate, PHP_ZLIB_ENCODING_RAW);`: the macro's
    body with the invocation's arguments substituted, and `if (<argument>)
    { A } else { B }` folded when the argument is an integer constant (so a
    body with one zpp block per branch has the single block of that
    invocation)."""
    out = []
    for m in re.finditer(r"^#define\s+(\w+)\(([^)]*)\)((?:[^\n]*\\\n)+[^\n]*)", txt, re.M):
        mname, params = m.group(1), [x.strip() for x in m.group(2).split(",")]
        text = re.sub(r"\\\n", "\n", m.group(3))
        fm = re.search(r"(?:PHP|ZEND)_FUNCTION\(\s*(\w+)\s*\)\s*\{", text)
        if not fm or fm.group(1) not in params:
            continue
        body = brace_block(text, fm.end() - 1)
        if body is None:
            continue
        for inv in re.finditer(r"^%s\(([^)]*)\)" % re.escape(mname), txt, re.M):
            args = [x.strip() for x in inv.group(1).split(",")]
            if len(args) != len(params):
                continue
            env = dict(zip(params, args))
            b = re.sub(r"\b(%s)\b" % "|".join(map(re.escape, params)), lambda x: env[x.group(1)], body)
            out.append((env[fm.group(1)], fold_ifs(b, consts)))
    return out


def brace_block(text, start):
    """The text inside the brace block opening at text[start] == "{"."""
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[start + 1:i]
    return None


def fold_ifs(body, consts):
    """Fold `if (<integer constant>) { A } else { B }` to A or B."""
    while True:
        for m in re.finditer(r"\bif\s*\(\s*(-?\w+)\s*\)\s*\{", body):
            c = m.group(1)
            try:
                val = int(c, 0)
            except ValueError:
                if c not in consts:
                    continue
                val = consts[c]
            then = brace_block(body, m.end() - 1)
            if then is None:
                continue
            end = m.end() + len(then) + 1
            other, tail = "", end
            rest = re.match(r"\s*else\s*\{", body[end:])
            if rest:
                other = brace_block(body, end + rest.end() - 1)
                if other is None:
                    continue
                tail = end + rest.end() + len(other) + 1
            body = body[:m.start()] + (then if val else other) + body[tail:]
            break
        else:
            return body


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


def zpp_signature(body, method=False):
    """(min, max, [mask or None per parameter]) of the function's single zpp
    block, or None when it does not have exactly one. `method`: the body
    runs as a method (zend_parse_method_parameters() then takes its leading
    "O" from $this)."""
    starts = re.findall(r"ZEND_PARSE_PARAMETERS_START\(\s*(\d+)\s*,\s*(-?\d+)\s*\)(.*?)ZEND_PARSE_PARAMETERS_END", body, re.S)
    none = len(re.findall(r"ZEND_PARSE_PARAMETERS_NONE\(\)|zend_parse_parameters_none\(\)", body))
    old = re.findall(r"zend_parse_parameters(?:_ex)?\s*\((?:[^,]*,)?\s*ZEND_NUM_ARGS\(\)\s*,\s*\"([^\"]*)\"", body)
    mold = re.findall(r"zend_parse_method_parameters\s*\(\s*ZEND_NUM_ARGS\(\)\s*,[^,]*,\s*\"([^\"]*)\"", body)
    allold = len(re.findall(r"zend_parse_parameters(?!_none)|zend_parse_method_parameters", body))
    if len(starts) + none + allold != 1:
        return None
    if mold:
        old = [mold[0][1:] if method and mold[0].startswith("O") else mold[0]]
    if none:
        return (0, 0, [])
    if starts:
        mn, mx, inner = starts[0]
        masks = []
        for m in re.finditer(r"Z_PARAM_(\w+)\s*\(([^;]*?)\)\s*;?[ \t]*(?:\n|$)", inner):
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
    """CHECK_* flags allowed by php-src's implementation of `f`.

    4 (CHECK_PAST_STOP): the parameters the generic layer leaves to the
    function ("stop", see checkable()) are parsed as plain zvals, or are
    by-reference arrays/objects zpp checks the way the generic layer does,
    so the parameters after them are checked too; 8 (CHECK_RESOURCE): the
    untyped by-value ones of them are resources (Z_PARAM_RESOURCE, "r")."""
    bodies = c_bodies().get(f["impl"].lower(), [])
    if len(bodies) != 1:
        return 0
    sig = zpp_signature(zpp_body(bodies[0]), f.get("setup") is not None and not f.get("static"))
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
        past, kinds, after = len(masks) >= len(params), set(), False
        for i, p in enumerate(params):
            if not past:
                break
            c, m, pm = checkable(p), masks[i], p["mask"]
            if c == "skip":
                continue
            if c == "check":
                past = m == pm
                after = bool(kinds) or after
            elif m is not None and m & ZVAL_MARK and not (p["by_ref"] and pm & ~(SCALAR | T_NULL)):
                kinds.add("z" if not p["by_ref"] and pm == 0 else "zt")
            elif m is not None and m & RESOURCE_MARK and not p["by_ref"] and pm == 0:
                # (arginfo.rs: `$x = null` ones are Z_PARAM_RESOURCE_OR_NULL)
                if bool(m & T_NULL) != (p["default"] == "null"):
                    past = False
                kinds.add("r")
            elif p["by_ref"] and m == pm and not pm & (SCALAR | T_CALLABLE):
                kinds.add("a")
            else:
                past = False
        # (only where there is a parameter to check after one of them)
        if past and after and not ("r" in kinds and "z" in kinds):
            flags |= 4 | (8 if "r" in kinds else 0)
    return flags


def stop_placeholder(p):
    """An argument for a parameter checkable() stops at, when the function
    has CHECK_PAST_STOP (zpp accepts anything there)."""
    m = p["mask"]
    if p["by_ref"] and m & T_ARRAY:
        return "[]"
    if m == 0:
        return "fopen('php://memory', 'r')"
    if m & T_STRING:
        return "'stdClass'"
    if m & T_ARRAY:
        return "[]"
    if m & T_CALLABLE:
        return "'strlen'"
    return "new stdClass"


def rs_unescape(lit):
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(m.group(1), m.group(1)), lit)


_LIT = r'"((?:[^"\\]|\\.)*)"'


def parse_stub_func(block):
    """A `StubFunc { ... }` literal of stub_sigs.rs (the text after `StubFunc {`)."""
    name = rs_unescape(re.search(r"name: " + _LIT, block).group(1))
    flags = int(re.search(r"flags: (0x[0-9a-f]+|\d+)", block).group(1), 0)
    args = []
    for a in re.finditer(r"StubArg \{\s*name: " + _LIT + r",\s*ty: " + _LIT + r",\s*default: (None|Some\(\s*" + _LIT + r",?\s*\)),\s*flags: (\d+),", block):
        af = int(a.group(5))
        args.append(dict(name=a.group(1), type=rs_unescape(a.group(2)),
                         default=None if a.group(3) == "None" else rs_unescape(a.group(4)),
                         by_ref=2 if af & 2 else (1 if af & 1 else 0), variadic=bool(af & 4),
                         unknown_default=bool(af & 16)))
    nargs = len(re.findall(r"StubArg \{", block))
    assert nargs == len(args), "unparsed StubArg in " + name
    return dict(name=name, flags=flags, params=args, deprecated=bool(flags & 0x20000))


def stub_text():
    return open(os.path.join(HERE, "..", "crates/goro-core/src/stub_sigs.rs")).read()


def stub_aliases():
    """`@alias` / `@implementation-alias` targets (stub_sigs.rs ALIASES)."""
    txt = stub_text()
    txt = txt[txt.index("pub static ALIASES"):]
    return {rs_unescape(k): rs_unescape(v) for k, v in re.findall(r"\(\s*" + _LIT + r",\s*" + _LIT + r",?\s*\)", txt)}


def stub_functions():
    """Internal functions from crates/goro-core/src/stub_sigs.rs (generated
    by tools/gen_stub_sigs.php), keyed by lowercase name."""
    txt = stub_text()
    start = txt.index("pub static FUNCTIONS: &[StubFunc] = &[")
    txt = txt[start:txt.index("\n];\n", start)]
    aliases = stub_aliases()
    funcs = {}
    for block in re.split(r"\n    StubFunc \{", txt)[1:]:
        f = parse_stub_func(block)
        name = f["name"]
        f.update(disp=name, call=name + "(%s)", impl="zif_" + aliases.get(name, name))
        funcs[name.lower()] = f
    return funcs


def c_class(name):
    """C identifier of a class in PHP_METHOD() / class_<X>_methods."""
    return name.replace("\\", "_")


def stub_methods():
    """Public methods of the internal classes in stub_sigs.rs, keyed by
    lowercase "class::method" of the declaring class."""
    txt = stub_text()
    txt = txt[txt.index("pub static CLASSES"):]
    aliases = stub_aliases()
    out = {}
    for block in re.split(r"\n    StubClass \{", txt)[1:]:
        cname = rs_unescape(re.search(r"name: " + _LIT, block).group(1))
        kind = int(re.search(r"kind: (\d+)", block).group(1))
        cflags = int(re.search(r"flags: (0x[0-9a-f]+)", block).group(1), 16)
        if kind not in (0, 3) or "methods: &[" not in block:
            continue
        ms = block[block.index("methods: &["):]
        for mb in re.split(r"StubFunc \{", ms)[1:]:
            f = parse_stub_func(mb)
            mname = f["name"]
            if f["flags"] & (2 | 4 | 0x40):  # protected, private, abstract
                continue
            static = bool(f["flags"] & 0x10)
            key = "%s::%s" % (cname, mname)
            target = aliases.get(key)
            if target is None:
                impl = "zim_%s_%s" % (c_class(cname), mname)
            elif "::" in target:
                tc, tm = target.split("::")
                impl = "zim_%s_%s" % (c_class(tc), tm)
            else:
                impl = "zif_" + target
            recipe = RECIPES.get(cname.lower())
            if mname.lower() == "__construct":
                if cflags & 0x40:
                    continue
                setup, call = "", "new \\%s(%%s)" % cname
            elif static:
                setup, call = "", "\\%s::%s(%%s)" % (cname, mname)
            elif recipe is not None:
                setup, call = "$o = %s;" % recipe, "$o->%s(%%s)" % mname
            else:
                setup, call = None, None
            f.update(disp=key, call=call, setup=setup, impl=impl, static=static, cls=cname)
            out[key.lower()] = f
    return out


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


def existing(funcs, methods):
    """Lowercase names of the functions / "class::method"s PHP_BIN has."""
    with tempfile.TemporaryDirectory() as d:
        lst = os.path.join(d, "names.txt")
        with open(lst, "w") as fo:
            for k in funcs:
                fo.write("f:%s\n" % k)
            for k in methods:
                fo.write("m:%s\n" % k)
        script = os.path.join(d, "exists.php")
        with open(script, "w") as fo:
            fo.write("""<?php
foreach (file($argv[1], FILE_IGNORE_NEW_LINES) as $l) {
    [$k, $n] = explode(':', $l, 2);
    if ($k === 'f') { if (function_exists($n)) echo $l, "\\n"; continue; }
    [$c, $m] = explode('::', $n);
    if ((class_exists($c) || enum_exists($c)) && method_exists($c, $m)) echo $l, "\\n";
}
""")
        out = subprocess.run([PHP_BIN, script, lst], capture_output=True).stdout.decode()
    return {l[2:] for l in out.split("\n") if l}


def prepare(f):
    params = [dict(p, mask=type_mask(p["type"]), ty=type_display(p["type"])) for p in f["params"]]
    req = 0
    for p in params:
        if p["default"] is not None or p["variadic"] or p["unknown_default"]:
            break
        req += 1
    return dict(f, params=params, required=req)


def verdict(f, probes, first):
    """CHECK_* flags of `f` from its probe results (None: did not run)."""
    if not f["exists"]:
        # Not in PHP_BIN: php-src's C implementation alone decides.
        return 15 & source_flags(f)
    if not first:
        return 0
    flags = 0
    ok = {"arity": True, "types": True, "past": True}
    for idx, (kind, _, exp) in enumerate(probes):
        got = first.get(idx)
        if got != exp:
            ok[kind] = False
            if os.environ.get("VERBOSE"):
                print("%s: %s\n   expected %r\n   got      %r" % (f["disp"], kind, exp, got))
    if ok["arity"]:
        flags |= 1
    if ok["types"]:
        flags |= 2
        if ok["past"]:
            flags |= 12
    sf = source_flags(f)
    if os.environ.get("VERBOSE") and flags & ~sf:
        print("%s: source check drops flags %d" % (f["disp"], flags & ~sf))
    return flags & sf


def goro_mismatches(f, probes, php_out, flags):
    """Probes of the verified kinds whose first diagnostic (the zpp one)
    differs under goro."""
    out = run_probes(GORO, f, probes)
    bad = []
    for idx, (kind, args, _) in enumerate(probes):
        if not flags & {"arity": 1, "types": 2, "past": 4}[kind]:
            continue
        want = (php_out.get(idx) or [None])[0]
        got = ((out or {}).get(idx) or [None])[0]
        if want != got:
            bad.append((f["call"] % ", ".join(args), want, got))
    return bad


def main():
    funcs = stub_functions()
    methods = stub_methods()
    wanted = [l.strip().lower() for l in open(FN_LIST) if l.strip()]
    have = existing(funcs, methods)
    items = []
    for name in wanted:
        f = funcs.get(name)
        if f is None or f["deprecated"] or name in SKIP or name.startswith(SKIP_PREFIX):
            continue
        f = prepare(dict(f, exists=name in have))
        items.append((f, build_probes(f)))
    for key, f in sorted(methods.items()):
        if f["deprecated"] or key in SKIP_METHODS or f["cls"].lower() in SKIP_CLASSES:
            continue
        exists = key in have
        if exists and f["call"] is None:
            continue
        f = prepare(dict(f, exists=exists))
        items.append((f, build_probes(f) if exists else []))
    results, mresults = {}, {}
    report = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:
        for f, probes, first, out in ex.map(run_one, items):
            flags = verdict(f, probes, first)
            (mresults if f.get("cls") else results)[f["disp"].lower()] = flags
            if GORO and flags and f["exists"] and out is not None:
                for call, want, got in goro_mismatches(f, probes, out, flags):
                    report.append("%s: %s\n   php:  %r\n   goro: %r" % (f["disp"], call, want, got))
    with open(OUT, "w") as fo:
        fo.write("// Generated by tools/verify_arginfo.py (probing %s); do not edit.\n" % os.path.basename(PHP_BIN))
        fo.write("// Functions and methods whose generated arginfo reproduces PHP's zpp errors: 1 = argument count, 2 = parameter types, 4 = also past the parameters left to the function (parsed as zvals), 8 = the untyped ones of them are resources.\n")
        fo.write("#![cfg_attr(rustfmt, rustfmt_skip)]\n")
        fo.write("pub static VERIFIED: &[(&str, u8)] = &[\n")
        for k in sorted(results):
            if results[k]:
                fo.write("    (\"%s\", %d),\n" % (k, results[k]))
        fo.write("];\n")
        fo.write("// Methods, by lowercase \"declaring class::method\".\n")
        fo.write("pub static VERIFIED_METHODS: &[(&str, u8)] = &[\n")
        for k in sorted(mresults):
            if mresults[k]:
                fo.write("    (\"%s\", %d),\n" % (k.replace("\\", "\\\\"), mresults[k]))
        fo.write("];\n")
    for kind, res in (("functions", results), ("methods", mresults)):
        n1 = sum(1 for v in res.values() if v & 1)
        n2 = sum(1 for v in res.values() if v & 2)
        print("%d %s: %d arity-verified, %d type-verified" % (len(res), kind, n1, n2))
    if GORO:
        rep = "\n".join(sorted(report)) + "\n"
        if os.environ.get("GORO_REPORT"):
            with open(os.environ["GORO_REPORT"], "w") as fo:
                fo.write(rep)
        else:
            sys.stdout.write(rep)
        print("goro: %d mismatching probes" % len(report))


main()
