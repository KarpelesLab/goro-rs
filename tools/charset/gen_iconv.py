#!/usr/bin/env python3
"""Generate the iconv tables of crates/goro-charset/src/iconv/tables/.

usage: python3 -I gen_iconv.py [--verify <obs-dir>]

Inputs:
- the public tables listed in iconv_bases.py and iconv_translit_base.py,
  downloaded into $ICONV_DATA (default /tmp/claude-1000/goro-work/charset-iconv/data);
- the committed observation data in tools/charset/iconv-data/ (written by
  iconv_mkdata.py and iconv_mktranslit.py from PHP 8.5 / glibc 2.43 runs).

With --verify, every reconstructed table is compared with the raw
observation dumps (tables.out) before anything is written.
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import iconv_bases  # noqa: E402
import iconv_translit_base  # noqa: E402

DATA_DIR = os.path.join(HERE, 'iconv-data')
OUT_DIR = os.path.join(HERE, '..', '..', 'crates', 'goro-charset', 'src', 'iconv', 'tables')

# Entry encoding, shared with crates/goro-charset/src/iconv/table.rs.
ILLEGAL1, ILLEGAL2, ILLEGAL3, ILLEGAL4, REPLACE2, EMPTY, FOUR, REPLACE1 = 0, 1, 2, 3, 4, 5, 6, 7
CHAR_BASE = 8
NODE_FLAG = 0x8000_0000
MULTI_FLAG = 0x4000_0000
NO_BYTES = 0xFFFF_FFFF


def parse_ranges(s):
    out = []
    for part in s.split(','):
        if '-' in part:
            a, b = part.split('-')
            out.extend(range(int(a, 16), int(b, 16) + 1))
        else:
            out.append(int(part, 16))
    return out


def norm_name(n):
    return ''.join(c for c in n.upper() if c.isascii() and (c.isalnum() or c in '-_.:,/'))


def load_charsets():
    out = []
    with open(os.path.join(DATA_DIR, 'charsets.txt')) as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            p = line.split()
            out.append((p[0], p[1], p[2:]))
    return out


class TableData:
    def __init__(self, cid):
        self.cid = cid
        self.d, self.i, self.s, self.e, self.c, self.g = {}, {}, {}, {}, [], []
        self.r = {}
        self.base = 'none'
        with open(os.path.join(DATA_DIR, cid + '.txt')) as f:
            for line in f:
                if line.startswith('#') or not line.strip():
                    continue
                p = line.split()
                if p[0] == 'base':
                    self.base = p[1]
                elif p[0] == 'D':
                    v = p[2]
                    self.d[bytes.fromhex(p[1])] = None if v == '-' else () if v == 'e' else \
                        tuple(int(x, 16) for x in v.split('+'))
                elif p[0] == 'I':
                    pre = b'' if p[1] == '-' else bytes.fromhex(p[1])
                    self.i.setdefault(pre, set()).update(parse_ranges(p[2]))
                elif p[0] == 'S':
                    pre = b'' if p[1] == '-' else bytes.fromhex(p[1])
                    for b in parse_ranges(p[3]):
                        self.s[pre + bytes([b])] = int(p[2])
                elif p[0] == 'R':
                    pre = b'' if p[1] == '-' else bytes.fromhex(p[1])
                    for b in parse_ranges(p[3]):
                        self.r[pre + bytes([b])] = int(p[2])
                elif p[0] == 'E':
                    self.e[int(p[1], 16)] = None if p[2] == 'x' else bytes.fromhex(p[2])
                elif p[0] == 'C':
                    self.c.append((int(p[1], 16), int(p[2], 16), None if p[3] == '-' else int(p[3], 16)))
                elif p[0] == 'G':
                    self.g.append((int(p[1]), None if p[2] == '-' else int(p[2], 16)))

    def build(self):
        """{seq: ('c', cps) | ('i',) | ('x', skip)} for every sequence that
        starts at an incomplete prefix (or the root)."""
        if self.base == 'yes':
            valid = dict(iconv_bases.BASES[self.cid]())
        elif self.base == 'ascii':
            valid = iconv_bases.ascii_part()
        else:
            valid = {}
        for k, v in self.d.items():
            if v is None:
                valid.pop(k, None)
            else:
                valid[k] = v
        prefixes = {b''}
        for s in valid:
            for k in range(1, len(s)):
                prefixes.add(s[:k])
        four = set()
        for pre, bs in self.i.items():
            for b in bs:
                if self.cid == 'GB18030' and len(pre) == 1:
                    # Second byte of a four-byte sequence, decoded by code.
                    four.add(pre + bytes([b]))
                else:
                    prefixes.add(pre + bytes([b]))
        t = {}
        for pre in prefixes:
            for b in range(256):
                s = pre + bytes([b])
                if s in valid:
                    t[s] = ('c', valid[s])
                elif s in prefixes:
                    t[s] = ('i',)
                elif s in four:
                    t[s] = ('4',)
                elif s in self.r:
                    t[s] = ('r', self.r[s])
                else:
                    t[s] = ('x', self.s.get(s, 1))
        return t


def natural_inverse(t):
    inv = {}
    for seq in sorted((s for s, e in t.items() if e[0] == 'c' and len(e[1]) == 1),
                      key=lambda s: (len(s), s)):
        inv.setdefault(t[seq][1][0], seq)
    return inv


def pack(bs):
    assert 1 <= len(bs) <= 4, bs
    return int.from_bytes(bs, 'big')


def verify(cid, t, dump_lines):
    """Compare a reconstructed table with the raw observation dump."""
    import iconv_mkdata
    obs = iconv_mkdata.observed_table(dump_lines)
    t = {s: ('i',) if e == ('4',) else e for s, e in t.items()}
    bad = [s for s in set(obs) | set(t) if obs.get(s) != t.get(s)]
    if bad:
        bad.sort(key=lambda s: (len(s), s))
        raise SystemExit('%s: %d differences, first %s: obs %s gen %s' % (
            cid, len(bad), bad[0].hex(), obs.get(bad[0]), t.get(bad[0])))


class Emitter:
    def __init__(self):
        self.out = []

    def line(self, s=''):
        self.out.append(s)

    def array(self, name, ty, vals, per=16):
        self.line('pub(crate) static %s: [%s; %d] = [' % (name, ty, len(vals)))
        for i in range(0, len(vals), per):
            self.line('    ' + ', '.join(str(v) for v in vals[i:i + per]) + ',')
        self.line('];')

    def text(self):
        return '\n'.join(self.out) + '\n'


def rust_ident(cid):
    s = re.sub(r'[^A-Za-z0-9]', '_', cid.upper())
    if s[0].isdigit():
        s = 'T' + s
    return s


class Pool:
    """All the tables' node entries in one array; a node whose entries already
    appear in it (the same rows in GB2312 and GBK, EUC-KR and CP949, ...)
    points there instead of adding a copy."""

    def __init__(self):
        self.values = []
        self.raw = bytearray()

    def add(self, vals):
        b = b''.join(v.to_bytes(4, 'little') for v in vals)
        i = self.raw.find(b)
        while i != -1 and i % 4:
            i = self.raw.find(b, i + 1)
        if i != -1:
            return i // 4
        start = len(self.values)
        self.values.extend(vals)
        self.raw.extend(b)
        return start


POOL = Pool()


def emit_table(em, cid, t, td):
    ident = rust_ident(cid)
    prefixes = sorted({s[:-1] for s in t}, key=lambda s: (len(s), s))
    node_idx = {p: i for i, p in enumerate(prefixes)}
    multi = []

    def value(e):
        if e[0] == 'c':
            cps = e[1]
            if len(cps) == 0:
                return EMPTY
            if len(cps) == 1:
                return cps[0] + CHAR_BASE
            multi.append(cps)
            return MULTI_FLAG | (len(multi) - 1)
        if e[0] == 'i':
            return None
        if e[0] == '4':
            return FOUR
        if e[0] == 'r':
            return {1: REPLACE1, 2: REPLACE2}[e[1]]
        return {1: ILLEGAL1, 2: ILLEGAL2, 3: ILLEGAL3, 4: ILLEGAL4}[e[1]]

    nodes = []
    values = []
    ident_upto = 0
    for p in prefixes:
        ents = []
        for b in range(256):
            s = p + bytes([b])
            e = t[s]
            v = value(e)
            if v is None:
                v = NODE_FLAG | node_idx[s]
            ents.append(v)
        lo_skip = 0
        if p == b'':
            while lo_skip < 256 and ents[lo_skip] == lo_skip + CHAR_BASE:
                lo_skip += 1
            ident_upto = lo_skip
        rest = ents[lo_skip:]
        counts = {}
        for v in rest:
            counts[v] = counts.get(v, 0) + 1
        default = max(counts, key=lambda v: (counts[v], -v)) if rest else ILLEGAL1
        idx = [i + lo_skip for i, v in enumerate(rest) if v != default]
        if idx:
            lo, hi = idx[0], idx[-1]
        else:
            lo, hi = 1, 0
        nodes.append((lo, hi, default, POOL.add(ents[lo:hi + 1]) if idx else 0))
    fields = {}

    def static(field, ty, items, fmt):
        if not items:
            fields[field] = '&[]'
            return
        name = '%s_%s' % (ident, field.upper())
        em.line('pub(crate) static %s: [%s; %d] = [' % (name, ty, len(items)))
        for it in items:
            em.line('    ' + fmt(it) + ',')
        em.line('];')
        fields[field] = '&' + name

    fields['values'] = '&POOL'
    static('nodes', 'Node', nodes, lambda n: 'Node { lo: %d, hi: %d, default: %d, start: %d }' % n)
    # Encoder deviations from the natural inverse.
    enc = sorted(td.e.items())
    for cp, bs in enc:
        if bs is not None and len(bs) > 4:
            raise ValueError((cid, cp, bs))
    static('enc', '(u32, u32)', enc, lambda e: '(%d, %d)' % (e[0], NO_BYTES if e[1] is None else pack(e[1])))
    static('multi', '&[u32]', multi, lambda m: '&[%s]' % ', '.join(str(c) for c in m))
    comp = compositions(cid, t, td)
    static('compose', '(u32, u32, u32)', comp, lambda c: '(%d, %d, %d)' % c)
    # Pairs of code points the encoder writes as one sequence.
    pairs = sorted((e[1][0], e[1][1], pack(s)) for s, e in t.items() if e[0] == 'c' and len(e[1]) == 2)
    static('pairs', '(u32, u32, u32)', pairs, lambda c: '(%d, %d, %d)' % c)
    ascii = ident_upto >= 0x80 and all(
        td.e.get(c, bytes([c])) == bytes([c]) for c in range(0x80))
    em.line('pub(crate) static %s: Table = Table {' % ident)
    em.line('    ident_upto: %d,' % ident_upto)
    em.line('    ascii: %s,' % ('true' if ascii else 'false'))
    for f in ('nodes', 'values', 'enc', 'multi', 'compose', 'pairs'):
        em.line('    %s: %s,' % (f, fields[f]))
    em.line('};')
    em.line()


UCD = None


def compositions(cid, t, td):
    if not td.c and cid not in ('CP1255', 'CP1258', 'TCVN'):
        return []
    global UCD
    if UCD is None:
        UCD = iconv_translit_base.load_ucd()
    decomp, cat = UCD
    chars = {e[1][0] for s, e in t.items() if len(s) == 1 and e[0] == 'c' and len(e[1]) == 1}
    comp = {}
    for cp, (compat, parts) in decomp.items():
        if not compat and len(parts) == 2 and parts[0] in chars and parts[1] in chars \
                and cat.get(parts[1]) == 'Mn':
            comp[parts] = cp
    for a, b, c in td.c:
        if c is None:
            comp.pop((a, b), None)
        else:
            comp[(a, b)] = c
    return sorted((a, b, c) for (a, b), c in comp.items())


def gb18030_runs(td):
    """Four-byte linear index -> code point, as (index, cp, count) runs."""
    m = {}
    rng = iconv_bases.gb18030_ranges()
    j = 0
    for idx in range(39420):
        while j + 1 < len(rng) and rng[j + 1][0] <= idx:
            j += 1
        m[idx] = rng[j][1] + idx - rng[j][0]
    # Supplementary planes: linear from 90 30 81 30 (GB 18030-2005 6.3.3).
    for cp in range(0x10000, 0x110000):
        m[189000 + cp - 0x10000] = cp
    for idx, cp in td.g:
        if cp is None:
            m.pop(idx, None)
        else:
            m[idx] = cp
    runs = []
    for idx in sorted(m):
        cp = m[idx]
        if runs and runs[-1][0] + runs[-1][2] == idx and runs[-1][1] + runs[-1][2] == cp:
            runs[-1][2] += 1
        else:
            runs.append([idx, cp, 1])
    return runs


def emit_translit(em):
    base = iconv_translit_base.Base()
    over = {}
    with open(os.path.join(DATA_DIR, 'translit.txt')) as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            cp, rest = line.split()
            if rest == 'none':
                over[int(cp, 16)] = []
            else:
                over[int(cp, 16)] = [() if c == '-' else tuple(int(x, 16) for x in c.split('+'))
                                     for c in rest.split(';')]
    rules = {}
    for cp in range(0x80, 0x110000):
        if 0xD800 <= cp < 0xE000:
            continue
        c = over[cp] if cp in over else base.candidates(cp)
        if c:
            rules[cp] = c
    em.line('// //TRANSLIT replacement lists, one line per code point:')
    em.line('// "<hex code point> <replacement>\\t<replacement>..." (an empty replacement is')
    em.line('// the empty string).')
    em.line('pub(crate) static TRANSLIT: &str = "\\')
    for cp in sorted(rules):
        em.line('%X %s\\n\\' % (cp, '\\t'.join(rust_escape(''.join(chr(x) for x in c)) for c in rules[cp])))
    em.line('";')


def rust_escape(text):
    import unicodedata
    out = []
    for ch in text:
        o = ord(ch)
        cat = unicodedata.category(ch) if o < 0x110000 else 'Cn'
        if ch in '"\\' or o < 0x20 or o == 0x7F or cat[0] in 'CMZ':
            out.append('\\u{%X}' % o)
        else:
            out.append(ch)
    return ''.join(out)


HEADER = '''// Generated by tools/charset/gen_iconv.py; do not edit.
//
// Sources: Unicode MAPPINGS, the WHATWG Encoding Standard indexes, UnicodeData.txt,
// CLDR Latin-ASCII, and the differences observed on PHP 8.5 (glibc 2.43 iconv)
// recorded in tools/charset/iconv-data/.
'''


def main():
    charsets = load_charsets()
    dumps = None
    if len(sys.argv) > 2 and sys.argv[1] == '--verify':
        import iconv_mkdata
        dumps = iconv_mkdata.parse_dump(sys.argv[2])
        reps = iconv_mkdata.representatives(sys.argv[2])
    em = Emitter()
    em.line(HEADER)
    em.line('#![allow(clippy::unreadable_literal)]')
    em.line()
    em.line('use super::super::table::{Node, Table};')
    em.line()
    names = []
    kinds = []
    for cid, kind, aliases in charsets:
        if kind == 'skip':
            continue
        if kind in ('table', 'compose') or cid == 'GB18030':
            td = TableData(cid)
            t = td.build()
            if dumps is not None:
                verify(cid, t, dumps[reps[cid]])
            emit_table(em, cid, t, td)
            if cid == 'GB18030':
                runs = gb18030_runs(td)
                em.line('/// GB18030 four-byte sequences: (linear index, first code point, count).')
                em.line('pub(crate) static GB18030_FOUR: [(u32, u32, u32); %d] = [' % len(runs))
                for r in runs:
                    em.line('    (%d, %d, %d),' % tuple(r))
                em.line('];')
                kinds.append('Kind::Builtin(Builtin::Gb18030)')
            else:
                kinds.append('Kind::Table(&cp::%s)' % rust_ident(cid))
        else:
            kinds.append('Kind::Builtin(Builtin::%s)' % kind.split(':')[1])
        names.append((cid, aliases))
    em.line('/// The node entries of every table (see `Pool` in tools/charset/gen_iconv.py).')
    em.array('POOL', 'u32', POOL.values, 20)
    with open(os.path.join(OUT_DIR, 'cp.rs'), 'w') as f:
        f.write(em.text())
    # Charset list and aliases.
    em = Emitter()
    em.line(HEADER)
    em.line('use super::super::charset::{Builtin, CharsetInfo, Kind};')
    em.line('use super::cp;')
    em.line()
    em.line('pub(crate) static CHARSETS: [CharsetInfo; %d] = [' % len(names))
    for (cid, _), kind in zip(names, kinds):
        em.line('    CharsetInfo { name: "%s", kind: %s },' % (cid, kind))
    em.line('];')
    alias_map = {}
    for i, (cid, aliases) in enumerate(names):
        for a in [cid] + aliases:
            n = norm_name(a)
            if n.endswith('/') and not n.endswith('//'):
                n = n[:-1]
            prev = alias_map.get(n)
            if prev is not None and prev != i:
                raise ValueError('alias %s maps to %s and %s' % (n, names[prev][0], cid))
            alias_map[n] = i
    em.line()
    em.line('/// Normalised names (see `names::normalise`), sorted, with their index in CHARSETS.')
    em.line('pub(crate) static ALIASES: [(&str, u16); %d] = [' % len(alias_map))
    for n in sorted(alias_map):
        em.line('    ("%s", %d),' % (n, alias_map[n]))
    em.line('];')
    with open(os.path.join(OUT_DIR, 'names.rs'), 'w') as f:
        f.write(em.text())
    em = Emitter()
    em.line(HEADER)
    emit_translit(em)
    with open(os.path.join(OUT_DIR, 'translit.rs'), 'w') as f:
        f.write(em.text())


if __name__ == '__main__':
    main()
