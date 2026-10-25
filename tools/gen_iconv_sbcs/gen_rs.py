#!/usr/bin/env python3
"""sbcs.json + aliases.json -> crates/goro-ext-iconv/src/sbcs_tables.rs

Pipeline (with a php built against glibc iconv):
  python3 gconv.py OUT                   # glibc gconv-modules -> OUT/names.txt, OUT/aliases.json
  php -n dump_sbcs.php OUT/names.txt OUT/sbcs.json
  python3 gen_rs.py OUT crates/goro-ext-iconv/src/sbcs_tables.rs

BIG5 overrides: run mbmap.php BIG5 with php (> p.txt) and goro built
without big5_glibc.rs entries (> g.txt), then
  python3 gen_big5.py p.txt g.txt crates/goro-ext-iconv/src/big5_glibc.rs
"""
import json, sys
d = sys.argv[1]
sb = json.load(open(d + '/sbcs.json'))
al = json.load(open(d + '/aliases.json'))
skip = set()
tables = []   # (dec tuple, extra tuple)
index = {}
names = {}
for cs, t in sb.items():
    if cs in skip:
        continue
    key = (tuple(t['dec']), tuple(tuple(x) for x in t['extra']))
    if key not in index:
        index[key] = len(tables)
        tables.append(key)
    names[cs.upper()] = index[key]
for a, target in al.items():
    if target in names and a.upper() not in names:
        names[a.upper()] = names[target]
o = []
o.append('//! glibc\'s stateless single-byte charsets (generated from glibc iconv by\n'
         '//! decoding every byte and encoding every BMP character; 0xFFFF marks an\n'
         '//! undefined byte). `extra` lists the encodings that are not the reverse\n'
         '//! of the decode table (one-way mappings, or a character two bytes decode to).\n\n')
o.append('pub struct Sbcs {\n    /// Bytes 0x80-0xFF, or 0x00-0xFF when the low half is not ASCII.\n    pub dec: &\'static [u16],\n    pub extra: &\'static [(u16, u8)],\n}\n\n')
o.append('pub static TABLES: [Sbcs; %d] = [\n' % len(tables))
for dec, extra in tables:
    o.append('    Sbcs {\n        dec: &[\n')
    vals = ['0x%X' % (0xFFFF if v < 0 else v) for v in dec]
    if list(dec[:128]) == list(range(128)):
        vals = vals[128:]
    for i in range(0, len(vals), 16):
        o.append('            ' + ','.join(vals[i:i + 16]) + ',\n')
    o.append('        ],\n        extra: &[')
    if extra:
        o.append('\n')
        ex = ['(0x%X, 0x%X)' % (c, b) for c, b in sorted(extra)]
        for i in range(0, len(ex), 6):
            o.append('            ' + ', '.join(ex[i:i + 6]) + ',\n')
        o.append('        ')
    o.append('],\n    },\n')
o.append('];\n\n')
o.append('/// Charset names and aliases (upper case, sorted) to TABLES indexes.\n')
o.append('pub static NAMES: [(&str, u16); %d] = [\n' % len(names))
for n in sorted(names):
    o.append('    ("%s", %d),\n' % (n, names[n]))
o.append('];\n')
# aliases of the other (multi-byte, stateful) charsets, resolved by
# charset::lookup() to the canonical names it knows
others = {}
for a, target in al.items():
    if a.upper() not in names and target.upper() not in names:
        others[a.upper()] = target.upper()
o.append('\n/// glibc\'s aliases of the other charsets (upper case, sorted) to their\n/// canonical names.\n')
o.append('pub static ALIASES: [(&str, &str); %d] = [\n' % len(others))
for n in sorted(others):
    o.append('    ("%s", "%s"),\n' % (n, others[n]))
o.append('];\n')
open(sys.argv[2], 'w').write(''.join(o))
print(len(tables), len(names))
