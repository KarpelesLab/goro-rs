#!/usr/bin/env python3
"""glibc dumps (dump_all.sh) -> crates/goro-ext-iconv/src/cjk_tables.bin

  bash dump_all.sh /path/to/php DUMPS
  python3 gen.py DUMPS crates/goro-ext-iconv/src/cjk_tables.bin

The blob is a u32 (LE) uncompressed length followed by a zlib stream of
records: name length (u8), name, type (u8) and payload, varints LEB128:
  b'G' grid: lead_lo, lead_hi, trail_lo, trail_hi (u8 each), then one
       varint per cell: 0 undefined, else zigzag(value - previous value) + 1.
       Values: a code point, PAIR + i (two code points: pairs[i]), LEAD (a
       lead byte, single-byte grids only).
  b'E' encodings: count, then (code point delta, code) sorted by code point.
  b'P' pairs: count, then (first, second) code points.
  b'B' bitmap: byte count, bytes.
"""
import struct
import sys
import zlib

D = sys.argv[1]
PAIR = 0x200000
LEAD = 0x1FFFFF


def load(name):
    out = {}
    for ln in open('%s/%s' % (D, name)):
        k, *v = ln.split()
        if not v:
            # (glibc's encoders drop the Unicode tag characters)
            assert 0xE0000 <= int(k, 16) <= 0xE007F
            continue
        v = v[0]
        out[int(k, 16) if k else 0] = v
    return out


def cps(v):
    return [int(v[i:i + 8], 16) for i in range(0, len(v), 8)]


def varint(n):
    assert n >= 0
    b = bytearray()
    while True:
        x = n & 0x7F
        n >>= 7
        if n:
            b.append(x | 0x80)
        else:
            b.append(x)
            return bytes(b)


def zz(n):
    return n * 2 if n >= 0 else -n * 2 - 1


recs = []


def rec(name, typ, payload):
    nb = name.encode()
    recs.append(bytes([len(nb)]) + nb + typ + payload)


class Pairs:
    def __init__(self):
        self.list = []

    def get(self, a, b):
        if (a, b) not in self.list:
            self.list.append((a, b))
        return PAIR + self.list.index((a, b))


def grid(name, cells, llo, lhi, tlo, thi):
    """cells: {(lead, trail): value}"""
    p = bytearray([llo, lhi, tlo, thi])
    prev = 0
    for l in range(llo, lhi + 1):
        for t in range(tlo, thi + 1):
            v = cells.get((l, t))
            if v is None:
                p += varint(0)
            else:
                p += varint(zz(v - prev) + 1)
                prev = v
    for k in cells:
        assert llo <= k[0] <= lhi and tlo <= k[1] <= thi, (name, k)
    rec(name, b'G', bytes(p))


def enc(name, pairs):
    p = bytearray(varint(len(pairs)))
    prev = 0
    for cp, code in sorted(pairs):
        p += varint(cp - prev) + varint(code)
        prev = cp
    rec(name, b'E', bytes(p))


def pairs_rec(name, pr):
    p = bytearray(varint(len(pr.list)))
    for a, b in pr.list:
        p += varint(a) + varint(b)
    rec(name, b'P', bytes(p))


def single(name, d1):
    cells = {}
    for b, v in d1.items():
        if v == 'INC':
            cells[(0, b)] = LEAD
        elif v != 'ILL':
            c = cps(v)
            assert len(c) == 1
            cells[(0, b)] = c[0]
    grid(name, cells, 0, 0, 0, 255)
    return cells


def dbcs(name, d1, d2, rng, pr):
    """Two-byte cells of a charset whose lead bytes decode alone as INC."""
    cells = {}
    for k, v in d2.items():
        l, t = k >> 8, k & 0xFF
        if d1.get(l) != 'INC' or v in ('INC', 'ILL'):
            continue
        c = cps(v)
        cells[(l, t)] = c[0] if len(c) == 1 else pr.get(*c)
    lo = min(l for l, _ in cells), max(l for l, _ in cells)
    to = min(t for _, t in cells), max(t for _, t in cells)
    if rng:
        lo, to = rng[:2], rng[2:]
    grid(name, cells, lo[0], lo[1], to[0], to[1])
    return cells


