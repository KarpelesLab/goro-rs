#!/usr/bin/env python3
"""Solve mb_detect_encoding() code point demerits from probe thresholds.

Usage: python3 -I tools/charset/detect_weights.py PROBE_OUTPUT > tools/charset/detect_common.txt

PROBE_OUTPUT is the concatenated output of tools/charset/fixtures/
mb_detect_weights.php over 0..65536. The comparison model was itself
established by observation on PHP 8.5: a candidate's score is the sum of its
code points' demerits, the candidate at list position p of n has its score
raised by floor(score * 3p / 10n), and the lowest raised score wins (the
earliest on ties). With two candidates that is a 15% advantage for the first.

For each unit the thresholds determine the demerits of the unit read as
UTF-16BE (x) and as UTF-16LE (y). The output lists the BMP ranges whose
demerit is not the default 30.
"""

import collections
import sys

M = 200
CANDIDATES = list(range(0, 62)) + [100, 200, 300, 500, 1000, 1001, 1006, 1030, 1500, 2000]


def winner(sb, sl):
    """'<' when UTF-16BE wins in both list orders, '>' for UTF-16LE, '=' when the first listed wins."""
    if sl + sl * 3 // 20 < sb:
        return '>'
    if sb + sb * 3 // 20 < sl:
        return '<'
    return '='


def first(b, l, db, dl, pred):
    if pred(winner(b, l)):
        return 0
    lo, hi = 0, 1
    while not pred(winner(b + db * hi, l + dl * hi)):
        lo, hi = hi, hi * 2
        if hi > (1 << 20):
            return -1
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if pred(winner(b + db * mid, l + dl * mid)):
            hi = mid
        else:
            lo = mid
    return hi


def thresholds(x, y):
    b, l = M * x, M * y
    return (first(b, l, 1, 30, lambda r: r != '>'), first(b, l, 1, 30, lambda r: r == '<'),
            first(b, l, 30, 1, lambda r: r != '<'), first(b, l, 30, 1, lambda r: r == '>'))


def main():
    table = collections.defaultdict(list)
    for x in CANDIDATES:
        for y in CANDIDATES:
            table[thresholds(x, y)].append((x, y))
    w = {}
    for line in open(sys.argv[1]):
        p = line.split()
        u = int(p[0], 16)
        c = table.get(tuple(int(v) for v in p[1:5]))
        if not c or len(c) > 1:
            # U+FEFF/U+FFFE (a leading byte order mark costs nothing).
            continue
        x, y = c[0]
        s = ((u & 0xFF) << 8) | (u >> 8)
        for cp, v in ((u, x), (s, y)):
            if w.get(cp, v) != v:
                sys.exit('inconsistent demerits for U+%04X' % cp)
            w[cp] = v
    print('# mb_detect_encoding() demerits other than the default 30, observed on')
    print('# PHP 8.5 (tools/charset/fixtures/mb_detect_weights.php, solved by')
    print('# tools/charset/detect_weights.py). Surrogates (1000) are decoding errors.')
    print('# Format: FIRST LAST DEMERIT (hex code points).')
    start = 0
    for cp in range(1, 0x10001):
        if cp == 0x10000 or w.get(cp, 30) != w.get(start, 30):
            v = w.get(start, 30)
            if v != 30 and not 0xD800 <= start <= 0xDFFF:
                print('%04X %04X %d' % (start, cp - 1, v))
            start = cp


if __name__ == '__main__':
    main()
