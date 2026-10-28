"""Base //TRANSLIT rules for iconv, from public Unicode data.

A rule is the ordered list of replacement strings tried for a code point the
target charset can't encode; the first one the target can encode wins, and
"?" is used when none can (PHP 8.5 with glibc 2.43 iconv, C.UTF-8 locale:
observed). The base list for a code point is, in order:

1. its full canonical decomposition from UnicodeData.txt, or its one-level
   compatibility decomposition (each part canonically decomposed), with
   nonspacing marks (gc=Mn) removed;
2. its full compatibility decomposition (NFKD), marks removed;
3. CLDR's Latin-ASCII transform (common/transforms/Latin-ASCII.xml) applied to
   each character of (2), when that gives pure ASCII.

Nonspacing marks themselves transliterate to nothing.

Sources: https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt,
https://github.com/unicode-org/cldr/blob/main/common/transforms/Latin-ASCII.xml
"""
import os
import re

DATA = os.environ.get('ICONV_DATA', '/tmp/claude-1000/goro-work/charset-iconv/data')


def load_ucd():
    decomp, cat = {}, {}
    with open(os.path.join(DATA, 'UnicodeData.txt'), encoding='utf-8') as f:
        rows = [line.split(';') for line in f]
    i = 0
    while i < len(rows):
        r = rows[i]
        cp = int(r[0], 16)
        if r[1].endswith('First>'):
            last = int(rows[i + 1][0], 16)
            for c in range(cp, last + 1):
                cat[c] = r[2]
            i += 2
            continue
        cat[cp] = r[2]
        if r[5]:
            p = r[5].split()
            compat = p[0].startswith('<')
            if compat:
                p = p[1:]
            decomp[cp] = (compat, tuple(int(x, 16) for x in p))
        i += 1
    return decomp, cat


def load_latin_ascii():
    """The context-free single-character rules of CLDR Latin-ASCII."""
    rules = {}
    with open(os.path.join(DATA, 'Latin-ASCII.xml'), encoding='utf-8') as f:
        text = f.read()
    for line in text.split('\n'):
        line = line.strip()
        if '→' not in line or line.startswith('#') or line.startswith('::'):
            continue
        left, right = line.split('→', 1)
        left = left.strip()
        if len(left) != 1:
            continue
        out, i, q = [], 0, False
        right = right.strip()
        while i < len(right):
            c = right[i]
            if c == '\\' and i + 1 < len(right):
                if right[i + 1] == 'u':
                    out.append(chr(int(right[i + 2:i + 6], 16)))
                    i += 6
                    continue
                out.append(right[i + 1])
                i += 2
                continue
            if c == "'":
                if q and i + 1 < len(right) and right[i + 1] == "'":
                    out.append("'")
                    i += 2
                    continue
                q = not q
                i += 1
                continue
            if not q and c == ';':
                break
            if not q and c in ' \t':
                i += 1
                continue
            out.append(c)
            i += 1
        rules[ord(left)] = ''.join(out)
    return rules


class Base:
    def __init__(self):
        self.decomp, self.cat = load_ucd()
        self.latin = load_latin_ascii()

    def is_mark(self, cp):
        return self.cat.get(cp) == 'Mn'

    def nfkd(self, cp):
        d = self.decomp.get(cp)
        if d is None:
            if 0xAC00 <= cp <= 0xD7A3:
                s = cp - 0xAC00
                out = [0x1100 + s // 588, 0x1161 + (s % 588) // 28]
                if s % 28:
                    out.append(0x11A7 + s % 28)
                return out
            return [cp]
        out = []
        for c in d[1]:
            out.extend(self.nfkd(c))
        return out

    def nfd(self, cp):
        d = self.decomp.get(cp)
        if d is None or d[0]:
            return [cp]
        out = []
        for c in d[1]:
            out.extend(self.nfd(c))
        return out

    def strip(self, cps):
        return tuple(c for c in cps if not self.is_mark(c))

    def candidates(self, cp):
        if self.is_mark(cp):
            return [()]
        out = []
        d = self.decomp.get(cp)
        if d is not None:
            if not d[0]:
                # Canonical: the full decomposition without its marks.
                out.append(self.strip(self.nfd(cp)))
            else:
                # Compatibility: one level, each part without its marks.
                one = []
                for c in d[1]:
                    one.extend(self.nfd(c))
                out.append(self.strip(one))
        full = self.strip(self.nfkd(cp))
        if full != (cp,):
            out.append(full)
        conv = []
        for c in (full or (cp,)):
            if c < 0x80:
                conv.append(c)
            elif c in self.latin:
                conv.extend(ord(x) for x in self.latin[c])
            else:
                conv = None
                break
        if conv is not None:
            out.append(tuple(conv))
        res = []
        for c in out:
            if c and c != (cp,) and c not in res:
                res.append(c)
        return res