def inverse(grids):
    """The encoder the runtime derives from decode grids [(cells, code)]:
    the first cell of a character wins (single code points only; not the
    ASCII bytes that decode to themselves)."""
    inv = {}
    for cells, code in grids:
        for (l, t) in sorted(cells):
            v = cells[(l, t)]
            if v >= LEAD or (code(l, t) == v < 0x80):
                continue
            inv.setdefault(v, code(l, t))
    return inv


def exceptions(name, inv, full, ident=lambda cp: True, drop=lambda cp: True):
    """The encodings `full` (code point -> code) differs from the derived
    ones in (code 0: none). The runtime encodes an ASCII character that is
    not in the table as itself when ident(cp)."""
    def model(cp):
        if cp in inv:
            return inv[cp]
        return cp if cp < 0x80 and ident(cp) else None
    out = []
    for cp, c in full.items():
        if model(cp) != c:
            out.append((cp, c))
    for cp in list(inv) + list(range(0x80)):
        if cp not in full and model(cp) is not None and drop(cp):
            out.append((cp, 0))
    enc(name, out)
    print(name, len(out), 'exceptions', file=sys.stderr)


def full_enc(e, code=lambda b: int.from_bytes(b, 'big')):
    out = {}
    for cp, v in e.items():
        c = code(bytes.fromhex(v))
        if c is not None:
            out[cp] = c
    return out


