#!/usr/bin/env python3
"""Generate Rust table modules for goro-ext-mbstring from php-src's libmbfl headers.

Usage: tools/gen_mbfl_tables.py /path/to/php-src

Every `static const T name[] = {...};` array, scalar constant and simple
`#define NAME number` in the selected C headers is converted into a Rust item
with the same (lower-case) name, so ported C code can reference the tables
unchanged. Output goes to crates/goro-ext-mbstring/src/mbfl/tables/.
"""
import os
import re
import sys

HEADERS = [
    "libmbfl/filters/unicode_table_jis.h",
    "libmbfl/filters/unicode_table_jis2004.h",
    "libmbfl/filters/unicode_table_big5.h",
    "libmbfl/filters/unicode_table_cns11643.h",
    "libmbfl/filters/unicode_table_cp932_ext.h",
    "libmbfl/filters/unicode_table_cp936.h",
    "libmbfl/filters/unicode_table_gb18030.h",
    "libmbfl/filters/unicode_table_gb2312.h",
    "libmbfl/filters/unicode_table_uhc.h",
    "libmbfl/filters/unicode_table_cp1252.h",
    "libmbfl/filters/cp932_table.h",
    "libmbfl/filters/sjis_mac2uni.h",
    "libmbfl/filters/translit_kana_jisx0201_jisx0208.h",
    "libmbfl/filters/emoji2uni.h",
    "libmbfl/filters/unicode_prop.h",
    "libmbfl/mbfl/eaw_table.h",
    "unicode_data.h",
    "rare_cp_bitvec.h",
]

# C source files from which only constant numeric arrays are extracted
ARRAY_SOURCES = [
    "libmbfl/filters/mbfilter_singlebyte.c",
    "libmbfl/filters/mbfilter_cjk.c",
    "libmbfl/filters/mbfilter_utf8.c",
    "libmbfl/filters/mbfilter_htmlent.c",
]

CTYPES = {
    "unsigned short": "u16",
    "short": "i16",
    "unsigned int": "u32",
    "unsigned": "u32",
    "uint32_t": "u32",
    "int": "i32",
    "unsigned char": "u8",
    "char": "u8",
    "int8_t": "i8",
}

TYPE_RE = r"(unsigned short|unsigned int|unsigned char|unsigned|short|int|uint32_t|char|int8_t)"


def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", " ", s, flags=re.S)
    s = re.sub(r"//[^\n]*", " ", s)
    return s


def conv_num(tok, rtype):
    tok = tok.strip()
    if not tok:
        return None
    if tok.startswith("'"):
        return str(ord(tok[1:-1].encode().decode("unicode_escape")))
    tok = re.sub(r"[uUlL]+$", "", tok)
    tok = re.sub(r"^0X", "0x", tok)
    return tok


def flatten(body):
    """Split a C initializer body into nested python lists of tokens."""
    pos = 0
    stack = [[]]
    cur = ""
    while pos < len(body):
        ch = body[pos]
        if ch == "{":
            stack.append([])
        elif ch == "}":
            if cur.strip():
                stack[-1].append(cur.strip())
            cur = ""
            inner = stack.pop()
            stack[-1].append(inner)
        elif ch == ",":
            if cur.strip():
                stack[-1].append(cur.strip())
            cur = ""
        else:
            cur += ch
        pos += 1
    if cur.strip():
        stack[-1].append(cur.strip())
    return stack[0]


def rust_size_expr(expr, known):
    e = expr
    # sizeof(x)/sizeof(T) or sizeof(x)/(sizeof(T)*k) or sizeof(x)/(k*sizeof(T))
    e = re.sub(
        r"sizeof\s*\(\s*(\w+)\s*\)\s*/\s*(?:\(\s*(?:\d+\s*\*\s*)?sizeof\s*\(\s*[\w ]+\s*\)\s*(?:\*\s*\d+\s*)?\)|sizeof\s*\(\s*[\w ]+\s*\))",
        lambda m: "(%s.len() as i32)" % m.group(1),
        e,
    )
    e = re.sub(r"(?<![\w.])(\w+)(?![\w(])", lambda m: m.group(1), e)
    return e


