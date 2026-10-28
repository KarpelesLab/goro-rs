#!/usr/bin/env python3
"""Port the input/digest pairs of php-src's ext/hash tests into Rust.

Reads the .phpt files (black-box tests: their --EXPECT-- sections are the
required output) and writes crates/goro-ext-hash/tests/phpt_vectors_data.rs.
Two test shapes are understood:

  * scripts made only of ``echo hash('algo', <expr>)`` lines, paired in
    order with the expected output lines;
  * ``array("input", "digest")`` tables checked against one algorithm
    (fnv*.phpt, joaat.phpt).

<expr> may be a quoted literal, str_repeat(<literal>, n), or a
concatenation of those. Anything else is skipped.

Usage: gen_hash_phpt_vectors.py PHP_TESTS_DIR > crates/goro-ext-hash/tests/phpt_vectors_data.rs
"""
import os
import re
import sys

ESC = {'n': b'\n', 't': b'\t', 'r': b'\r', '0': b'\0', '\\': b'\\', '"': b'"', '$': b'$', 'v': b'\v', 'e': b'\x1b', 'f': b'\f'}


def php_string(tok):
    q, body = tok[0], tok[1:-1]
    if q == "'":
        return body.replace("\\'", "'").replace('\\\\', '\\').encode('latin-1')
    out = b''
    i = 0
    while i < len(body):
        c = body[i]
        if c == '\\' and i + 1 < len(body):
            n = body[i + 1]
            if n == 'x' and re.match(r'[0-9A-Fa-f]', body[i + 2:i + 3]):
                m = re.match(r'[0-9A-Fa-f]{1,2}', body[i + 2:])
                out += bytes([int(m.group(0), 16)])
                i += 2 + len(m.group(0))
                continue
            if n in '01234567':
                m = re.match(r'[0-7]{1,3}', body[i + 1:])
                out += bytes([int(m.group(0), 8) & 255])
                i += 1 + len(m.group(0))
                continue
            if n in ESC:
                out += ESC[n]
                i += 2
                continue
            out += b'\\'
            i += 1
            continue
        if c == '$':
            raise ValueError('interpolation')
        out += c.encode('latin-1')
        i += 1
    return out


STR = r"""'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*\""""


def php_expr(e):
    e = e.strip()
    parts = []
    pos = 0
    pat = re.compile(r'\s*(?:(%s)|str_repeat\(\s*(%s)\s*,\s*(\d+)\s*\))\s*(\.|$)' % (STR, STR), re.S)
    while pos < len(e):
        m = pat.match(e, pos)
        if not m:
            raise ValueError(e)
        if m.group(1):
            parts.append((php_string(m.group(1)), 1))
        else:
            parts.append((php_string(m.group(2)), int(m.group(3))))
        pos = m.end()
    return parts


def rust_bytes(b):
    s = ''
    for c in b:
        if c in (0x22, 0x5c):
            s += '\\' + chr(c)
        elif 0x20 <= c < 0x7f:
            s += chr(c)
        else:
            s += '\\x%02x' % c
    return 'b"%s"' % s


def sections(text):
    out = {}
    for m in re.finditer(r'^--([A-Z_]+)--\n(.*?)(?=^--[A-Z_]+--\n|\Z)', text, re.S | re.M):
        out[m.group(1)] = m.group(2)
    return out


def main(root):
    d = os.path.join(root, 'ext/hash/tests')
    vectors = []
    for name in sorted(os.listdir(d)):
        if not name.endswith('.phpt'):
            continue
        text = open(os.path.join(d, name), encoding='latin-1').read()
        sec = sections(text)
        if 'FILE' not in sec or 'EXPECT' not in sec:
            continue
        code = sec['FILE']
        expect = [l for l in sec['EXPECT'].split('\n') if l != '']
        # Shape 1: echo hash('algo', expr) lines only.
        lines = [l.strip() for l in code.split('\n')]
        body = [l for l in lines if l not in ('<?php', '?>', '')
                and not l.startswith('//') and not l.startswith('/*')]
        got = []
        ok = True
        for l in body:
            # Lines that print one non-digest line (labels, var_dump()).
            if re.match(r'echo\s+"[^"$\\]*\\n"\s*;$', l) or l.startswith('var_dump('):
                got.append(None)
                continue
            m = re.match(r"""echo\s+hash\(\s*(%s)\s*,\s*(?:\$\w+\s*=\s*)?(.*?)\s*\)\s*(?:,|\.)\s*"\\n"\s*;$""" % STR, l)
            if not m:
                ok = False
                break
            try:
                got.append((php_string(m.group(1)).decode(), php_expr(m.group(2))))
            except ValueError:
                ok = False
                break
        if ok and got and len(got) == len(expect):
            for g, h in zip(got, expect):
                if g is not None:
                    vectors.append((name, g[0], g[1], h.strip()))
            continue
        # Shape 3: hash_copy_001/hash_serialize_001 print, per algorithm,
        # two digests of "I can't remember anything" in a first loop and
        # of it followed by "Can’t tell if this is true or dream" in a
        # second loop.
        if name in ('hash_copy_001.phpt', 'hash_serialize_001.phpt'):
            first = "I can't remember anything".encode()
            second = first + 'Can’t tell if this is true or dream'.encode('utf-8')
            seen = set()
            for m in re.finditer(r'string\(\d+\) "([^"]+)"\nstring\(\d+\) "([0-9a-f]+)"\nstring\(\d+\) "([0-9a-f]+)"', sec['EXPECT']):
                algo, h1, h2 = m.groups()
                data = first if algo not in seen else second
                h = h1 if algo not in seen else h2
                seen.add(algo)
                vectors.append((name, algo, [(data, 1)], h))
            continue
        # Shape 2: array("input", "digest") tables.
        m = re.search(r"""hash\(\s*(%s)\s*,\s*\$test\[0\]\s*\)""" % STR, code)
        if m:
            algo = php_string(m.group(1)).decode()
            for t in re.finditer(r"""array\(\s*(%s)\s*,\s*["']([0-9a-f]+)["']\s*\)""" % STR, code):
                if algo == 'joaat' and len(t.group(2)) != 8:
                    continue  # compared loosely by the test
                try:
                    vectors.append((name, algo, [(php_string(t.group(1)), 1)], t.group(2)))
                except ValueError:
                    pass
            for t in re.finditer(r"""array\(\s*R(10|500)\(\s*(%s)\s*\)\s*,\s*["']([0-9a-f]+)["']\s*\)""" % STR, code):
                vectors.append((name, algo, [(php_string(t.group(2)), int(t.group(1)))], t.group(3)))
    print('// Generated by tools/gen_hash_phpt_vectors.py from the input/digest')
    print('// pairs of php-src ext/hash/tests (used as black-box tests). Do not edit.')
    print()
    print('/// (test file, algorithm, input as (piece, repeat count) parts, expected hex digest)')
    print('#[allow(clippy::type_complexity)]')
    print('pub const VECTORS: &[(&str, &str, &[(&[u8], usize)], &str)] = &[')
    for name, a, parts, h in vectors:
        ps = ', '.join('(%s, %d)' % (rust_bytes(b), n) for b, n in parts)
        print('    ("%s", "%s", &[%s], "%s"),' % (name, a, ps, h))
    print('];')
    print('// %d vectors' % len(vectors), file=sys.stderr)


if __name__ == '__main__':
    main(sys.argv[1])