# JOHAB
d1 = load('JOHAB.d1')
sb = single('JOHAB1', d1)
pr = Pairs()
cells = dbcs('JOHAB', d1, load('JOHAB.d2'), None, pr)
assert not pr.list
# //IGNORE skips both bytes of these illegal pairs (else one)
ign = load('JOHAB.ign')
g = [0] * (256 * 256 // 8)
for k, v in ign.items():
    if v == '2':
        g[k >> 3] |= 1 << (k & 7)
rec('JOHAB_SKIP2', b'B', varint(len(g)) + bytes(g))
exceptions('JOHAB_E', inverse([(sb, lambda l, t: t), (cells, lambda l, t: l << 8 | t)]),
           full_enc(load('JOHAB.e')), lambda cp: sb.get((0, cp)) == cp)

# BIG5-HKSCS
d1 = load('BIG5-HKSCS.d1')
sb = single('HKSCS1', d1)
pr = Pairs()
cells = dbcs('HKSCS', d1, load('BIG5-HKSCS.d2'), None, pr)
pairs_rec('HKSCS_P', pr)
exceptions('HKSCS_E', inverse([(sb, lambda l, t: t), (cells, lambda l, t: l << 8 | t)]),
           full_enc(load('BIG5-HKSCS.e')), lambda cp: sb.get((0, cp)) == cp)

# JIS X 0213 (from EUC-JISX0213): plane 1, plane 2 (0x8F)
d1 = load('EUC-JISX0213.d1')
pr = Pairs()
p1 = {}
for k, v in load('EUC-JISX0213.d2').items():
    l, t = k >> 8, k & 0xFF
    if 0xA1 <= l <= 0xFE and 0xA1 <= t <= 0xFE and v not in ('INC', 'ILL'):
        c = cps(v)
        p1[(l - 0x80, t - 0x80)] = c[0] if len(c) == 1 else pr.get(*c)
p2 = {}
for k, v in load('EUC-JISX0213.p2').items():
    l, t = k >> 8, k & 0xFF
    if 0xA1 <= l <= 0xFE and 0xA1 <= t <= 0xFE and v not in ('INC', 'ILL'):
        c = cps(v)
        assert len(c) == 1
        p2[(l - 0x80, t - 0x80)] = c[0]
grid('JISX0213_1', p1, 0x21, 0x7E, 0x21, 0x7E)
grid('JISX0213_2', p2, 0x21, 0x7E, 0x21, 0x7E)
pairs_rec('JISX0213_P', pr)
assert all(d1[b] == 'INC' for b in range(0xA1, 0xFF))


def euc_code(b):
    if len(b) == 1:
        return b[0]
    if b[0] == 0x8E:
        return 0x8E00 | b[1]
    if b[0] == 0x8F:
        return 0x10000 | (b[1] << 8 | b[2]) - 0x8080
    return (b[0] << 8 | b[1]) - 0x8080


exceptions('JISX0213_E', inverse([(p1, lambda l, t: l << 8 | t),
                                  (p2, lambda l, t: 0x10000 | l << 8 | t)]),
           full_enc(load('EUC-JISX0213.e'), euc_code))


# SHIFT_JISX0213 is the same table through the JIS X 0213 Shift_JIS
# arrangement (checked here).
def sjis(row, col, plane):
    odd = row % 2 == 1
    if plane == 1:
        lead = (row + 1) // 2 + (0x80 if row <= 62 else 0xC0)
    elif row >= 79:
        lead = (row + 1) // 2 + 0xCD
    else:
        lead, odd = {1: (0xF0, 1), 8: (0xF0, 0), 3: (0xF1, 1), 4: (0xF1, 0),
                     5: (0xF2, 1), 12: (0xF2, 0), 13: (0xF3, 1), 14: (0xF3, 0),
                     15: (0xF4, 1), 78: (0xF4, 0)}[row]
    if odd:
        trail = col + (0x3F if col <= 63 else 0x40)
    else:
        trail = col + 0x9E
    return lead, trail


sd2 = load('SHIFT_JISX0213.d2')
sd1 = load('SHIFT_JISX0213.d1')
seen = {}
for (row, col), v in list(p1.items()):
    seen[sjis(row - 0x20, col - 0x20, 1)] = v
for (row, col), v in list(p2.items()):
    seen[sjis(row - 0x20, col - 0x20, 2)] = v
bad = 0
for k, v in sd2.items():
    l, t = k >> 8, k & 0xFF
    if sd1.get(l) != 'INC' or v in ('INC', 'ILL'):
        continue
    c = cps(v)
    w = c[0] if len(c) == 1 else pr.get(*c)
    if seen.pop((l, t), None) != w:
        bad += 1
        if bad < 5:
            print('sjis mismatch', hex(k), v, file=sys.stderr)
assert not bad and not seen, (bad, len(seen))

# CNS 11643 planes 3-7 and 15 (EUC-TW 0x8E 0xA3-0xA7, 0xAF)
planes = []
for p in (3, 4, 5, 6, 7, 15):
    cells = {}
    planes.append((cells, lambda l, t, p=p: p << 16 | l << 8 | t))
    for k, v in load('EUC-TW.p%d' % p).items():
        l, t = k >> 8, k & 0xFF
        if 0xA1 <= l <= 0xFE and 0xA1 <= t <= 0xFE and v not in ('INC', 'ILL'):
            c = cps(v)
            assert len(c) == 1
            cells[(l - 0x80, t - 0x80)] = c[0]
    grid('CNS%d' % p, cells, 0x21, 0x7E, 0x21, 0x7E)
tw = load('EUC-TW.e')
exceptions('CNS_E', inverse(planes),
           full_enc(tw, lambda b: (b[1] - 0xA0) << 16 | (b[2] << 8 | b[3]) - 0x8080
                    if len(b) == 4 and b[1] >= 0xA3 else None),
           lambda cp: False, lambda cp: cp not in tw)

# ISO-IR-165 (ISO-2022-CN-EXT ESC $ ) E)
cells = {}
for k, v in load('ISO-IR-165.d2').items():
    l, t = k >> 8, k & 0xFF
    if 0x21 <= l <= 0x7E and 0x21 <= t <= 0x7E and v not in ('INC', 'ILL'):
        c = cps(v)
        assert len(c) == 1
        cells[(l, t)] = c[0]
grid('IR165', cells, 0x21, 0x7E, 0x21, 0x7E)
raw = b''.join(recs)
open(sys.argv[2], 'wb').write(struct.pack('<I', len(raw)) + zlib.compress(raw, 9))
print(len(raw), 'bytes,', len(zlib.compress(raw, 9)), 'compressed')
for r in recs:
    print(r[1:1 + r[0]].decode(), len(r), len(zlib.compress(r, 9)), file=sys.stderr)