def gen(header_path, arrays_only=False):
    src = open(header_path, encoding="latin-1").read()
    src = strip_comments(src)
    out = []
    known = set()
    # #defines with numeric values
    for m in re.finditer(r"^\s*#define\s+(\w+)\s+(\(?-?(?:0x[0-9A-Fa-f]+|\d+)\)?)\s*$", src, flags=re.M):
        name, val = m.group(1), m.group(2)
        if name.endswith("_H") or arrays_only:
            continue
        out.append("pub const %s: i32 = %s;" % (name, val))
    # struct arrays (eaw table)
    for m in re.finditer(r"static\s+const\s+struct\s*\{([^}]*)\}\s*(\w+)\s*\[\s*\]\s*=\s*\{(.*?)\};", src, flags=re.S):
        fields = [f for f in m.group(1).split(";") if f.strip()]
        name = m.group(2)
        rows = flatten(m.group(3))
        n = len(fields)
        out.append("pub static %s: [[i32; %d]; %d] = [" % (name, n, len(rows)))
        for r in rows:
            out.append("    [%s]," % ", ".join(conv_num(x, "i32") for x in r))
        out.append("];")
        known.add(name)
    src2 = re.sub(r"static\s+const\s+struct\s*\{[^}]*\}\s*\w+\s*\[\s*\]\s*=\s*\{.*?\};", "", src, flags=re.S)
    # arrays
    arr_re = re.compile(
        r"^(?:static\s+)?const\s+" + TYPE_RE + r"\s*(\*?)\s*(\w+)\s*\[\s*\d*\s*\]((?:\s*\[\s*\d+\s*\])?)\s*(?:ZEND_NONSTRING\s*)?=\s*\{(.*?)\}\s*;",
        flags=re.S | re.M,
    )
    for m in arr_re.finditer(src2):
        ctype, ptr, name, dim2, body = m.groups()
        if '"' in body:
            continue
        if arrays_only and ptr:
            continue
        rtype = CTYPES[ctype]
        if ptr:
            elems = [x for x in flatten(body) if isinstance(x, str) and x != "NULL"]
            out.append("pub static %s: [&[%s]; %d] = [%s];" % (name, rtype, len(elems), ", ".join("&" + e for e in elems)))
        elif dim2:
            k = int(re.search(r"\d+", dim2).group(0))
            rows = flatten(body)
            out.append("pub static %s: [[%s; %d]; %d] = [" % (name, rtype, k, len(rows)))
            for r in rows:
                vals = [conv_num(x, rtype) for x in r]
                vals += ["0"] * (k - len(vals))
                out.append("    [%s]," % ", ".join(vals))
            out.append("];")
        else:
            vals = [conv_num(x, rtype) for x in flatten(body)]
            vals = [v for v in vals if v is not None]
            # Some tables contain values like 0xFFFF in a signed type; cast via `as`
            if rtype.startswith("i"):
                vals = ["(%s) as %s" % (v, rtype) if v.startswith("0x") else v for v in vals]
            else:
                vals = ["(%si64) as %s" % (v, rtype) if v.startswith("-") else v for v in vals]
            lines = []
            for i in range(0, len(vals), 12):
                lines.append("    " + ", ".join(vals[i:i + 12]) + ",")
            out.append("pub static %s: [%s; %d] = [" % (name, rtype, len(vals)))
            out.extend(lines)
            out.append("];")
        known.add(name)
    if arrays_only:
        return out
    src3 = arr_re.sub("", src2)
    # scalar constants
    sc_re = re.compile(r"^\s*(?:static\s+)?(?:const\s+)?" + TYPE_RE + r"\s+(\w+)\s*=\s*([^;{]*);", flags=re.M | re.S)
    for m in sc_re.finditer(src3):
        ctype, name, expr = m.groups()
        expr = " ".join(expr.split())
        e = rust_size_expr(expr, known)
        out.append("pub const %s: i32 = (%s) as i32;" % (name, e))
    return out


def main():
    php = sys.argv[1] if len(sys.argv) > 1 else "/home/magicaltux/php-src"
    base = os.path.join(php, "ext/mbstring")
    here = os.path.dirname(os.path.abspath(__file__))
    outdir = os.path.join(here, "..", "crates/goro-ext-mbstring/src/mbfl/tables")
    os.makedirs(outdir, exist_ok=True)
    mods = []
    for h in HEADERS:
        mod = os.path.basename(h)[:-2]
        lines = gen(os.path.join(base, h))
        with open(os.path.join(outdir, mod + ".rs"), "w") as f:
            f.write("// Generated by tools/gen_mbfl_tables.py from php-src ext/mbstring/%s; do not edit.\n" % h)
            f.write("#![allow(non_upper_case_globals, dead_code, clippy::all)]\n#![cfg_attr(rustfmt, rustfmt_skip)]\n")
            f.write("\n".join(lines))
            f.write("\n")
        mods.append(mod)
    for h in ARRAY_SOURCES:
        mod = "filt_" + os.path.basename(h)[:-2].replace("mbfilter_", "")
        lines = gen(os.path.join(base, h), arrays_only=True)
        with open(os.path.join(outdir, mod + ".rs"), "w") as f:
            f.write("// Generated by tools/gen_mbfl_tables.py from php-src ext/mbstring/%s; do not edit.\n" % h)
            f.write("#![allow(non_upper_case_globals, dead_code, clippy::all)]\n#![cfg_attr(rustfmt, rustfmt_skip)]\n")
            f.write("\n".join(lines))
            f.write("\n")
        mods.append(mod)
    with open(os.path.join(outdir, "mod.rs"), "w") as f:
        f.write("// Generated by tools/gen_mbfl_tables.py; do not edit.\n")
        for m in mods:
            f.write("pub mod %s;\n" % m)


if __name__ == "__main__":
    main()
