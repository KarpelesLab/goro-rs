#!/usr/bin/env python3
"""Compare the base //TRANSLIT rules (iconv_translit_base.py, from UCD and
CLDR) with what PHP 8.5 (glibc 2.43 iconv, C.UTF-8 locale) does, and write
the code points where they differ to tools/charset/iconv-data/translit.txt.

usage: python3 -I iconv_mktranslit.py <obs-dir> <out-dir>

<obs-dir> holds translit.out (tools/charset/iconv_obs_translit.php) and
tables.out (iconv_obs_tables.php, for the target charsets' repertoires).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import iconv_translit_base  # noqa: E402

TARGETS = {
    'ASCII': None, 'ISO-8859-1': 'ISO_8859-1:1987', 'ISO-8859-2': 'ISO_8859-2:1987',
    'ISO-8859-15': 'ISO-8859-15', 'CP1252': 'windows-1252', 'ISO-8859-5': 'ISO_8859-5:1988',
    'KOI8-R': 'KOI8-R', 'ISO-8859-7': 'ISO_8859-7:1987', 'SJIS': 'Shift_JIS',
    'EUC-KR': 'EUC-KR', 'GBK': 'GBK', 'EUC-JP': 'EUC-JP', 'BIG5': 'Big5',
    'CP1250': 'windows-1250', 'CP1251': 'windows-1251',
}


def load_repertoires(obs_dir):
    """Per target: {bytes: cps} decoding and {cp: bytes} encoding maps
    (shortest, then lowest, sequence, corrected by the observed E lines)."""
    path = os.path.join(obs_dir, 'tables.out')
    import re
    reps = {v: k for k, v in TARGETS.items() if v}
    dec = {k: {} for k in TARGETS}
    extra = {k: {} for k in TARGETS}
    entry = re.compile(r'^[0-9A-F]+(\+[0-9A-F]+)*$')
    with open(path, encoding='latin-1') as f:
        for line in f:
            p = line.split()
            if len(p) < 3 or p[1] not in reps:
                continue
            t = reps[p[1]]
            if p[0] == 'D':
                pre = b'' if p[2] == '-' else bytes.fromhex(p[2])
                for b, e in enumerate(p[3:]):
                    if entry.match(e):
                        dec[t][pre + bytes([b])] = tuple(int(x, 16) for x in e.split('+'))
            elif p[0] == 'E':
                extra[t][int(p[2], 16)] = None if p[3] == 'x' else bytes.fromhex(p[3])
    for i in range(0x80):
        dec['ASCII'][bytes([i])] = (i,)
    enc = {}
    for t in TARGETS:
        m = {}
        for seq in sorted((s for s, c in dec[t].items() if len(c) == 1), key=lambda s: (len(s), s)):
            m.setdefault(dec[t][seq][0], seq)
        for cp, b in extra[t].items():
            if b is None:
                m.pop(cp, None)
            else:
                m[cp] = b
        enc[t] = m
    return dec, enc


def decode(table, data):
    out, i = [], 0
    while i < len(data):
        for n in (1, 2, 3, 4):
            if data[i:i + n] in table:
                out.extend(table[data[i:i + n]])
                i += n
                break
        else:
            raise ValueError(data.hex())
    return tuple(out)


def main():
    obs_dir, out_dir = sys.argv[1], sys.argv[2]
    dec, enc = load_repertoires(obs_dir)
    obs = {}
    with open(os.path.join(obs_dir, 'translit.out')) as f:
        for line in f:
            _, t, cp, r = line.split()
            obs.setdefault(int(cp, 16), {})[t] = r
    base = iconv_translit_base.Base()

    def encode(c, t):
        if all(x in enc[t] for x in c):
            return b''.join(enc[t][x] for x in c)
        return None

    def result(cp, t):
        """Observed bytes for target t (None when the conversion fails)."""
        o = obs.get(cp, {})
        a = o.get('ASCII', '3f')
        r = o.get(t) if t != 'ASCII' else a
        if r is None:
            # Not printed: the ASCII result, encoded in t.
            if a == 'x':
                return None
            return b'?' if a == '3f' else encode(tuple(bytes.fromhex(a if a != '-' else '')), t)
        if r == 'x':
            return None
        return b'' if r == '-' else bytes.fromhex(r)

    def simulate(cands, t):
        for c in cands:
            b = encode(c, t)
            if b is not None:
                return b
        return b'?'

    cps = set(obs)
    for cp in range(0x80, 0x110000):
        if 0xD800 <= cp < 0xE000:
            continue
        if base.candidates(cp):
            cps.add(cp)
    overrides = []
    agree = 0
    unexplained = []
    for cp in sorted(cps):
        targets = [t for t in TARGETS if cp not in enc[t]]
        if not targets:
            continue
        want = {t: result(cp, t) for t in targets}
        cands = base.candidates(cp)
        if all(simulate(cands, t) == want[t] for t in targets):
            agree += 1
            continue
        # Which replacement each target used: a string from the base list or
        # from decoding a result, that encodes to the observed bytes; when
        # several do, the one most targets agree on.
        pool = list(cands)
        for t in targets:
            r = want[t]
            if r is not None and r != b'?':
                c = decode(dec[t], r)
                if c not in pool:
                    pool.append(c)
        used = {}
        for t in targets:
            r = want[t]
            if r is None or r == b'?':
                continue
            m = [c for c in pool if encode(c, t) == r]
            if m:
                score = {c: sum(1 for u in targets if encode(c, u) is not None and encode(c, u) == want[u]) for c in m}
                used[t] = max(m, key=lambda c: (score[c], -pool.index(c)))
        found = []
        for t in targets:
            if t in used and used[t] not in found:
                found.append(used[t])
        after = {c: set() for c in found}
        for t, r in used.items():
            for c in found:
                if c != r and encode(c, t) is not None:
                    after[r].add(c)
        order = []
        def rank(c):
            # Base order first, then the richer replacement.
            return (cands.index(c) if c in cands else len(cands), -sum(1 for x in c if x >= 0x80), -len(c), c)
        pending = sorted(found, key=rank)
        while pending:
            for c in pending:
                if not any(c in after[d] for d in pending if d != c):
                    order.append(c)
                    pending.remove(c)
                    break
            else:
                order.extend(pending)
                break
        # Keep the base replacements no observed target would have used: they
        # matter for the charsets that were not observed. Not after an ASCII
        # replacement, which ends the lists (observed: EBCDIC-US gets "?" for
        # U+3378 rather than the NFKD "dm2" after "dm^2").
        for c in ([] if any(all(x < 0x80 for x in o) for o in order) else cands):
            if c in order:
                continue
            if all(encode(c, t) is None for t in targets if all(encode(o, t) is None for o in order)):
                order.append(c)
        if not all(simulate(order, t) == want[t] for t in targets):
            unexplained.append(cp)
        overrides.append((cp, order))
    with open(os.path.join(out_dir, 'translit.txt'), 'w') as f:
        f.write('# iconv //TRANSLIT: code points whose replacement list differs from the\n')
        f.write('# base rules (tools/charset/iconv_translit_base.py: UCD decompositions,\n')
        f.write('# CLDR Latin-ASCII). Observed on PHP 8.5 (glibc 2.43 iconv, C.UTF-8\n')
        f.write('# locale) with tools/charset/iconv_obs_translit.php.\n')
        f.write('# <cp> <replacement>;<replacement>... (hex code points joined by +,\n')
        f.write('# "-" for the empty string, "none" when there is no rule and "?" is used)\n')
        for cp, order in overrides:
            if not order:
                f.write('%X none\n' % cp)
            else:
                f.write('%X %s\n' % (cp, ';'.join('+'.join('%X' % x for x in c) if c else '-' for c in order)))
    print('base agrees for', agree, 'code points, overrides for', len(overrides),
          'unexplained', len(unexplained), ' '.join('%X' % c for c in unexplained[:20]))


if __name__ == '__main__':
    main()
